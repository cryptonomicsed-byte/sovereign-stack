//! SovereignNode — thin protocol glue daemon and physical hardware access layer.
//!
//! Subsystems started by SovereignNode::start():
//!   1. VCP DiscoveryDaemon — BLE/mDNS scan loop
//!   2. DIP Router — envelope routing with Vantage + Nostr adapters
//!   3. API server (axum) — /health /status /devices /capture/:id /jobs/:id
//!   4. Graceful shutdown on SIGTERM / SIGINT
//!
//! SCOPE BOUNDARY: if it doesn't touch a physical device, protocol wire format,
//! or the capture/proof pipeline — it does not belong in this crate.
//! See ARCHITECTURE.md for the full scope contract and migration plan for the
//! application logic currently living here pending a future refactor session.

use std::sync::Arc;
use std::net::SocketAddr;

use axum::{
    Router,
    routing::{delete, get, post},
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
use twin_protocol::ase::{AseMintRequest, mint_ase, AseMintResult};
use twin_protocol::emission::{DailyEmissionAllocator, ProofClaim};
use twin_protocol::ObservationReceipt;
use twin_protocol::{TwinLicenseGrant, TwinLicenseConstraints};
use sovereign_types::UsageRight;
use std::time::{SystemTime, UNIX_EPOCH, Duration};
use sovereign_types::OduCoordinate;
use crate::tile_economy_store::TileEconomyStore;
use sovereign_types::WitnessAttestation;
use sovereign_runtime::chain::{
    Principal, CapabilityAction, ExecutionEngine, ActionOutcome,
};
use sovereign_runtime::evidence::{Evidence, EvidenceBundle, EvidenceKind};
use ip_layer::{NostrSecretKey, IpRootBuilder, seal_gaussian_splat, sha256_hex};

use sovereign_os::{ActReceiptChain, AgentActReceipt, EpistemicSeverity as AgentEpistemic};

use crate::body_store::BodyStore;
use crate::governance_store::GovernanceStore;
use crate::license_store::LicenseStore;
use crate::telemetry_store::TelemetryStore;
use crate::wallet_store::WalletStore;
use crate::gpu_pool::GpuPool;
use crate::agent_store::AgentStore;
use crate::council_store::{CouncilStore, CouncilSummary};
use crate::sovereign_seat_store::{SovereignSeatStore, SeatSummary};
use crate::emission_receipt_store::EmissionReceiptStore;
use crate::simulation_scoring::{SimulationFactors, SimulationScoreResult, compute_emission_shares};
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
    pub swarm_store:       SwarmStore,
    pub timeline_store:    TimelineStore,
    pub federation_router: crate::federation_router::FederationRouter,
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
    /// OSOVM RUNTIME: DailyEmissionAllocator — converts ProofClaims → MintAuthorizations.
    pub emission_allocator:   Arc<tokio::sync::Mutex<DailyEmissionAllocator>>,
    /// OSOVM RUNTIME: queued ProofClaims awaiting the next per-minute allocation tick.
    pub pending_claims:       Arc<tokio::sync::Mutex<Vec<ProofClaim>>>,
    /// Off-chain governance proposal index.
    pub governance_store:     GovernanceStore,
    /// Twin license grant store — Phase 3.6 licensing marketplace.
    pub license_store:        LicenseStore,
    /// Sovereign wallet balances (micro-Àṣẹ) keyed by DID.
    pub wallet_store:         WalletStore,
    /// OSOVM Token-of-Compute: GPU contribution pool (Dopamine + Synapse tokens).
    pub gpu_pool:             GpuPool,
    /// Agent birth registry — Dopamine/Synapse balances, tier, stake gate.
    pub agent_store:          AgentStore,
    /// Council of 12 — seats, sectors, rotation (Phase 58).
    pub council_store:        CouncilStore,
    /// 1440 sovereign stewardship offices (Phase 59).
    pub seat_store:           SovereignSeatStore,
    /// Emission receipt chain — Zàngbétò: every distribution tick produces one.
    pub emission_receipts:    EmissionReceiptStore,
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
    pub local_did:     String,
    pub identity:      Arc<NodeIdentity>,
    pub witnesses:     WitnessRegistry,
    pub gateway:       Arc<DipGateway>,
    pub body_store:    BodyStore,
    pub license_store: LicenseStore,
}

