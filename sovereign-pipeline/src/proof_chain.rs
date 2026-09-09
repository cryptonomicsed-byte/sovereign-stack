//! Phase 4.3 — Proof chain: ỌSỌVM simulation → Sui anchor.
//!
//! Wires the complete post-capture proof pipeline:
//!
//!   TwinAsset (from pipeline) →
//!     ỌSỌVM Proof-of-Simulation (SimulationReceipt) →
//!       Sui anchor (Twin NFT minted, object_id written back) →
//!         DIP event bus (all receipts propagated)
//!
//! Invariants enforced end-to-end:
//!   • F1 gate 0.777 — already enforced by CaptureReceipt / TwinAsset
//!   • >= 2 candidate policies — enforced by OsovmEngine
//!   • >= 2 witnesses — enforced by SimulationReceipt::build
//!   • Sui object_id written back into SceneReceipt before DIP broadcast
//!   • DIP events emitted in order: capture → scene (with Sui) → session

use sovereign_types::{IdentityChain, WitnessAttestation, crypto::generate_keypair};
use twin_protocol::{
    TwinAsset, SceneReceipt, SimulationReceipt,
    osovm::{OsovmEngine, SimScenario, ProofOfSimulation},
    sui_anchor::{SuiAnchor, SuiMintResponse},
};
use crate::capture_pipeline::PipelineOutput;
use crate::event_bus::PipelineEventBus;
use crate::error::{PipelineError, PipelineResult};

/// Witnesses for a simulation run: (did, signing_key) pairs.
pub type WitnessPair<'a> = (&'a str, &'a str);

/// Full output of the proof chain.
pub struct ProofChainOutput {
    /// The original capture + scene + session receipts
    pub pipeline:   PipelineOutput,
    /// Proof-of-Simulation receipt
    pub simulation: SimulationReceipt,
    /// Sui mint response (stub in dev, real tx in prod)
    pub sui_mint:   SuiMintResponse,
    /// Message IDs of all DIP events emitted
    pub dip_message_ids: Vec<String>,
}

/// Configuration for the proof chain.
pub struct ProofChainConfig {
    pub scenario:       SimScenario,
    pub osovm_version:  String,
    pub sui_rpc:        String,
    pub sui_address:    String,
    pub sui_key:        String,
    pub nostr_npub:     Option<String>,
    pub vantage_did:    String,
}

impl Default for ProofChainConfig {
    fn default() -> Self {
        Self {
            scenario: SimScenario {
                name:               "default_navigation".into(),
                robot_model:        "Go2".into(),
                trajectory_count:   6,
                selection_objective: "balanced".into(),
                params:             None,
            },
            osovm_version: "osovm/2.0".into(),
            sui_rpc:        "https://fullnode.mainnet.sui.io".into(),
            sui_address:    "0xdefault".into(),
            sui_key:        "stubkey".into(),
            nostr_npub:     None,
            vantage_did:    "did:vantage:api:receipts".into(),
        }
    }
}

/// Orchestrates ỌSỌVM → Sui → DIP in a single async call.
pub struct ProofChain {
    pub config:     ProofChainConfig,
    pub agent_key:  String,
    pub identity:   IdentityChain,
}

impl ProofChain {
    pub fn new(
        config:    ProofChainConfig,
        agent_key: impl Into<String>,
        identity:  IdentityChain,
    ) -> Self {
        Self { config, agent_key: agent_key.into(), identity }
    }

