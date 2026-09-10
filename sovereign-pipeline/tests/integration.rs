//! Integration tests for sovereign-pipeline.
//!
//! Tests run the full VCP → TSP capture pipeline with stub drivers and validate:
//!   - CaptureReceipt (31020) structure and F1 gate
//!   - SceneReceipt (31030) content-addressing and provenance links
//!   - DIP envelope construction from scene receipt
//!   - VCP session lifecycle (open → commands → close)
//!   - Splat engine frame splitting (unit test)

use sovereign_pipeline::{
    capture_pipeline::{CapturePipeline, Go2CaptureDriver, PipelineConfig},
};
use sovereign_types::{IdentityChain, crypto::{generate_keypair, did_from_pubkey}};
use vcp::{
    VcpSession, VcpCapabilityGrant,
    handshake::{VcpCapabilityRequest, VcpDuration},
    adapters::{Go2Adapter, Go2ConnectionMode},
};

// ─── helpers ─────────────────────────────────────────────────────────────────

fn make_keys() -> (String, String, String) {
    let (private_key, pub_key) = generate_keypair();
    let did = did_from_pubkey(&pub_key, "agent");
    (private_key, pub_key, did)
}

fn make_grant(device_key: &str, agent_key: &str, agent_did: &str) -> VcpCapabilityGrant {
    let adapter  = Go2Adapter::new("unitree:go2:test-integration", Go2ConnectionMode::default());
    let manifest = adapter.manifest("base64url:fakepub");
    let identity = IdentityChain::new(agent_did.into(), agent_did.into());
    let request  = VcpCapabilityRequest::new(
        identity,
        vec!["camera".into(), "lidar".into(), "telemetry".into()],
        "twin_capture",
        VcpDuration::minutes(60),
        agent_key,
    ).unwrap();
    VcpCapabilityGrant::issue(&manifest, &request, device_key).unwrap()
}

fn make_pipeline(agent_key: &str, agent_did: &str) -> CapturePipeline {
    let identity = IdentityChain::new(agent_did.into(), agent_did.into());
    CapturePipeline::new(
        PipelineConfig { owner_did: agent_did.into(), ..Default::default() },
        agent_key,
        identity,
    )
}

// ─── pipeline integration tests ──────────────────────────────────────────────

#[test]
fn pipeline_produces_capture_and_scene_receipts() {
    let (agent_key, _, agent_did) = make_keys();
    let (device_key, _, _)        = make_keys();
    let grant   = make_grant(&device_key, &agent_key, &agent_did);
    let session = VcpSession::new(grant);
    let out = make_pipeline(&agent_key, &agent_did)
        .run(session, &Go2CaptureDriver::default())
        .expect("pipeline should succeed");

    assert_eq!(out.capture_receipt.kind, 31020, "capture receipt must be kind 31020");
    assert_eq!(out.scene_receipt.kind,   31030, "scene receipt must be kind 31030");
}

#[test]
fn capture_receipt_f1_gate_enforced() {
    let (agent_key, _, agent_did) = make_keys();
    let (device_key, _, _)        = make_keys();
    let grant   = make_grant(&device_key, &agent_key, &agent_did);
    let session = VcpSession::new(grant);
    let out = make_pipeline(&agent_key, &agent_did)
        .run(session, &Go2CaptureDriver::default())
        .expect("pipeline run");

    assert!(out.capture_receipt.f1_score >= 0.777,
        "F1 score {:.3} below 0.777 gate", out.capture_receipt.f1_score);
}

#[test]
fn scene_receipt_provenance_links_capture_receipt() {
    let (agent_key, _, agent_did) = make_keys();
    let (device_key, _, _)        = make_keys();
    let grant   = make_grant(&device_key, &agent_key, &agent_did);
    let session = VcpSession::new(grant);
    let out = make_pipeline(&agent_key, &agent_did)
        .run(session, &Go2CaptureDriver::default())
        .expect("pipeline run");

    assert!(
        out.scene_receipt.capture_receipt_ids.contains(&out.capture_receipt.receipt_id),
        "scene receipt must reference capture receipt"
    );
    assert_eq!(out.scene_receipt.twin_id, out.twin.twin_id,
        "scene receipt twin_id must match TwinAsset twin_id");
}

#[test]
fn scene_receipt_splat_hash_present() {
    let (agent_key, _, agent_did) = make_keys();
    let (device_key, _, _)        = make_keys();
    let grant   = make_grant(&device_key, &agent_key, &agent_did);
    let session = VcpSession::new(grant);
    let out = make_pipeline(&agent_key, &agent_did)
        .run(session, &Go2CaptureDriver::default())
        .expect("pipeline run");

    assert!(!out.scene_receipt.splat_hash.is_empty(), "splat_hash must be set");
    assert!(out.scene_receipt.splat_hash.starts_with("sha256:"),
        "splat_hash must be prefixed sha256:");
}

