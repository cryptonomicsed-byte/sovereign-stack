use sovereign_runtime::chain::*;
use sovereign_runtime::receipt::ActionOutcome;
use sovereign_types::identity::SafetyLevel;
use serde_json::json;

fn now() -> u64 { 1_700_000_000_000 }

fn make_principal() -> Principal {
    Principal::from_did(
        "did:vantage:principal:abc123".into(),
        "did:vantage:agent:xyz789".into(),
    )
}

fn make_capability(principal_id: &str, action: CapabilityAction, resource: &str) -> Capability {
    Capability {
        capability_id: "cap-001".into(),
        granted_to:    principal_id.to_string(),
        action,
        resource:      resource.to_string(),
        constraints:   vec![],
        safety_level:  SafetyLevel::Standard,
        granted_at:    now() - 1000,
        expires_at:    None,
        delegated_by:  None,
        signature:     "stub-sig".into(),
    }
}

// ── Principal ─────────────────────────────────────────────────────────────

#[test]
fn principal_validates_with_minimal_fields() {
    let p = make_principal();
    assert!(p.validate().is_ok());
}

#[test]
fn principal_validates_nostr_pubkey_length() {
    let p = make_principal().with_nostr("abc"); // too short
    assert!(p.validate().is_err());
}

#[test]
fn principal_validates_sui_address_prefix() {
    let p = make_principal().with_sui("deadbeef"); // no 0x
    assert!(p.validate().is_err());
}

#[test]
fn principal_valid_nostr_64_hex() {
    let p = make_principal()
        .with_nostr("a".repeat(64))
        .with_sui("0xdeadbeef");
    assert!(p.validate().is_ok());
}

#[test]
fn principal_hardware_bound_flag() {
    let p = make_principal();
    assert!(!p.is_hardware_bound());
}

// ── Capability Kernel ─────────────────────────────────────────────────────

#[test]
fn capability_kernel_grants_exact_match() {
    let p   = make_principal();
    let cap = make_capability(p.principal_id(), CapabilityAction::Capture, "twin:001");
    let d   = CapabilityKernel::evaluate(&p, &cap, &CapabilityAction::Capture, "twin:001", now());
    assert!(d.granted);
    assert!(d.denial_reason.is_none());
}

#[test]
fn capability_kernel_grants_glob_resource() {
    let p   = make_principal();
    let cap = make_capability(p.principal_id(), CapabilityAction::Capture, "twin:*");
    let d   = CapabilityKernel::evaluate(&p, &cap, &CapabilityAction::Capture, "twin:abc", now());
    assert!(d.granted);
}

#[test]
fn capability_kernel_denies_wrong_principal() {
    let p   = make_principal();
    let cap = make_capability("did:vantage:principal:other", CapabilityAction::Capture, "twin:*");
    let d   = CapabilityKernel::evaluate(&p, &cap, &CapabilityAction::Capture, "twin:abc", now());
    assert!(!d.granted);
    assert!(d.denial_reason.as_deref().unwrap().contains("different principal"));
}

#[test]
fn capability_kernel_denies_wrong_action() {
    let p   = make_principal();
    let cap = make_capability(p.principal_id(), CapabilityAction::Read, "twin:*");
    let d   = CapabilityKernel::evaluate(&p, &cap, &CapabilityAction::Write, "twin:abc", now());
    assert!(!d.granted);
}

#[test]
fn capability_kernel_denies_expired() {
    let p   = make_principal();
    let mut cap = make_capability(p.principal_id(), CapabilityAction::Capture, "twin:*");
    cap.expires_at = Some(now() - 1); // already expired
    let d = CapabilityKernel::evaluate(&p, &cap, &CapabilityAction::Capture, "twin:abc", now());
    assert!(!d.granted);
    assert!(d.denial_reason.as_deref().unwrap().contains("expired"));
}

#[test]
fn capability_kernel_denies_safety_ceiling_exceeded() {
    let p   = make_principal(); // SafetyLevel::Standard
    let mut cap = make_capability(p.principal_id(), CapabilityAction::Execute, "robot:arm");
    cap.safety_level = SafetyLevel::Critical; // exceeds Standard ceiling
    let d = CapabilityKernel::evaluate(&p, &cap, &CapabilityAction::Execute, "robot:arm", now());
    assert!(!d.granted);
    assert!(d.denial_reason.as_deref().unwrap().contains("safety"));
}

#[test]
fn capability_kernel_denies_resource_not_covered() {
    let p   = make_principal();
    let cap = make_capability(p.principal_id(), CapabilityAction::Capture, "twin:001");
    let d   = CapabilityKernel::evaluate(&p, &cap, &CapabilityAction::Capture, "twin:002", now());
    assert!(!d.granted);
}

// ── Execution Engine ─────────────────────────────────────────────────────

#[test]
fn execution_engine_begin_returns_context_when_authorized() {
    let p   = make_principal();
    let cap = make_capability(p.principal_id(), CapabilityAction::Capture, "twin:*");
    let ctx = ExecutionEngine::begin(
        p, Some(cap), CapabilityAction::Capture, "twin:abc", json!({}), now()
    );
    assert!(ctx.is_ok());
}

