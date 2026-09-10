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
use tokio::sync::broadcast;
use tracing::{info, warn, error};

use crate::events::TwinEvent;
use crate::delegation::{DelegateRequest, delegate_capture};

use vcp::{DiscoveryDaemon, DeviceRegistry, VcpSession};
use vcp::adapters::{Go2Adapter, Go2ConnectionMode};
use vcp::handshake::{VcpCapabilityRequest, VcpDuration};
use vcp::grant::VcpCapabilityGrant;
use dip;
use sovereign_types::IdentityChain;
use sovereign_pipeline::{
    CapturePipeline, PipelineConfig, Go2CaptureDriver, PipelineOutput,
    ProofChain, ProofChainConfig,
};
use twin_protocol::osovm::{OsovmEngine, ProofOfSimulation, SimScenario};
use twin_protocol::ase::{AseMintRequest, mint_ase};
use crate::tile_economy_store::TileEconomyStore;
use sovereign_types::WitnessAttestation;
use sovereign_runtime::chain::{
    Principal, CapabilityAction, ExecutionEngine, ActionOutcome,
};
use sovereign_runtime::evidence::{Evidence, EvidenceBundle, EvidenceKind};
use ip_layer::{NostrSecretKey, IpRootBuilder, seal_gaussian_splat, sha256_hex};

use sovereign_os::{ActReceiptChain, AgentActReceipt, EpistemicSeverity as AgentEpistemic};

use crate::body_store::BodyStore;
use crate::telemetry_store::TelemetryStore;
use crate::config::NodeConfig;
use crate::dip_gateway::DipGateway;
use crate::federation::discover_sovereign_nodes;
use crate::identity::NodeIdentity;
use crate::jobs::{Job, JobStatus, JobStore};
use crate::nostr_relay::{spawn_nostr_relay, NostrRelayHandle};
use crate::proof_engine::ProofEngine;
use crate::receipt_merkle::build_receipt_tree;
use crate::receipt_store::{ReceiptRecord, ReceiptStore};
use crate::swarm::{SwarmJob, SwarmRequest, SwarmStatus, SwarmStore, monitor_swarm};
use crate::timeline_store::TimelineStore;
use crate::vantage::VantageClient;
use crate::witness_registry::WitnessRegistry;

/// Shared node state visible to all axum handlers.
#[derive(Clone)]
pub struct NodeState {
    pub identity:        Arc<NodeIdentity>,
    pub config:          Arc<NodeConfig>,
    pub registry:        DeviceRegistry,
    pub job_store:       JobStore,
    pub receipt_store:   ReceiptStore,
    pub witnesses:       WitnessRegistry,
    pub dip_gateway:     Arc<DipGateway>,
    pub inbound_ctx:     InboundDipContext,
    pub nostr_relay:     Option<NostrRelayHandle>,
    pub a2a:             sovereign_a2a::A2aState,
    pub swarm_store:     SwarmStore,
    pub timeline_store:  TimelineStore,
    /// Broadcast channel: all subscribers receive real-time TwinEvents.
    /// Capacity 256 — slow subscribers lag and get RecvError::Lagged.
    pub twin_events:          broadcast::Sender<TwinEvent>,
    pub tile_economy_store:   TileEconomyStore,
    pub started_at:           u64,
    /// Cached IP Root event (kind 31900) built at startup from nostr_nsec.
    /// None if nsec is not configured (offline mode).
    pub ip_root_event:        Option<Arc<ip_layer::nostr::NostrEvent>>,
    /// Append-only chain of agent-level ActReceipts (Layer 3 provenance).
    pub act_chain:            Arc<tokio::sync::RwLock<ActReceiptChain>>,
    /// Body session store — active VCP body sessions and flight receipts.
    pub body_store:           BodyStore,
    /// Inbound telemetry frames per body session.
    pub telemetry_store:      TelemetryStore,
    /// Proof-of-Evolution engine — evaluates SimulationProofs.
    pub proof_engine:         Arc<ProofEngine>,
}

impl axum::extract::FromRef<NodeState> for sovereign_a2a::A2aState {
    fn from_ref(state: &NodeState) -> Self {
        state.a2a.clone()
    }
}

pub struct SovereignNode {
    pub identity: Arc<NodeIdentity>,
    pub config:   Arc<NodeConfig>,
}

/// Context passed to the inbound DIP envelope dispatcher.
#[derive(Clone)]
pub struct InboundDipContext {
    pub local_did: String,
    pub identity:  Arc<NodeIdentity>,
    pub witnesses: WitnessRegistry,
    pub gateway:   Arc<DipGateway>,
}

impl InboundDipContext {
    fn from_state(state: &NodeState) -> Self {
        Self {
            local_did: state.identity.did.clone(),
            identity:  state.identity.clone(),
            witnesses: state.witnesses.clone(),
            gateway:   state.dip_gateway.clone(),
        }
    }
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

        // --- 2. Unified inbound DIP channel (Nostr + Meshtastic both push here) ---
        let (dip_inbound_tx, mut dip_inbound_rx) =
            tokio::sync::mpsc::channel::<dip::DipEnvelope>(128);

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
                    Some(dip_inbound_tx.clone()),
                ))
            } else {
                warn!("nostr_enabled=true but nostr_relay or nostr_npub not configured");
                None
            }
        } else {
            None
        };

        // --- 3. DIP Gateway ---
        let vantage_client = self.config.vantage.as_ref()
            .map(|v| VantageClient::new(&v.base_url, &v.api_token));

        let dip_gateway = Arc::new(DipGateway::new(
            self.identity.did.clone(),
            vantage_client,
            nostr_relay.clone(),
            &self.identity,
            self.config.meshtastic.as_ref(),
        ));

        // --- 3b. HTTP API ---
        let receipt_store = ReceiptStore::open(&self.config.node.data_dir).await;

        // Load witnesses from config
        let witnesses = WitnessRegistry::new();
        for w in &self.config.witnesses {
            witnesses.register(crate::witness_registry::WitnessPeer {
                did:         w.did.clone(),
                public_key:  w.public_key.clone(),
                private_key: None, // remote witness — signs via DIP exchange in production
            }).await;
        }
        if self.config.witnesses.is_empty() {
            warn!("no witnesses configured — stub witnesses will be used for proof chain");
            info!("add [[witnesses]] entries to config.toml to register real witnesses");
        } else {
            info!(count = self.config.witnesses.len(), "witnesses loaded from config");
        }

        // Build inbound context before state so it can be cloned into the dispatch task
        let inbound_ctx = InboundDipContext {
            local_did: self.identity.did.clone(),
            identity:  self.identity.clone(),
            witnesses: witnesses.clone(),
            gateway:   dip_gateway.clone(),
        };

        // Load manual devices from config (USB-connected, pre-configured)
        for manifest_path in &self.config.vcp.manual_devices {
            match std::fs::read_to_string(manifest_path) {
                Ok(text) => match serde_json::from_str::<vcp::manifest::AgentDeviceManifest>(&text) {
                    Ok(manifest) => {
                        info!(path = %manifest_path, device_id = %manifest.device_id, "manual device loaded");
                        registry.upsert(
                            vcp::discovery::DiscoveredDevice::from_manifest(
                                &manifest, None,
                                vcp::discovery::DiscoveryMethod::Manual,
                            )
                        ).await;
                    }
                    Err(e) => warn!(path = %manifest_path, error = %e, "invalid device manifest JSON"),
                },
                Err(e) => warn!(path = %manifest_path, error = %e, "cannot read device manifest"),
            }
        }

        let a2a_cfg = sovereign_a2a::A2aConfig {
            name:        self.config.node.name.clone(),
            base_url:    format!("http://{}", self.config.api.bind),
            description: "Sovereign Node — physical twin capture and provenance".into(),
            version:     env!("CARGO_PKG_VERSION").into(),
            skills:      sovereign_a2a::A2aConfig::default().skills,
            provider:    None,
        };

        // A2A → capture pipeline dispatch channel
        let (a2a_dispatch_tx, mut a2a_dispatch_rx) =
            tokio::sync::mpsc::channel::<sovereign_a2a::A2aDispatchRequest>(32);
        let a2a_state = sovereign_a2a::A2aState::new(a2a_cfg)
            .with_dispatch(a2a_dispatch_tx);

        let (twin_events_tx, _twin_events_rx) = broadcast::channel::<TwinEvent>(256);
        let timeline_store = TimelineStore::open(&self.config.node.data_dir).await;

        // Build the IP Root event at startup (once) if nostr_nsec is configured.
        // User publishes it manually at agent birth; we cache it for /ip/root.
        let ip_root_event = self.config.dip.nostr_nsec.as_deref().and_then(|nsec| {
            match NostrSecretKey::from_hex(nsec) {
                Ok(key) => {
                    let pubkey = key.pubkey_hex();
                    let now = ip_layer::now_secs();
                    match IpRootBuilder::for_agent(&pubkey)
                        .with_display_name(&self.config.node.name)
                        .sign(&key, now)
                    {
                        Ok(ev) => {
                            info!(ip_root_id = %pubkey, "IP Root event built and cached");
                            Some(Arc::new(ev))
                        }
                        Err(e) => {
                            warn!(error = %e, "failed to build IP Root event");
                            None
                        }
                    }
                }
                Err(e) => {
                    warn!(error = %e, "invalid nostr_nsec — IP Root event not built");
                    None
                }
            }
        });

        let state = NodeState {
            identity:    self.identity.clone(),
            config:      self.config.clone(),
            registry,
            job_store:   JobStore::new(),
            receipt_store,
            witnesses,
            dip_gateway,
            inbound_ctx,
            nostr_relay,
            a2a:         a2a_state,
            swarm_store:         SwarmStore::new(),
            timeline_store,
            twin_events:         twin_events_tx,
            tile_economy_store:  TileEconomyStore::new(),
            started_at,
            ip_root_event,
            act_chain:        Arc::new(tokio::sync::RwLock::new(ActReceiptChain::new())),
            body_store:       BodyStore::new(),
            telemetry_store:  TelemetryStore::new(),
            proof_engine:     Arc::new(ProofEngine::new()),
        };

        // --- 3a. A2A dispatch loop (routes skill requests → capture jobs) ---
        {
            let dispatch_state = state.clone();
            tokio::spawn(async move {
                while let Some(req) = a2a_dispatch_rx.recv().await {
                    let s = dispatch_state.clone();
                    let task_id = req.task_id.clone();
                    tokio::spawn(async move {
                        handle_a2a_dispatch(req, s).await;
                    });
                    tracing::debug!(task_id = %task_id, "A2A dispatch forwarded to pipeline");
                }
            });
        }

        // --- 3b. Timeline appender (subscribes to TwinEvents → appends 4D timeline entries) ---
        {
            use twin_protocol::{TwinTimeline, entry_from_capture};
            let tl_store = state.timeline_store.clone();
            let mut tl_rx = state.twin_events.subscribe();
            tokio::spawn(async move {
                loop {
                    match tl_rx.recv().await {
                        Ok(crate::events::TwinEvent::CaptureComplete {
                            twin_id, device_id, receipt_id, ..
                        }) => {
                            let entry = entry_from_capture(
                                &twin_id, &receipt_id, &device_id,
                                now_ms(), None, None, 0.85, true, true,
                            );
                            let tid = TwinTimeline::device_id(&device_id);
                            tl_store.append(&tid, entry).await;
                        }
                        Ok(_) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            });
        }

        // --- 3c. Active Perception Loop (device health monitoring + StatusUpdate events) ---
        {
            let perc_registry  = state.registry.clone();
            let perc_events    = state.twin_events.clone();
            let perc_interval  = self.config.vcp.scan_interval_secs;
            let perc_stale     = self.config.vcp.device_ttl_secs;
            let perc_client    = Arc::new(reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(3))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()));
            crate::perception::spawn_perception_loop(
                perc_registry, perc_events, perc_interval, perc_stale, perc_client,
            );
        }

        // --- 3c. Meshtastic inbound poll (feeds dip_inbound_tx) ---
        if let Some(mesh_cfg) = &self.config.meshtastic {
            info!(url = %mesh_cfg.device_url, "starting Meshtastic inbound poll");
            state.dip_gateway.spawn_mesh_inbound(&mesh_cfg.device_url, dip_inbound_tx.clone());
        }

        // --- 3c. Vantage outbound DIP poll (NAT traversal) ---
        if let Some(vantage_cfg) = &self.config.vantage {
            let poll_secs = self.config.dip.poll_interval_secs;
            if poll_secs > 0 {
                let vc   = VantageClient::new(&vantage_cfg.base_url, &vantage_cfg.api_token);
                let did  = self.identity.did.clone();
                let tx   = dip_inbound_tx.clone();
                tokio::spawn(async move {
                    let interval = std::time::Duration::from_secs(poll_secs);
                    loop {
                        tokio::time::sleep(interval).await;
                        for envelope in vc.poll_dip_outbound(&did).await {
                            if tx.send(envelope).await.is_err() { break; }
                        }
                    }
                });
                info!(interval_secs = poll_secs, "Vantage DIP outbound poll started");
            }
        }

        // --- 3d. Inbound DIP dispatch (Nostr + Meshtastic + Vantage → single handler) ---
        {
            let ctx = state.inbound_ctx.clone();
            tokio::spawn(async move {
                while let Some(envelope) = dip_inbound_rx.recv().await {
                    handle_inbound_dip(envelope, &ctx).await;
                }
            });
        }

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