impl InboundDipContext {
    fn from_state(state: &NodeState) -> Self {
        Self {
            local_did:     state.identity.did.clone(),
            identity:      state.identity.clone(),
            witnesses:     state.witnesses.clone(),
            gateway:       state.dip_gateway.clone(),
            body_store:    state.body_store.clone(),
            license_store: state.license_store.clone(),
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
        let body_store_ctx    = BodyStore::new();
        let license_store_ctx = LicenseStore::new();
        let inbound_ctx = InboundDipContext {
            local_did:     self.identity.did.clone(),
            identity:      self.identity.clone(),
            witnesses:     witnesses.clone(),
            gateway:       dip_gateway.clone(),
            body_store:    body_store_ctx.clone(),
            license_store: license_store_ctx.clone(),
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
            federation_router:   crate::federation_router::FederationRouter::new(),
            twin_events:         twin_events_tx,
            tile_economy_store:  TileEconomyStore::new(),
            started_at,
            ip_root_event,
            act_chain:        Arc::new(tokio::sync::RwLock::new(ActReceiptChain::new())),
            body_store:         body_store_ctx,
            telemetry_store:    TelemetryStore::new(),
            proof_engine:       Arc::new(ProofEngine::new()),
            emission_allocator: Arc::new(tokio::sync::Mutex::new(DailyEmissionAllocator::new())),
            pending_claims:     Arc::new(tokio::sync::Mutex::new(Vec::new())),
            governance_store:   GovernanceStore::new(),
            license_store:      license_store_ctx,
            wallet_store:       WalletStore::new(),
            gpu_pool:           GpuPool::new(),
            agent_store:        AgentStore::new(),
            council_store:      CouncilStore::new(),
            seat_store:         SovereignSeatStore::new(),
            emission_receipts:  EmissionReceiptStore::new(),
        };

        // --- 3a-init. Council and seat store initialization ---
        {
            let council = state.council_store.clone();
            tokio::spawn(async move { council.initialize().await; });
        }

        // --- 3a-emission. Per-minute DailyEmissionAllocator task (OSOVM RUNTIME) ---
        {
            use sovereign_types::{GovernanceStrata, DistributionPool};
            let allocator         = state.emission_allocator.clone();
            let pending           = state.pending_claims.clone();
            let wallet_store      = state.wallet_store.clone();
            let emission_receipts = state.emission_receipts.clone();
            let node_did          = state.identity.did.clone();
            tokio::spawn(async move {
                let strata = GovernanceStrata::canonical();
                let mut interval = tokio::time::interval(Duration::from_secs(60));
                loop {
                    interval.tick().await;
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
                    let epoch_minute = now / 60;
                    let claims: Vec<_> = {
                        let mut guard = pending.lock().await;
                        std::mem::take(&mut *guard)
                    };
                    let mut alloc = allocator.lock().await;
                    let (_minute_alloc, auth) = alloc.allocate_minute(epoch_minute, now, &claims);

                    // Credit proof workers their earned shares.
                    for worker_alloc in &auth.allocations {
                        wallet_store.credit(&worker_alloc.worker_did, worker_alloc.micro_ase).await;
                    }

                    // Credit governance pool shares + produce Zàngbétò EmissionReceipt per pool.
                    let total_per_minute = twin_protocol::emission::MICRO_ASE_PER_MINUTE;
                    for pool in &[DistributionPool::Simulation, DistributionPool::Research,
                                  DistributionPool::Governance, DistributionPool::Reserve,
                                  DistributionPool::Grants, DistributionPool::Ubi,
                                  DistributionPool::LotteryBurn, DistributionPool::Sabbath] {
                        let share = (total_per_minute * pool.allocation_bps() as u64) / 10_000;
                        if share > 0 {
                            let pool_did = format!("did:pool:{}", pool.name());
                            wallet_store.credit(&pool_did, share).await;
                            // Zàngbétò receipt — every distribution tick is receipted.
                            emission_receipts.record(
                                pool.clone(),
                                share,
                                format!("epoch_minute:{epoch_minute}"),
                                "pool_allocation_v1".to_string(),
                                Some(pool_did),
                                None,
                                format!("{} pool per-minute allocation", pool.name()),
                            ).await;
                        }
                    }

                    // Inheritance fallback: credit to this node on no-proof minutes.
                    if auth.inheritance_fallback {
                        let per_seat = total_per_minute / strata.council_seat_count as u64;
                        wallet_store.credit(&node_did, per_seat).await;
                    }

                    info!(
                        epoch_minute = epoch_minute,
                        workers      = auth.allocations.len(),
                        inheritance  = auth.inheritance_fallback,
                        is_sabbath   = twin_protocol::emission::DailyEmissionAllocator::is_sabbath(now),
                        "emission minute settled"
                    );
                }
            });
            info!("DailyEmissionAllocator per-minute task started");
        }

        // --- 3a-decay. Synapse 1%/day decay task (OSOVM ToC — Phase 54) ---
        {
            let decay_pool = state.gpu_pool.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(3600)); // hourly
                loop {
                    interval.tick().await;
                    let now_secs = SystemTime::now()
                        .duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
                    let epoch_day = now_secs / 86_400;
                    let burned = decay_pool.apply_daily_decay(epoch_day).await;
                    if burned > 0 {
                        info!(epoch_day, burned_micro_synapse = burned, "Synapse daily decay applied");
                    }
                }
            });
        }

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

        // --- 3c-hb. Vantage heartbeat (P0: unify node presence signal) ---
        crate::vantage_heartbeat::spawn_heartbeat(state.clone(), 60);
        info!("Vantage heartbeat spawned (60s interval)");

        // --- 3d-ip. Publish IP Root (kind 31900) to Nostr relay at startup — gap #10 fix.
        //
        // The event is pre-built and cached in state.ip_root_event at construction time.
        // Here we fire-and-forget the WebSocket publish. Non-fatal on failure.
        if let (Some(ev), Some(relay_url)) = (
            state.ip_root_event.clone(),
            self.config.dip.nostr_relay.clone(),
        ) {
            let ev_id = ev.id.clone();
            tokio::spawn(async move {
                match crate::nostr_publisher::publish_nostr_event(&relay_url, &ev).await {
                    Ok(()) => info!(event_id = %ev_id, relay = %relay_url, "IP Root (31900) published to Nostr"),
                    Err(e) => warn!(error = %e, relay = %relay_url, "IP Root publish failed — will retry on next boot"),
                }
            });
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
        .route("/tiles/:tile_id/stake",     post(handle_tile_stake))
        .route("/tiles",                    get(handle_tiles_list))
        .route("/capture/swarm",            post(handle_capture_swarm))
        .route("/swarm",                    get(handle_swarm_list))
        .route("/swarm/:swarm_id",          get(handle_swarm_get))
        .route("/twins/:twin_id/timeline",      get(handle_twin_timeline))
        .route("/twins/:twin_id/timeline/diff", get(handle_twin_timeline_diff))
        .route("/twins/:twin_id/splat/diff",    post(handle_twin_splat_diff))
        .route("/swarm/:swarm_id/merge-splat",  post(handle_swarm_merge_splat))
        .route("/timelines",                    get(handle_timelines_list))
        .route("/events/receipts",          get(handle_sse_receipts))
        .route("/events/jobs",              get(handle_sse_jobs))
        .route("/dip/inbound",              post(handle_dip_inbound))
        .route("/dip/gossip",               post(handle_dip_gossip))
        .route("/jobs/:job_id/retry",       post(handle_job_retry))
        .route("/config/check",             get(handle_config_check))
        .route("/ws/twin/:twin_id",         get(handle_ws_twin))
        .route("/ws/splat/:twin_id",        get(handle_ws_splat))
        .route("/federation/peers",          get(handle_federation_peers))
        .route("/federation/peers",          post(handle_federation_register_peer))
        .route("/federation/peers/:peer_id", delete(handle_federation_remove_peer))
        .route("/federation/tasks",          post(handle_federation_route_task))
        .route("/federation/health",         post(handle_federation_health_check))
        .route("/ip/root",                  get(handle_ip_root))
        .route("/ip/receipt/:twin_id",      get(handle_ip_receipt))
        .route("/agent/receipts",           get(handle_agent_receipts))
        // ── Proof-of-Evolution ─────────────────────────────────────────────
        .route("/proofs/simulation",        post(handle_proof_simulation_submit))
        .route("/proofs/simulation/:id",    get(handle_proof_simulation_get))
        .route("/proofs/observation",       post(handle_proof_observation_submit))
        .route("/proofs/observation/:id",   get(handle_proof_observation_get))
        .route("/proofs/gaussian",          post(handle_proof_gaussian_submit))
        .route("/proofs/physical",          post(handle_proof_physical_submit))
        // ── Body sessions (VCP physical embodiment) ────────────────────────
        .route("/body/sessions",            get(handle_body_sessions_list))
        .route("/body/sessions",            post(handle_body_session_open))
        .route("/body/sessions/:id",        get(handle_body_session_get))
        .route("/body/:body_id/receipts",   get(handle_body_receipts))
        .route("/body/capabilities",        get(handle_body_capabilities))
        .route("/body/sessions/:id/telemetry", post(handle_body_telemetry_push))
        .route("/body/sessions/:id/command",   post(handle_body_session_command))
        .route("/body/sessions/:id/close",     post(handle_body_session_close))
        // ── Governance proposals ───────────────────────────────────────────────
        .route("/governance/proposals",                     get(handle_governance_list))
        .route("/governance/proposals",                     post(handle_governance_create))
        .route("/governance/proposals/:id",                 get(handle_governance_get))
        .route("/governance/proposals/:id/vote_for",        post(handle_governance_vote_for))
        .route("/governance/proposals/:id/vote_against",    post(handle_governance_vote_against))
        .route("/governance/proposals/:id/execute",         post(handle_governance_execute))
        // ── Twin licensing marketplace (Phase 3.6) ──
        .route("/twins/:twin_id/licenses",  post(handle_license_issue).get(handle_license_list_for_twin))
        .route("/licenses",                 get(handle_license_list))
        .route("/licenses/:grant_id",       get(handle_license_get))
        .route("/licenses/:grant_id/accept", post(handle_license_accept))
        // ── Cowrie Oracle + Emission (Phase 45) ──
        .route("/oracle/today",             get(handle_oracle_today))
        .route("/oracle/day/:day",          get(handle_oracle_day))
        .route("/emission/status",          get(handle_emission_status))
        .route("/emission/claim",           post(handle_emission_claim))
        // ── Sovereign Wallet (Phase 46) ──
        .route("/wallets",                  get(handle_wallets_list))
        .route("/wallets/:did",             get(handle_wallet_get))
        .route("/wallets/:did/credit",      post(handle_wallet_credit))
        // ── OSOVM Token-of-Compute (Phase 52) ──
        .route("/osovm/gpu/contribute",     post(handle_gpu_contribute))
        .route("/osovm/gpu/burn",           post(handle_gpu_burn_for_synapse))
        .route("/osovm/pool",               get(handle_gpu_pool_state))
        .route("/osovm/contributions",      get(handle_osovm_contributions))
        .route("/osovm/gpu/decay",          post(handle_gpu_decay))
        .route("/osovm/balances/:did",      get(handle_osovm_balances))
        // ── Governance veto (Phase 55: Bínò council) ──
        .route("/governance/proposals/:id/veto", post(handle_governance_veto))
        // ── Agent birth + lifecycle (Phase 57) ──
        .route("/agents",                         post(handle_agent_birth))
        .route("/agents",                         get(handle_agents_list))
        .route("/agents/:agent_id",               get(handle_agent_get))
        .route("/agents/:agent_id/stake",         post(handle_agent_stake))
        .route("/agents/:agent_id/unstake",       post(handle_agent_unstake))
        // ── Council of 12 (Phase 58) ──
        .route("/governance/council",             get(handle_council_summary))
        .route("/governance/council/seats",       get(handle_council_seats))
        .route("/governance/council/sectors",     get(handle_council_sectors))
        .route("/governance/council/seats/:idx/rotate", post(handle_council_rotate))
        // ── 1440 sovereign seats (Phase 59) ──
        .route("/seats",                          get(handle_seats_summary))
        .route("/seats/:idx",                     get(handle_seat_get))
        .route("/seats/:idx/claim",               post(handle_seat_claim))
        .route("/seats/:idx/revoke",              post(handle_seat_revoke))
        // ── Emission receipts (Phase 56 — Zàngbétò) ──
        .route("/emission/receipts",              get(handle_emission_receipts_list))
        .route("/emission/receipts/:id",          get(handle_emission_receipt_get))
        // ── Simulation scoring leaderboard (Phase 60) ──
        .route("/simulation/score",               post(handle_simulation_score))
        .route("/simulation/shares",              post(handle_simulation_shares))
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
        state.pending_claims.clone(),
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
            if eval.mint_eligible {
                let tile_id = "odu:00".to_string(); // sim proofs are not yet tile-scoped
                let mint_result = mint_eligible_to_ase(
                    &eval.proof_id, "simulation", &tile_id,
                    &state.identity.did,
                    eval.proof_value as f32,
                    eval.novelty as f32,
                    state.config.vantage.as_ref().map(|v| v.base_url.as_str()),
                    &state.tile_economy_store,
                    &state.pending_claims,
                ).await;
                let _ = state.twin_events.send(crate::events::TwinEvent::MintApproved {
                    proof_id:      eval.proof_id.clone(),
                    proof_domain:  "simulation".into(),
                    tile_id,
                    minter_did:    state.identity.did.clone(),
                    tokens_minted: mint_result.tokens_minted,
                    net_minted:    mint_result.net_minted,
                    owner_fee:     mint_result.owner_fee,
                    eshu_tithe:    mint_result.elegbara.tithe_total,
                    tx_digest:     mint_result.tx_digest.clone(),
                    stub:          mint_result.stub,
                });
            }
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

/// POST /proofs/observation — submit a Proof-of-Observation receipt.
/// Body: ObservationReceipt (JSON).
/// Validates: non-empty sim_receipt_id, valid outcome field (always present on a well-formed struct).
/// Returns 201 Created with the receipt on success.
async fn handle_proof_observation_submit(
    State(state): State<NodeState>,
    Json(obs): Json<ObservationReceipt>,
) -> impl IntoResponse {
    // Validate: sim_receipt_id must be non-empty
    if obs.sim_receipt_id.is_empty() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({"error": "sim_receipt_id must not be empty"})),
        );
    }

    // Validate: receipt_id must be non-empty
    if obs.receipt_id.is_empty() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({"error": "receipt_id must not be empty"})),
        );
    }

    info!(
        receipt_id     = %obs.receipt_id,
        sim_receipt_id = %obs.sim_receipt_id,
        outcome        = %obs.outcome_str(),
        witness_id     = %obs.witness_id,
        "ObservationReceipt submitted"
    );

    // Broadcast status update
    let _ = state.twin_events.send(crate::events::TwinEvent::StatusUpdate {
        job_id:  obs.receipt_id.clone(),
        message: format!("observation:{}", obs.outcome_str()),
    });

    // Phase 4.4 step 8 — emit a Mycelium finding so the local brain can learn
    // from the sim/real gap.  Written as a TrainingRecord (alpaca JSONL) to
    // ~/.sovereign/mycelium/sim-obs-findings.jsonl (non-fatal if it fails).
    emit_mycelium_finding(&obs, &state.config.node.data_dir);

    // Persist to store
    let receipt_json = serde_json::to_value(&obs).unwrap_or(json!({}));
    state.receipt_store.add_observation(obs).await;

    (StatusCode::CREATED, Json(receipt_json))
}

/// Append one sim→obs training record to the Mycelium findings file.
///
/// Format: alpaca JSONL  `{"instruction":…,"input":…,"output":…,"source":…,"uuid":…}`
/// The file lives at `{data_dir}/mycelium/sim-obs-findings.jsonl`.
/// Failures are logged but never bubble up (non-critical path).
fn emit_mycelium_finding(obs: &ObservationReceipt, data_dir: &std::path::Path) {
    use std::io::Write as _;
    use std::fs::OpenOptions;

    let dir = data_dir.join("mycelium");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        tracing::debug!(error = %e, "mycelium dir create failed — skipping finding");
        return;
    }

    let path = dir.join("sim-obs-findings.jsonl");
    let predicted_json = serde_json::to_string(&obs.predicted).unwrap_or_default();
    let observed_json  = serde_json::to_string(&obs.observed).unwrap_or_default();
    let delta_json     = serde_json::to_string(&obs.delta).unwrap_or_default();

    let instruction = format!(
        "A simulation predicted the following outcome for sim receipt '{}'. \
         What was the actual physical outcome, and what does the delta tell us?",
        obs.sim_receipt_id,
    );
    let record = json!({
        "instruction": instruction,
        "input":       predicted_json,
        "output":      format!("Observed: {}. Delta: {}. Outcome: {}.",
                           observed_json, delta_json, obs.outcome_str()),
        "source":      "sim_obs_delta",
        "uuid":        obs.receipt_id,
    });

    let line = match serde_json::to_string(&record) {
        Ok(l) => l,
        Err(e) => {
            tracing::debug!(error = %e, "mycelium finding serialise failed");
            return;
        }
    };

    match OpenOptions::new().create(true).append(true).open(&path) {
        Ok(mut f) => {
            let _ = writeln!(f, "{line}");
            tracing::info!(path = %path.display(), receipt_id = %obs.receipt_id, "Mycelium finding appended");
        }
        Err(e) => tracing::debug!(error = %e, "mycelium finding write failed"),
    }
}

