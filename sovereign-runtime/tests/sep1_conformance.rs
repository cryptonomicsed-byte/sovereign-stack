//! SEP-1 conformance gate — the protocol obligations from
//! `sovereign-stack/CANONICAL_PILLAR_CONTRACT.md`.
//!
//! The canonical architecture states: *"Every state transition MUST produce an
//! `ActionReceipt`"* and lists eight required fields. It further states that a
//! consequential action which does not emit one is a **P0 bug**.
//!
//! These tests are that obligation made executable. They exist so the rule can
//! fail a build rather than relying on convention — which is how it was
//! previously violated (four of the eight required fields were simply absent).
//!
//! Required fields, per the contract:
//!   receipt_id, work_id, principal_id, agent_id,
//!   action, input_hash, output_hash, timestamp_ms, signature

use serde_json::json;
use sovereign_runtime::chain::{CapabilityAction, ExecutionEngine, Principal};
use sovereign_runtime::receipt::{
    ActionOutcome, ActionReceipt, Confidence, EnsembleDisagreement, PoCWProof, SimPlane,
};
use sovereign_types::proof::{blake3_hex, blake3_of_json};
use sovereign_types::work_id::{ActionReceipt as Sep1ActionReceipt, WorkId};

fn now() -> u64 {
    1_700_000_000_000
}

fn principal() -> Principal {
    Principal::from_did(
        "did:vantage:principal:sep1test".to_string(),
        "did:vantage:agent:sep1test".to_string(),
    )
}

fn ctx() -> sovereign_runtime::chain::ExecutionContext {
    ExecutionEngine::begin(
        principal(),
        None,
        CapabilityAction::Capture,
        "res://sep1/target",
        json!({ "hint": "splat" }),
        now(),
    )
    .expect("begin should succeed for a valid principal")
}

// ── Required-field presence ───────────────────────────────────────────────────

#[test]
fn completed_receipt_carries_every_sep1_field() {
    let r = ctx().complete(json!({ "ok": true }), now() + 10);

    assert!(!r.receipt_id.is_empty(), "receipt_id missing");
    assert!(!r.work_id.as_str().is_empty(), "work_id missing");
    assert!(!r.principal_id.is_empty(), "principal_id missing");
    assert!(!r.agent_id.is_empty(), "agent_id missing");
    assert!(!r.input_hash.is_empty(), "input_hash missing");
    assert!(!r.output_hash.is_empty(), "output_hash missing");
    assert!(r.completed_at > 0, "timestamp_ms missing");
}

#[test]
fn work_id_is_well_formed() {
    let r = ctx().complete(json!({}), now());
    assert_eq!(r.work_id.namespace(), Some("real"));
    assert!(
        r.work_id.as_str().starts_with("wk:real:"),
        "work_id must be wk:{{namespace}}:{{id}}, got {}",
        r.work_id
    );
}

// ── Derivation, not assignment ────────────────────────────────────────────────

#[test]
fn receipt_id_is_blake3_over_canonical_fields() {
    let r = ctx().complete(json!({ "v": 1 }), now());

    // Recompute exactly as SEP-1 specifies and compare.
    let expected = Sep1ActionReceipt::compute_id(
        &r.work_id,
        &r.principal_id,
        &r.agent_id,
        &format!("{:?}", r.action),
        &r.input_hash,
        &r.output_hash,
        r.completed_at,
    );
    assert_eq!(
        r.receipt_id, expected,
        "receipt_id is not BLAKE3 of the canonical fields"
    );
}

#[test]
fn input_and_output_hashes_are_blake3_of_the_payloads() {
    let params = json!({ "hint": "splat" });
    let result = json!({ "ok": true });

    let r = ExecutionEngine::begin(
        principal(),
        None,
        CapabilityAction::Capture,
        "res://sep1/target",
        params.clone(),
        now(),
    )
    .unwrap()
    .complete(result.clone(), now() + 10);

    assert_eq!(r.input_hash, blake3_of_json(&params));
    assert_eq!(r.output_hash, blake3_of_json(&result));
}

#[test]
fn receipt_id_changes_when_payload_changes() {
    let a = ctx().complete(json!({ "v": 1 }), now());
    let b = ctx().complete(json!({ "v": 2 }), now());
    assert_ne!(a.receipt_id, b.receipt_id, "payload is not covered by receipt_id");
}

// ── Signature ─────────────────────────────────────────────────────────────────

#[test]
fn signing_populates_signature_and_makes_receipt_conformant() {
    let (private_b64, _public_b64) = sovereign_types::crypto::generate_keypair();
    let mut r = ctx().complete(json!({ "ok": true }), now());

    assert!(!r.is_signed());
    assert!(
        !r.is_sep1_conformant(),
        "an unsigned completed action must NOT be SEP-1 conformant"
    );

    r.sign(&private_b64).expect("signing should succeed");

    assert!(r.is_signed());
    assert!(r.is_sep1_conformant());
}

#[test]
fn signing_fails_on_a_malformed_key() {
    let mut r = ctx().complete(json!({}), now());
    assert!(r.sign("not-a-valid-base64-key").is_err());
}

#[test]
fn denial_is_conformant_without_a_signature() {
    // A refusal has no result to attest, so it needs no signature — but it
    // still must carry every required field.
    let r = ActionReceipt::denied(
        &principal(),
        CapabilityAction::Execute,
        "res://sep1/denied",
        "capability not held",
        now(),
    );
    assert_eq!(r.outcome, ActionOutcome::Denied);
    assert!(r.is_sep1_conformant());
}