/// Build the axum Router bound to `state`. Public for integration tests.
pub fn build_router(state: NodeState) -> Router {
    use crate::mcp_server::handle_mcp;
    use crate::ws::{handle_ws_twin, handle_ws_splat};

    Router::new()
        .route("/health",                   get(handle_health))
        .route("/status",                   get(handle_status))
        .route("/devices",                  get(handle_devices))
        .route("/devices/register",         post(handle_device_register))
        .route("/capture/:device",          post(handle_capture))
        .route("/capture/delegate",         post(handle_capture_delegate))
        .route("/jobs",                     get(handle_jobs_list))
        .route("/jobs/:job_id",             get(handle_job_get))
        .route("/mcp",                      post(handle_mcp))
        .route("/receipts",                 get(handle_receipts))
        .route("/receipts/export",          get(handle_receipt_export))
        .route("/receipts/:twin_id",        get(handle_receipt_get))
        .route("/receipts/root",            get(handle_receipt_merkle_root))
        .route("/receipts/verify/:id",      get(handle_receipt_verify))
        .route("/tiles/:tile_id/receipts",  get(handle_tile_receipts))
        .route("/tiles/:tile_id/economy",   get(handle_tile_economy))
        .route("/tiles/:tile_id/claim",     post(handle_tile_claim))
        .route("/tiles",                    get(handle_tiles_list))
        .route("/capture/swarm",            post(handle_capture_swarm))
        .route("/swarm",                    get(handle_swarm_list))
        .route("/swarm/:swarm_id",          get(handle_swarm_get))
        .route("/twins/:twin_id/timeline",  get(handle_twin_timeline))
        .route("/timelines",                get(handle_timelines_list))
        .route("/events/receipts",          get(handle_sse_receipts))
        .route("/events/jobs",              get(handle_sse_jobs))
        .route("/dip/inbound",              post(handle_dip_inbound))
        .route("/dip/gossip",               post(handle_dip_gossip))
        .route("/jobs/:job_id/retry",       post(handle_job_retry))
        .route("/config/check",             get(handle_config_check))
        .route("/ws/twin/:twin_id",         get(handle_ws_twin))
        .route("/ws/splat/:twin_id",        get(handle_ws_splat))
        .route("/federation/peers",         get(handle_federation_peers))
        .route("/ip/root",                  get(handle_ip_root))
        .route("/ip/receipt/:twin_id",      get(handle_ip_receipt))
        .route("/agent/receipts",           get(handle_agent_receipts))
        // ── Proof-of-Evolution ─────────────────────────────────────────────
        .route("/proofs/simulation",        post(handle_proof_simulation_submit))
        .route("/proofs/simulation/:id",    get(handle_proof_simulation_get))
        .route("/proofs/gaussian",          post(handle_proof_gaussian_submit))
        .route("/proofs/physical",          post(handle_proof_physical_submit))
        // ── Body sessions (VCP physical embodiment) ────────────────────────
        .route("/body/sessions",            get(handle_body_sessions_list))
        .route("/body/sessions",            post(handle_body_session_open))
        .route("/body/sessions/:id",        get(handle_body_session_get))
        .route("/body/:body_id/receipts",   get(handle_body_receipts))
        .route("/body/capabilities",        get(handle_body_capabilities))
        .route("/body/sessions/:id/telemetry", post(handle_body_telemetry_push))
        .route("/body/sessions/:id/close",     post(handle_body_session_close))
        .nest("/a2a",                       sovereign_a2a::a2a_router::<NodeState>())
        .with_state(state)
}

// GET /health
async fn handle_health() -> impl IntoResponse {
    Json(json!({"ok": true}))
}

// GET /status
async fn handle_status(State(state): State<NodeState>) -> impl IntoResponse {
    let uptime_secs    = (now_ms() - state.started_at) / 1000;
    let device_count   = state.registry.count().await;
    let jobs           = state.job_store.all().await;
    let receipt_count  = state.receipt_store.count().await;
    Json(json!({
        "node":          state.config.node.name,
        "did":           state.identity.did,
        "uptime_secs":   uptime_secs,
        "device_count":  device_count,
        "job_count":     jobs.len(),
        "receipt_count": receipt_count,
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

// POST /jobs/:job_id/retry — re-queue a failed job for another attempt.
async fn handle_job_retry(
    State(state): State<NodeState>,
    Path(job_id): Path<String>,
) -> impl IntoResponse {
    let job = match state.job_store.get(&job_id).await {
        Some(j) => j,
        None => return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "job_not_found", "job_id": job_id })),
        ).into_response(),
    };

    let reason = match &job.status {
        JobStatus::Failed { reason } => reason.clone(),
        _ => return (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "job_not_failed",
                "job_id": job_id,
                "current_status": format!("{:?}", job.status),
            })),
        ).into_response(),
    };

    let device_id = job.device_id.clone();
    // Find the device model from registry (fall back to unknown).
    let model = state.registry.get(&device_id).await
        .map(|d| d.model)
        .unwrap_or_else(|| "unknown".into());

    let new_job_id = format!("job:{}", uuid::Uuid::new_v4());
    let new_job = Job::new(new_job_id.clone(), device_id.clone());
    state.job_store.insert(new_job).await;

    info!(
        original_job = %job_id,
        new_job      = %new_job_id,
        device_id    = %device_id,
        reason       = %reason,
        "retrying failed job"
    );

    let identity    = state.identity.clone();
    let config      = state.config.clone();
    let job_store   = state.job_store.clone();
    let receipts    = state.receipt_store.clone();
    let witnesses   = state.witnesses.clone();
    let dip_gateway = state.dip_gateway.clone();
    let events_tx   = state.twin_events.clone();
    let tile_econ   = state.tile_economy_store.clone();

    tokio::spawn(run_capture_job(
        new_job_id.clone(), device_id, model,
        identity, config, job_store, receipts, witnesses, dip_gateway, events_tx, tile_econ,
        state.act_chain.clone(),
    ));

    (StatusCode::ACCEPTED, Json(json!({
        "ok":            true,
        "new_job_id":    new_job_id,
        "original_job":  job_id,
    }))).into_response()
}