    /// Run the full proof chain.
    ///
    /// `pipeline_output` — the result of `CapturePipeline::run()`
    /// `witnesses`       — at least 2 (did, signing_key) pairs
    pub async fn run(
        &self,
        mut pipeline_output: PipelineOutput,
        witnesses: &[WitnessPair<'_>],
    ) -> PipelineResult<ProofChainOutput> {
        // --- Step 1: ỌSỌVM Proof-of-Simulation ---
        let engine = OsovmEngine::new(&self.config.osovm_version);
        let proof  = ProofOfSimulation::new(engine);

        let simulation = proof.prove(
            &pipeline_output.twin,
            &self.config.scenario,
            self.identity.clone(),
            &self.agent_key,
            witnesses,
        )?;

        // --- Step 2: Sui anchor — mint Twin NFT ---
        let anchor = SuiAnchor::new(
            &self.config.sui_rpc,
            &self.config.sui_address,
            &self.config.sui_key,
        );

        let sui_mint = anchor.mint_twin(&mut pipeline_output.scene_receipt).await?;

        // Propagate the object_id back into the twin's provenance
        pipeline_output.twin.sui_object_id = pipeline_output.scene_receipt.sui_object_id.clone();

        // --- Step 3: DIP event bus — broadcast all receipts ---
        let mut bus = PipelineEventBus::new(
            self.identity.agent_id.clone(),
            self.config.vantage_did.clone(),
            self.agent_key.clone(),
            self.identity.clone(),
        );
        if let Some(npub) = &self.config.nostr_npub {
            bus = bus.with_nostr(npub.clone());
        }

        let mut dip_message_ids = vec![];

        // Emit capture → scene (with Sui object_id) → session
        let events = bus.emit_pipeline_output(
            &pipeline_output.capture_receipt,
            &pipeline_output.scene_receipt,
            &pipeline_output.session_receipt,
        ).map_err(|e| PipelineError::CaptureFailed(e.to_string()))?;

        for ev in events {
            dip_message_ids.push(ev.envelope.message_id.clone());
        }

        // Cross-post scene receipt to Nostr if configured
        if self.config.nostr_npub.is_some() {
            let nostr_ev = bus.cross_post_to_nostr(&pipeline_output.scene_receipt)
                .map_err(|e| PipelineError::CaptureFailed(e.to_string()))?;
            dip_message_ids.push(nostr_ev.envelope.message_id);
        }

        Ok(ProofChainOutput {
            pipeline:       pipeline_output,
            simulation,
            sui_mint,
            dip_message_ids,
        })
    }

    /// Skip the ỌSỌVM run and witness collection — use a pre-built SimulationReceipt.
    ///
    /// Called by sovereign-node after the two-phase witness protocol completes:
    /// local signers sign immediately, remote witnesses sign via DIP exchange,
    /// then this method handles Sui anchoring and DIP event bus broadcasting.
    pub async fn run_with_simulation(
        &self,
        mut pipeline_output: PipelineOutput,
        simulation: SimulationReceipt,
    ) -> PipelineResult<ProofChainOutput> {
        // Sui anchor
        let anchor = SuiAnchor::new(
            &self.config.sui_rpc,
            &self.config.sui_address,
            &self.config.sui_key,
        );
        let sui_mint = anchor.mint_twin(&mut pipeline_output.scene_receipt).await?;
        pipeline_output.twin.sui_object_id = pipeline_output.scene_receipt.sui_object_id.clone();

        // DIP event bus
        let mut bus = PipelineEventBus::new(
            self.identity.agent_id.clone(),
            self.config.vantage_did.clone(),
            self.agent_key.clone(),
            self.identity.clone(),
        );
        if let Some(npub) = &self.config.nostr_npub {
            bus = bus.with_nostr(npub.clone());
        }

        let mut dip_message_ids = vec![];
        let events = bus.emit_pipeline_output(
            &pipeline_output.capture_receipt,
            &pipeline_output.scene_receipt,
            &pipeline_output.session_receipt,
        ).map_err(|e| PipelineError::CaptureFailed(e.to_string()))?;
        for ev in events {
            dip_message_ids.push(ev.envelope.message_id.clone());
        }
        if self.config.nostr_npub.is_some() {
            let nostr_ev = bus.cross_post_to_nostr(&pipeline_output.scene_receipt)
                .map_err(|e| PipelineError::CaptureFailed(e.to_string()))?;
            dip_message_ids.push(nostr_ev.envelope.message_id);
        }

        Ok(ProofChainOutput { pipeline: pipeline_output, simulation, sui_mint, dip_message_ids })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sovereign_types::crypto::{generate_keypair, did_from_pubkey};
    use vcp::{
        VcpSession, VcpCapabilityGrant,
        handshake::{VcpCapabilityRequest, VcpDuration},
        adapters::{Go2Adapter, Go2ConnectionMode},
    };
    use crate::capture_pipeline::{CapturePipeline, PipelineConfig, Go2CaptureDriver};

    fn build_pipeline_output(agent_key: &str, agent_did: &str) -> PipelineOutput {
        let (device_key, _) = generate_keypair();
        let adapter  = Go2Adapter::new("unitree:go2:test", Go2ConnectionMode::default());
        let manifest = adapter.manifest("base64url:fakepub");
        let identity = IdentityChain::new(agent_did.into(), agent_did.into());
        let request  = VcpCapabilityRequest::new(
            identity.clone(),
            vec!["camera".into(), "lidar".into(), "telemetry".into()],
            "twin_capture",
            VcpDuration::minutes(60),
            agent_key,
        ).unwrap();
        let grant   = VcpCapabilityGrant::issue(&manifest, &request, &device_key).unwrap();
        let session = VcpSession::new(grant);
        let pipeline = CapturePipeline::new(
            PipelineConfig { owner_did: agent_did.into(), ..Default::default() },
            agent_key,
            identity,
        );
        pipeline.run(session, &Go2CaptureDriver).unwrap()
    }

    #[tokio::test]
    async fn proof_chain_mints_and_routes() {
        let (agent_key, agent_pub) = generate_keypair();
        let agent_did = did_from_pubkey(&agent_pub, "agent");
        let (w1_key, _) = generate_keypair();
        let (w2_key, _) = generate_keypair();

        let output = build_pipeline_output(&agent_key, &agent_did);
        let identity = IdentityChain::new(agent_did.clone(), agent_did.clone());

        let chain = ProofChain::new(
            ProofChainConfig::default(),
            &agent_key,
            identity,
        );

        let result = chain.run(
            output,
            &[("did:witness:node01", &w1_key), ("did:witness:node02", &w2_key)],
        ).await.unwrap();

        // SimulationReceipt
        assert_eq!(result.simulation.kind, "proof_of_simulation");
        assert!(result.simulation.all_policies.len() >= 2);
        assert_eq!(result.simulation.witness_ids.len(), 2);
        assert_eq!(result.simulation.twin_id, result.pipeline.twin.twin_id);

        // Sui anchor
        assert_eq!(result.sui_mint.status, twin_protocol::sui_anchor::SuiTxStatus::Success);
        assert!(result.pipeline.scene_receipt.sui_object_id.is_some());
        assert!(result.pipeline.twin.sui_object_id.is_some());
        assert!(result.pipeline.scene_receipt.sui_object_id.as_deref().unwrap().starts_with("0x"));

        // DIP events: capture + scene + session = 3 minimum
        assert!(result.dip_message_ids.len() >= 3);
    }

    #[tokio::test]
    async fn scene_sui_object_id_matches_twin() {
        let (agent_key, agent_pub) = generate_keypair();
        let agent_did = did_from_pubkey(&agent_pub, "agent");
        let (w1_key, _) = generate_keypair();
        let (w2_key, _) = generate_keypair();

        let output   = build_pipeline_output(&agent_key, &agent_did);
        let identity = IdentityChain::new(agent_did.clone(), agent_did.clone());
        let chain    = ProofChain::new(ProofChainConfig::default(), &agent_key, identity);

        let result = chain.run(
            output,
            &[("did:witness:w1", &w1_key), ("did:witness:w2", &w2_key)],
        ).await.unwrap();

        assert_eq!(
            result.pipeline.scene_receipt.sui_object_id,
            result.pipeline.twin.sui_object_id,
            "scene_receipt and twin should share the same Sui object ID"
        );
    }

    #[tokio::test]
    async fn nostr_cross_post_adds_extra_dip_event() {
        let (agent_key, agent_pub) = generate_keypair();
        let agent_did = did_from_pubkey(&agent_pub, "agent");
        let (w1_key, _) = generate_keypair();
        let (w2_key, _) = generate_keypair();

        let output   = build_pipeline_output(&agent_key, &agent_did);
        let identity = IdentityChain::new(agent_did.clone(), agent_did.clone());

        let chain = ProofChain::new(
            ProofChainConfig {
                nostr_npub: Some("npub1testqqqtest".into()),
                ..Default::default()
            },
            &agent_key,
            identity,
        );

        let result = chain.run(
            output,
            &[("did:witness:w1", &w1_key), ("did:witness:w2", &w2_key)],
        ).await.unwrap();

        // 3 Vantage events + 1 Nostr cross-post = 4
        assert_eq!(result.dip_message_ids.len(), 4);
    }

    #[tokio::test]
    async fn insufficient_witnesses_fails_cleanly() {
        let (agent_key, agent_pub) = generate_keypair();
        let agent_did = did_from_pubkey(&agent_pub, "agent");
        let (w1_key, _) = generate_keypair();

        let output   = build_pipeline_output(&agent_key, &agent_did);
        let identity = IdentityChain::new(agent_did.clone(), agent_did.clone());
        let chain    = ProofChain::new(ProofChainConfig::default(), &agent_key, identity);

        // Only 1 witness — should fail at SimulationReceipt::build
        let result = chain.run(output, &[("did:witness:w1", &w1_key)]).await;
        assert!(result.is_err());
    }
}
