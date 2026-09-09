//! SovereignNode — wires all protocol subsystems together.
//!
//! Subsystems started by SovereignNode::start():
//!   1. VCP DiscoveryDaemon — BLE/mDNS scan loop
//!   2. DIP Router — envelope routing with Vantage + Nostr adapters
//!   3. API server (axum) — /health /status /devices /capture/:id /jobs/:id
//!   4. Graceful shutdown on SIGTERM / SIGINT

use std::sync::Arc;
use std::net::SocketAddr;

use axum::{
    Router,
    routing::{get, post},
    extract::{State, Path},
    Json,
    response::IntoResponse,
    http::StatusCode,
};
use serde_json::json;
use tracing::{info, warn, error};

use vcp::{DiscoveryDaemon, DeviceRegistry, VcpSession};
use vcp::adapters::{Go2Adapter, Go2ConnectionMode};
use vcp::handshake::{VcpCapabilityRequest, VcpDuration};
use vcp::grant::VcpCapabilityGrant;
use dip::{DipRouter, address::DipNetwork};
use sovereign_types::IdentityChain;
use sovereign_pipeline::{CapturePipeline, PipelineConfig, Go2CaptureDriver};

use crate::config::NodeConfig;
use crate::identity::NodeIdentity;
use crate::jobs::{Job, JobStatus, JobStore};
use crate::nostr_relay::{spawn_nostr_relay, NostrRelayHandle};
use crate::vantage::VantageClient;

/// Shared node state visible to all axum handlers.
#[derive(Clone)]
pub struct NodeState {
    pub identity:    Arc<NodeIdentity>,
    pub config:      Arc<NodeConfig>,
    pub registry:    DeviceRegistry,
    pub job_store:   JobStore,
    pub nostr_relay: Option<NostrRelayHandle>,
    pub started_at:  u64,
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
            info!(url = %vantage.base_url, "Vantage heartbeat configured");
        }

        let registry = daemon.registry.clone();
        daemon.clone().spawn();
        info!(
            scan_secs = self.config.vcp.scan_interval_secs,
            ttl_secs  = self.config.vcp.device_ttl_secs,
            "VCP discovery daemon started"
        );

        // --- 1b. Vantage heartbeat loop ---
        if let Some(vantage_cfg) = &self.config.vantage {
            let hb_client  = VantageClient::new(&vantage_cfg.base_url, &vantage_cfg.api_token);
            let hb_registry = registry.clone();
            let hb_name    = self.config.node.name.clone();
            let hb_did     = self.identity.did.clone();
            let hb_interval = self.config.vcp.scan_interval_secs;
            info!(url = %vantage_cfg.base_url, "Vantage heartbeat loop starting");
            tokio::spawn(async move {
                let mut tick = tokio::time::interval(
                    std::time::Duration::from_secs(hb_interval)
                );
                loop {
                    tick.tick().await;
                    let summary = hb_registry.heartbeat_summary().await;
                    hb_client.post_heartbeat(&hb_name, &hb_did, summary).await;
                }
            });
        }

        // --- 2. DIP Router ---
        let mut dip_router = DipRouter::new(self.identity.did.clone());
        dip_router.register_adapter(DipNetwork::Vantage);
        if self.config.dip.nostr_enabled {
            dip_router.register_adapter(DipNetwork::Nostr);
            info!(relay = ?self.config.dip.nostr_relay, "Nostr adapter registered");
        }
        info!(did = %self.identity.did, "DIP router initialized");

        // --- 2b. Nostr relay WebSocket connection ---
        let nostr_relay = if self.config.dip.nostr_enabled {
            if let (Some(relay_url), Some(npub)) = (
                &self.config.dip.nostr_relay,
                &self.config.dip.nostr_npub,
            ) {
                info!(url = %relay_url, npub = %npub, "Nostr relay connecting");
                Some(spawn_nostr_relay(
                    relay_url.clone(),
                    npub.clone(),
                    self.identity.private_key.clone(),
                ))
            } else {
                warn!("nostr_enabled=true but nostr_relay or nostr_npub not configured");
                None
            }
        } else {
            None
        };

        // --- 3. HTTP API ---
        let state = NodeState {
            identity:    self.identity.clone(),
            config:      self.config.clone(),
            registry,
            job_store:   JobStore::new(),
            nostr_relay,
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
        .route("/jobs",            get(handle_jobs_list))
        .route("/jobs/:job_id",    get(handle_job_get))
        .with_state(state)
}

// GET /health
async fn handle_health() -> impl IntoResponse {
    Json(json!({"ok": true}))
}

// GET /status
async fn handle_status(State(state): State<NodeState>) -> impl IntoResponse {
    let uptime_secs  = (now_ms() - state.started_at) / 1000;
    let device_count = state.registry.count().await;
    let jobs         = state.job_store.all().await;
    Json(json!({
        "node":         state.config.node.name,
        "did":          state.identity.did,
        "uptime_secs":  uptime_secs,
        "device_count": device_count,
        "job_count":    jobs.len(),
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

// GET /jobs
async fn handle_jobs_list(State(state): State<NodeState>) -> impl IntoResponse {
    let mut jobs = state.job_store.all().await;
    jobs.sort_by_key(|j| j.created_at);
    Json(json!({
        "count": jobs.len(),
        "jobs":  jobs,
    }))
}

// GET /jobs/:job_id
async fn handle_job_get(
    State(state): State<NodeState>,
    Path(job_id): Path<String>,
) -> impl IntoResponse {
    match state.job_store.get(&job_id).await {
        Some(job) => (StatusCode::OK, Json(serde_json::to_value(job).unwrap_or_default())),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error":  "job_not_found",
                "job_id": job_id,
            })),
        ),
    }
}