// GET /agent/receipts — full ActReceiptChain as JSON.
async fn handle_agent_receipts(State(state): State<NodeState>) -> impl IntoResponse {
    let chain = state.act_chain.read().await;
    Json(serde_json::json!({
        "count":    chain.len(),
        "verified": chain.verify_chain(),
        "receipts": chain.all(),
    }))
}

// ── Proof-of-Evolution handlers ───────────────────────────────────────────────

/// POST /proofs/simulation  — submit a SimulationProof for evaluation.
async fn handle_proof_simulation_submit(
    State(state): State<NodeState>,
    Json(proof): Json<sovereign_types::SimulationProof>,
) -> impl IntoResponse {
    match state.proof_engine.evaluate_simulation(&proof).await {
        Ok(eval) => {
            info!(
                proof_id   = %eval.proof_id,
                mint_eligible = eval.mint_eligible,
                proof_value   = %format!("{:.3}", eval.proof_value),
                "SimulationProof evaluated"
            );
            (StatusCode::OK, Json(serde_json::to_value(&eval).unwrap()))
        }
        Err(e) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({"error": e})),
        ),
    }
}

/// GET /proofs/simulation/:id — placeholder (proofs are not persisted in MVP).
async fn handle_proof_simulation_get(
    Path(id): Path<String>,
) -> impl IntoResponse {
    (StatusCode::NOT_FOUND, Json(json!({"error": format!("proof not found: {id}"), "note": "use POST /proofs/simulation to submit"})))
}

// ── Body session handlers ─────────────────────────────────────────────────────

/// GET /body/sessions
async fn handle_body_sessions_list(State(state): State<NodeState>) -> impl IntoResponse {
    let sessions = state.body_store.all_sessions().await;
    Json(json!({ "count": sessions.len(), "sessions": sessions }))
}

/// POST /body/sessions  — open a new body session.
/// Body: { agent_id, agent_tier, body_id, mode, capabilities?, sim_proof_id? }
async fn handle_body_session_open(
    State(state): State<NodeState>,
    Json(req): Json<serde_json::Value>,
) -> impl IntoResponse {
    let agent_id = match req.get("agent_id").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => return (StatusCode::BAD_REQUEST, Json(json!({"error": "missing agent_id"}))),
    };
    let body_id = match req.get("body_id").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => return (StatusCode::BAD_REQUEST, Json(json!({"error": "missing body_id"}))),
    };
    // Default to T0 if not supplied — caller must specify tier explicitly.
    let agent_tier: sovereign_types::TrustTier = req.get("agent_tier")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or(sovereign_types::TrustTier::T0);

    let mode: vcp::BodySessionMode = req.get("mode")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or(vcp::BodySessionMode::HumanSupervised);

    let capabilities: Vec<String> = req.get("capabilities")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    let sim_proof_id = req.get("sim_proof_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    match vcp::BodySession::new(agent_id, agent_tier, body_id, mode, capabilities, sim_proof_id) {
        Ok(session) => {
            info!(session_id = %session.session_id, "body session opened");
            let val = serde_json::to_value(&session).unwrap();
            state.body_store.insert_session(session).await;
            (StatusCode::CREATED, Json(val))
        }
        Err(e) => (StatusCode::FORBIDDEN, Json(json!({"error": e}))),
    }
}

/// GET /body/sessions/:id
async fn handle_body_session_get(
    State(state): State<NodeState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.body_store.get_session(&id).await {
        Some(s) => (StatusCode::OK, Json(serde_json::to_value(&s).unwrap())),
        None    => (StatusCode::NOT_FOUND, Json(json!({"error": format!("body session not found: {id}")}))),
    }
}

/// GET /body/:body_id/receipts
async fn handle_body_receipts(
    State(state): State<NodeState>,
    Path(body_id): Path<String>,
) -> impl IntoResponse {
    let receipts = state.body_store.receipts_for_body(&body_id).await;
    Json(json!({ "body_id": body_id, "count": receipts.len(), "receipts": receipts }))
}

/// GET /body/capabilities  — return the StampFly capability catalogue.
async fn handle_body_capabilities() -> impl IntoResponse {
    let caps = vcp::stampfly_capabilities();
    Json(json!({ "body": "stampfly_v1_1", "capabilities": caps }))
}

/// POST /body/sessions/:id/telemetry  — ingest a telemetry frame.
async fn handle_body_telemetry_push(
    State(state): State<NodeState>,
    Path(session_id): Path<String>,
    Json(frame): Json<vcp::FlightTelemetry>,
) -> impl IntoResponse {
    state.telemetry_store.push(&session_id, frame).await;
    (StatusCode::OK, Json(json!({ "ok": true, "session_id": session_id })))
}

/// POST /body/sessions/:id/close  — close session, generate FlightReceipt.
/// Body: { mission_success: bool, witness_ids?: [..] }
async fn handle_body_session_close(
    State(state): State<NodeState>,
    Path(session_id): Path<String>,
    Json(req): Json<serde_json::Value>,
) -> impl IntoResponse {
    let session = match state.body_store.get_session(&session_id).await {
        Some(s) => s,
        None => return (
            StatusCode::NOT_FOUND,
            Json(json!({"error": format!("session not found: {session_id}")})),
        ),
    };
    let mission_success = req.get("mission_success")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let witness_ids: Vec<String> = req.get("witness_ids")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    let receipt = state.telemetry_store.close_session(
        &session_id,
        &session.agent_id,
        &session.body_id,
        session.sim_proof_id.clone(),
        witness_ids,
        mission_success,
    ).await;

    info!(
        receipt_id = %receipt.receipt_id,
        session_id = %session_id,
        frames     = receipt.telemetry_count,
        success    = mission_success,
        "flight session closed"
    );

    let val = serde_json::to_value(&receipt).unwrap();
    state.body_store.add_receipt(receipt).await;
    (StatusCode::OK, Json(val))
}

/// POST /proofs/gaussian  — submit a GaussianProof for Spatial-domain evaluation.
async fn handle_proof_gaussian_submit(
    State(state): State<NodeState>,
    Json(proof): Json<sovereign_types::GaussianProof>,
) -> impl IntoResponse {
    match state.proof_engine.evaluate_gaussian(&proof).await {
        Ok(eval) => {
            info!(
                proof_id = %eval.proof_id,
                mint_eligible = eval.mint_eligible,
                proof_value   = %format!("{:.3}", eval.proof_value),
                "GaussianProof evaluated"
            );
            (StatusCode::OK, Json(serde_json::to_value(&eval).unwrap()))
        }
        Err(e) => (StatusCode::UNPROCESSABLE_ENTITY, Json(json!({"error": e}))),
    }
}

/// POST /proofs/physical  — submit a RealityTransferScore for Physical-domain evaluation.
async fn handle_proof_physical_submit(
    State(state): State<NodeState>,
    Json(rts): Json<sovereign_types::RealityTransferScore>,
) -> impl IntoResponse {
    match state.proof_engine.evaluate_physical(&rts).await {
        Ok(eval) => {
            info!(
                proof_id = %eval.proof_id,
                mint_eligible = eval.mint_eligible,
                "PhysicalProof evaluated"
            );
            (StatusCode::OK, Json(serde_json::to_value(&eval).unwrap()))
        }
        Err(e) => (StatusCode::UNPROCESSABLE_ENTITY, Json(json!({"error": e}))),
    }
}

// GET /receipts
async fn handle_receipts(State(state): State<NodeState>) -> impl IntoResponse {
    let records = state.receipt_store.list().await;
    Json(json!({
        "count":    records.len(),
        "receipts": records,
    }))
}

// GET /receipts/:twin_id
async fn handle_receipt_get(
    State(state): State<NodeState>,
    Path(twin_id): Path<String>,
) -> impl IntoResponse {
    match state.receipt_store.get_by_twin(&twin_id).await {
        Some(record) => (
            StatusCode::OK,
            Json(serde_json::to_value(record).unwrap_or_default()),
        ),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "receipt_not_found", "twin_id": twin_id })),
        ),
    }
}

// GET /receipts/root — Merkle root over all receipt_ids.
async fn handle_receipt_merkle_root(State(state): State<NodeState>) -> impl IntoResponse {
    let records = state.receipt_store.list().await;
    let root = build_receipt_tree(&records);
    Json(serde_json::to_value(root).unwrap_or_default())
}