// ── Tamper detection ──────────────────────────────────────────────────────────

#[test]
fn tampering_with_receipt_id_breaks_conformance() {
    let mut r = ctx().complete(json!({}), now());
    r.receipt_id = "deadbeef".repeat(8);
    assert!(
        !r.is_sep1_conformant(),
        "a receipt_id that does not match its contents must be rejected"
    );
}

#[test]
fn tampering_with_recomputed_id_detects_content_change() {
    let (private_b64, _) = sovereign_types::crypto::generate_keypair();
    let mut r = ctx().complete(json!({ "amount": 100 }), now());
    r.sign(&private_b64).unwrap();
    assert!(r.is_sep1_conformant());

    // Mutate the payload AFTER signing without re-deriving the id.
    r.result = json!({ "amount": 999_999 });
    assert!(
        !r.is_sep1_conformant(),
        "mutating a signed payload must invalidate conformance"
    );
}

#[test]
fn malformed_work_id_is_rejected() {
    let mut r = ctx().complete(json!({}), now());
    r.work_id = WorkId("garbage".to_string());
    assert!(!r.is_sep1_conformant());
}

// ── Canonical projection integrity ────────────────────────────────────────────

#[test]
fn to_canonical_carries_previous_hash_not_a_placeholder() {
    let prev = ctx().complete(json!({}), now());
    let cur = ctx()
        .complete(json!({}), now() + 5)
        .with_previous(prev.receipt_id.clone());

    let canonical = cur.to_canonical();

    // Regression guard: the previous revision wrote the literal "sha256:" here,
    // discarding the real chain link and breaking SRP-1 on every round trip.
    assert_eq!(
        canonical.previous_hash,
        cur.previous_hash.clone().unwrap(),
        "to_canonical dropped the real previous_hash"
    );
    assert!(canonical.previous_hash.starts_with("blake3:"));
}

#[test]
fn to_canonical_uses_genesis_marker_when_unchained() {
    let r = ctx().complete(json!({}), now());
    let canonical = r.to_canonical();
    assert_eq!(
        canonical.previous_hash,
        sovereign_runtime::receipt::GENESIS_PREVIOUS_HASH
    );
}

#[test]
fn to_canonical_carries_the_signature_through() {
    let (private_b64, _) = sovereign_types::crypto::generate_keypair();
    let mut r = ctx().complete(json!({}), now());
    r.sign(&private_b64).unwrap();

    let canonical = r.to_canonical();

    // Regression guard: the previous revision emitted String::new() here.
    assert!(!canonical.signature.is_empty(), "to_canonical dropped the signature");
    assert_eq!(canonical.signature, r.signature);
    assert_eq!(canonical.receipt_id, r.receipt_id);
}

#[test]
fn to_canonical_does_not_fabricate_evidence() {
    let r = ctx().complete(json!({}), now());
    assert_eq!(r.evidence_count, 0);
    let canonical = r.to_canonical();
    assert!(
        canonical.evidence_ids.is_empty(),
        "no evidence was attached, so none may be projected"
    );
}

// ── Two-axis epistemic separation ─────────────────────────────────────────────

#[test]
fn confidence_and_ensemble_disagreement_are_independent_axes() {
    let r = ctx()
        .complete(json!({}), now())
        .with_confidence(Confidence::Observed)
        .with_ensemble_disagreement(EnsembleDisagreement::Severe);

    assert_eq!(r.epistemic_severity, Some(Confidence::Observed));
    assert_eq!(r.ensemble_disagreement, Some(EnsembleDisagreement::Severe));

    // Both signals are retained rather than one overwriting the other.
    assert!(EnsembleDisagreement::Severe.is_blocking());
    assert!(Confidence::Observed.is_economically_assertable());
}

// ── Chain linkage ─────────────────────────────────────────────────────────────

#[test]
fn previous_hash_is_blake3_of_the_prior_receipt_id() {
    let prev = ctx().complete(json!({}), now());
    let cur = ctx()
        .complete(json!({}), now() + 1)
        .with_previous(prev.receipt_id.clone());

    assert_eq!(
        cur.previous_hash.clone().unwrap(),
        blake3_hex(prev.receipt_id.as_bytes())
    );
}

// ── PoCW floor ────────────────────────────────────────────────────────────────

#[test]
fn pocw_floor_blocks_tier_elevation_without_a_proof() {
    let r = ctx().complete(json!({}), now());
    assert!(r.meets_pocw_floor(0));
    assert!(!r.meets_pocw_floor(1));
    assert!(!r.meets_pocw_floor(3));
}

#[test]
fn pocw_floor_clears_with_a_sufficient_proof() {
    let proof = PoCWProof::new(47_176_870, 47_176_870, blake3_hex(b"tape"));
    let r = ctx().complete(json!({}), now()).with_pocw(proof);
    assert!(r.meets_pocw_floor(3));
}

#[test]
fn plane_defaults_to_unverified() {
    let r = ctx().complete(json!({}), now());
    assert_eq!(r.plane, SimPlane::Physical);
    assert!(!r.plane.is_verified());

    let upgraded = ctx().complete(json!({}), now()).with_plane(SimPlane::Verified);
    assert!(upgraded.plane.is_verified());
}
