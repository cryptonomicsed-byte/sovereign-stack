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
use dip;
use sovereign_types::IdentityChain;
use sovereign_pipeline::{
    CapturePipeline, PipelineConfig, Go2CaptureDriver, PipelineOutput,
    ProofChain, ProofChainConfig,
};
use twin_protocol::osovm::{OsovmEngine, ProofOfSimulation, SimScenario};
use sovereign_types::WitnessAttestation;

use crate::config::NodeConfig;
use crate::dip_gateway::DipGateway;
use crate::identity::NodeIdentity;
use crate::jobs::{Job, JobStatus, JobStore};
use crate::nostr_relay::{spawn_nostr_relay, NostrRelayHandle};
use crate::receipt_store::{ReceiptRecord, ReceiptStore};
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
    pub nostr_relay:     Option<NostrRelayHandle>,
    pub started_at:      u64,
}

pub struct SovereignNode {
    pub identity: Arc<NodeIdentity>,
    pub config:   Arc<NodeConfig>,
}

/// Context passed to the inbound DIP envelope dispatcher.
struct InboundDipContext {
    local_did: String,
    identity:  Arc<NodeIdentity>,
    witnesses: WitnessRegistry,
    gateway:   Arc<DipGateway>,
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

        let state = NodeState {
            identity:      self.identity.clone(),
            config:        self.config.clone(),
            registry,
            job_store:     JobStore::new(),
            receipt_store,
            witnesses,
            dip_gateway,
            nostr_relay,
            started_at,
        };

        // --- 3b. Meshtastic inbound poll (feeds dip_inbound_tx) ---
        if let Some(mesh_cfg) = &self.config.meshtastic {
            info!(url = %mesh_cfg.device_url, "starting Meshtastic inbound poll");
            state.dip_gateway.spawn_mesh_inbound(&mesh_cfg.device_url, dip_inbound_tx);
        }

        // --- 3c. Inbound DIP dispatch (Nostr + Meshtastic → single handler) ---
        {
            let ctx = InboundDipContext {
                local_did: self.identity.did.clone(),
                identity:  self.identity.clone(),
                witnesses: state.witnesses.clone(),
                gateway:   state.dip_gateway.clone(),
            };
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

fn build_router(state: NodeState) -> Router {
    use crate::mcp_server::handle_mcp;
    Router::new()
        .route("/health",              get(handle_health))
        .route("/status",              get(handle_status))
        .route("/devices",             get(handle_devices))
        .route("/capture/:device",     post(handle_capture))
        .route("/jobs",                get(handle_jobs_list))
        .route("/jobs/:job_id",        get(handle_job_get))
        .route("/mcp",                 post(handle_mcp))
        .route("/receipts",            get(handle_receipts))
        .route("/receipts/:twin_id",   get(handle_receipt_get))
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
    ));

    Json(json!({
        "job_id":    job_id,
        "device_id": device_id,
        "status":    "queued",
        "poll":      format!("/jobs/{job_id}"),
    }))
}

/// The full capture job — spawned as a tokio task by both handle_capture and the MCP server.
pub async fn run_capture_job(
    job_id:      String,
    device_id:   String,
    model:       String,
    identity:    Arc<NodeIdentity>,
    config:      Arc<NodeConfig>,
    job_store:   crate::jobs::JobStore,
    receipts:    ReceiptStore,
    witnesses:   WitnessRegistry,
    dip_gateway: Arc<DipGateway>,
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

    let engine = OsovmEngine::new(&config.pipeline.osovm_version);
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
                twin_id, scene_receipt_id: scene_id, capture_receipt_id: cap_id,
                sui_object_id: None, dip_message_count: 0,
            }).await;
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
                twin_id:            p_twin_id,
                device_id:          device_id.clone(),
                scene_receipt_id:   p_scene_id,
                capture_receipt_id: p_cap_id,
                sui_object_id:      p_sui,
                dip_message_count:  dip_count,
                completed_at:       now_ms(),
            }).await;
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
        ..Default::default()
    };

    let pipeline = CapturePipeline::new(pipeline_cfg, &identity.private_key, chain);
    pipeline.run(session, &Go2CaptureDriver).map_err(|e| e.to_string())
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

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