// GET /receipts/verify/:receipt_id — verify a receipt_id is in the current Merkle root.
async fn handle_receipt_verify(
    State(state): State<NodeState>,
    Path(receipt_id): Path<String>,
) -> impl IntoResponse {
    use crate::receipt_merkle::verify_receipt_in_root;
    let records = state.receipt_store.list().await;
    let root    = build_receipt_tree(&records);
    let present = verify_receipt_in_root(&receipt_id, &records, &root.root);
    if present {
        (StatusCode::OK, Json(json!({
            "verified":    true,
            "receipt_id":  receipt_id,
            "merkle_root": root.root,
        }))).into_response()
    } else {
        (StatusCode::NOT_FOUND, Json(json!({
            "verified":    false,
            "receipt_id":  receipt_id,
            "merkle_root": root.root,
        }))).into_response()
    }
}

// GET /receipts/export — returns all receipts as JSONL (one JSON object per line).
async fn handle_receipt_export(State(state): State<NodeState>) -> impl IntoResponse {
    use axum::http::header;
    let records = state.receipt_store.list().await;
    let body = records
        .iter()
        .filter_map(|r| serde_json::to_string(r).ok())
        .collect::<Vec<_>>()
        .join("\n");
    (
        [(header::CONTENT_TYPE, "application/x-ndjson")],
        body,
    )
        .into_response()
}

// GET /tiles/:tile_id/economy — return the Àṣẹ tile economy state for a specific tile.
async fn handle_tile_economy(
    State(state): State<NodeState>,
    Path(tile_id): Path<String>,
) -> impl IntoResponse {
    match state.tile_economy_store.get(&tile_id).await {
        Some(economy) => (StatusCode::OK, Json(serde_json::to_value(&economy).unwrap_or_default())).into_response(),
        None => (StatusCode::NOT_FOUND, Json(json!({ "error": "tile_economy_not_found", "tile_id": tile_id }))).into_response(),
    }
}

// POST /tiles/:tile_id/claim — claim ownership of an Odù tile.
//
// Body: { "owner_did": "did:vantage:..." }
// Stub: records ownership in TileEconomy. Production: calls Sui Move `ase::claim_tile`.
#[derive(serde::Deserialize)]
struct TileClaimRequest {
    owner_did: String,
}

async fn handle_tile_claim(
    State(state): State<NodeState>,
    Path(tile_id): Path<String>,
    Json(req):    Json<TileClaimRequest>,
) -> impl IntoResponse {
    // Validate tile_id format
    if !tile_id.starts_with("odu:") || tile_id.len() != 6 {
        return (StatusCode::BAD_REQUEST, Json(json!({
            "error": "invalid_tile_id",
            "hint":  "tile_id must be 'odu:XY' where X and Y are hex nibbles",
        }))).into_response();
    }

    // Get or create economy entry, then set owner
    state.tile_economy_store.claim(&tile_id, &req.owner_did).await;

    info!(tile_id = %tile_id, owner = %req.owner_did, "tile ownership claimed (stub)");

    (StatusCode::OK, Json(json!({
        "ok":        true,
        "tile_id":   tile_id,
        "owner_did": req.owner_did,
        "stub":      true,
    }))).into_response()
}

// POST /capture/swarm — trigger simultaneous captures across multiple devices.
async fn handle_capture_swarm(
    State(state): State<NodeState>,
    Json(req):    Json<SwarmRequest>,
) -> impl IntoResponse {
    if req.device_ids.is_empty() {
        return (StatusCode::BAD_REQUEST,
            Json(json!({ "error": "no device_ids provided" }))).into_response();
    }

    let swarm_id  = format!("swarm:{}", uuid::Uuid::new_v4());
    let mut child_jobs = Vec::with_capacity(req.device_ids.len());

    for device_id in &req.device_ids {
        let job_id = format!("job:{}", uuid::Uuid::new_v4());
        let job = Job::new(job_id.clone(), device_id.clone());
        state.job_store.insert(job).await;
        child_jobs.push(job_id.clone());

        let device = state.registry.get(device_id).await;
        let model  = device.map(|d| d.model.clone()).unwrap_or_else(|| "Go2".into());
        tokio::spawn(run_capture_job(
            job_id, device_id.clone(), model,
            state.identity.clone(), state.config.clone(),
            state.job_store.clone(), state.receipt_store.clone(),
            state.witnesses.clone(), state.dip_gateway.clone(),
            state.twin_events.clone(),
            state.tile_economy_store.clone(),
            state.act_chain.clone(),
        ));
    }

    let swarm_job = SwarmJob {
        swarm_id:   swarm_id.clone(),
        device_ids: req.device_ids.clone(),
        child_jobs:  child_jobs.clone(),
        status:     SwarmStatus::Pending,
        created_at: now_ms(),
        updated_at: now_ms(),
    };
    state.swarm_store.insert(swarm_job).await;

    // Background monitor
    tokio::spawn(monitor_swarm(
        swarm_id.clone(), req.device_ids.clone(), child_jobs.clone(),
        state.job_store.clone(), state.swarm_store.clone(), state.twin_events.clone(),
    ));

    info!(swarm_id = %swarm_id, devices = req.device_ids.len(), "swarm capture started");
    (StatusCode::ACCEPTED, Json(json!({
        "swarm_id":   swarm_id,
        "device_ids": req.device_ids,
        "child_jobs": child_jobs,
        "status":     "pending",
        "poll":       format!("/swarm/{swarm_id}"),
    }))).into_response()
}

// GET /swarm — list all swarms.
async fn handle_swarm_list(State(state): State<NodeState>) -> impl IntoResponse {
    let swarms = state.swarm_store.all().await;
    Json(json!({ "count": swarms.len(), "swarms": swarms }))
}

// GET /swarm/:swarm_id — poll swarm status.
async fn handle_swarm_get(
    State(state):      State<NodeState>,
    Path(swarm_id):    Path<String>,
) -> impl IntoResponse {
    match state.swarm_store.get(&swarm_id).await {
        Some(s) => (StatusCode::OK,   Json(serde_json::to_value(s).unwrap_or_default())).into_response(),
        None    => (StatusCode::NOT_FOUND,
            Json(json!({ "error": "swarm_not_found", "swarm_id": swarm_id }))).into_response(),
    }
}

// GET /twins/:twin_id/timeline — 4D provenance timeline for a twin.
async fn handle_twin_timeline(
    State(state):  State<NodeState>,
    Path(twin_id): Path<String>,
) -> impl IntoResponse {
    // Timeline keyed by device_id (extracted from twin_id prefix) or direct twin_id lookup
    let device_timeline_id = twin_protocol::TwinTimeline::device_id(&twin_id);
    let timeline = state.timeline_store.get(&device_timeline_id).await
        .or_else(|| None);  // also try direct lookup
    match timeline {
        Some(tl) => (StatusCode::OK, Json(serde_json::to_value(tl).unwrap_or_default())).into_response(),
        None     => (StatusCode::NOT_FOUND,
            Json(json!({ "error": "timeline_not_found", "twin_id": twin_id }))).into_response(),
    }
}

// GET /timelines — list all known timelines.
async fn handle_timelines_list(State(state): State<NodeState>) -> impl IntoResponse {
    let timelines = state.timeline_store.all().await;
    Json(json!({
        "count":     timelines.len(),
        "timelines": timelines.iter().map(|tl| json!({
            "timeline_id": tl.timeline_id,
            "entry_count": tl.entries.len(),
            "version":     tl.version,
            "latest_twin": tl.latest().map(|e| &e.twin_id),
        })).collect::<Vec<_>>(),
    }))
}

// GET /tiles — list all 256 Odù tile IDs with receipt counts.
async fn handle_tiles_list(State(state): State<NodeState>) -> impl IntoResponse {
    use sovereign_types::all_tiles;
    let records = state.receipt_store.list().await;
    let tiles: Vec<_> = all_tiles().map(|coord| {
        let tile_id = coord.tile_id();
        let count = records.iter()
            .filter(|r| r.odu_tile.as_deref() == Some(&tile_id))
            .count();
        json!({ "tile_id": tile_id, "x": coord.x, "y": coord.y, "receipt_count": count })
    }).collect();
    Json(json!({ "count": tiles.len(), "tiles": tiles }))
}

// GET /tiles/:tile_id/receipts — list receipts for a specific Odù tile.
async fn handle_tile_receipts(
    State(state): State<NodeState>,
    Path(tile_id): Path<String>,
) -> impl IntoResponse {
    use sovereign_types::OduCoordinate;
    // Validate tile_id format
    if OduCoordinate::from_tile_id(&tile_id).is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid_tile_id", "hint": "format: odu:XY (hex nibbles)" })),
        ).into_response();
    }
    let records = state.receipt_store.get_by_tile(&tile_id).await;
    (StatusCode::OK, Json(json!({ "tile_id": tile_id, "count": records.len(), "receipts": records }))).into_response()
}

