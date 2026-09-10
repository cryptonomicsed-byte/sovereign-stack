/// Tests for machine-facing sovereign-runtime modules:
/// device, safety, power, embodiment, and unified ActionReceipt.

use sovereign_runtime::receipt::{ActionReceipt, ActionOutcome, PoCWProof, EpistemicSeverity, SimPlane};
use sovereign_runtime::chain::{Principal, CapabilityAction, ExecutionEngine};
use sovereign_runtime::safety::{SafetySupervisor, SafetyTier, SafetyCommand, SafetyCommandType, SafetyVerdict};
use sovereign_runtime::power::{PowerSnapshot, PowerState, PowerSource, PowerPolicy};
use sovereign_runtime::embodiment::{StubEmbodiment, EmbodimentManager, SensorKind};
use sovereign_runtime::device::{SoftwareKeyProvider, HardwareKeyProvider, KeyProviderKind};
use sovereign_types::identity::SafetyLevel;
use serde_json::json;

fn now() -> u64 { 1_700_000_000_000 }

fn make_principal() -> Principal {
    Principal::from_did(
        "did:vantage:principal:abc123".into(),
        "did:vantage:agent:xyz".into(),
    )
}

// ── PoCWProof ─────────────────────────────────────────────────────────────

#[test]
fn pocw_valid_when_steps_meet_bb_bound() {
    let p = PoCWProof { steps: 21, bb_bound: 21, tape_hash: "abc".into() };
    assert!(p.is_valid());
}

#[test]
fn pocw_invalid_when_steps_below_bound() {
    let p = PoCWProof { steps: 20, bb_bound: 21, tape_hash: "abc".into() };
    assert!(!p.is_valid());
}

#[test]
fn pocw_invalid_with_empty_tape_hash() {
    let p = PoCWProof { steps: 21, bb_bound: 21, tape_hash: String::new() };
    assert!(!p.is_valid());
}

#[test]
fn pocw_min_for_tier_1_is_bb3() {
    assert_eq!(PoCWProof::min_for_tier(1), 21);
}

#[test]
fn pocw_min_for_tier_2_is_bb4() {
    assert_eq!(PoCWProof::min_for_tier(2), 107);
}

#[test]
fn pocw_min_for_tier_3_is_bb5() {
    assert_eq!(PoCWProof::min_for_tier(3), 47_176_870);
}

// ── ActionReceipt with PoCW + Epistemic ──────────────────────────────────

#[test]
fn action_receipt_defaults_to_physical_plane_and_no_pocw() {
    let p   = make_principal();
    let ctx = ExecutionEngine::begin(
        p, None, CapabilityAction::Capture, "twin:001", json!({}), now()
    ).unwrap();
    let r = ctx.complete(json!({}), now() + 100);
    assert_eq!(r.plane, SimPlane::Physical);
    assert!(r.proof_of_work.is_none());
    assert!(r.epistemic_severity.is_none());
    assert!(r.previous_hash.is_none());
}

#[test]
fn action_receipt_meets_pocw_floor_tier_0_always() {
    let p   = make_principal();
    let ctx = ExecutionEngine::begin(
        p, None, CapabilityAction::Execute, "sim:001", json!({}), now()
    ).unwrap();
    let r = ctx.complete(json!({}), now());
    assert!(r.meets_pocw_floor(0));
}

#[test]
fn action_receipt_fails_pocw_floor_tier_1_without_proof() {
    let p   = make_principal();
    let ctx = ExecutionEngine::begin(
        p, None, CapabilityAction::Execute, "sim:001", json!({}), now()
    ).unwrap();
    let r = ctx.complete(json!({}), now());
    assert!(!r.meets_pocw_floor(1));
}

#[test]
fn action_receipt_meets_pocw_floor_with_valid_proof() {
    let p   = make_principal();
    let ctx = ExecutionEngine::begin(
        p, None, CapabilityAction::Execute, "sim:001", json!({}), now()
    ).unwrap();
    let proof = PoCWProof { steps: 107, bb_bound: 107, tape_hash: "abc".into() };
    let r = ctx.complete(json!({}), now()).with_pocw(proof);
    assert!(r.meets_pocw_floor(2));
}