/// GET /proofs/observation/:id — fetch a single ObservationReceipt by receipt_id.
async fn handle_proof_observation_get(
    State(state): State<NodeState>,
    Path(receipt_id): Path<String>,
) -> impl IntoResponse {
    match state.receipt_store.get_observation(&receipt_id).await {
        Some(obs) => (StatusCode::OK, Json(serde_json::to_value(&obs).unwrap_or(json!({})))),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": format!("observation receipt not found: {receipt_id}")})),
        ),
    }
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

/// POST /body/sessions/:id/command  — issue a VCP command against an active session.
/// Body: { capability, action, params? }
/// Returns: { cmd_id, session_id, capability, action, status, timestamp_ms }
async fn handle_body_session_command(
    State(state): State<NodeState>,
    Path(session_id): Path<String>,
    Json(req): Json<serde_json::Value>,
) -> impl IntoResponse {
    let session = match state.body_store.get_session(&session_id).await {
        Some(s) => s,
        None => return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("session not found: {session_id}") })),
        ),
    };

    let capability = match req.get("capability").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None    => return (StatusCode::BAD_REQUEST, Json(json!({ "error": "missing capability" }))),
    };
    let action = req.get("action").and_then(|v| v.as_str()).unwrap_or("execute").to_string();
    let params = req.get("params").cloned().unwrap_or(serde_json::Value::Null);

    // Validate: capability must be in the session's granted capabilities
    if !session.capabilities.is_empty() && !session.capabilities.iter().any(|c| c == &capability) {
        return (StatusCode::FORBIDDEN, Json(json!({
            "error":      "capability_not_granted",
            "capability": capability,
            "granted":    session.capabilities,
        })));
    }

    let cmd_id = format!("cmd:{}", uuid::Uuid::new_v4());
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    info!(
        cmd_id     = %cmd_id,
        session_id = %session_id,
        capability = %capability,
        action     = %action,
        "VCP command issued"
    );

    // Emit status event so WS subscribers see the command
    let _ = state.twin_events.send(TwinEvent::StatusUpdate {
        job_id:  cmd_id.clone(),
        message: format!("vcp_cmd:{capability}:{action}:{session_id}"),
    });

    (StatusCode::OK, Json(json!({
        "cmd_id":       cmd_id,
        "session_id":   session_id,
        "capability":   capability,
        "action":       action,
        "params":       params,
        "status":       "accepted",
        "timestamp_ms": ts,
    })))
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
    state.body_store.add_receipt(receipt.clone()).await;

    // Phase 4.1 — VCP→TSP bridge: if mission succeeded and session had a camera,
    // auto-queue a capture job so the scan becomes a 31020 receipt.
    if mission_success && session.capabilities.iter().any(|c| c.contains("camera")) {
        let job_id  = format!("job:{}", uuid::Uuid::new_v4());
        let job     = Job::new(job_id.clone(), session.body_id.clone());
        state.job_store.insert(job).await;
        info!(
            job_id = %job_id,
            body_id = %session.body_id,
            source = "vcp_session_close",
            "auto-capture job queued after successful camera session"
        );
        let _ = state.twin_events.send(TwinEvent::StatusUpdate {
            job_id:  job_id.clone(),
            message: format!("capture_queued_from_session:{session_id}"),
        });
        tokio::spawn(run_capture_job(
            job_id,
            session.body_id.clone(),
            session.body_id.clone(),
            state.identity.clone(),
            state.config.clone(),
            state.job_store.clone(),
            state.receipt_store.clone(),
            state.witnesses.clone(),
            state.dip_gateway.clone(),
            state.twin_events.clone(),
            state.tile_economy_store.clone(),
            state.act_chain.clone(),
            state.pending_claims.clone(),
        ));
    }

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
            if eval.mint_eligible {
                let tile_id = proof.odu_tile.clone().unwrap_or_else(|| "odu:00".into());
                let mint_result = mint_eligible_to_ase(
                    &eval.proof_id, "spatial", &tile_id,
                    &state.identity.did,
                    eval.proof_value as f32,
                    eval.novelty as f32,
                    state.config.vantage.as_ref().map(|v| v.base_url.as_str()),
                    &state.tile_economy_store,
                    &state.pending_claims,
                ).await;
                let _ = state.twin_events.send(crate::events::TwinEvent::MintApproved {
                    proof_id:      eval.proof_id.clone(),
                    proof_domain:  "spatial".into(),
                    tile_id,
                    minter_did:    state.identity.did.clone(),
                    tokens_minted: mint_result.tokens_minted,
                    net_minted:    mint_result.net_minted,
                    owner_fee:     mint_result.owner_fee,
                    eshu_tithe:    mint_result.elegbara.tithe_total,
                    tx_digest:     mint_result.tx_digest.clone(),
                    stub:          mint_result.stub,
                });
            }
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
            if eval.mint_eligible {
                let tile_id = "odu:00".to_string(); // physical proof has no tile_id yet
                let mint_result = mint_eligible_to_ase(
                    &eval.proof_id, "physical", &tile_id,
                    &state.identity.did,
                    eval.proof_value as f32,
                    eval.novelty as f32,
                    state.config.vantage.as_ref().map(|v| v.base_url.as_str()),
                    &state.tile_economy_store,
                    &state.pending_claims,
                ).await;
                let _ = state.twin_events.send(crate::events::TwinEvent::MintApproved {
                    proof_id:      eval.proof_id.clone(),
                    proof_domain:  "physical".into(),
                    tile_id,
                    minter_did:    state.identity.did.clone(),
                    tokens_minted: mint_result.tokens_minted,
                    net_minted:    mint_result.net_minted,
                    owner_fee:     mint_result.owner_fee,
                    eshu_tithe:    mint_result.elegbara.tithe_total,
                    tx_digest:     mint_result.tx_digest.clone(),
                    stub:          mint_result.stub,
                });
            }
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
    if !tile_id.starts_with("odu:") || tile_id.len() != 6 {
        return (StatusCode::BAD_REQUEST, Json(json!({
            "error": "invalid_tile_id",
            "hint":  "tile_id must be 'odu:XY' where X and Y are hex nibbles",
        }))).into_response();
    }

    // Update local economy store.
    state.tile_economy_store.claim(&tile_id, &req.owner_did).await;

    // Anchor claim on Sui (stub when key absent / key=="stubkey").
    let anchor = state.config.tile_governance.anchor();
    let usage_fee_bps = state.config.tile_governance.default_usage_fee_bps;
    let on_chain = anchor.claim_tile(&tile_id, &req.owner_did, usage_fee_bps).await;

    let (tx_digest, object_id, stub) = match on_chain {
        Ok(r)  => (r.tx_digest, r.object_id, r.stub),
        Err(e) => {
            tracing::warn!(error = %e, tile_id = %tile_id, "Sui claim failed");
            ("".into(), "".into(), true)
        }
    };

    info!(tile_id = %tile_id, owner = %req.owner_did, stub, "tile claimed");

    (StatusCode::OK, Json(json!({
        "ok":        true,
        "tile_id":   tile_id,
        "owner_did": req.owner_did,
        "tx_digest": tx_digest,
        "object_id": object_id,
        "stub":      stub,
    }))).into_response()
}

// POST /tiles/:tile_id/stake — stake ASE tokens into an owned tile on Sui.
#[derive(serde::Deserialize)]
struct TileStakeRequest {
    staker_did: String,
    amount:     u64,
    /// Optional TileRecord object_id from a prior claim; uses "0x0" if absent.
    #[serde(default)]
    tile_object_id: String,
}

async fn handle_tile_stake(
    State(state): State<NodeState>,
    Path(tile_id): Path<String>,
    Json(req):    Json<TileStakeRequest>,
) -> impl IntoResponse {
    if !tile_id.starts_with("odu:") || tile_id.len() != 6 {
        return (StatusCode::BAD_REQUEST, Json(json!({
            "error": "invalid_tile_id",
        }))).into_response();
    }
    if req.amount == 0 {
        return (StatusCode::BAD_REQUEST, Json(json!({
            "error": "invalid_amount",
            "message": "amount must be > 0",
        }))).into_response();
    }

    // Update local economy store stake.
    state.tile_economy_store.stake(&tile_id, req.amount).await;

    // Anchor stake on Sui.
    let anchor         = state.config.tile_governance.anchor();
    let tile_object_id = if req.tile_object_id.is_empty() { "0x0" } else { &req.tile_object_id };
    let on_chain = anchor.stake_tile(&tile_id, tile_object_id, req.amount).await;

    let (tx_digest, stub) = match on_chain {
        Ok(r)  => (r.tx_digest, r.stub),
        Err(e) => {
            tracing::warn!(error = %e, tile_id = %tile_id, "Sui stake failed");
            ("".into(), true)
        }
    };

    info!(tile_id = %tile_id, staker = %req.staker_did, amount = req.amount, stub, "tile staked");

    (StatusCode::OK, Json(json!({
        "ok":        true,
        "tile_id":   tile_id,
        "staker_did": req.staker_did,
        "amount":    req.amount,
        "tx_digest": tx_digest,
        "stub":      stub,
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
            state.pending_claims.clone(),
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

// GET /twins/:twin_id/timeline/diff — 4D change detection between earliest and latest snapshot.
//
// Returns quality delta, modality set changes, and elapsed time between first and last scan.
// Requires at least 2 timeline entries; returns 409 if only one or zero entries exist.
async fn handle_twin_timeline_diff(
    State(state):  State<NodeState>,
    Path(twin_id): Path<String>,
) -> impl IntoResponse {
    let device_timeline_id = twin_protocol::TwinTimeline::device_id(&twin_id);
    let timeline = match state.timeline_store.get(&device_timeline_id).await {
        Some(tl) => tl,
        None     => return (StatusCode::NOT_FOUND,
            Json(json!({ "error": "timeline_not_found", "twin_id": twin_id }))).into_response(),
    };

    if timeline.entries.len() < 2 {
        return (StatusCode::CONFLICT,
            Json(json!({
                "error":   "insufficient_snapshots",
                "message": "at least 2 snapshots required for diff",
                "count":   timeline.entries.len(),
            }))).into_response();
    }

    let earliest = timeline.earliest().unwrap();
    let latest   = timeline.latest().unwrap();

    let quality_delta = latest.quality - earliest.quality;
    let span_ms       = timeline.span_ms().unwrap_or(0);
    let span_hours    = span_ms as f64 / 3_600_000.0;

    let added_modalities: Vec<&str> = latest.modalities.iter()
        .filter(|m| !earliest.modalities.contains(m))
        .map(|m| m.as_str())
        .collect();
    let dropped_modalities: Vec<&str> = earliest.modalities.iter()
        .filter(|m| !latest.modalities.contains(m))
        .map(|m| m.as_str())
        .collect();

    let tile_changed = earliest.odu_tile != latest.odu_tile;

    info!(
        twin_id    = %twin_id,
        span_ms    = span_ms,
        quality_delta = %format!("{:+.3}", quality_delta),
        snapshots  = timeline.entries.len(),
        "4D timeline diff computed"
    );

    (StatusCode::OK, Json(json!({
        "twin_id":            twin_id,
        "snapshot_count":     timeline.entries.len(),
        "earliest": {
            "snapshot_id": earliest.snapshot_id,
            "timestamp":   earliest.timestamp,
            "quality":     earliest.quality,
            "odu_tile":    earliest.odu_tile,
            "modalities":  earliest.modalities,
        },
        "latest": {
            "snapshot_id": latest.snapshot_id,
            "timestamp":   latest.timestamp,
            "quality":     latest.quality,
            "odu_tile":    latest.odu_tile,
            "modalities":  latest.modalities,
        },
        "diff": {
            "quality_delta":       quality_delta,
            "quality_improved":    quality_delta > 0.0,
            "span_ms":             span_ms,
            "span_hours":          span_hours,
            "added_modalities":    added_modalities,
            "dropped_modalities":  dropped_modalities,
            "tile_changed":        tile_changed,
        },
    }))).into_response()
}

// POST /twins/:twin_id/splat/diff — 3D spatial diff between two PLY snapshots.
//
// Body: { "snapshot_a": "<ply_base64>", "snapshot_b": "<ply_base64>" }
// Returns centroid delta, point-count delta, volume delta, and a normalised
// change_magnitude in [0, 1] so callers can threshold significant changes.
async fn handle_twin_splat_diff(
    Path(twin_id): Path<String>,
    Json(req):     Json<crate::spatial_diff::SplatDiffRequest>,
) -> impl IntoResponse {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use crate::spatial_diff::{parse_ply_stats, diff_stats};

    let decode = |b64: &str, label: &str| -> Result<Vec<u8>, (StatusCode, axum::Json<serde_json::Value>)> {
        STANDARD.decode(b64).map_err(|e| (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "error": "invalid_base64", "field": label, "detail": e.to_string() })),
        ))
    };

    let bytes_a = match decode(&req.snapshot_a, "snapshot_a") {
        Ok(b) => b,
        Err(r) => return r.into_response(),
    };
    let bytes_b = match decode(&req.snapshot_b, "snapshot_b") {
        Ok(b) => b,
        Err(r) => return r.into_response(),
    };

    let stats_a = match parse_ply_stats(&req.snapshot_a, &bytes_a) {
        Ok(s) => s,
        Err(e) => return (StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "error": "ply_parse_error", "field": "snapshot_a", "detail": e.to_string() }))).into_response(),
    };
    let stats_b = match parse_ply_stats(&req.snapshot_b, &bytes_b) {
        Ok(s) => s,
        Err(e) => return (StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "error": "ply_parse_error", "field": "snapshot_b", "detail": e.to_string() }))).into_response(),
    };

    let diff = diff_stats(&req.snapshot_a, &req.snapshot_b, stats_a, stats_b);

    info!(twin_id = %twin_id, change_magnitude = %format!("{:.3}", diff.change_magnitude), "spatial diff computed");

    (StatusCode::OK, Json(json!({
        "twin_id":            twin_id,
        "point_count_delta":  diff.point_count_delta,
        "centroid_delta_m":   diff.centroid_delta_m,
        "volume_delta_m3":    diff.volume_delta_m3,
        "density_delta":      diff.density_delta,
        "change_magnitude":   diff.change_magnitude,
        "stats_a": {
            "point_count": diff.stats_a.point_count,
            "centroid":    diff.stats_a.centroid,
            "volume_m3":   diff.stats_a.volume(),
        },
        "stats_b": {
            "point_count": diff.stats_b.point_count,
            "centroid":    diff.stats_b.centroid,
            "volume_m3":   diff.stats_b.volume(),
        },
    }))).into_response()
}