#[test]
fn twin_asset_twin_id_is_content_addressed() {
    let (agent_key, _, agent_did) = make_keys();
    let (device_key, _, _)        = make_keys();
    let grant   = make_grant(&device_key, &agent_key, &agent_did);
    let session = VcpSession::new(grant);
    let out = make_pipeline(&agent_key, &agent_did)
        .run(session, &Go2CaptureDriver::default())
        .expect("pipeline run");

    assert!(out.twin.twin_id.starts_with("twin:sha256:"),
        "twin_id must be content-addressed: {}", out.twin.twin_id);
}

#[test]
fn session_receipt_commands_tracked() {
    let (agent_key, _, agent_did) = make_keys();
    let (device_key, _, _)        = make_keys();
    let grant   = make_grant(&device_key, &agent_key, &agent_did);
    let session = VcpSession::new(grant);
    let out = make_pipeline(&agent_key, &agent_did)
        .run(session, &Go2CaptureDriver::default())
        .expect("pipeline run");

    assert_eq!(out.session_receipt.commands_issued,  3, "three VCP commands expected");
    assert_eq!(out.session_receipt.commands_success, 3, "all commands should succeed");
    assert!(!out.session_receipt.evidence_ids.is_empty(), "session must link capture receipt");
}

// ─── DIP envelope construction ───────────────────────────────────────────────

#[test]
fn dip_round_trip_envelope() {
    use dip::{DipEnvelope, DipKind};
    use dip::address::DipAddress;

    let (sender_key, _, sender_did) = make_keys();
    let (_, _, recipient_did) = make_keys();

    let identity = IdentityChain::new(sender_did.clone(), sender_did.clone());

    let env = DipEnvelope::build(
        DipAddress::vantage(&sender_did),
        DipAddress::vantage(&recipient_did),
        identity,
        DipKind::Receipt,
        serde_json::json!({ "receipt_id": "test-receipt-001", "kind": 31020 }),
        3600,
        &sender_key,
    ).expect("envelope build");

    assert!(!env.message_id.is_empty());
    assert_eq!(env.origin.address,      sender_did);
    assert_eq!(env.destination.address, recipient_did);
    assert!(matches!(env.kind, DipKind::Receipt));

    // Round-trip through JSON
    let json  = serde_json::to_string(&env).expect("serialize");
    let back: DipEnvelope = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.message_id,         env.message_id);
    assert_eq!(back.origin.address,     env.origin.address);
}

// ─── VCP session tests ───────────────────────────────────────────────────────

#[test]
fn vcp_session_lifecycle() {
    use vcp::{VcpSession, VcpSessionOutcome};

    let (agent_key, _, agent_did) = make_keys();
    let (device_key, _, _)        = make_keys();
    let grant   = make_grant(&device_key, &agent_key, &agent_did);
    let mut session = VcpSession::new(grant);

    session.record_command("camera",    true);
    session.record_command("lidar",     true);
    session.record_command("telemetry", false);

    let receipt = session.close(VcpSessionOutcome::Completed, &agent_key);
    assert_eq!(receipt.commands_issued,  3);
    assert_eq!(receipt.commands_success, 2);
    assert!(matches!(receipt.outcome, vcp::VcpSessionOutcome::Completed));
}

// ─── Splat frame splitting unit test ─────────────────────────────────────────

#[test]
fn jpeg_frame_splitting_counts_markers() {
    use std::path::PathBuf;
    use sovereign_pipeline::splat_engine::{SplatConfig, run_splat};

    // Build a fake 3-frame JPEG stream: three minimal JPEG stubs (SOI + data + EOI)
    let make_jpeg = |id: u8| -> Vec<u8> {
        vec![0xFF, 0xD8, 0xFF, 0xE0, id, 0xFF, 0xD9]
    };
    let mut stream = make_jpeg(1);
    stream.extend(make_jpeg(2));
    stream.extend(make_jpeg(3));

    let tmp = std::env::var("TMPDIR").unwrap_or_else(|_| "/tmp".into());
    let output_dir = PathBuf::from(format!("{tmp}/sovereign-test-splat"));
    let cfg = SplatConfig {
        bin:        "nonexistent-ns-train".into(),
        output_dir: output_dir.clone(),
        steps:      1,
        method:     "nerfacto".into(),
    };

    // run_splat writes frames first, then fails at ns-train (not on PATH)
    let result = run_splat(&stream, 3, "test-twin", &cfg);
    assert!(result.is_err(), "ns-train not on PATH — expected training failure");

    // Verify the 3 JPEG frame files were written (twin_id "test-twin" → safe "test-twin")
    let frames_dir = output_dir.join("test-twin").join("images");
    let frame_count = std::fs::read_dir(&frames_dir)
        .expect("images dir should exist after write_jpeg_frames")
        .flatten()
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("jpg"))
        .count();
    assert_eq!(frame_count, 3, "expected 3 JPEG frame files, got {frame_count}");
}