#[test]
fn action_receipt_chains_via_previous_hash() {
    let p1  = make_principal();
    let ctx = ExecutionEngine::begin(
        p1, None, CapabilityAction::Read, "tile:odu:0a", json!({}), now()
    ).unwrap();
    let r1 = ctx.complete(json!({}), now());
    let r1_id = r1.receipt_id.clone();

    let p2  = make_principal();
    let ctx2 = ExecutionEngine::begin(
        p2, None, CapabilityAction::Write, "tile:odu:0a", json!({}), now()
    ).unwrap();
    let r2 = ctx2.complete(json!({}), now()).with_previous(r1_id.clone());

    assert!(r2.previous_hash.is_some());
    // previous_hash is sha256 of r1_id — not the raw id but a deterministic hash
    let prev = r2.previous_hash.unwrap();
    assert!(prev.starts_with("sha256:"));
}

#[test]
fn action_receipt_epistemic_severity_attached() {
    let p   = make_principal();
    let ctx = ExecutionEngine::begin(
        p, None, CapabilityAction::Publish, "event:001", json!({}), now()
    ).unwrap();
    let r = ctx.complete(json!({}), now())
        .with_epistemic(EpistemicSeverity::Moderate);
    assert_eq!(r.epistemic_severity, Some(EpistemicSeverity::Moderate));
}

#[test]
fn action_receipt_plane_upgradeable() {
    let p   = make_principal();
    let ctx = ExecutionEngine::begin(
        p, None, CapabilityAction::Simulate, "sim:orbit", json!({}), now()
    ).unwrap();
    let r = ctx.complete(json!({}), now()).with_plane(SimPlane::Verified);
    assert_eq!(r.plane, SimPlane::Verified);
}

// ── Safety Supervisor ─────────────────────────────────────────────────────

fn move_cmd(magnitude: f32) -> SafetyCommand {
    SafetyCommand {
        command_type:  SafetyCommandType::Move,
        target:        "robot:arm".into(),
        magnitude,
        requester_did: "did:vantage:principal:abc123".into(),
        safety_level:  SafetyLevel::Standard,
    }
}

#[test]
fn safety_production_clears_normal_magnitude() {
    let sup = SafetySupervisor::new(SafetyTier::Production);
    let v   = sup.evaluate(&move_cmd(0.5));
    assert!(v.is_clear());
}

#[test]
fn safety_production_blocks_high_magnitude() {
    let sup = SafetySupervisor::new(SafetyTier::Production);
    let v   = sup.evaluate(&move_cmd(0.9));
    assert!(v.is_blocked());
}

#[test]
fn safety_critical_zone_blocks_above_0_25() {
    let sup = SafetySupervisor::new(SafetyTier::CriticalZone);
    let v   = sup.evaluate(&move_cmd(0.3));
    assert!(v.is_blocked());
}

#[test]
fn safety_critical_zone_allows_low_magnitude() {
    let sup = SafetySupervisor::new(SafetyTier::CriticalZone);
    let v   = sup.evaluate(&move_cmd(0.2));
    assert!(v.is_clear());
}

#[test]
fn safety_emergency_stop_blocks_everything() {
    let mut sup = SafetySupervisor::new(SafetyTier::Production);
    sup.emergency_stop();
    let v = sup.evaluate(&move_cmd(0.1));
    assert!(v.is_blocked());
    assert!(matches!(v, SafetyVerdict::Halt { .. }));
}

#[test]
fn safety_development_tier_conditionally_allows_all() {
    let sup = SafetySupervisor::new(SafetyTier::Development);
    let v   = sup.evaluate(&move_cmd(0.99));
    assert!(v.is_clear()); // dev tier allows everything (with logging)
}

#[test]
fn safety_critical_safety_level_blocked_in_production() {
    let sup = SafetySupervisor::new(SafetyTier::Production);
    let cmd = SafetyCommand {
        command_type:  SafetyCommandType::Actuate,
        target:        "motor:hip".into(),
        magnitude:     0.5,
        requester_did: "did:vantage:principal:abc".into(),
        safety_level:  SafetyLevel::Critical,
    };
    let v = sup.evaluate(&cmd);
    assert!(v.is_blocked());
}

// ── Power ─────────────────────────────────────────────────────────────────