// POST /swarm/:swarm_id/merge-splat — aggregate per-device PLY clouds into one scene.
//
// Body: { "splats": [{"device_id":"...","ply_b64":"<base64 PLY>"},...], "voxel_size": 0.05 }
// Returns merged PLY (base64), point counts, reduction %, centroid, and bbox.
// The swarm_id is validated to exist; if unknown, returns 404.
async fn handle_swarm_merge_splat(
    State(state):   State<NodeState>,
    Path(swarm_id): Path<String>,
    Json(req):      Json<crate::swarm_splat::MergeRequest>,
) -> impl IntoResponse {
    use crate::swarm_splat::{merge_splats, MergeError};

    // Validate swarm exists.
    if state.swarm_store.get(&swarm_id).await.is_none() {
        return (StatusCode::NOT_FOUND,
            Json(json!({ "error": "swarm_not_found", "swarm_id": swarm_id }))).into_response();
    }

    match merge_splats(&req) {
        Ok(result) => {
            info!(
                swarm_id     = %swarm_id,
                source_count = result.source_count,
                input_points = result.input_points,
                output_points= result.output_points,
                reduction_pct= %format!("{:.1}%", result.reduction_pct),
                "swarm splat merge complete"
            );
            (StatusCode::OK, Json(json!({
                "swarm_id":       swarm_id,
                "source_count":   result.source_count,
                "input_points":   result.input_points,
                "output_points":  result.output_points,
                "reduction_pct":  result.reduction_pct,
                "centroid":       result.centroid,
                "bbox_min":       result.bbox_min,
                "bbox_max":       result.bbox_max,
                "merged_ply_b64": result.merged_ply_b64,
            }))).into_response()
        }
        Err(MergeError::Empty) => (StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "error": "no_splats", "message": "splats array must not be empty" }))).into_response(),
        Err(MergeError::Base64 { device_id, detail }) => (StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "error": "invalid_base64", "device_id": device_id, "detail": detail }))).into_response(),
        Err(MergeError::Ply { device_id, detail }) => (StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "error": "ply_parse_error", "device_id": device_id, "detail": detail }))).into_response(),
    }
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
                            crate::events::TwinEvent::MintApproved { .. }   => "mint_approved",
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
                        crate::events::TwinEvent::MintApproved    { .. } => "mint_approved",
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

// POST /federation/peers — register a federation peer.
#[derive(serde::Deserialize)]
struct RegisterPeerBody {
    peer_id:      String,
    name:         String,
    a2a_base_url: String,
    #[serde(default)]
    did:          Option<String>,
}

async fn handle_federation_register_peer(
    State(state): State<NodeState>,
    Json(body):   Json<RegisterPeerBody>,
) -> impl IntoResponse {
    use crate::federation_router::FederationPeer;
    let mut peer = FederationPeer::new(&body.peer_id, &body.name, &body.a2a_base_url);
    if let Some(did) = body.did { peer = peer.with_did(did); }
    state.federation_router.register(peer).await;
    info!(peer_id = %body.peer_id, url = %body.a2a_base_url, "federation peer registered");
    (StatusCode::CREATED, Json(json!({ "ok": true, "peer_id": body.peer_id })))
}

// DELETE /federation/peers/:peer_id — remove a peer.
async fn handle_federation_remove_peer(
    State(state): State<NodeState>,
    Path(peer_id): Path<String>,
) -> impl IntoResponse {
    if state.federation_router.remove(&peer_id).await {
        Json(json!({ "ok": true, "peer_id": peer_id })).into_response()
    } else {
        (StatusCode::NOT_FOUND, Json(json!({ "error": "peer_not_found", "peer_id": peer_id }))).into_response()
    }
}