// GET /events/receipts — SSE stream of TwinEvents (capture_complete / capture_failed).
//
// Clients connect and receive a stream of `data: <json>\n\n` lines.
// Compatible with the EventSource browser API and curl --no-buffer.
async fn handle_sse_receipts(State(state): State<NodeState>) -> impl IntoResponse {
    use axum::response::sse::{Event, Sse};
    use futures_util::stream::{self, StreamExt as _};
    use std::convert::Infallible;

    let mut rx = state.twin_events.subscribe();
    let stream = stream::unfold(rx, |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let json = serde_json::to_string(&event).unwrap_or_default();
                    let event_type = event.job_id().to_string(); // used as SSE id
                    let sse_event = Event::default()
                        .id(event_type)
                        .event(match &event {
                            crate::events::TwinEvent::CaptureComplete { .. } => "capture_complete",
                            crate::events::TwinEvent::CaptureFailed { .. }  => "capture_failed",
                            crate::events::TwinEvent::StatusUpdate { .. }   => "status_update",
                        })
                        .data(json);
                    return Some((Ok::<_, Infallible>(sse_event), rx));
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
            }
        }
    });

    Sse::new(stream)
        .keep_alive(axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(15))
            .text("ping"))
}

/// GET /events/jobs — SSE stream of all job lifecycle events.
/// Streams CaptureComplete, CaptureFailed, and StatusUpdate events.
/// Each event has `event:` type matching the TwinEvent variant name.
async fn handle_sse_jobs(State(state): State<NodeState>) -> impl IntoResponse {
    use axum::response::sse::{Event, Sse};
    use futures_util::stream::{self, StreamExt as _};
    use std::convert::Infallible;

    let rx = state.twin_events.subscribe();
    let stream = stream::unfold(rx, |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let json = serde_json::to_string(&event).unwrap_or_default();
                    let event_type = match &event {
                        crate::events::TwinEvent::CaptureComplete { .. } => "capture_complete",
                        crate::events::TwinEvent::CaptureFailed   { .. } => "capture_failed",
                        crate::events::TwinEvent::StatusUpdate    { .. } => "status_update",
                    };
                    let sse_event = Event::default()
                        .event(event_type)
                        .data(json);
                    return Some((Ok::<_, Infallible>(sse_event), rx));
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
            }
        }
    });

    Sse::new(stream)
        .keep_alive(
            axum::response::sse::KeepAlive::new()
                .interval(std::time::Duration::from_secs(15))
                .text("keep-alive"),
        )
        .into_response()
}

// GET /federation/peers — discover sovereign nodes on the local network via mDNS.
async fn handle_federation_peers(_state: State<NodeState>) -> impl IntoResponse {
    let peers = discover_sovereign_nodes().await;
    Json(json!({ "peers": peers, "count": peers.len() }))
}

// GET /ip/root — return the cached IP Root event (kind 31900) as JSON.
// 404 if nostr_nsec is not configured.
async fn handle_ip_root(State(state): State<NodeState>) -> impl IntoResponse {
    match state.ip_root_event.as_ref() {
        Some(ev) => (
            StatusCode::OK,
            Json(serde_json::to_value(ev.as_ref()).unwrap()),
        ).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "no_nostr_nsec_configured"})),
        ).into_response(),
    }
}

// GET /ip/receipt/:twin_id — reconstruct and return the twin_binding event for a twin.
// Looks up the stored receipt for twin_id, then calls seal_gaussian_splat to rebuild
// the twin binding event. Returns 404 if nsec not configured or receipt not found.
async fn handle_ip_receipt(
    State(state): State<NodeState>,
    Path(twin_id): Path<String>,
) -> impl IntoResponse {
    let Some(nsec) = state.config.dip.nostr_nsec.as_deref() else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "no_nostr_nsec_configured"})),
        ).into_response();
    };

    let nostr_key = match NostrSecretKey::from_hex(nsec) {
        Ok(k)  => k,
        Err(e) => return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("invalid nostr_nsec: {e}")})),
        ).into_response(),
    };

    let Some(record) = state.receipt_store.get_by_twin(&twin_id).await else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "receipt_not_found", "twin_id": twin_id})),
        ).into_response();
    };

    let ip_root_id  = nostr_key.pubkey_hex();
    let splat_hash  = sha256_hex(record.twin_id.as_bytes());

    match seal_gaussian_splat(
        &ip_root_id,
        &record.twin_id,
        &record.scene_receipt_id,
        &splat_hash,
        None,
        &format!("Twin {}", &record.twin_id),
        &nostr_key,
    ) {
        Ok((twin_binding, _creation_receipt)) => (
            StatusCode::OK,
            Json(serde_json::to_value(&twin_binding).unwrap()),
        ).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("seal failed: {e}")})),
        ).into_response(),
    }
}

// GET /config/check — validate node configuration and report any issues.
async fn handle_config_check(State(state): State<NodeState>) -> impl IntoResponse {
    let cfg  = &state.config;
    let mut warnings: Vec<&'static str> = vec![];
    let mut ok = true;

    if cfg.dip.nostr_enabled {
        if cfg.dip.nostr_relay.is_none() { warnings.push("dip.nostr_relay not set"); ok = false; }
        if cfg.dip.nostr_npub.is_none()  { warnings.push("dip.nostr_npub not set");  ok = false; }
    }
    if cfg.pipeline.osovm_endpoint.is_none() {
        warnings.push("pipeline.osovm_endpoint not set — stub ỌSỌVM will be used");
    }
    if cfg.pipeline.splat_bin.is_none() {
        warnings.push("pipeline.splat_bin not set — stub PLY output will be used");
    }
    if cfg.pipeline.splat_output_dir.is_none() {
        warnings.push("pipeline.splat_output_dir not set — /ws/splat will return empty");
    }
    if cfg.vantage.is_none() {
        warnings.push("vantage not configured — receipts won't be posted to explorer");
    }
    if cfg.peers.nodes.is_empty() {
        warnings.push("peers.nodes is empty — delegation and gossip unavailable");
    }

    Json(json!({
        "ok":          ok,
        "node":        cfg.node.name,
        "did":         state.identity.did,
        "warnings":    warnings,
        "warning_count": warnings.len(),
    }))
}

// POST /devices/register — accept a manifest JSON body and upsert into the registry.
async fn handle_device_register(
    State(state): State<NodeState>,
    Json(manifest): Json<vcp::manifest::AgentDeviceManifest>,
) -> impl IntoResponse {
    let device_id = manifest.device_id.clone();
    info!(device_id = %device_id, model = %manifest.model, "manual device registered via API");
    state.registry.upsert(
        vcp::discovery::DiscoveredDevice::from_manifest(
            &manifest, None,
            vcp::discovery::DiscoveryMethod::Manual,
        )
    ).await;
    Json(json!({ "ok": true, "device_id": device_id }))
}

// POST /dip/inbound — Vantage (or any peer) pushes a DIP envelope to this node.
async fn handle_dip_inbound(
    State(state): State<NodeState>,
    Json(envelope): Json<dip::DipEnvelope>,
) -> impl IntoResponse {
    let msg_id = envelope.message_id.clone();
    let kind   = format!("{:?}", envelope.kind);
    info!(msg_id = %msg_id, kind = %kind, "DIP envelope received via HTTP inbound");
    handle_inbound_dip(envelope, &state.inbound_ctx).await;
    Json(json!({ "ok": true, "message_id": msg_id }))
}

// POST /dip/gossip — push recent receipts to all configured peer nodes as DIP envelopes.
async fn handle_dip_gossip(
    State(state): State<NodeState>,
    body: Option<Json<serde_json::Value>>,
) -> impl IntoResponse {
    let count: usize = body
        .as_ref()
        .and_then(|Json(v)| v.get("count").and_then(|c| c.as_u64()))
        .unwrap_or(10) as usize;

    let all_records = state.receipt_store.list().await;
    let receipt_total = all_records.len();
    let records: Vec<_> = if all_records.len() <= count {
        all_records
    } else {
        all_records.into_iter().rev().take(count).collect()
    };

    let peers = &state.config.peers.nodes;
    let peer_count = peers.len();
    let client = reqwest::Client::new();
    let mut pushed = 0usize;

    for record in &records {
        if let Some(envelope) = dip_receipt_envelope(&state.identity, &state.config, &record.receipt_id) {
            for peer in peers {
                let base = peer.a2a_base_url.replace("/a2a", "");
                let url = format!("{base}/dip/inbound");
                match client.post(&url).json(&envelope).send().await {
                    Ok(resp) => {
                        info!(
                            peer   = %peer.name,
                            receipt = %record.receipt_id,
                            status = %resp.status(),
                            "DIP gossip pushed"
                        );
                        pushed += 1;
                    }
                    Err(e) => {
                        warn!(peer = %peer.name, error = %e, "DIP gossip push failed");
                    }
                }
            }
        }
    }

    Json(json!({
        "pushed":   pushed,
        "peers":    peer_count,
        "receipts": receipt_total,
    }))
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

    tokio::spawn(run_capture_job(
        job_id.clone(),
        device.device_id.clone(),
        device.model.clone(),
        state.identity.clone(),
        state.config.clone(),
        state.job_store.clone(),
        state.receipt_store.clone(),
        state.witnesses.clone(),
        state.dip_gateway.clone(),
        state.twin_events.clone(),
        state.tile_economy_store.clone(),
        state.act_chain.clone(),
    ));

    Json(json!({
        "job_id":    job_id,
        "device_id": device_id,
        "status":    "queued",
        "poll":      format!("/jobs/{job_id}"),
    }))
}