#[test]
fn execution_engine_begin_returns_denied_receipt_when_denied() {
    let p   = make_principal();
    let cap = make_capability("did:vantage:principal:other", CapabilityAction::Capture, "twin:*");
    let receipt = ExecutionEngine::begin(
        p, Some(cap), CapabilityAction::Capture, "twin:abc", json!({}), now()
    );
    assert!(receipt.is_err());
    let r = receipt.unwrap_err();
    assert_eq!(r.outcome, ActionOutcome::Denied);
}

#[test]
fn execution_context_complete_produces_success_receipt() {
    let p   = make_principal();
    let cap = make_capability(p.principal_id(), CapabilityAction::Capture, "twin:*");
    let ctx = ExecutionEngine::begin(
        p, Some(cap), CapabilityAction::Capture, "twin:abc", json!({"frames": 30}), now()
    ).unwrap();
    let receipt = ctx.complete(json!({"splat_id": "s-001"}), now() + 500);
    assert_eq!(receipt.outcome, ActionOutcome::Success);
    assert_eq!(receipt.action, CapabilityAction::Capture);
    assert!(receipt.error.is_none());
}

#[test]
fn execution_context_fail_produces_failure_receipt() {
    let p   = make_principal();
    let cap = make_capability(p.principal_id(), CapabilityAction::Simulate, "sim:*");
    let ctx = ExecutionEngine::begin(
        p, Some(cap), CapabilityAction::Simulate, "sim:orbit", json!({}), now()
    ).unwrap();
    let receipt = ctx.fail("sensor offline", now() + 100);
    assert_eq!(receipt.outcome, ActionOutcome::Failure);
    assert_eq!(receipt.error.as_deref(), Some("sensor offline"));
}

#[test]
fn execution_no_capability_still_runs() {
    let p = make_principal();
    let ctx = ExecutionEngine::begin(
        p, None, CapabilityAction::Read, "tile:odu:0a", json!({}), now()
    );
    assert!(ctx.is_ok());
}

// ── Evidence Bundle ───────────────────────────────────────────────────────

#[test]
fn evidence_bundle_merkle_root_is_deterministic() {
    use sovereign_runtime::evidence::{Evidence, EvidenceBundle, EvidenceKind};
    let mut b = EvidenceBundle::new();
    b.push(Evidence {
        evidence_id:  "ev-001".into(),
        kind:         EvidenceKind::ProofHash,
        content_hash: "sha256:abc".into(),
        uri:          None,
        metadata:     json!({}),
        captured_at:  now(),
    });
    let r1 = b.merkle_root();
    let r2 = b.merkle_root();
    assert_eq!(r1, r2);
    assert!(r1.starts_with("sha256:"));
}

#[test]
fn evidence_bundle_empty_has_distinct_root_from_non_empty() {
    use sovereign_runtime::evidence::{Evidence, EvidenceBundle, EvidenceKind};
    let empty = EvidenceBundle::new();
    let mut nonempty = EvidenceBundle::new();
    nonempty.push(Evidence {
        evidence_id:  "ev-001".into(),
        kind:         EvidenceKind::SensorCapture,
        content_hash: "sha256:deadbeef".into(),
        uri:          None,
        metadata:     json!({}),
        captured_at:  now(),
    });
    assert_ne!(empty.merkle_root(), nonempty.merkle_root());
}

// ── ActionReceipt → CanonicalReceipt projection ───────────────────────────

#[test]
fn action_receipt_projects_to_canonical() {
    let p   = make_principal();
    let cap = make_capability(p.principal_id(), CapabilityAction::Capture, "twin:*");
    let ctx = ExecutionEngine::begin(
        p, Some(cap), CapabilityAction::Capture, "twin:123", json!({}), now()
    ).unwrap();
    let receipt  = ctx.complete(json!({}), now() + 100);
    let canonical = receipt.to_canonical();
    assert_eq!(canonical.receipt_id, receipt.receipt_id);
    assert_eq!(canonical.identity.principal_id, receipt.principal_id);
}

// ── run_sovereign helper ──────────────────────────────────────────────────

#[tokio::test]
async fn run_sovereign_success_path() {
    let p   = make_principal();
    let cap = make_capability(p.principal_id(), CapabilityAction::Publish, "event:*");
    let receipt = run_sovereign(
        p, Some(cap), CapabilityAction::Publish, "event:001", json!({}), now(),
        |ctx| async move { (ctx, Ok(json!({"published": true}))) },
    ).await;
    assert_eq!(receipt.outcome, ActionOutcome::Success);
}

#[tokio::test]
async fn run_sovereign_denied_returns_denied_receipt() {
    let p   = make_principal();
    let cap = make_capability("did:vantage:principal:other", CapabilityAction::Admin, "node:*");
    let receipt = run_sovereign(
        p, Some(cap), CapabilityAction::Admin, "node:1", json!({}), now(),
        |ctx| async move { (ctx, Ok(json!({}))) },
    ).await;
    assert_eq!(receipt.outcome, ActionOutcome::Denied);
}
