/// sovereign-runtime — the spine between the Linux kernel and the sovereign ecosystem.
///
/// Enforces the universal execution model:
///   Principal → Capability → Authorization → Execution → Evidence → ActionReceipt
///
/// Every consequential operation in every repo must emit an ActionReceipt.
/// This crate is the single source of truth for that contract.

pub mod principal;
pub mod capability;
pub mod evidence;
pub mod receipt;
pub mod execution;
pub mod chain;

// Machine-facing interfaces (Layer 2 machine substrate)
pub mod device;
pub mod safety;
pub mod power;
pub mod embodiment;

pub use chain::run_sovereign;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("identity error: {0}")]
    Identity(String),

    #[error("invalid principal: {0}")]
    InvalidPrincipal(&'static str),

    #[error("capability denied: {0}")]
    CapabilityDenied(String),

    #[error("execution error: {0}")]
    Execution(String),

    #[error("evidence error: {0}")]
    Evidence(String),

    #[error("device error: {0}")]
    Device(#[from] device::DeviceError),

    #[error("embodiment error: {0}")]
    Embodiment(#[from] embodiment::EmbodimentError),

    #[error("safety blocked: {0}")]
    SafetyBlocked(String),
}

#[cfg(test)]
mod tests {
    use crate::principal::Principal;
    use crate::receipt::{ActionReceipt, ActionOutcome, PoCWProof};
    use crate::execution::ExecutionEngine;
    use crate::capability::CapabilityAction;
    use crate::chain::run_sovereign;
    use serde_json::{json, Value};

    fn did() -> String { "did:key:test-principal".into() }
    fn agent() -> String { "did:key:test-agent".into() }
    fn now() -> u64 { 1_700_000_000_000_u64 }
    fn vp() -> Principal { Principal::from_did(did(), agent()) }

    #[test]
    fn principal_from_did_valid() {
        assert!(vp().validate().is_ok());
    }

    #[test]
    fn principal_bad_nostr_fails() {
        assert!(vp().with_nostr("bad").validate().is_err());
    }

    #[test]
    fn principal_nostr_64hex_valid() {
        assert!(vp().with_nostr("a".repeat(64)).validate().is_ok());
    }

    #[test]
    fn principal_sui_with_prefix_valid() {
        assert!(vp().with_sui("0xdeadbeef").validate().is_ok());
    }

    #[test]
    fn principal_sui_no_prefix_fails() {
        assert!(vp().with_sui("deadbeef").validate().is_err());
    }

    #[test]
    fn pocw_valid_when_steps_meet_bound() {
        let p = PoCWProof { steps: 21, bb_bound: 21, tape_hash: "abc".into() };
        assert!(p.is_valid());
    }

    #[test]
    fn pocw_invalid_when_steps_below_bound() {
        let p = PoCWProof { steps: 20, bb_bound: 21, tape_hash: "abc".into() };
        assert!(!p.is_valid());
    }

    #[test]
    fn pocw_min_for_tier() {
        assert_eq!(PoCWProof::min_for_tier(0), 0);
        assert_eq!(PoCWProof::min_for_tier(1), 21);
        assert_eq!(PoCWProof::min_for_tier(2), 107);
        assert_eq!(PoCWProof::min_for_tier(3), 47_176_870);
    }

    #[test]
    fn engine_begin_valid_principal_no_cap() {
        let r = ExecutionEngine::begin(vp(), None, CapabilityAction::Execute,
            "res://test", json!({}), now());
        assert!(r.is_ok());
    }

    #[test]
    fn ctx_complete_success() {
        let ctx = ExecutionEngine::begin(vp(), None, CapabilityAction::Execute,
            "res://test", json!({}), now()).unwrap();
        let r = ctx.complete(json!("done"), now());
        assert_eq!(r.outcome, ActionOutcome::Success);
        assert_eq!(r.result, json!("done"));
    }

    #[test]
    fn ctx_fail_failure() {
        let ctx = ExecutionEngine::begin(vp(), None, CapabilityAction::Execute,
            "res://test", json!({}), now()).unwrap();
        let r = ctx.fail("boom", now());
        assert_eq!(r.outcome, ActionOutcome::Failure);
        assert_eq!(r.error.as_deref(), Some("boom"));
    }

    #[test]
    fn engine_begin_invalid_principal_denied() {
        let bad = vp().with_nostr("short");
        match ExecutionEngine::begin(bad, None, CapabilityAction::Execute,
            "res://test", json!({}), now()) {
            Err(r) => assert_eq!(r.outcome, ActionOutcome::Denied),
            Ok(_)  => panic!("expected Err"),
        }
    }

    #[test]
    fn action_receipt_denied_no_evidence() {
        let r = ActionReceipt::denied(&vp(), CapabilityAction::Execute,
            "res://test", "reason", now());
        assert_eq!(r.outcome, ActionOutcome::Denied);
        assert_eq!(r.evidence_count, 0);
    }

    #[test]
    fn receipt_with_previous_sets_sha256_hash() {
        let r = ActionReceipt::denied(&vp(), CapabilityAction::Execute,
            "res://test", "reason", now()).with_previous("prev_id");
        assert!(r.previous_hash.unwrap().starts_with("sha256:"));
    }

    #[test]
    fn receipt_meets_pocw_floor_tier0_always() {
        let r = ActionReceipt::denied(&vp(), CapabilityAction::Execute,
            "res://test", "reason", now());
        assert!(r.meets_pocw_floor(0));
        assert!(!r.meets_pocw_floor(1));
    }

    #[tokio::test]
    async fn run_sovereign_success() {
        let r = run_sovereign(
            vp(), None, CapabilityAction::Execute,
            "res://test", json!({}), now(),
            |ctx| async move { (ctx, Ok::<Value, String>(json!("all good"))) },
        ).await;
        assert_eq!(r.outcome, ActionOutcome::Success);
    }

    #[test]
    fn action_outcome_serde_roundtrip() {
        for v in &[ActionOutcome::Success, ActionOutcome::Partial,
                   ActionOutcome::Failure, ActionOutcome::Denied] {
            let s = serde_json::to_string(v).unwrap();
            let d: ActionOutcome = serde_json::from_str(&s).unwrap();
            assert_eq!(*v, d);
        }
    }
}