fn normal_power() -> PowerSnapshot {
    PowerSnapshot {
        state:          PowerState::Normal,
        source:         PowerSource::Mains,
        battery_pct:    Some(80),
        charge_rate_mw: Some(100),
        temperature_c:  Some(35.0),
    }
}

fn low_battery() -> PowerSnapshot {
    PowerSnapshot {
        state:          PowerState::LowBattery,
        source:         PowerSource::Battery,
        battery_pct:    Some(8),
        charge_rate_mw: Some(-500),
        temperature_c:  Some(30.0),
    }
}

#[test]
fn power_normal_allows_capture() {
    assert!(PowerPolicy::allows_capture(&normal_power()));
}

#[test]
fn power_normal_allows_gaussian_splat() {
    assert!(PowerPolicy::allows_gaussian_splat(&normal_power()));
}

#[test]
fn power_low_battery_disallows_embodiment() {
    assert!(!PowerPolicy::allows_embodiment(&low_battery()));
}

#[test]
fn power_low_battery_is_critical() {
    assert!(low_battery().is_critical());
}

#[test]
fn power_full_power_allows_gpu() {
    let snap = PowerSnapshot {
        state: PowerState::FullPower,
        source: PowerSource::Mains,
        battery_pct: None,
        charge_rate_mw: None,
        temperature_c: None,
    };
    assert!(snap.state.allows_gpu());
}

#[test]
fn power_hibernate_disallows_camera() {
    let snap = PowerSnapshot {
        state: PowerState::Hibernate,
        source: PowerSource::Battery,
        battery_pct: Some(50),
        charge_rate_mw: None,
        temperature_c: None,
    };
    assert!(!snap.state.allows_camera());
}

// ── Embodiment ────────────────────────────────────────────────────────────

#[test]
fn stub_embodiment_has_camera_sensor() {
    let e = StubEmbodiment::portable("did:vantage:device:stub");
    let s = e.descriptor().sensor_by_kind(&SensorKind::Camera);
    assert!(s.is_some());
}

#[test]
fn stub_embodiment_is_not_in_motion() {
    let e = StubEmbodiment::portable("did:vantage:device:stub");
    assert!(!e.is_in_motion());
}

#[test]
fn stub_embodiment_read_sensor_ok() {
    let e   = StubEmbodiment::portable("did:vantage:device:stub");
    let cam = e.descriptor().sensors.first().unwrap().sensor_id.clone();
    let v   = e.read_sensor(&cam);
    assert!(v.is_ok());
}

#[test]
fn stub_embodiment_read_unknown_sensor_errors() {
    let e = StubEmbodiment::portable("did:vantage:device:stub");
    let v = e.read_sensor("nonexistent:sensor");
    assert!(v.is_err());
}

#[test]
fn stub_embodiment_not_spatial_capable() {
    let e = StubEmbodiment::portable("did:vantage:device:stub");
    assert!(!e.descriptor().spatial_capable());
}

// ── SoftwareKeyProvider ───────────────────────────────────────────────────

#[test]
fn software_key_provider_kind_is_software() {
    let kp = SoftwareKeyProvider::ephemeral();
    assert_eq!(kp.provider_kind(), KeyProviderKind::Software);
}

#[test]
fn software_key_provider_sign_and_verify_roundtrip() {
    let kp  = SoftwareKeyProvider::ephemeral();
    let msg = b"sovereign principal attestation";
    let sig = kp.sign(msg).unwrap();
    assert!(kp.verify(msg, &sig).unwrap());
}

#[test]
fn software_key_provider_verify_fails_wrong_message() {
    let kp  = SoftwareKeyProvider::ephemeral();
    let sig = kp.sign(b"message").unwrap();
    assert!(!kp.verify(b"different", &sig).unwrap());
}

#[test]
fn software_key_provider_attest_returns_hex() {
    let kp  = SoftwareKeyProvider::ephemeral();
    let att = kp.attest("did:vantage:principal:abc", "nonce123").unwrap();
    assert!(!att.is_empty());
    // must be valid hex
    hex::decode(&att).expect("attestation must be valid hex");
}

#[test]
fn software_key_provider_derive_different_paths_differ() {
    let kp = SoftwareKeyProvider::ephemeral();
    let k1 = kp.derive("m/0/0").unwrap();
    let k2 = kp.derive("m/0/1").unwrap();
    assert_ne!(k1, k2);
}