// POST /capture/delegate — forward capture task to a peer node via A2A.
async fn handle_capture_delegate(
    State(state): State<NodeState>,
    Json(req):    Json<DelegateRequest>,
) -> impl IntoResponse {
    let peers = &state.config.peers.nodes;
    match delegate_capture(peers, &req.device_id, &req.hint, req.peer.as_deref()).await {
        Ok(result) => (
            StatusCode::ACCEPTED,
            Json(json!({
                "ok":       true,
                "peer":     result.peer_name,
                "peer_url": result.peer_url,
                "task_id":  result.task_id,
                "poll_url": result.poll_url,
            })),
        ),
        Err(e) => {
            warn!(error = %e, "capture delegation failed");
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": "delegation_failed", "reason": e })),
            )
        }
    }
}

/// The full capture job — spawned as a tokio task by handle_capture, MCP, and A2A dispatch.
pub async fn run_capture_job(
    job_id:         String,
    device_id:      String,
    model:          String,
    identity:       Arc<NodeIdentity>,
    config:         Arc<NodeConfig>,
    job_store:      crate::jobs::JobStore,
    receipts:       ReceiptStore,
    witnesses:      WitnessRegistry,
    dip_gateway:    Arc<DipGateway>,
    events_tx:      broadcast::Sender<TwinEvent>,
    tile_economy:   TileEconomyStore,
    act_chain:      Arc<tokio::sync::RwLock<ActReceiptChain>>,
) {
    job_store.update_status(&job_id, JobStatus::Running).await;

    // Phase A: blocking VCP capture
    let capture_result = {
        let identity2 = identity.clone();
        let config2   = config.clone();
        let dev_id2   = device_id.clone();
        let model2    = model.clone();
        tokio::task::spawn_blocking(move || {
            run_capture_pipeline(&identity2, &config2, &dev_id2, &model2)
        }).await
    };

    let pipeline_output = match capture_result {
        Err(e) => {
            error!(job_id = %job_id, error = %e, "capture task panicked");
            job_store.update_status(&job_id, JobStatus::Failed {
                reason: format!("task panicked: {e}")
            }).await;
            return;
        }
        Ok(Err(e)) => {
            warn!(job_id = %job_id, error = %e, "capture pipeline failed");
            job_store.update_status(&job_id, JobStatus::Failed { reason: e }).await;
            return;
        }
        Ok(Ok(output)) => output,
    };

    let twin_id  = pipeline_output.twin.twin_id.clone();
    let scene_id = pipeline_output.scene_receipt.receipt_id.clone();
    let cap_id   = pipeline_output.capture_receipt.receipt_id.clone();

    // Phase B: two-phase witness collection + ProofChain
    let scenario = SimScenario {
        name:                "node_capture".into(),
        robot_model:         "Go2".into(),
        trajectory_count:    config.pipeline.trajectory_count,
        selection_objective: config.pipeline.selection_objective.clone(),
        params:              None,
    };

    let mut engine = OsovmEngine::new(&config.pipeline.osovm_version);
    if let Some(ep) = &config.pipeline.osovm_endpoint {
        engine = engine.with_endpoint(ep.clone());
    }
    let proof  = ProofOfSimulation::new(engine);

    // Phase B1: run ỌSỌVM and get the commitment hash
    let (osovm_run, commitment) = match proof.run_and_commitment(&pipeline_output.twin, &scenario) {
        Ok(pair) => pair,
        Err(e) => {
            warn!(job_id = %job_id, error = %e, "ỌSỌVM run failed");
            job_store.update_status(&job_id, JobStatus::Failed {
                reason: format!("osovm: {e}")
            }).await;
            return;
        }
    };

    // Phase B2: collect witness attestations
    // Local witnesses sign immediately; remote witnesses sign via DIP (30s timeout).
    let mut attestations: Vec<WitnessAttestation> = vec![];

    let local_signers = witnesses.local_signers().await;
    for w in &local_signers {
        if let Some(key) = &w.private_key {
            let sig = sovereign_types::crypto::sign(&commitment, key)
                .unwrap_or_else(|_| "invalid".into());
            attestations.push(WitnessAttestation {
                witness_id:        w.did.clone(),
                merkle_commitment:  commitment.clone(),
                timestamp:         now_ms(),
                signature:         sig,
            });
        }
    }

    // Request remote witnesses (those without a local private key)
    let remote_witnesses: Vec<_> = witnesses.inner_peers().await
        .into_iter()
        .filter(|w| w.private_key.is_none())
        .collect();

    for remote in &remote_witnesses {
        let req_job_id = format!("{job_id}:{}", remote.did);
        if let Some(att) = witnesses.request_remote_signature(
            &req_job_id, &commitment, remote,
            &dip_gateway, &identity,
            std::time::Duration::from_secs(30),
        ).await {
            attestations.push(att);
        }
    }

    // Pad to >= 2 with ephemeral stubs if still short
    if attestations.len() < config.pipeline.min_witnesses {
        let needed = config.pipeline.min_witnesses.saturating_sub(attestations.len());
        warn!(
            have  = attestations.len(),
            need  = config.pipeline.min_witnesses,
            stubs = needed,
            "using ephemeral stub witnesses"
        );
        for i in 0..needed {
            let (stub_key, _) = sovereign_types::crypto::generate_keypair();
            let stub_did = format!("did:witness:stub:{i:02}");
            let sig = sovereign_types::crypto::sign(&commitment, &stub_key)
                .unwrap_or_else(|_| "invalid".into());
            attestations.push(WitnessAttestation {
                witness_id:        stub_did,
                merkle_commitment:  commitment.clone(),
                timestamp:         now_ms(),
                signature:         sig,
            });
        }
    }

    // Phase B3: build SimulationReceipt from pre-collected attestations
    let chain_identity = IdentityChain::new(identity.did.clone(), identity.did.clone());
    let simulation_receipt = match proof.prove_with_attestations(
        osovm_run, &twin_id, chain_identity.clone(), &identity.private_key, attestations,
    ) {
        Ok(r) => r,
        Err(e) => {
            warn!(job_id = %job_id, error = %e, "SimulationReceipt build failed");
            job_store.update_status(&job_id, JobStatus::Failed {
                reason: format!("sim_receipt: {e}")
            }).await;
            return;
        }
    };

    // Phase B4: Sui anchor + DIP event bus via ProofChain
    let proof_cfg = ProofChainConfig {
        osovm_version: config.pipeline.osovm_version.clone(),
        nostr_npub:    config.dip.nostr_npub.clone(),
        vantage_did:   config.dip.vantage_did.clone(),
        scenario:      scenario.clone(),
        ..Default::default()
    };
    let chain = ProofChain::new(proof_cfg, &identity.private_key, chain_identity);

    // Inject the pre-built simulation receipt — use run_with_simulation
    match chain.run_with_simulation(pipeline_output, simulation_receipt).await {
        Err(e) => {
            warn!(job_id = %job_id, error = %e, "proof chain failed — saving partial result");
            job_store.update_status(&job_id, JobStatus::Completed {
                twin_id:            twin_id.clone(),
                scene_receipt_id:   scene_id.clone(),
                capture_receipt_id: cap_id,
                sui_object_id: None, dip_message_count: 0,
            }).await;
            let _ = events_tx.send(TwinEvent::CaptureComplete {
                twin_id:    twin_id,
                device_id:  device_id.clone(),
                receipt_id: scene_id,
                job_id:     job_id.clone(),
            });
        }
        Ok(proof_output) => {
            info!(
                job_id   = %job_id,
                twin_id  = %twin_id,
                sui_id   = ?proof_output.pipeline.twin.sui_object_id,
                dip_msgs = %proof_output.dip_message_ids.len(),
                "proof chain complete"
            );

            // Route the DIP receipt envelope through all adapters (Nostr, Vantage, Mesh)
            if let Some(env) = dip_receipt_envelope(
                &identity, &config,
                &proof_output.pipeline.scene_receipt.receipt_id,
            ) {
                dip_gateway.send(env).await;
            }

            // Publish structured receipt to Vantage explorer if configured
            if let Some(vantage_cfg) = &config.vantage {
                let vc = VantageClient::new(&vantage_cfg.base_url, &vantage_cfg.api_token);
                vc.post_receipt(
                    &proof_output.pipeline.twin.twin_id,
                    &proof_output.pipeline.scene_receipt.receipt_id,
                    &device_id,
                    31030,
                    proof_output.pipeline.twin.sui_object_id.as_deref(),
                ).await;
            }

            let dip_count  = proof_output.dip_message_ids.len();
            let p_twin_id  = proof_output.pipeline.twin.twin_id.clone();
            let p_scene_id = proof_output.pipeline.scene_receipt.receipt_id.clone();
            let p_cap_id   = proof_output.pipeline.capture_receipt.receipt_id.clone();
            let p_sui      = proof_output.pipeline.twin.sui_object_id.clone();

            job_store.update_status(&job_id, JobStatus::Completed {
                twin_id:            p_twin_id.clone(),
                scene_receipt_id:   p_scene_id.clone(),
                capture_receipt_id: p_cap_id.clone(),
                sui_object_id:      p_sui.clone(),
                dip_message_count:  dip_count,
            }).await;

            // Persist receipt to disk for restart survival
            receipts.save(ReceiptRecord {
                kind:               31030,
                receipt_id:         p_scene_id.clone(),
                twin_id:            p_twin_id.clone(),
                device_id:          device_id.clone(),
                scene_receipt_id:   p_scene_id.clone(),
                capture_receipt_id: p_cap_id,
                sui_object_id:      p_sui,
                dip_message_count:  dip_count,
                completed_at:       now_ms(),
                odu_tile:           None, // populated by GPS-aware capture in future
            }).await;

            // Mint Àṣẹ tokens for this SceneReceipt.
            let sui_url = config.vantage.as_ref().map(|v| v.base_url.as_str());
            let ase_req = AseMintRequest {
                receipt_id:  p_scene_id.clone(),
                twin_id:     p_twin_id.clone(),
                tile_id:     "odu:00".into(), // default tile — GPS refinement in future
                minter_did:  identity.did.clone(),
                quality:     proof_output.pipeline.twin.quality.f1_score,
                novelty:     0.5, // novelty oracle not yet wired; default mid-range
                sui_address: identity.did.clone(),
            };
            let mint_result = mint_ase(&ase_req, sui_url).await;
            tile_economy.apply_mint(&ase_req.tile_id, &mint_result).await;
            info!(
                job_id        = %job_id,
                tokens_minted = mint_result.tokens_minted,
                stub          = mint_result.stub,
                "Àṣẹ tokens minted for receipt"
            );

            // Emit sovereign ActionReceipt for this capture — the universal execution proof.
            let principal = Principal::from_did(
                identity.did.clone(),
                format!("did:vantage:agent:{}", &identity.did[..identity.did.len().min(12)]),
            );
            let mut ev_bundle = EvidenceBundle::new();
            ev_bundle.push(Evidence {
                evidence_id:  p_scene_id.clone(),
                kind:         EvidenceKind::SensorCapture,
                content_hash: format!("sha256:{}", &p_scene_id),
                uri:          None,
                metadata:     serde_json::json!({ "twin_id": p_twin_id, "tokens": mint_result.tokens_minted }),
                captured_at:  now_ms(),
            });
            let now_ts = now_ms();
            match ExecutionEngine::begin(
                principal.clone(),
                None,
                CapabilityAction::Capture,
                format!("twin:{p_twin_id}"),
                serde_json::json!({ "job_id": job_id, "tile": ase_req.tile_id }),
                now_ts,
            ) {
                Ok(ctx) => {
                    let action_receipt = ctx.complete(
                        serde_json::json!({
                            "scene_receipt_id": p_scene_id,
                            "tokens_minted":    mint_result.tokens_minted,
                        }),
                        now_ts,
                    );
                    info!(
                        receipt_id = %action_receipt.receipt_id,
                        outcome    = ?action_receipt.outcome,
                        "ActionReceipt emitted for capture"
                    );
                }
                Err(denied) => {
                    warn!(
                        receipt_id = %denied.receipt_id,
                        error      = ?denied.error,
                        "ActionReceipt denied — principal validation failed"
                    );
                }
            }

            // Append AgentActReceipt (Layer 3) for this capture.
            {
                let receipt = AgentActReceipt::new(
                    &identity.did,
                    "capture",
                    format!("twin:{p_twin_id}"),
                    serde_json::json!({ "device_id": device_id }),
                    serde_json::json!({ "receipt_id": p_scene_id, "tokens_minted": mint_result.tokens_minted }),
                    now_ts,
                ).with_epistemic(AgentEpistemic::Observed);
                act_chain.write().await.push(receipt);
            }

            // Seal IP provenance on Nostr — Twin Binding (1903) + Creation Receipt (1901).
            // The agent's Nostr secret key is the secp256k1 key derived from its identity.
            // If no nostr_key is configured, this is silently skipped (offline-sovereign invariant).
            if let Some(nostr_nsec) = config.dip.nostr_nsec.as_deref() {
                match NostrSecretKey::from_hex(nostr_nsec) {
                    Ok(nostr_key) => {
                        let ip_root_id = nostr_key.pubkey_hex();
                        // Use the twin asset content hash as the splat hash.
                        // In production this is sha256(ply_bytes); here we derive from twin_id.
                        let splat_hash = sha256_hex(p_twin_id.as_bytes());
                        let f1 = proof_output.pipeline.twin.quality.f1_score;
                        match seal_gaussian_splat(
                            &ip_root_id,
                            &p_twin_id,
                            &p_scene_id,
                            &splat_hash,
                            Some(f1),
                            &format!("Scene capture by {}", &identity.did[..identity.did.len().min(20)]),
                            &nostr_key,
                        ) {
                            Ok((twin_binding, creation_receipt)) => {
                                info!(
                                    twin_binding_id   = %twin_binding.id,
                                    creation_rcpt_id  = %creation_receipt.id,
                                    ip_root_id        = %ip_root_id,
                                    "IP provenance sealed on Nostr"
                                );
                                // Publish both events to the configured Nostr relay (fire-and-forget).
                                if let Some(relay_url) = config.dip.nostr_relay.as_deref() {
                                    let relay = relay_url.to_string();
                                    let tb = twin_binding.clone();
                                    let cr = creation_receipt.clone();
                                    tokio::spawn(async move {
                                        if let Err(e) = crate::nostr_publisher::publish_nostr_event(&relay, &tb).await {
                                            warn!(error = %e, "twin binding publish failed");
                                        }
                                        if let Err(e) = crate::nostr_publisher::publish_nostr_event(&relay, &cr).await {
                                            warn!(error = %e, "creation receipt publish failed");
                                        }
                                    });
                                }
                            }
                            Err(e) => {
                                warn!(error = %e, "IP provenance sealing failed — continuing");
                            }
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "invalid nostr_nsec in config — skipping IP provenance");
                    }
                }
            }

            // Broadcast completion to all WebSocket subscribers
            let _ = events_tx.send(TwinEvent::CaptureComplete {
                twin_id:    p_twin_id,
                device_id:  device_id.clone(),
                receipt_id: p_scene_id,
                job_id:     job_id.clone(),
            });
        }
    }
}

