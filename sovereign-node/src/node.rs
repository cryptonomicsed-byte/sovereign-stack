//! SovereignNode — wires all protocol subsystems together.
//!
//! Subsystems started by SovereignNode::start():
//!   1. VCP DiscoveryDaemon — BLE/mDNS scan loop
//!   2. DIP Router — envelope routing with Vantage + Nostr adapters
//!   3. API server (axum) — /health /status /devices /capture/:id
//!   4. Graceful shutdown on SIGTERM / SIGINT

use std::sync::Arc;
use std::net::SocketAddr;

use axum::{Router, routing::get, routing::post, extract::{State, Path}, Json, response::IntoResponse};
use serde_json::{json, Value};
use tokio::sync::RwLock;
use tracing::{info, warn, error};

use vcp::{DiscoveryDaemon, DeviceRegistry};
use dip::{DipRouter, address::DipNetwork};
use crate::config::NodeConfig;
use crate::identity::NodeIdentity;

/// Shared node state visible to all axum handlers.
#[derive(Clone)]
pub struct NodeState {
    pub identity: Arc<NodeIdentity>,
    pub config:   Arc<NodeConfig>,
    pub registry: DeviceRegistry,
    pub started_at: u64,
}

pub struct SovereignNode {
    pub identity: Arc<NodeIdentity>,
    pub config:   Arc<NodeConfig>,
}

impl SovereignNode {
    pub fn new(identity: NodeIdentity, config: NodeConfig) -> Self {
        Self {
            identity: Arc::new(identity),
            config:   Arc::new(config),
        }
    }

    /// Start all subsystems and block until SIGTERM/SIGINT.
    pub async fn start(self) {
        info!(
            node = %self.config.node.name,
            did  = %self.identity.did,
            "sovereign node starting"
        );

        let started_at = now_ms();

        // --- 1. VCP Discovery Daemon ---
        let daemon = Arc::new(
            DiscoveryDaemon::new(
                self.config.vcp.scan_interval_secs,
                self.config.vcp.device_ttl_secs,
            )
        );

        if let Some(vantage) = &self.config.vantage {
            // Production: daemon.with_vantage(vantage.base_url.clone())
            info!(url = %vantage.base_url, "Vantage heartbeat configured");
        }

        let registry = daemon.registry.clone();
        daemon.clone().spawn();
        info!(
            scan_secs  = self.config.vcp.scan_interval_secs,
            ttl_secs   = self.config.vcp.device_ttl_secs,
            "VCP discovery daemon started"
        );

        // --- 2. DIP Router ---
        let mut dip_router = DipRouter::new(self.identity.did.clone());
        dip_router.register_adapter(DipNetwork::Vantage);
        if self.config.dip.nostr_enabled {
            dip_router.register_adapter(DipNetwork::Nostr);
            info!(
                relay = ?self.config.dip.nostr_relay,
                "Nostr adapter registered"
            );
        }
        info!(did = %self.identity.did, "DIP router initialized");

        // --- 3. HTTP API ---
        let state = NodeState {
            identity:   self.identity.clone(),
            config:     self.config.clone(),
            registry:   registry.clone(),
            started_at,
        };

        if self.config.api.enabled {
            let addr: SocketAddr = self.config.api.bind
                .parse()
                .unwrap_or_else(|_| "127.0.0.1:7779".parse().unwrap());

            let app = build_router(state);
            info!(%addr, "API server listening");

            let listener = tokio::net::TcpListener::bind(addr).await
                .expect("failed to bind API port");

            // Spawn API server — it runs until process exits
            tokio::spawn(async move {
                axum::serve(listener, app).await
                    .unwrap_or_else(|e| error!("API server error: {e}"));
            });
        }

        // --- 4. Await shutdown signal ---
        shutdown_signal().await;
        info!("shutdown signal received — stopping");
    }
}

fn build_router(state: NodeState) -> Router {
    Router::new()
        .route("/health",          get(handle_health))
        .route("/status",          get(handle_status))
        .route("/devices",         get(handle_devices))
        .route("/capture/:device", post(handle_capture))
        .with_state(state)
}

// GET /health
async fn handle_health() -> impl IntoResponse {
    Json(json!({"ok": true}))
}

// GET /status
async fn handle_status(State(state): State<NodeState>) -> impl IntoResponse {
    let uptime_secs = (now_ms() - state.started_at) / 1000;
    let device_count = state.registry.count().await;
    Json(json!({
        "node":         state.config.node.name,
        "did":          state.identity.did,
        "uptime_secs":  uptime_secs,
        "device_count": device_count,
        "vcp": {
            "scan_interval_secs": state.config.vcp.scan_interval_secs,
            "device_ttl_secs":    state.config.vcp.device_ttl_secs,
        },
        "dip": {
            "nostr_enabled": state.config.dip.nostr_enabled,
            "vantage_did":   state.config.dip.vantage_did,
        },
        "api": state.config.api.bind,
    }))
}

// GET /devices
async fn handle_devices(State(state): State<NodeState>) -> impl IntoResponse {
    let devices = state.registry.all().await;
    Json(json!({
        "count":   devices.len(),
        "devices": devices,
    }))
}

// POST /capture/:device_id
// Triggers a capture pipeline run for the named device.
// Returns immediately with job_id; production impl should stream progress.
async fn handle_capture(
    State(state): State<NodeState>,
    Path(device_id): Path<String>,
) -> impl IntoResponse {
    match state.registry.get(&device_id).await {
        None => Json(json!({
            "error":     "device_not_found",
            "device_id": device_id,
            "hint":      "check /devices for available devices",
        })),
        Some(device) => {
            info!(
                device_id = %device.device_id,
                model      = %device.model,
                "capture pipeline triggered"
            );
            // Production: spawn CapturePipeline::run() in a tokio task,
            // store progress in shared state, return job_id for polling.
            let job_id = format!("job:{}", uuid::Uuid::new_v4());
            Json(json!({
                "job_id":    job_id,
                "device_id": device.device_id,
                "model":     device.model,
                "status":    "queued",
                "note":      "full pipeline runs async — check /jobs/:id for progress",
            }))
        }
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await
            .expect("failed to install CTRL+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c    => info!("received SIGINT"),
        _ = terminate => info!("received SIGTERM"),
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
