//! Phase 4.2 — DIP event bus for pipeline stages.
//!
//! Every consequential pipeline event is wrapped in a DIP Receipt envelope
//! and routed through the DipRouter. This ensures:
//!
//!   • CaptureReceipt (31020) events propagate to Vantage and Nostr
//!   • SceneReceipt (31030) events propagate for licensing / IP registration
//!   • VcpSessionReceipt events propagate for audit trail
//!   • All routing goes through deduplication + TTL logic
//!
//! The bus is intentionally fire-and-forget at this layer.
//! Delivery confirmation is the responsibility of the adapter.

use serde_json::Value;
use sovereign_types::IdentityChain;
use dip::{
    DipEnvelope, DipKind,
    address::DipAddress,
    router::{DipRouter, RouteDecision},
    envelope::ReceiptPayload,
    error::{DipError, DipResult},
};
use twin_protocol::{CaptureReceipt, SceneReceipt};
use vcp::VcpSessionReceipt;
use crate::error::PipelineResult;

/// Pipeline event types that flow through the DIP event bus.
#[derive(Debug, Clone)]
pub enum PipelineEvent {
    Capture(CaptureReceipt),
    Scene(SceneReceipt),
    Session(VcpSessionReceipt),
}

impl PipelineEvent {
    pub fn kind_label(&self) -> &'static str {
        match self {
            PipelineEvent::Capture(_) => "capture_receipt_31020",
            PipelineEvent::Scene(_)   => "scene_receipt_31030",
            PipelineEvent::Session(_) => "vcp_session_receipt",
        }
    }

    pub fn receipt_id(&self) -> &str {
        match self {
            PipelineEvent::Capture(r) => &r.receipt_id,
            PipelineEvent::Scene(r)   => &r.receipt_id,
            PipelineEvent::Session(r) => &r.receipt_id,
        }
    }

    pub fn to_payload(&self) -> Value {
        match self {
            PipelineEvent::Capture(r) => serde_json::to_value(r).unwrap_or(Value::Null),
            PipelineEvent::Scene(r)   => serde_json::to_value(r).unwrap_or(Value::Null),
            PipelineEvent::Session(r) => serde_json::to_value(r).unwrap_or(Value::Null),
        }
    }
}

/// Outcome of routing a pipeline event.
#[derive(Debug)]
pub struct RoutedEvent {
    pub event_kind: &'static str,
    pub receipt_id: String,
    pub envelope:   DipEnvelope,
    pub decision:   RouteDecision,
}

/// DIP-aware event bus for the sovereign pipeline.
pub struct PipelineEventBus {
    router:      DipRouter,
    local_addr:  DipAddress,
    signing_key: String,
    identity:    IdentityChain,
    /// Default destination for pipeline receipts (Vantage API DID).
    vantage_did: String,
    /// Optional Nostr npub for cross-network propagation.
    nostr_npub:  Option<String>,
}

impl PipelineEventBus {
    pub fn new(
        local_did:   impl Into<String>,
        vantage_did: impl Into<String>,
        signing_key: impl Into<String>,
        identity:    IdentityChain,
    ) -> Self {
        let local_did   = local_did.into();
        let vantage_did = vantage_did.into();
        let mut router  = DipRouter::new(local_did.clone());
        // Register Vantage and Nostr adapters by default
        router.register_adapter(dip::address::DipNetwork::Vantage);
        router.register_adapter(dip::address::DipNetwork::Nostr);

        Self {
            router,
            local_addr:  DipAddress::vantage(local_did),
            signing_key: signing_key.into(),
            identity,
            vantage_did,
            nostr_npub:  None,
        }
    }

    pub fn with_nostr(mut self, npub: impl Into<String>) -> Self {
        self.nostr_npub = Some(npub.into());
        self
    }

    /// Wrap a pipeline event in a DIP Receipt envelope and route it.
    pub fn emit(&mut self, event: PipelineEvent) -> DipResult<RoutedEvent> {
        let event_kind = event.kind_label();
        let receipt_id = event.receipt_id().to_string();
        let payload    = event.to_payload();

        // Wrap in DIP Receipt envelope addressed to Vantage
        let dest = DipAddress::vantage(self.vantage_did.clone());
        let envelope = DipEnvelope::build(
            self.local_addr.clone(),
            dest,
            self.identity.clone(),
            DipKind::Receipt,
            payload,
            3600,  // receipts have 1hr TTL for reliable delivery
            &self.signing_key,
        )?;

        let decision = self.router.route(&envelope)?;

        Ok(RoutedEvent { event_kind, receipt_id, envelope, decision })
    }

    /// Emit all three receipts from a pipeline output at once.
    pub fn emit_pipeline_output(
        &mut self,
        capture:  &CaptureReceipt,
        scene:    &SceneReceipt,
        session:  &VcpSessionReceipt,
    ) -> DipResult<Vec<RoutedEvent>> {
        let mut routed = vec![];

        routed.push(self.emit(PipelineEvent::Capture(capture.clone()))?);
        routed.push(self.emit(PipelineEvent::Scene(scene.clone()))?);
        routed.push(self.emit(PipelineEvent::Session(session.clone()))?);

        Ok(routed)
    }