/// Blocking capture pipeline run — called via spawn_blocking.
fn run_capture_pipeline(
    identity:  &NodeIdentity,
    config:    &NodeConfig,
    device_id: &str,
    model:     &str,
) -> Result<PipelineOutput, String> {
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
        splat_bin:             config.pipeline.splat_bin.clone(),
        splat_output_dir:      config.pipeline.splat_output_dir.clone(),
        splat_steps:           config.pipeline.splat_steps,
        ..Default::default()
    };

    let pipeline = CapturePipeline::new(pipeline_cfg, &identity.private_key, chain);
    // Use real WebSocket driver if the device_id looks like a live Go2 address
    let live_id = if device_id.starts_with("unitree:go2:") {
        Some(device_id.to_string())
    } else {
        None
    };
    let driver = Go2CaptureDriver { live_device_id: live_id };
    pipeline.run(session, &driver).map_err(|e| e.to_string())
}

/// Build a DIP Receipt envelope wrapping a scene receipt for Nostr publication.
fn dip_receipt_envelope(
    identity: &NodeIdentity,
    config:   &NodeConfig,
    scene_receipt_id: &str,
) -> Option<dip::DipEnvelope> {
    use dip::{DipEnvelope, DipKind, DipAddress, address::DipNetwork};
    use sovereign_types::IdentityChain;

    let npub     = config.dip.nostr_npub.as_deref()?;
    let chain    = IdentityChain::new(identity.did.clone(), identity.did.clone());
    let origin   = DipAddress::vantage(&identity.did);
    let dest     = DipAddress { network: DipNetwork::Nostr, address: npub.into(), did: None };

    DipEnvelope::build(
        origin, dest, chain,
        DipKind::Receipt,
        serde_json::json!({ "receipt_id": scene_receipt_id, "kind": 31030 }),
        3600,
        &identity.private_key,
    ).ok()
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

/// Dispatch an inbound DIP envelope received from an external transport (Nostr, Mesh).
async fn handle_inbound_dip(envelope: dip::DipEnvelope, ctx: &InboundDipContext) {
    use dip::{DipKind, DipEnvelope, DipAddress};

    let addressed_here = envelope.destination.did.as_deref() == Some(&ctx.local_did)
        || envelope.destination.address == ctx.local_did;

    if !addressed_here {
        return; // not for us
    }

    match envelope.kind {
        DipKind::Capability => {
            // Could be a witness sign request from a peer node
            if let Some(req_type) = envelope.payload.get("type").and_then(|v| v.as_str()) {
                if req_type == "witness_sign_request" {
                    handle_witness_sign_request(&envelope, ctx).await;
                    return;
                }
            }
            info!(msg_id = %envelope.message_id, "inbound DIP Capability (no handler)");
        }

        DipKind::Receipt => {
            // Could be a witness sign response completing a pending request
            if let Some(req_type) = envelope.payload.get("type").and_then(|v| v.as_str()) {
                if req_type == "witness_sign_response" {
                    ctx.witnesses.complete_pending_signature(&envelope.payload).await;
                    return;
                }
            }
            info!(
                msg_id  = %envelope.message_id,
                payload = %envelope.payload,
                "inbound DIP Receipt delivered"
            );
        }

        DipKind::Message => {
            info!(
                msg_id  = %envelope.message_id,
                payload = %envelope.payload,
                "inbound DIP Message delivered"
            );
        }

        _ => {
            info!(
                msg_id = %envelope.message_id,
                kind   = ?envelope.kind,
                "inbound DIP envelope (no local handler)"
            );
        }
    }
}

/// Handle an inbound witness sign request: sign the commitment and reply via DIP.
async fn handle_witness_sign_request(
    envelope: &dip::DipEnvelope,
    ctx:      &InboundDipContext,
) {
    use dip::{DipEnvelope, DipKind, DipAddress};
    use sovereign_types::{IdentityChain, crypto::sign};

    let payload = &envelope.payload;
    let job_id     = payload.get("job_id").and_then(|v| v.as_str()).unwrap_or("");
    let commitment = payload.get("commitment").and_then(|v| v.as_str()).unwrap_or("");
    let requester  = payload.get("requester_did").and_then(|v| v.as_str()).unwrap_or("");

    if job_id.is_empty() || commitment.is_empty() || requester.is_empty() {
        warn!(msg_id = %envelope.message_id, "malformed witness_sign_request");
        return;
    }

    // Check if this node has a witness private key to sign with
    let signers = ctx.witnesses.local_signers().await;
    let Some(signer) = signers.into_iter().find(|w| w.private_key.is_some()) else {
        info!(
            job_id = %job_id,
            "received witness sign request but no local signing key available"
        );
        return;
    };

    let signer_key = signer.private_key.unwrap();
    let signature  = sign(commitment, &signer_key).unwrap_or_else(|_| "invalid".into());

    info!(
        job_id    = %job_id,
        signer    = %signer.did,
        "signed witness commitment — replying via DIP"
    );

    let response_payload = serde_json::json!({
        "type":       "witness_sign_response",
        "job_id":     job_id,
        "signer_did": signer.did,
        "signature":  signature,
        "public_key": signer.public_key,
    });

    let chain  = IdentityChain::new(ctx.identity.did.clone(), ctx.identity.did.clone());
    let origin = dip::DipAddress::vantage(&ctx.identity.did);
    let dest   = dip::DipAddress {
        network: dip::address::DipNetwork::Vantage,
        address: requester.into(),
        did:     Some(requester.into()),
    };

    match DipEnvelope::build(origin, dest, chain, DipKind::Receipt, response_payload, 120, &ctx.identity.private_key) {
        Ok(reply) => ctx.gateway.send(reply).await,
        Err(e)    => warn!(error = %e, "failed to build witness sign response envelope"),
    }
}

/// Handle an A2A skill dispatch request from the A2A router.
///
/// Routes "capture" requests into the sovereign-node capture job queue.
/// On completion, writes the result back into the A2aState task map so the
/// A2A poller / client can observe the final state.
async fn handle_a2a_dispatch(req: sovereign_a2a::A2aDispatchRequest, state: NodeState) {
    use sovereign_a2a::{Artifact, Part, TaskState};

    match req.skill.as_str() {
        "capture" => {
            // Extract device_id from text (e.g. "capture unitree:go2:192.168.1.10")
            let device_id = req.text.split_whitespace()
                .find(|w| w.contains(':'))
                .unwrap_or("unitree:go2:stub")
                .to_string();

            info!(task_id = %req.task_id, device_id = %device_id, "A2A capture dispatch");

            // Queue a capture job (reuse the same logic as POST /capture/:device)
            let job_id = uuid::Uuid::new_v4().to_string();
            let job = crate::jobs::Job::new(&job_id, &device_id);
            state.job_store.insert(job).await;
            state.a2a.update_task_state(
                &req.task_id,
                TaskState::Working,
                Some(format!("Capture job queued: {job_id}")),
            ).await;

            let capture_state = state.clone();
            let a2a_task_id  = req.task_id.clone();
            // Determine model from registry or default
            let model = capture_state.registry
                .get(&device_id).await
                .and_then(|d| Some(d.model.clone()))
                .unwrap_or_else(|| "Go2".into());
            tokio::spawn(run_capture_job(
                job_id.clone(),
                device_id.clone(),
                model,
                capture_state.identity.clone(),
                capture_state.config.clone(),
                capture_state.job_store.clone(),
                capture_state.receipt_store.clone(),
                capture_state.witnesses.clone(),
                capture_state.dip_gateway.clone(),
                capture_state.twin_events.clone(),
                capture_state.tile_economy_store.clone(),
                capture_state.act_chain.clone(),
            ));

            // Poll for job completion and update the A2A task
            let poll_state  = state.clone();
            let poll_job_id = job_id.clone();
            let poll_task   = a2a_task_id.clone();
            tokio::spawn(async move {
                // Wait up to 10 minutes for job completion (poll every 2s)
                for _ in 0..300u32 {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    if let Some(job) = poll_state.job_store.get(&poll_job_id).await {
                        match &job.status {
                            crate::jobs::JobStatus::Completed { twin_id, .. } => {
                                let tid = twin_id.clone();
                                poll_state.a2a.complete_task(&poll_task, vec![Artifact {
                                    name:  "capture_result".into(),
                                    parts: vec![Part::Text {
                                        text: format!(
                                            "Twin capture complete. Job: {poll_job_id}. \
                                             Twin ID: {tid}"
                                        )
                                    }],
                                    index: vec![0],
                                }]).await;
                                return;
                            }
                            crate::jobs::JobStatus::Failed { reason } => {
                                let r = reason.clone();
                                poll_state.a2a.update_task_state(
                                    &poll_task,
                                    TaskState::Failed,
                                    Some(format!("Capture job failed: {r}")),
                                ).await;
                                return;
                            }
                            _ => {}
                        }
                    }
                }
                // Timeout
                poll_state.a2a.update_task_state(
                    &poll_task,
                    TaskState::Failed,
                    Some("Capture job timed out after 10 minutes".into()),
                ).await;
            });
        }

        "receipt" => {
            // Look up receipts matching device_id or twin_id mentioned in text
            let query = req.text.split_whitespace()
                .find(|w| w.starts_with("twin:") || w.contains(':'))
                .unwrap_or("")
                .to_string();

            let records = state.receipt_store.list().await;
            let matching: Vec<_> = records.iter()
                .filter(|r| query.is_empty() || r.twin_id.contains(&query) || r.device_id.contains(&query))
                .collect();

            let result_text = if matching.is_empty() {
                format!("No receipts found for query: '{query}'")
            } else {
                matching.iter()
                    .map(|r| format!("twin_id={} receipt={} device={}", r.twin_id, r.receipt_id, r.device_id))
                    .collect::<Vec<_>>()
                    .join("\n")
            };

            state.a2a.complete_task(&req.task_id, vec![Artifact {
                name:  "receipts".into(),
                parts: vec![Part::Text { text: result_text }],
                index: vec![0],
            }]).await;
        }

        other => {
            warn!(skill = %other, "A2A dispatch: unknown skill");
            state.a2a.update_task_state(
                &req.task_id,
                TaskState::Failed,
                Some(format!("Unknown skill: {other}")),
            ).await;
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Build a minimal in-memory `NodeState` for integration tests.
///
/// No files on disk, no external connections. Suitable for handler-level testing
/// via `build_router(make_test_state())`.
#[cfg(any(test, feature = "test-helpers", debug_assertions))]
pub fn make_test_state() -> NodeState {
    use crate::{
        identity::NodeIdentity,
        jobs::JobStore,
        receipt_store::ReceiptStore,
        witness_registry::WitnessRegistry,
        dip_gateway::DipGateway,
    };

    let (private_key, pub_key) = sovereign_types::crypto::generate_keypair();
    let did = sovereign_types::crypto::did_from_pubkey(&pub_key, "node");

    let identity = Arc::new(NodeIdentity {
        did:         did.clone(),
        public_key:  pub_key,
        private_key: private_key.clone(),
    });
    let config = Arc::new(NodeConfig::default());

    let registry = vcp::discovery::DeviceRegistry::new(config.vcp.device_ttl_secs);

    let dip_gateway = Arc::new(DipGateway::new(
        did.clone(),
        None,     // no vantage client
        None,     // no nostr relay
        &identity,
        None,     // no meshtastic
    ));

    let a2a_cfg = sovereign_a2a::A2aConfig {
        name:        config.node.name.clone(),
        base_url:    format!("http://{}", config.api.bind),
        description: "test node".into(),
        version:     "0.0.0-test".into(),
        skills:      sovereign_a2a::A2aConfig::default().skills,
        provider:    None,
    };

    let witnesses = WitnessRegistry::new();
    let inbound_ctx = InboundDipContext {
        local_did: did.clone(),
        identity:  identity.clone(),
        witnesses: witnesses.clone(),
        gateway:   dip_gateway.clone(),
    };

    let (twin_events_tx, _) = broadcast::channel::<TwinEvent>(64);

    NodeState {
        identity,
        config,
        registry,
        job_store:           JobStore::new(),
        receipt_store:       ReceiptStore::in_memory(),
        witnesses,
        dip_gateway,
        inbound_ctx,
        nostr_relay:         None,
        a2a:                 sovereign_a2a::A2aState::new(a2a_cfg),
        swarm_store:         SwarmStore::new(),
        timeline_store:      TimelineStore::in_memory(),
        twin_events:         twin_events_tx,
        tile_economy_store:  TileEconomyStore::new(),
        started_at:          now_ms(),
        ip_root_event:       None,
        act_chain:           Arc::new(tokio::sync::RwLock::new(ActReceiptChain::new())),
        body_store:          BodyStore::new(),
        telemetry_store:     TelemetryStore::new(),
        proof_engine:        Arc::new(ProofEngine::new()),
    }
}