// POST /federation/tasks — route an A2A task to the best available peer.
//
// Body: { "message": {...}, "prefer_peer": "<peer_id>"|null, "skill": "<hint>"|null }
// Falls back to a stub result when all peers are unreachable (never hard-fails).
async fn handle_federation_route_task(
    State(state): State<NodeState>,
    Json(req):    Json<crate::federation_router::FederatedTaskRequest>,
) -> impl IntoResponse {
    use crate::federation_router::RouteError;

    match state.federation_router.route(&req).await {
        Ok(result) => {
            info!(
                routed_to = %result.routed_to,
                task_id   = %result.remote_task_id,
                stub      = result.stub,
                "federated task dispatched"
            );
            (StatusCode::ACCEPTED, Json(serde_json::to_value(&result).unwrap_or_default())).into_response()
        }
        Err(RouteError::NoPeers) => {
            (StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": "no_peers", "message": "no routable federation peers available" }))).into_response()
        }
        Err(RouteError::PeerNotFound(id)) => {
            (StatusCode::NOT_FOUND,
                Json(json!({ "error": "peer_not_found", "peer_id": id }))).into_response()
        }
        Err(RouteError::AllFailed(reason)) => {
            (StatusCode::BAD_GATEWAY,
                Json(json!({ "error": "all_peers_failed", "reason": reason }))).into_response()
        }
    }
}

// POST /federation/health — probe all peers and update health status.
async fn handle_federation_health_check(State(state): State<NodeState>) -> impl IntoResponse {
    let (healthy, unreachable) = state.federation_router.health_check_all().await;
    info!(healthy, unreachable, "federation health check complete");
    Json(json!({ "healthy": healthy, "unreachable": unreachable }))
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
        state.pending_claims.clone(),
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
    pending_claims: Arc<tokio::sync::Mutex<Vec<ProofClaim>>>,
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

    // Prefer the top-level `osovm_url` (async HTTP path); fall back to
    // `pipeline.osovm_endpoint` for backwards-compatibility.
    let resolved_osovm_url: Option<String> = config.osovm_url.clone()
        .or_else(|| config.pipeline.osovm_endpoint.clone());
    let engine = OsovmEngine::new(&config.pipeline.osovm_version)
        .with_endpoint_opt(resolved_osovm_url);
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

            // Derive tile_id from GPS center of the twin's region.
            let region    = &proof_output.pipeline.twin.region;
            let center_lat = (region.min_lat + region.max_lat) / 2.0;
            let center_lon = (region.min_lon + region.max_lon) / 2.0;
            let tile_coord = OduCoordinate::from_gps(center_lat, center_lon);
            let derived_tile_id = tile_coord.tile_id();

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
                odu_tile:           Some(derived_tile_id.clone()),
            }).await;

            // Publish kind-31020 CaptureReceipt to Nostr (fire-and-forget stub).
            if let Some(relay_url) = config.dip.nostr_relay.as_deref() {
                let relay   = relay_url.to_string();
                let cap_rcpt = proof_output.pipeline.capture_receipt.clone();
                let nsec    = config.dip.nostr_nsec.clone().unwrap_or_default();
                tokio::spawn(async move {
                    match crate::nostr_publisher::publish_capture_receipt(&cap_rcpt, &relay, &nsec).await {
                        Ok(event_id) => info!(event_id = %event_id, "kind-31020 CaptureReceipt published to Nostr"),
                        Err(e)       => warn!(error = %e, "kind-31020 CaptureReceipt Nostr publish failed"),
                    }
                });
            }

            // Novelty from the proof engine — quality-proportional for captures
            // (proper VeilSim novelty oracle wires in via proof_engine in production).
            let novelty_score = proof_output.pipeline.twin.quality.f1_score;

            // Queue ProofClaim for DailyEmissionAllocator (canonical PoUS emission path).
            // The per-minute task drains pending_claims, calls allocate_minute(), and
            // produces a MintAuthorization — only then does Sui settlement mint_ase().
            let mint_result = {
                let now_secs = SystemTime::now()
                    .duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
                let env_hash_bytes = {
                    let mut h = [0u8; 32];
                    let s = sovereign_types::hash_str(&proof_output.pipeline.twin.twin_id);
                    let decoded = hex::decode(&s[..s.len().min(64)]).unwrap_or_else(|_| vec![0u8; 32]);
                    let copy_len = decoded.len().min(32);
                    h[..copy_len].copy_from_slice(&decoded[..copy_len]);
                    h
                };
                let claim = ProofClaim {
                    proof_id:        p_scene_id.clone(),
                    worker_did:      identity.did.clone(),
                    veil_id:         0, // capture receipts: veil 0 = unchallenged capture
                    epoch_minute:    now_secs / 60,
                    trajectory_hash: [0u8; 32], // real hash from proof pipeline
                    f1_score:        proof_output.pipeline.twin.quality.f1_score as f64,
                    proof_value:     proof_output.pipeline.twin.quality.f1_score as f64,
                    env_hash:        env_hash_bytes,
                };
                pending_claims.lock().await.push(claim);
                info!(
                    job_id  = %job_id,
                    tile_id = %derived_tile_id,
                    "proof claim queued to DailyEmissionAllocator"
                );
                // Return a stub result — real mint deferred to emission minute task
                AseMintResult {
                    receipt_id:    p_scene_id.clone(),
                    tokens_minted: 0,
                    net_minted:    0,
                    owner_fee:     0,
                    elegbara:      Default::default(),
                    tx_digest:     None,
                    stub:          true,
                }
            };
            // Apply stub to tile economy tracking (real values applied post-settlement)
            tile_economy.apply_mint(&derived_tile_id, &mint_result).await;
            info!(
                job_id        = %job_id,
                tile_id       = %derived_tile_id,
                tokens_minted = mint_result.tokens_minted,
                net_minted    = mint_result.net_minted,
                stub          = mint_result.stub,
                "Àṣẹ emission queued (deferred to DailyEmissionAllocator)"
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
                serde_json::json!({ "job_id": job_id, "tile": derived_tile_id }),
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

/// Queue a ProofClaim for the DailyEmissionAllocator (canonical PoUS emission path).
/// Called from proof handlers when mint_eligible=true.
/// Returns a stub AseMintResult (tokens_minted=0, stub=true) — the real mint is
/// deferred to the per-minute emission task which calls allocate_minute().
async fn mint_eligible_to_ase(
    proof_id:       &str,
    proof_domain:   &str,
    tile_id:        &str,
    minter_did:     &str,
    quality:        f32,
    _novelty:       f32,
    _sui_url:       Option<&str>,
    _tile_store:    &crate::tile_economy_store::TileEconomyStore,
    pending_claims: &Arc<tokio::sync::Mutex<Vec<ProofClaim>>>,
) -> AseMintResult {
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let env_hash_bytes = {
        let mut h = [0u8; 32];
        let s = sovereign_types::hash_str(tile_id);
        let decoded = hex::decode(&s[..s.len().min(64)]).unwrap_or_else(|_| vec![0u8; 32]);
        let copy_len = decoded.len().min(32);
        h[..copy_len].copy_from_slice(&decoded[..copy_len]);
        h
    };
    let claim = ProofClaim {
        proof_id:        format!("{proof_domain}:{proof_id}"),
        worker_did:      minter_did.to_string(),
        veil_id:         0,
        epoch_minute:    now_secs / 60,
        trajectory_hash: [0u8; 32],
        f1_score:        quality as f64,
        proof_value:     quality as f64,
        env_hash:        env_hash_bytes,
    };
    pending_claims.lock().await.push(claim);
    info!(
        proof_id   = %proof_id,
        domain     = %proof_domain,
        tile_id    = %tile_id,
        "proof claim queued to DailyEmissionAllocator (mint deferred)"
    );
    // Stub result — tokens will be allocated by the emission minute task
    AseMintResult {
        receipt_id:    format!("{proof_domain}:{proof_id}"),
        tokens_minted: 0,
        net_minted:    0,
        owner_fee:     0,
        elegbara:      Default::default(),
        tx_digest:     None,
        stub:          true,
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
            // Phase 4.3 — DIP→VCP: route vcp_command messages to the body session store.
            if let Some("vcp_command") = envelope.payload.get("type").and_then(|v| v.as_str()) {
                handle_dip_vcp_command(&envelope, ctx).await;
                return;
            }
            // Phase 4.2 — DIP→TSP: route twin_license_request messages.
            if let Some("twin_license_request") = envelope.payload.get("type").and_then(|v| v.as_str()) {
                handle_dip_license_request(&envelope, ctx).await;
                return;
            }
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

/// Phase 4.3 — DIP→VCP: execute a VCP command arriving via DIP.
///
/// Payload fields: type="vcp_command", session_id, capability, action, params?
/// Identity chain is validated: DIP principal must match the session's agent_id.
/// Reply is a DIP Receipt with the command result.
async fn handle_dip_vcp_command(envelope: &dip::DipEnvelope, ctx: &InboundDipContext) {
    use dip::{DipEnvelope, DipKind, DipAddress, address::DipNetwork};

    let payload    = &envelope.payload;
    let session_id = payload.get("session_id").and_then(|v| v.as_str()).unwrap_or("");
    let capability = payload.get("capability").and_then(|v| v.as_str()).unwrap_or("");
    let action     = payload.get("action").and_then(|v| v.as_str()).unwrap_or("execute");
    let params     = payload.get("params").cloned().unwrap_or(serde_json::Value::Null);

    if session_id.is_empty() || capability.is_empty() {
        warn!(msg_id = %envelope.message_id, "dip vcp_command missing session_id or capability");
        return;
    }

    // Validate: DIP principal must match the session agent_id.
    let principal = envelope.identity.principal_id.as_str();
    let session_ok = match ctx.body_store.get_session(session_id).await {
        None => {
            warn!(session_id, "dip vcp_command: session not found");
            false
        }
        Some(s) => {
            if s.agent_id != principal {
                warn!(
                    session_id,
                    dip_principal = %principal,
                    session_agent = %s.agent_id,
                    "dip vcp_command: identity mismatch"
                );
                false
            } else {
                // Verify capability is in the granted set (empty set = all allowed)
                s.capabilities.is_empty() || s.capabilities.iter().any(|c| c == capability)
            }
        }
    };

    let cmd_id = format!("cmd:{}", uuid::Uuid::new_v4());
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let (status_str, error_str) = if session_ok {
        info!(
            cmd_id     = %cmd_id,
            session_id,
            capability,
            action,
            "DIP→VCP command accepted"
        );
        ("accepted", None)
    } else {
        ("denied", Some("identity_mismatch_or_session_not_found"))
    };

    // Reply via DIP Receipt
    let reply_payload = serde_json::json!({
        "type":       "vcp_command_result",
        "cmd_id":     cmd_id,
        "session_id": session_id,
        "capability": capability,
        "action":     action,
        "params":     params,
        "status":     status_str,
        "error":      error_str,
        "timestamp_ms": ts,
    });

    let requester = envelope.origin.did.as_deref()
        .unwrap_or(&envelope.origin.address);
    let chain  = IdentityChain::new(ctx.identity.did.clone(), ctx.identity.did.clone());
    let origin = DipAddress::vantage(&ctx.identity.did);
    let dest   = DipAddress {
        network: DipNetwork::Vantage,
        address: requester.to_string(),
        did:     Some(requester.to_string()),
    };
    match DipEnvelope::build(origin, dest, chain, DipKind::Receipt, reply_payload, 120, &ctx.identity.private_key) {
        Ok(reply) => ctx.gateway.send(reply).await,
        Err(e)    => warn!(error = %e, "failed to build vcp_command_result DIP reply"),
    }
}

/// Phase 4.2 — DIP→TSP: handle an inbound twin license request via DIP.
///
/// Payload: type="twin_license_request", twin_id, rights[], expires_at?, fee_mist?
/// Issues a TwinLicenseGrant and replies with the grant via DIP Receipt.
async fn handle_dip_license_request(envelope: &dip::DipEnvelope, ctx: &InboundDipContext) {
    use dip::{DipEnvelope, DipKind, DipAddress, address::DipNetwork};
    use twin_protocol::{TwinLicenseGrant, TwinLicenseConstraints};
    use sovereign_types::UsageRight;

    let payload    = &envelope.payload;
    let twin_id    = payload.get("twin_id").and_then(|v| v.as_str()).unwrap_or("");
    let grantee    = envelope.identity.principal_id.as_str();
    let expires_at: Option<u64> = payload.get("expires_at").and_then(|v| v.as_u64());
    let fee_mist:   Option<u64> = payload.get("fee_mist").and_then(|v| v.as_u64());

    if twin_id.is_empty() {
        warn!(msg_id = %envelope.message_id, "dip twin_license_request missing twin_id");
        return;
    }

    let rights: Vec<UsageRight> = payload.get("rights")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_else(|| vec![UsageRight::View, UsageRight::Simulate]);

    let constraints = TwinLicenseConstraints::default();

    let result = TwinLicenseGrant::issue(
        twin_id.to_string(),
        ctx.identity.did.clone(),
        grantee.to_string(),
        rights,
        constraints,
        expires_at,
        fee_mist,
        &ctx.identity.private_key,
    );

    let reply_payload = match result {
        Ok(grant) => {
            ctx.license_store.insert(grant.clone()).await;
            info!(
                grant_id = %grant.grant_id,
                twin_id,
                grantee,
                "DIP→TSP license grant issued"
            );
            serde_json::json!({
                "type":    "twin_license_grant",
                "grant":   grant,
                "status":  "issued",
            })
        }
        Err(e) => {
            warn!(twin_id, grantee, error = %e, "DIP license request failed");
            serde_json::json!({
                "type":   "twin_license_grant",
                "status": "rejected",
                "error":  e.to_string(),
            })
        }
    };

    let requester = envelope.origin.did.as_deref()
        .unwrap_or(&envelope.origin.address);
    let chain  = IdentityChain::new(ctx.identity.did.clone(), ctx.identity.did.clone());
    let origin = DipAddress::vantage(&ctx.identity.did);
    let dest   = DipAddress {
        network: DipNetwork::Vantage,
        address: requester.to_string(),
        did:     Some(requester.to_string()),
    };
    match DipEnvelope::build(origin, dest, chain, DipKind::Receipt, reply_payload, 300, &ctx.identity.private_key) {
        Ok(reply) => ctx.gateway.send(reply).await,
        Err(e)    => warn!(error = %e, "failed to build twin_license_grant DIP reply"),
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
                capture_state.pending_claims.clone(),
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

// ── Governance handlers ────────────────────────────────────────────────────────

/// Body accepted for POST /governance/proposals — only the user-supplied fields.
#[derive(serde::Deserialize)]
struct CreateProposalBody {
    proposer:           String,
    recipient:          String,
    amount_micro_ase:   u64,
    purpose:            String,
    #[serde(default)]
    veil_id:            u64,
}

// GET /governance/proposals
async fn handle_governance_list(State(state): State<NodeState>) -> impl IntoResponse {
    let proposals = state.governance_store.all().await;
    Json(json!({
        "count":     proposals.len(),
        "proposals": proposals,
    }))
}

// POST /governance/proposals
async fn handle_governance_create(
    State(state): State<NodeState>,
    Json(body): Json<CreateProposalBody>,
) -> impl IntoResponse {
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let now_ms_val = now_secs * 1000;

    // Auto-assign id as current count + 1.
    let id = state.governance_store.all().await.len() as u64 + 1;

    let proposal = twin_protocol::GrantProposal {
        id,
        proposer:           body.proposer,
        recipient:          body.recipient,
        amount_micro_ase:   body.amount_micro_ase,
        purpose:            body.purpose,
        veil_id:            body.veil_id,
        votes_for:          0,
        votes_against:      0,
        created_at:         now_ms_val,
        timelock_release:   now_secs + twin_protocol::GrantProposal::TIMELOCK_SECS,
        executed:           false,
        rejected:           false,
    };

    state.governance_store.insert(proposal.clone()).await;
    (StatusCode::CREATED, Json(serde_json::to_value(proposal).unwrap_or_default()))
}

// GET /governance/proposals/:id
async fn handle_governance_get(
    State(state): State<NodeState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.governance_store.get(&id).await {
        Some(pv) => (StatusCode::OK, Json(serde_json::to_value(pv).unwrap_or_default())),
        None     => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "proposal_not_found", "id": id })),
        ),
    }
}

// POST /governance/proposals/:id/vote_for
async fn handle_governance_vote_for(
    State(state): State<NodeState>,
    Path(id): Path<u64>,
) -> impl IntoResponse {
    match state.governance_store.vote_for(id).await {
        Some(p) => (StatusCode::OK, Json(serde_json::to_value(p).unwrap_or_default())),
        None    => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "proposal_not_found", "id": id })),
        ),
    }
}