// POST /capture/:device_id
// Queues a real async capture pipeline task; returns immediately with job_id.
async fn handle_capture(
    State(state): State<NodeState>,
    Path(device_id): Path<String>,
) -> impl IntoResponse {
    // Verify device is known
    let device = match state.registry.get(&device_id).await {
        None => {
            return Json(json!({
                "error":     "device_not_found",
                "device_id": device_id,
                "hint":      "check /devices for available devices",
            }))
        }
        Some(d) => d,
    };

    let job_id  = format!("job:{}", uuid::Uuid::new_v4());
    let job     = Job::new(job_id.clone(), device_id.clone());
    state.job_store.insert(job).await;

    info!(job_id = %job_id, device_id = %device.device_id, model = %device.model, "capture job queued");

    // Clone what the task needs
    let identity   = state.identity.clone();
    let config     = state.config.clone();
    let job_store  = state.job_store.clone();
    let task_job   = job_id.clone();

    tokio::spawn(async move {
        job_store.update_status(&task_job, JobStatus::Running).await;

        let result = tokio::task::spawn_blocking(move || {
            run_capture_pipeline(&identity, &config, &device.device_id, &device.model)
        }).await;

        let status = match result {
            Ok(Ok(output)) => {
                info!(
                    job_id    = %task_job,
                    twin_id   = %output.0,
                    scene_id  = %output.1,
                    "capture pipeline completed"
                );
                JobStatus::Completed {
                    twin_id:            output.0,
                    scene_receipt_id:   output.1,
                    capture_receipt_id: output.2,
                }
            }
            Ok(Err(e)) => {
                warn!(job_id = %task_job, error = %e, "capture pipeline failed");
                JobStatus::Failed { reason: e }
            }
            Err(e) => {
                error!(job_id = %task_job, error = %e, "capture task panicked");
                JobStatus::Failed { reason: format!("task panicked: {e}") }
            }
        };

        job_store.update_status(&task_job, status).await;
    });

    Json(json!({
        "job_id":    job_id,
        "device_id": device_id,
        "status":    "queued",
        "poll":      format!("/jobs/{job_id}"),
    }))
}

/// Blocking capture pipeline run — called via spawn_blocking.
/// Returns (twin_id, scene_receipt_id, capture_receipt_id) on success.
fn run_capture_pipeline(
    identity:  &NodeIdentity,
    config:    &NodeConfig,
    device_id: &str,
    model:     &str,
) -> Result<(String, String, String), String> {
    use vcp::manifest::{VcpCapabilityDecl, VcpSafetyConfig, VcpTransport, VcpDeviceIdentity, AgentDeviceManifest};
    use sovereign_types::SafetyLevel;

    // Build a minimal manifest so we can issue a stub grant.
    // Production: this comes from the real VCP handshake (device signs grant).
    let manifest = if model.to_lowercase().contains("go2") {
        let adapter = Go2Adapter::new(device_id, Go2ConnectionMode::default());
        adapter.manifest(&identity.public_key)
    } else {
        // Generic fallback for non-Go2 devices
        AgentDeviceManifest {
            device_id:        device_id.into(),
            manufacturer:     "Unknown".into(),
            model:            model.into(),
            protocol_version: "vcp/1".into(),
            firmware_version: "1.0".into(),
            dip_identity:     format!("did:device:{device_id}"),
            capabilities: vec![
                VcpCapabilityDecl {
                    id: "camera".into(), description: "camera capture".into(),
                    params: None, requires_grant: true,
                    safety_level: SafetyLevel::None, ungrantable: false,
                },
                VcpCapabilityDecl {
                    id: "lidar".into(), description: "lidar capture".into(),
                    params: None, requires_grant: true,
                    safety_level: SafetyLevel::None, ungrantable: false,
                },
                VcpCapabilityDecl {
                    id: "telemetry".into(), description: "telemetry polling".into(),
                    params: None, requires_grant: true,
                    safety_level: SafetyLevel::None, ungrantable: false,
                },
            ],
            safety: VcpSafetyConfig {
                emergency_stop: true,
                geofence: false,
                collision_avoidance: None,
                max_speed_ms: None,
                ungrantable: vec![],
            },
            transport: vec![VcpTransport::Wifi],
            identity: VcpDeviceIdentity {
                public_key: identity.public_key.clone(),
                cert_chain: None,
            },
            timestamp: 0, merkle_root: String::new(), signature: String::new(),
        }
    };

    let chain = IdentityChain::new(identity.did.clone(), identity.did.clone());

    let request = VcpCapabilityRequest::new(
        chain.clone(),
        vec!["camera".into(), "lidar".into(), "telemetry".into()],
        "twin_capture",
        VcpDuration::minutes(60),
        &identity.private_key,
    ).map_err(|e| e.to_string())?;

    // Node signs as both agent and device (stub — real: device signs during handshake)
    let grant = VcpCapabilityGrant::issue(&manifest, &request, &identity.private_key)
        .map_err(|e| e.to_string())?;

    let session = VcpSession::new(grant);

    let pipeline_cfg = PipelineConfig {
        owner_did:             identity.did.clone(),
        reconstruction_engine: config.pipeline.reconstruction_engine.clone(),
        ..Default::default()
    };

    let pipeline = CapturePipeline::new(pipeline_cfg, &identity.private_key, chain);
    let output   = pipeline.run(session, &Go2CaptureDriver).map_err(|e| e.to_string())?;

    Ok((
        output.twin.twin_id,
        output.scene_receipt.receipt_id,
        output.capture_receipt.receipt_id,
    ))
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