    /// Cross-post a scene receipt to Nostr (for public IP registration).
    /// Uses replaceable kind (30001) via the Nostr adapter conventions.
    pub fn cross_post_to_nostr(&mut self, scene: &SceneReceipt) -> DipResult<RoutedEvent> {
        let npub = self.nostr_npub.clone()
            .ok_or_else(|| DipError::RoutingFailed("no Nostr npub configured".into()))?;
        let dest = DipAddress::nostr(npub);
        let payload = serde_json::to_value(scene)
            .map_err(|e| DipError::RoutingFailed(e.to_string()))?;

        let envelope = DipEnvelope::build(
            self.local_addr.clone(),
            dest,
            self.identity.clone(),
            DipKind::Receipt,
            payload,
            86400,  // 24hr TTL for replaceable Nostr events
            &self.signing_key,
        )?;

        let decision = self.router.route(&envelope)?;

        Ok(RoutedEvent {
            event_kind: "scene_receipt_31030_nostr",
            receipt_id: scene.receipt_id.clone(),
            envelope,
            decision,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sovereign_types::{IdentityChain, LicenseType, crypto::generate_keypair};
    use twin_protocol::SceneReceipt;
    use vcp::VcpSessionOutcome;
    use dip::router::RouteDecision;

    fn make_identity(did: &str) -> IdentityChain {
        IdentityChain::new(did.into(), did.into())
    }

    fn make_scene_receipt() -> SceneReceipt {
        SceneReceipt {
            kind:                  31030,
            receipt_id:            "rcpt:31030:bus-test".into(),
            twin_id:               "twin:sha256:bus01".into(),
            version:               1,
            identity:              make_identity("did:p:1"),
            capture_receipt_ids:   vec!["rcpt:31020:01".into()],
            reconstruction_engine: "nerfstudio".into(),
            splat_hash:            "sha256:splat01".into(),
            geometry_hash:         None,
            semantic_hash:         None,
            f1_score:              0.845,
            coverage_pct:          82.1,
            gaussian_count:        None,
            sui_object_id:         None,
            ip_root_tx:            None,
            license_type:          LicenseType::NonExclusive,
            merkle_root:           "sha256:root01".into(),
            signature:             "base64url:sig01".into(),
            timestamp:             1725734400000,
        }
    }

    fn make_bus(did: &str, key: &str) -> PipelineEventBus {
        PipelineEventBus::new(
            did,
            "did:vantage:api:receipts",
            key,
            make_identity(did),
        )
    }

    #[test]
    fn emit_scene_receipt_routes_to_vantage() {
        let (key, pub_key) = generate_keypair();
        let did = sovereign_types::crypto::did_from_pubkey(&pub_key, "agent");
        let mut bus = make_bus(&did, &key);

        let scene = make_scene_receipt();
        let routed = bus.emit(PipelineEvent::Scene(scene)).unwrap();

        assert_eq!(routed.event_kind, "scene_receipt_31030");
        assert_eq!(routed.envelope.kind, DipKind::Receipt);
        assert!(matches!(routed.decision, RouteDecision::Forward(_)));
    }

    #[test]
    fn emit_capture_receipt_routed() {
        let (key, pub_key) = generate_keypair();
        let did = sovereign_types::crypto::did_from_pubkey(&pub_key, "agent");
        let mut bus = make_bus(&did, &key);

        let capture = twin_protocol::CaptureReceipt {
            kind:           31020,
            receipt_id:     "rcpt:31020:test".into(),
            identity:       make_identity("did:p:1"),
            device_ids:     vec!["go2:01".into()],
            modalities:     vec![],
            region:         twin_protocol::TwinRegion::new(0.0, 0.0, 0.001, 0.001),
            capture_epoch:  [0, 1000],
            f1_score:       0.812,
            coverage_pct:   70.0,
            frame_count:    100,
            duration_ms:    5000,
            raw_hashes:     Default::default(),
            novelty_score:  0.5,
            delta_coverage: 10.0,
            privacy_flags:  vec![],
            merkle_root:    "sha256:root".into(),
            signature:      "sig".into(),
            timestamp:      0,
        };

        let routed = bus.emit(PipelineEvent::Capture(capture)).unwrap();
        assert_eq!(routed.event_kind, "capture_receipt_31020");
        assert!(matches!(routed.decision, RouteDecision::Forward(_)));
    }

    #[test]
    fn cross_post_to_nostr_requires_npub() {
        let (key, pub_key) = generate_keypair();
        let did = sovereign_types::crypto::did_from_pubkey(&pub_key, "agent");
        let mut bus = make_bus(&did, &key);
        // No nostr npub configured — should fail
        let result = bus.cross_post_to_nostr(&make_scene_receipt());
        assert!(result.is_err());
    }

    #[test]
    fn cross_post_to_nostr_with_npub_routes() {
        let (key, pub_key) = generate_keypair();
        let did = sovereign_types::crypto::did_from_pubkey(&pub_key, "agent");
        let mut bus = make_bus(&did, &key)
            .with_nostr("npub1qqqtest");

        let routed = bus.cross_post_to_nostr(&make_scene_receipt()).unwrap();
        assert_eq!(routed.event_kind, "scene_receipt_31030_nostr");
        assert!(matches!(routed.decision, RouteDecision::Forward(dip::address::DipNetwork::Nostr)));
    }

    #[test]
    fn deduplication_prevents_double_emit() {
        let (key, pub_key) = generate_keypair();
        let did = sovereign_types::crypto::did_from_pubkey(&pub_key, "agent");
        let mut bus = make_bus(&did, &key);
        let scene = make_scene_receipt();

        let first  = bus.emit(PipelineEvent::Scene(scene.clone())).unwrap();
        // Re-emit the same envelope — router sees duplicate message_id
        let second = bus.router.route(&first.envelope).unwrap();
        assert!(matches!(second, RouteDecision::Drop(_)));
    }
}