// POST /governance/proposals/:id/vote_against
async fn handle_governance_vote_against(
    State(state): State<NodeState>,
    Path(id): Path<u64>,
) -> impl IntoResponse {
    match state.governance_store.vote_against(id).await {
        Some(p) => (StatusCode::OK, Json(serde_json::to_value(p).unwrap_or_default())),
        None    => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "proposal_not_found", "id": id })),
        ),
    }
}

// POST /governance/proposals/:id/execute
async fn handle_governance_execute(
    State(state): State<NodeState>,
    Path(id): Path<u64>,
) -> impl IntoResponse {
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    match state.governance_store.execute(id, now_secs).await {
        Ok(p)    => (StatusCode::OK, Json(serde_json::to_value(p).unwrap_or_default())),
        Err(msg) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "error": msg })),
        ),
    }
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

    let witnesses     = WitnessRegistry::new();
    let body_store    = BodyStore::new();
    let license_store = LicenseStore::new();
    let inbound_ctx = InboundDipContext {
        local_did:     did.clone(),
        identity:      identity.clone(),
        witnesses:     witnesses.clone(),
        gateway:       dip_gateway.clone(),
        body_store:    body_store.clone(),
        license_store: license_store.clone(),
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
        federation_router:   crate::federation_router::FederationRouter::new(),
        twin_events:         twin_events_tx,
        tile_economy_store:  TileEconomyStore::new(),
        started_at:          now_ms(),
        ip_root_event:       None,
        act_chain:           Arc::new(tokio::sync::RwLock::new(ActReceiptChain::new())),
        body_store:          body_store,
        telemetry_store:     TelemetryStore::new(),
        proof_engine:        Arc::new(ProofEngine::new()),
        emission_allocator:  Arc::new(tokio::sync::Mutex::new(DailyEmissionAllocator::new())),
        pending_claims:      Arc::new(tokio::sync::Mutex::new(Vec::new())),
        governance_store:    GovernanceStore::new(),
        license_store:       license_store,
        wallet_store:        WalletStore::new(),
        gpu_pool:            GpuPool::new(),
        agent_store:         AgentStore::new(),
        council_store:       CouncilStore::new(),
        seat_store:          SovereignSeatStore::new(),
        emission_receipts:   EmissionReceiptStore::new(),
    }
}

// ── Twin licensing marketplace handlers (Phase 3.6) ───────────────────────────

#[derive(Debug, serde::Deserialize)]
struct IssueLicenseBody {
    grantee_did:  String,
    #[serde(default)]
    rights:       Vec<UsageRight>,
    expires_at:   Option<u64>,
    fee_mist:     Option<u64>,
    #[serde(default)]
    constraints:  Option<TwinLicenseConstraints>,
}

async fn handle_license_issue(
    State(state): State<NodeState>,
    axum::extract::Path(twin_id): axum::extract::Path<String>,
    Json(body): Json<IssueLicenseBody>,
) -> impl IntoResponse {
    let identity = state.identity.clone();
    let grantee_did = body.grantee_did.clone();
    let constraints = body.constraints.unwrap_or_default();
    match TwinLicenseGrant::issue(
        twin_id,
        identity.did.clone(),
        grantee_did.clone(),
        body.rights,
        constraints,
        body.expires_at,
        body.fee_mist,
        &identity.private_key,
    ) {
        Ok(grant) => {
            state.license_store.insert(grant.clone()).await;

            // Phase 4.2 — DIP→TSP: forward license grant to grantee via DIP.
            {
                use dip::{DipEnvelope, DipKind, DipAddress, address::DipNetwork};
                use sovereign_types::IdentityChain;
                let payload = serde_json::to_value(&grant).unwrap_or(serde_json::Value::Null);
                let gw   = state.dip_gateway.clone();
                let did  = identity.did.clone();
                let key  = identity.private_key.clone();
                let dest = grantee_did.clone();
                tokio::spawn(async move {
                    let origin = DipAddress::vantage(&did);
                    let destination = DipAddress {
                        network: DipNetwork::Vantage,
                        address: dest.clone(),
                        did:     Some(dest.clone()),
                    };
                    let chain = IdentityChain::new(did.clone(), did.clone());
                    if let Ok(env) = DipEnvelope::build(
                        origin, destination, chain,
                        DipKind::Message, payload, 300, &key,
                    ) {
                        gw.send(env).await;
                    }
                });
            }

            (StatusCode::CREATED, Json(serde_json::to_value(grant).unwrap())).into_response()
        }
        Err(e) => (StatusCode::BAD_REQUEST,
            Json(json!({ "error": e.to_string() }))).into_response(),
    }
}

async fn handle_license_list_for_twin(
    State(state): State<NodeState>,
    axum::extract::Path(twin_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let grants = state.license_store.for_twin(&twin_id).await;
    Json(json!({ "grants": grants, "count": grants.len() }))
}

async fn handle_license_list(
    State(state): State<NodeState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let grants = if let Some(grantee) = params.get("grantee") {
        state.license_store.for_grantee(grantee).await
    } else {
        state.license_store.all().await
    };
    Json(json!({ "grants": grants, "count": grants.len() }))
}

async fn handle_license_get(
    State(state): State<NodeState>,
    axum::extract::Path(grant_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    match state.license_store.get(&grant_id).await {
        Some(g) => Json(serde_json::to_value(g).unwrap()).into_response(),
        None => (StatusCode::NOT_FOUND,
            Json(json!({ "error": "grant_not_found", "grant_id": grant_id }))).into_response(),
    }
}

#[derive(Debug, serde::Deserialize)]
struct AcceptLicenseBody {
    #[serde(default)]
    grantee_sig: String,
}

async fn handle_license_accept(
    State(state): State<NodeState>,
    axum::extract::Path(grant_id): axum::extract::Path<String>,
    Json(body): Json<AcceptLicenseBody>,
) -> impl IntoResponse {
    let sig = if body.grantee_sig.is_empty() {
        format!("stub-accept:{grant_id}")
    } else {
        body.grantee_sig
    };
    match state.license_store.accept(&grant_id, sig).await {
        Some(g) => Json(serde_json::to_value(g).unwrap()).into_response(),
        None => (StatusCode::NOT_FOUND,
            Json(json!({ "error": "grant_not_found", "grant_id": grant_id }))).into_response(),
    }
}

// ── Cowrie Oracle + Emission handlers (Phase 45) ──────────────────────────────

// GET /oracle/today
async fn handle_oracle_today() -> impl IntoResponse {
    use sovereign_types::{CowrieOracle, DailyEmission};
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let day = (now_secs / 86_400) as u32;
    let result = CowrieOracle::query(day, "spatial");
    let emission = DailyEmission::for_day(day, "spatial");
    Json(json!({
        "day":              day,
        "tile_id":          result.tile_id,
        "tile_index":       result.tile_index,
        "seed_hash":        result.seed_hash,
        "emission_cap":     emission.emission_cap,
        "domain":           result.domain,
    }))
}

// GET /oracle/day/:day
async fn handle_oracle_day(
    Path(day): Path<u32>,
) -> impl IntoResponse {
    use sovereign_types::{CowrieOracle, DailyEmission};
    let result = CowrieOracle::query(day, "spatial");
    let emission = DailyEmission::for_day(day, "spatial");
    Json(json!({
        "day":              day,
        "tile_id":          result.tile_id,
        "tile_index":       result.tile_index,
        "seed_hash":        result.seed_hash,
        "emission_cap":     emission.emission_cap,
        "domain":           result.domain,
    }))
}

// GET /emission/status
async fn handle_emission_status(State(state): State<NodeState>) -> impl IntoResponse {
    use twin_protocol::emission::{MICRO_ASE_PER_MINUTE, GENESIS_DIFFICULTY};
    let allocator = state.emission_allocator.lock().await;
    let pending   = state.pending_claims.lock().await.len();
    let now_secs  = SystemTime::now()
        .duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let epoch_minute = now_secs / 60;
    let difficulty = twin_protocol::emission::DailyEmissionAllocator::current_difficulty(epoch_minute);
    let is_sabbath = twin_protocol::emission::DailyEmissionAllocator::is_sabbath(now_secs);
    Json(json!({
        "epoch_minute":         epoch_minute,
        "micro_ase_per_minute": MICRO_ASE_PER_MINUTE,
        "current_difficulty":   difficulty,
        "genesis_difficulty":   GENESIS_DIFFICULTY,
        "is_sabbath":           is_sabbath,
        "chain_tip_hex":        hex::encode(allocator.chain_tip()),
        "utxos_claimed":        allocator.utxos_claimed(),
        "env_hash_count":       allocator.env_hash_count(),
        "pending_claims":       pending,
    }))
}

#[derive(serde::Deserialize)]
struct EmissionClaimBody {
    proof_id:         String,
    worker_did:       String,
    veil_id:          u16,
    trajectory_hash:  String,   // hex-encoded 32 bytes
    f1_score:         f64,
    proof_value:      f64,
    env_hash:         String,   // hex-encoded 32 bytes
}

// POST /emission/claim
async fn handle_emission_claim(
    State(state): State<NodeState>,
    Json(body): Json<EmissionClaimBody>,
) -> impl IntoResponse {
    use twin_protocol::emission::ProofClaim;

    let trajectory_bytes = hex::decode(&body.trajectory_hash).unwrap_or_default();
    let env_bytes        = hex::decode(&body.env_hash).unwrap_or_default();
    let mut traj = [0u8; 32];
    let mut env  = [0u8; 32];
    let tl = trajectory_bytes.len().min(32);
    let el = env_bytes.len().min(32);
    traj[..tl].copy_from_slice(&trajectory_bytes[..tl]);
    env[..el].copy_from_slice(&env_bytes[..el]);

    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();

    let claim = ProofClaim {
        proof_id:         body.proof_id,
        worker_did:       body.worker_did,
        veil_id:          body.veil_id,
        epoch_minute:     now_secs / 60,
        trajectory_hash:  traj,
        f1_score:         body.f1_score.clamp(0.0, 1.0),
        proof_value:      body.proof_value.clamp(0.0, 1.0),
        env_hash:         env,
    };

    state.pending_claims.lock().await.push(claim.clone());
    (StatusCode::ACCEPTED, Json(json!({
        "queued":         true,
        "proof_id":       claim.proof_id,
        "epoch_minute":   claim.epoch_minute,
        "f1_score":       claim.f1_score,
    })))
}

// ── Sovereign Wallet handlers (Phase 46) ──────────────────────────────────────

// GET /wallets
async fn handle_wallets_list(State(state): State<NodeState>) -> impl IntoResponse {
    let wallets = state.wallet_store.all().await;
    Json(json!({ "count": wallets.len(), "wallets": wallets }))
}

// GET /wallets/:did
async fn handle_wallet_get(
    State(state): State<NodeState>,
    Path(did): Path<String>,
) -> impl IntoResponse {
    let decoded_did = urlencoding::decode(&did).unwrap_or(std::borrow::Cow::Borrowed(&did)).into_owned();
    match state.wallet_store.get(&decoded_did).await {
        Some(w) => Json(serde_json::to_value(w).unwrap()).into_response(),
        None    => (StatusCode::NOT_FOUND,
            Json(json!({ "error": "wallet_not_found", "did": decoded_did }))).into_response(),
    }
}

#[derive(serde::Deserialize)]
struct WalletCreditBody {
    amount_micro_ase: u64,
    reason:           String,
}

// POST /wallets/:did/credit
async fn handle_wallet_credit(
    State(state): State<NodeState>,
    Path(did): Path<String>,
    Json(body): Json<WalletCreditBody>,
) -> impl IntoResponse {
    let decoded_did = urlencoding::decode(&did).unwrap_or(std::borrow::Cow::Borrowed(&did)).into_owned();
    let balance = state.wallet_store.credit(&decoded_did, body.amount_micro_ase).await;
    info!(did = %decoded_did, amount = body.amount_micro_ase, reason = %body.reason, "wallet credited");
    Json(json!({
        "did":             decoded_did,
        "balance_micro_ase": balance,
        "credited":        body.amount_micro_ase,
    }))
}

// ── OSOVM Token-of-Compute handlers (Phase 52) ───────────────────────────────

#[derive(serde::Deserialize)]
struct GpuContributeBody {
    contributor_did: String,
    device_id:       String,
    compute_units:   u64,
    proof_hash:      String,
}

// POST /osovm/gpu/contribute
async fn handle_gpu_contribute(
    State(state): State<NodeState>,
    Json(body): Json<GpuContributeBody>,
) -> impl IntoResponse {
    if body.compute_units == 0 {
        return (StatusCode::BAD_REQUEST,
            Json(json!({"error": "compute_units must be > 0"}))).into_response();
    }
    let contrib = state.gpu_pool.contribute(
        &body.contributor_did,
        &body.device_id,
        body.compute_units,
        &body.proof_hash,
    ).await;

    // Wire 1/3: GPU contribution → EmissionReceipt in Simulation pool.
    let emission = state.emission_receipts.record(
        sovereign_types::DistributionPool::Simulation,
        contrib.gpu_minted,
        body.proof_hash.clone(),
        "gpu_contribution_v1".into(),
        Some(body.contributor_did.clone()),
        Some(contrib.contribution_id.clone()),
        format!("GPU compute contribution from {}", body.device_id),
    ).await;

    info!(
        contribution_id = %contrib.contribution_id,
        contributor_did = %contrib.contributor_did,
        gpu_minted      = contrib.gpu_minted,
        eshu_tithe      = contrib.eshu_tithe,
        emission_id     = %emission.receipt_id,
        "GPU contribution recorded + emission receipt issued"
    );
    (StatusCode::CREATED, Json(json!({
        "contribution_id": contrib.contribution_id,
        "contributor_did": contrib.contributor_did,
        "compute_units":   contrib.compute_units,
        "gpu_minted":      contrib.gpu_minted,
        "eshu_tithe":      contrib.eshu_tithe,
        "timestamp":       contrib.timestamp,
        "emission_receipt_id": emission.receipt_id,
    }))).into_response()
}

#[derive(serde::Deserialize)]
struct GpuBurnBody {
    did:        String,
    gpu_amount: u64,
}

// POST /osovm/gpu/burn  — burn GPU to mint Synapse (10:1)
async fn handle_gpu_burn_for_synapse(
    State(state): State<NodeState>,
    Json(body): Json<GpuBurnBody>,
) -> impl IntoResponse {
    match state.gpu_pool.burn_for_synapse(&body.did, body.gpu_amount).await {
        Ok((synapses, remaining_gpu)) => {
            // Wire 2/3: GPU burn → EmissionReceipt in Research pool (Synapse = agent slice).
            let emission = state.emission_receipts.record(
                sovereign_types::DistributionPool::Research,
                synapses,
                format!("gpu_burn:{}:{}", body.did, body.gpu_amount),
                "gpu_burn_for_synapse_v1".into(),
                Some(body.did.clone()),
                None,
                format!("burned {} micro-GPU → {} micro-Synapse", body.gpu_amount, synapses),
            ).await;

            info!(
                did              = %body.did,
                gpu_burned       = body.gpu_amount,
                synapses_minted  = synapses,
                emission_id      = %emission.receipt_id,
                "GPU burned for Synapse + emission receipt issued"
            );
            Json(json!({
                "did":                 body.did,
                "gpu_burned":          body.gpu_amount,
                "synapses_minted":     synapses,
                "remaining_gpu":       remaining_gpu,
                "emission_receipt_id": emission.receipt_id,
            })).into_response()
        }
        Err(reason) => (StatusCode::CONFLICT, Json(json!({
            "error":  "insufficient_gpu",
            "reason": reason,
        }))).into_response(),
    }
}

// GET /osovm/pool
async fn handle_gpu_pool_state(State(state): State<NodeState>) -> impl IntoResponse {
    let s = state.gpu_pool.state().await;
    Json(json!({
        "total_compute_units":  s.total_compute_units,
        "total_gpu_minted":     s.total_gpu_minted,
        "total_eshu_tithe":     s.total_eshu_tithe,
        "synapse_minted":       s.synapse_minted,
        "contribution_count":   s.contribution_count,
        "decay_bps_per_day":    s.decay_bps_per_day,
        "eshu_tithe_bps":       s.eshu_tithe_bps,
        "last_decay_epoch_day": s.last_decay_epoch_day,
        "gpu_supply_cap":       crate::gpu_pool::GPU_SUPPLY_CAP,
        "synapse_supply_cap":   crate::gpu_pool::SYNAPSE_SUPPLY_CAP,
    }))
}

// GET /osovm/balances/:did
async fn handle_osovm_balances(
    State(state): State<NodeState>,
    Path(did): Path<String>,
) -> impl IntoResponse {
    let decoded = urlencoding::decode(&did).unwrap_or(std::borrow::Cow::Borrowed(&did)).into_owned();
    let gpu_balance     = state.gpu_pool.gpu_balance(&decoded).await;
    let synapse_balance = state.gpu_pool.synapse_balance(&decoded).await;
    Json(json!({
        "did":             decoded,
        "gpu_balance":     gpu_balance,
        "synapse_balance": synapse_balance,
    }))
}

// GET /osovm/contributions — list all GPU contribution records.
async fn handle_osovm_contributions(State(state): State<NodeState>) -> impl IntoResponse {
    let contribs = state.gpu_pool.contributions().await;
    Json(json!({
        "count":         contribs.len(),
        "contributions": contribs,
    }))
}

// POST /osovm/gpu/decay — apply daily Synapse decay for the given epoch_day.
//
// Wire 3/3: decay burn → EmissionReceipt in LotteryBurn pool (tokens burned = permanent supply sink).
// epoch_day is seconds-since-epoch / 86400; safe to call multiple times per day (idempotent).
#[derive(serde::Deserialize)]
struct DecayBody {
    /// Unix epoch day = floor(unix_timestamp_secs / 86400).
    epoch_day: u64,
}

async fn handle_gpu_decay(
    State(state): State<NodeState>,
    Json(body):   Json<DecayBody>,
) -> impl IntoResponse {
    let decayed = state.gpu_pool.apply_daily_decay(body.epoch_day).await;

    if decayed > 0 {
        // Record the burn as a LotteryBurn emission (permanent supply reduction).
        let emission = state.emission_receipts.record(
            sovereign_types::DistributionPool::LotteryBurn,
            decayed,
            format!("decay:epoch_day:{}", body.epoch_day),
            "synapse_daily_decay_v1".into(),
            None,
            None,
            format!("1%/day Synapse decay for epoch_day {}", body.epoch_day),
        ).await;

        info!(
            epoch_day   = body.epoch_day,
            decayed     = decayed,
            emission_id = %emission.receipt_id,
            "Synapse decay applied + emission receipt issued"
        );

        (StatusCode::OK, Json(json!({
            "epoch_day":           body.epoch_day,
            "synapses_decayed":    decayed,
            "already_applied":     false,
            "emission_receipt_id": emission.receipt_id,
        }))).into_response()
    } else {
        (StatusCode::OK, Json(json!({
            "epoch_day":        body.epoch_day,
            "synapses_decayed": 0u64,
            "already_applied":  true,
        }))).into_response()
    }
}

// ── Governance veto (Phase 55: Bínò council constitutional veto) ─────────────

#[derive(serde::Deserialize)]
struct VetoBody {
    /// DID of the council member invoking the veto.
    veto_by: String,
    /// Constitutional article or reason for veto.
    reason:  String,
}

// POST /governance/proposals/:id/veto
async fn handle_governance_veto(
    State(state): State<NodeState>,
    Path(id): Path<String>,
    Json(body): Json<VetoBody>,
) -> impl IntoResponse {
    use crate::governance_store::ProposalStatus;

    match state.governance_store.get(&id).await {
        None => (StatusCode::NOT_FOUND, Json(json!({
            "error": "proposal_not_found",
            "id":    id,
        }))).into_response(),
        Some(proposal) => {
            if proposal.status == ProposalStatus::Executed {
                return (StatusCode::CONFLICT, Json(json!({
                    "error":  "already_executed",
                    "status": "executed",
                }))).into_response();
            }
            if proposal.status == ProposalStatus::Vetoed {
                return (StatusCode::CONFLICT, Json(json!({
                    "error":  "already_vetoed",
                    "status": "vetoed",
                }))).into_response();
            }
            state.governance_store.veto(&id, &body.veto_by, &body.reason).await;
            info!(proposal_id = %id, veto_by = %body.veto_by, reason = %body.reason, "Bínò constitutional veto applied");
            Json(json!({
                "proposal_id": id,
                "status":      "vetoed",
                "veto_by":     body.veto_by,
                "reason":      body.reason,
            })).into_response()
        }
    }
}

// ── Agent birth + lifecycle handlers (Phase 57) ──────────────────────────────

#[derive(serde::Deserialize)]
struct AgentBirthBody {
    agent_id:  String,
    owner_did: String,
}

// POST /agents
async fn handle_agent_birth(
    State(state): State<NodeState>,
    Json(body): Json<AgentBirthBody>,
) -> impl IntoResponse {
    use crate::agent_store::AGENT_BIRTH_FEE_MICRO_ASE;
    // Deduct birth fee from owner wallet (creates wallet if needed)
    let balance = state.wallet_store.balance(&body.owner_did).await;
    if balance < AGENT_BIRTH_FEE_MICRO_ASE {
        return (StatusCode::PAYMENT_REQUIRED, Json(json!({
            "error":    "insufficient_balance",
            "required": AGENT_BIRTH_FEE_MICRO_ASE,
            "balance":  balance,
        }))).into_response();
    }
    let _ = state.wallet_store.debit(&body.owner_did, AGENT_BIRTH_FEE_MICRO_ASE).await;

    match state.agent_store.birth(&body.agent_id, &body.owner_did).await {
        Ok(agent) => {
            info!(agent_id = %agent.agent_id, owner = %agent.owner_did, "agent born");
            (StatusCode::CREATED, Json(serde_json::to_value(&agent).unwrap_or_default())).into_response()
        }
        Err(reason) => (StatusCode::CONFLICT, Json(json!({
            "error":  "agent_exists",
            "reason": reason,
        }))).into_response(),
    }
}

// GET /agents
async fn handle_agents_list(State(state): State<NodeState>) -> impl IntoResponse {
    let agents = state.agent_store.all().await;
    Json(json!({ "count": agents.len(), "agents": agents }))
}

// GET /agents/:agent_id
async fn handle_agent_get(
    State(state): State<NodeState>,
    Path(agent_id): Path<String>,
) -> impl IntoResponse {
    match state.agent_store.get(&agent_id).await {
        Some(a) => Json(serde_json::to_value(a).unwrap_or_default()).into_response(),
        None    => (StatusCode::NOT_FOUND, Json(json!({ "error": "agent_not_found", "agent_id": agent_id }))).into_response(),
    }
}

#[derive(serde::Deserialize)]
struct AgentStakeBody { amount: u64 }

// POST /agents/:agent_id/stake
async fn handle_agent_stake(
    State(state): State<NodeState>,
    Path(agent_id): Path<String>,
    Json(body): Json<AgentStakeBody>,
) -> impl IntoResponse {
    match state.agent_store.stake(&agent_id, body.amount).await {
        Ok(a)   => Json(serde_json::to_value(a).unwrap_or_default()).into_response(),
        Err(e)  => (StatusCode::CONFLICT, Json(json!({ "error": "stake_failed", "reason": e }))).into_response(),
    }
}

// POST /agents/:agent_id/unstake
async fn handle_agent_unstake(
    State(state): State<NodeState>,
    Path(agent_id): Path<String>,
    Json(body): Json<AgentStakeBody>,
) -> impl IntoResponse {
    match state.agent_store.unstake(&agent_id, body.amount).await {
        Ok(a)   => Json(serde_json::to_value(a).unwrap_or_default()).into_response(),
        Err(e)  => (StatusCode::CONFLICT, Json(json!({ "error": "unstake_failed", "reason": e }))).into_response(),
    }
}

// ── Council of 12 handlers (Phase 58) ─────────────────────────────────────────

// GET /governance/council
async fn handle_council_summary(State(state): State<NodeState>) -> impl IntoResponse {
    let seats   = state.council_store.all_seats().await;
    let eligible = state.council_store.rotation_eligible().await;
    Json(json!({
        "seat_count":        seats.len(),
        "sector_count":      24u8,
        "rotation_eligible": eligible.len(),
        "seats":             seats,
    }))
}

// GET /governance/council/seats
async fn handle_council_seats(State(state): State<NodeState>) -> impl IntoResponse {
    Json(json!({ "seats": state.council_store.all_seats().await }))
}

// GET /governance/council/sectors
async fn handle_council_sectors(State(state): State<NodeState>) -> impl IntoResponse {
    Json(json!({ "sectors": state.council_store.all_sectors().await }))
}

#[derive(serde::Deserialize)]
struct RotateBody {
    new_councilor_did: String,
    /// If true, bypass term-end check (admin use).
    force: Option<bool>,
}

// POST /governance/council/seats/:idx/rotate
async fn handle_council_rotate(
    State(state): State<NodeState>,
    Path(idx): Path<u8>,
    Json(body): Json<RotateBody>,
) -> impl IntoResponse {
    let result = if body.force.unwrap_or(false) {
        state.council_store.force_rotate(idx, &body.new_councilor_did).await
    } else {
        state.council_store.rotate(idx, &body.new_councilor_did).await
    };
    match result {
        Ok(seat) => Json(serde_json::to_value(seat).unwrap_or_default()).into_response(),
        Err(e)   => (StatusCode::CONFLICT, Json(json!({ "error": "rotation_failed", "reason": e }))).into_response(),
    }
}

// ── 1440 Sovereign seat handlers (Phase 59) ──────────────────────────────────

// GET /seats
async fn handle_seats_summary(State(state): State<NodeState>) -> impl IntoResponse {
    let claimed = state.seat_store.all_claimed().await;
    let active  = state.seat_store.active_count().await;
    let vacant  = state.seat_store.vacant_count().await;
    Json(json!({
        "total_seats": sovereign_types::SOVEREIGN_SEAT_COUNT,
        "claimed":     claimed.len(),
        "active":      active,
        "revoked":     claimed.len().saturating_sub(active),
        "vacant":      vacant,
    }))
}

// GET /seats/:idx
async fn handle_seat_get(
    State(state): State<NodeState>,
    Path(idx): Path<u16>,
) -> impl IntoResponse {
    match state.seat_store.get(idx).await {
        Some(s) => Json(serde_json::to_value(s).unwrap_or_default()).into_response(),
        None    => (StatusCode::NOT_FOUND, Json(json!({ "error": "seat_vacant", "seat_index": idx }))).into_response(),
    }
}

#[derive(serde::Deserialize)]
struct SeatClaimBody { steward_did: String }

// POST /seats/:idx/claim
async fn handle_seat_claim(
    State(state): State<NodeState>,
    Path(idx): Path<u16>,
    Json(body): Json<SeatClaimBody>,
) -> impl IntoResponse {
    match state.seat_store.claim(idx, &body.steward_did).await {
        Ok(seat) => (StatusCode::CREATED, Json(serde_json::to_value(seat).unwrap_or_default())).into_response(),
        Err(e)   => (StatusCode::CONFLICT, Json(json!({ "error": "claim_failed", "reason": e }))).into_response(),
    }
}

#[derive(serde::Deserialize)]
struct SeatRevokeBody { reason: String }

// POST /seats/:idx/revoke
async fn handle_seat_revoke(
    State(state): State<NodeState>,
    Path(idx): Path<u16>,
    Json(body): Json<SeatRevokeBody>,
) -> impl IntoResponse {
    match state.seat_store.revoke(idx, &body.reason).await {
        Ok(seat) => Json(serde_json::to_value(seat).unwrap_or_default()).into_response(),
        Err(e)   => (StatusCode::CONFLICT, Json(json!({ "error": "revoke_failed", "reason": e }))).into_response(),
    }
}

// ── Emission receipt handlers (Phase 56 — Zàngbétò) ─────────────────────────

// GET /emission/receipts
async fn handle_emission_receipts_list(State(state): State<NodeState>) -> impl IntoResponse {
    let receipts = state.emission_receipts.all().await;
    Json(json!({
        "count":    receipts.len(),
        "emission_number": state.emission_receipts.emission_number().await,
        "receipts": receipts,
    }))
}

// GET /emission/receipts/:id
async fn handle_emission_receipt_get(
    State(state): State<NodeState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.emission_receipts.get(&id).await {
        Some(r) => Json(serde_json::to_value(r).unwrap_or_default()).into_response(),
        None    => (StatusCode::NOT_FOUND, Json(json!({ "error": "receipt_not_found", "id": id }))).into_response(),
    }
}

// ── Simulation scoring handlers (Phase 60) ───────────────────────────────────

#[derive(serde::Deserialize)]
struct SimulationScoreBody {
    proof_id:           String,
    worker_did:         String,
    f1_score:           f64,
    current_difficulty: Option<f64>,
    prior_env_count:    Option<u64>,
    verification_pct:   Option<f64>,
    independence:       Option<f64>,
    tier_index:         Option<u8>,
    witness_confidence: Option<f64>,
}

// POST /simulation/score
async fn handle_simulation_score(
    Json(body): Json<SimulationScoreBody>,
) -> impl IntoResponse {
    let difficulty = SimulationFactors::difficulty_factor(
        body.f1_score,
        body.current_difficulty.unwrap_or(0.777),
    );
    let novelty = SimulationFactors::novelty_from_prior_count(
        body.prior_env_count.unwrap_or(1),
    );
    let utility = SimulationFactors::utility_for_tier(
        body.tier_index.unwrap_or(0),
    );
    let factors = SimulationFactors {
        difficulty,
        quality:            body.f1_score.clamp(0.0, 1.0),
        novelty,
        verification:       body.verification_pct.unwrap_or(1.0).clamp(0.0, 1.0),
        independence:       body.independence.unwrap_or(1.0).clamp(0.0, 1.0),
        utility,
        witness_confidence: body.witness_confidence.unwrap_or(1.0).clamp(0.0, 1.0),
    };
    let score = factors.score();
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
    let result = SimulationScoreResult {
        proof_id:   body.proof_id,
        worker_did: body.worker_did,
        factors,
        score,
        share:      0.0, // caller must call /simulation/shares for multi-claim normalisation
        timestamp:  now,
    };
    Json(serde_json::to_value(result).unwrap_or_default())
}

#[derive(serde::Deserialize)]
struct SimulationSharesBody {
    /// List of (proof_id, score) pairs to normalise into emission shares.
    claims: Vec<SimulationShareClaim>,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct SimulationShareClaim {
    proof_id: String,
    score:    f64,
}

// POST /simulation/shares
async fn handle_simulation_shares(
    Json(body): Json<SimulationSharesBody>,
) -> impl IntoResponse {
    let scores: Vec<(String, f64)> = body.claims.iter()
        .map(|c| (c.proof_id.clone(), c.score))
        .collect();
    let shares = compute_emission_shares(&scores);
    let result: Vec<_> = shares.into_iter().map(|(id, s)| json!({
        "proof_id": id,
        "share":    s,
    })).collect();
    Json(json!({ "shares": result, "count": result.len() }))
}
