//! VCP session → TSP capture pipeline.
//!
//! Orchestrates the full physical capture flow:
//!
//!   1. Open VCP session from an existing CapabilityGrant
//!   2. Issue camera/lidar/telemetry commands to the Go2 via VCP
//!   3. Collect raw frame/point-cloud data
//!   4. Build CaptureReceipt (31020) — hard F1 gate enforced
//!   5. Assemble TwinAsset from content-addressed hashes
//!   6. Build SceneReceipt (31030) — IP Root creation event
//!   7. Close VCP session, link 31020 receipt as session evidence
//!
//! The capture driver is stubbed — production replaces it with real
//! Go2 WebSocket reads and a nerfstudio/gaussian-splatting pipeline.

use std::collections::BTreeMap;

use sovereign_types::{IdentityChain, Modality, PrivacyFlag};
use vcp::{
    VcpSession, VcpSessionOutcome, VcpSessionReceipt,
    VcpCapabilityGrant, VcpCommand,
};
use twin_protocol::{
    CaptureReceipt, SceneReceipt, TwinAsset,
    TwinDataHashes, TwinProvenance, TwinLicenseConfig,
    TwinQuality, TwinRegion,
};
use crate::error::PipelineResult;

/// Raw bytes collected from the device in one capture run.
#[derive(Debug, Default)]
pub struct RawCapture {
    pub rgb_frames:     Option<Vec<u8>>,
    pub lidar_cloud:    Option<Vec<u8>>,
    pub imu_log:        Option<Vec<u8>>,
    /// Gaussian splat bytes (.ply) — assembled by splatting engine post-capture
    pub splat:          Option<Vec<u8>>,
    pub frame_count:    u32,
    pub duration_ms:    u64,
    /// F1 score from reconstruction engine
    pub f1_score:       f32,
    pub coverage_pct:   f32,
    pub novelty_score:  f32,
    pub delta_coverage: f32,
}

impl RawCapture {
    pub fn to_raw_data_map(&self) -> BTreeMap<String, Vec<u8>> {
        let mut m = BTreeMap::new();
        if let Some(b) = &self.rgb_frames  { m.insert("rgb".into(),   b.clone()); }
        if let Some(b) = &self.lidar_cloud { m.insert("lidar".into(), b.clone()); }
        if let Some(b) = &self.imu_log     { m.insert("imu".into(),   b.clone()); }
        if let Some(b) = &self.splat       { m.insert("splat".into(), b.clone()); }
        m
    }

    pub fn active_modalities(&self) -> Vec<Modality> {
        let mut mods = vec![];
        if self.rgb_frames.is_some()  { mods.push(Modality::Rgb); }
        if self.lidar_cloud.is_some() { mods.push(Modality::Lidar); }
        if self.imu_log.is_some()     { mods.push(Modality::Imu); }
        mods
    }
}

/// Stub capture driver — issues VCP commands and returns synthetic raw data.
/// In production: sends to Go2 WebSocket, runs nerfstudio, returns real bytes.
pub struct Go2CaptureDriver;

impl Go2CaptureDriver {
    pub fn capture(
        &self,
        grant:     &VcpCapabilityGrant,
        agent_key: &str,
    ) -> PipelineResult<(RawCapture, Vec<(&'static str, bool)>)> {
        let start = now_ms();

        // Issue Go2 commands (stubs — production sends to WebSocket)
        let _cam_cmd  = VcpCommand::new(grant, "camera",    "capture_frame",    serde_json::json!({}), agent_key)?;
        let _lidar_cmd = VcpCommand::new(grant, "lidar",    "capture_pointcloud", serde_json::json!({}), agent_key)?;
        let _telem_cmd = VcpCommand::new(grant, "telemetry","poll",             serde_json::json!({}), agent_key)?;

        let end = now_ms();

        let raw = RawCapture {
            rgb_frames:    Some(b"STUB_RGB_FRAME_DATA".to_vec()),
            lidar_cloud:   Some(b"STUB_LIDAR_PCD_DATA".to_vec()),
            imu_log:       Some(b"STUB_IMU_LOG_DATA".to_vec()),
            splat:         Some(b"STUB_SPLAT_PLY_DATA".to_vec()),
            frame_count:   120,
            duration_ms:   end.saturating_sub(start).max(1),
            f1_score:      0.832,
            coverage_pct:  78.4,
            novelty_score: 0.61,
            delta_coverage: 12.3,
        };

        // (capability, success) pairs for session tracking
        let recorded = vec![
            ("camera", true),
            ("lidar", true),
            ("telemetry", true),
        ];

        Ok((raw, recorded))
    }
}

/// Full output of the capture pipeline.
pub struct PipelineOutput {
    pub capture_receipt: CaptureReceipt,
    pub twin:            TwinAsset,
    pub scene_receipt:   SceneReceipt,
    pub session_receipt: VcpSessionReceipt,
}

/// Configuration for a pipeline run.
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    pub region:                TwinRegion,
    pub reconstruction_engine: String,
    pub owner_did:             String,
    pub privacy_flags:         Vec<PrivacyFlag>,
    pub license:               TwinLicenseConfig,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            region:                TwinRegion::new(0.0, 0.0, 0.001, 0.001),
            reconstruction_engine: "nerfstudio/gaussian-splatting".into(),
            owner_did:             "did:vantage:principal:default".into(),
            privacy_flags:         vec![],
            license:               TwinLicenseConfig::default(),
        }
    }
}

/// Orchestrates the full VCP → TSP pipeline.
pub struct CapturePipeline {
    pub config:    PipelineConfig,
    pub agent_key: String,
    pub identity:  IdentityChain,
}

impl CapturePipeline {
    pub fn new(
        config:    PipelineConfig,
        agent_key: impl Into<String>,
        identity:  IdentityChain,
    ) -> Self {
        Self { config, agent_key: agent_key.into(), identity }
    }

    /// Run the full pipeline. Takes ownership of `session` and closes it.
    pub fn run(
        &self,
        mut session: VcpSession,
        driver:      &Go2CaptureDriver,
    ) -> PipelineResult<PipelineOutput> {
        let grant = session.grant.clone();

        // Phase 1: capture raw data via VCP commands
        let (raw, recorded) = driver.capture(&grant, &self.agent_key)?;
        for (cap, success) in recorded {
            session.record_command(cap, success);
        }

        let ts_end   = now_ms();
        let ts_start = ts_end.saturating_sub(raw.duration_ms);
        let raw_map  = raw.to_raw_data_map();
        let mods     = raw.active_modalities();

        // Phase 2: CaptureReceipt (31020)
        let capture_receipt = CaptureReceipt::build(
            self.identity.clone(),
            vec![grant.device_id.clone()],
            mods,
            self.config.region.clone(),
            [ts_start, ts_end],
            raw.f1_score,
            raw.coverage_pct,
            raw.frame_count,
            raw.duration_ms,
            &raw_map,
            raw.novelty_score,
            raw.delta_coverage,
            self.config.privacy_flags.clone(),
            &self.agent_key,
        )?;

        // Phase 3: Assemble TwinAsset
        let data_hashes = TwinDataHashes {
            rgb:   capture_receipt.raw_hashes.get("rgb").cloned(),
            lidar: capture_receipt.raw_hashes.get("lidar").cloned(),
            imu:   capture_receipt.raw_hashes.get("imu").cloned(),
            splat: capture_receipt.raw_hashes.get("splat").cloned(),
            ..Default::default()
        };
        let twin_id = TwinAsset::twin_id_from_hashes(&data_hashes);
        let quality = TwinQuality::new(
            raw.f1_score,
            raw.coverage_pct,
            &self.config.reconstruction_engine,
        )?;
        let now = now_ms();
        let twin = TwinAsset {
            twin_id,
            version:      1,
            owner_did:    self.config.owner_did.clone(),
            creator_did:  self.identity.agent_id.clone(),
            contributors: vec![],
            region:       self.config.region.clone(),
            data_hashes,
            quality,
            provenance: TwinProvenance {
                capture_receipt_ids:  vec![capture_receipt.receipt_id.clone()],
                capture_epoch:        Some([ts_start, ts_end]),
                device_ids:           vec![grant.device_id.clone()],
                camera_ids:           vec![format!("{}:front_rgb", grant.device_id)],
                pose_estimate_hashes: vec![],
                calibration_version:  "vcp/1".into(),
                evidence_ids:         vec![],
            },
            license:       self.config.license.clone(),
            sui_object_id: None,
            merkle_root:   String::new(),
            signature:     String::new(),
            created_at:    now,
            updated_at:    now,
        };

        // Phase 4: SceneReceipt (31030) — IP Root creation event
        let scene_receipt = SceneReceipt::build(
            self.identity.clone(),
            &twin,
            &[&capture_receipt],
            &self.config.reconstruction_engine,
            &self.agent_key,
        )?;

        // Phase 5: close VCP session, link 31020 as evidence
        session.add_evidence(capture_receipt.receipt_id.clone());
        let session_receipt = session.close(VcpSessionOutcome::Completed, &self.agent_key);

        Ok(PipelineOutput {
            capture_receipt,
            twin,
            scene_receipt,
            session_receipt,
        })
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use sovereign_types::crypto::{generate_keypair, did_from_pubkey};
    use vcp::{
        VcpCapabilityGrant, VcpSession,
        handshake::{VcpCapabilityRequest, VcpDuration},
        adapters::{Go2Adapter, Go2ConnectionMode},
    };

    fn make_grant(device_key: &str, agent_key: &str, agent_did: &str) -> VcpCapabilityGrant {
        let adapter  = Go2Adapter::new("unitree:go2:test", Go2ConnectionMode::default());
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

    #[test]
    fn full_pipeline_produces_all_receipts() {
        let (agent_key, agent_pub) = generate_keypair();
        let agent_did = did_from_pubkey(&agent_pub, "agent");
        let (device_key, _) = generate_keypair();

        let grant = make_grant(&device_key, &agent_key, &agent_did);
        let session = VcpSession::new(grant);
        let pipeline = make_pipeline(&agent_key, &agent_did);
        let output = pipeline.run(session, &Go2CaptureDriver).unwrap();

        assert_eq!(output.capture_receipt.kind, 31020);
        assert_eq!(output.scene_receipt.kind,   31030);
        assert!(output.twin.data_hashes.splat.is_some());
        assert!(output.scene_receipt.f1_score >= 0.777);
        assert!(!output.session_receipt.evidence_ids.is_empty());
        assert_eq!(
            output.session_receipt.evidence_ids[0],
            output.capture_receipt.receipt_id
        );
    }

    #[test]
    fn twin_id_bound_to_scene_receipt() {
        let (agent_key, agent_pub) = generate_keypair();
        let agent_did = did_from_pubkey(&agent_pub, "agent");
        let (device_key, _) = generate_keypair();

        let grant = make_grant(&device_key, &agent_key, &agent_did);
        let session = VcpSession::new(grant);
        let output = make_pipeline(&agent_key, &agent_did)
            .run(session, &Go2CaptureDriver).unwrap();

        assert_eq!(output.scene_receipt.twin_id, output.twin.twin_id);
        assert!(output.twin.twin_id.starts_with("twin:sha256:"));
    }

    #[test]
    fn capture_receipt_linked_in_scene() {
        let (agent_key, agent_pub) = generate_keypair();
        let agent_did = did_from_pubkey(&agent_pub, "agent");
        let (device_key, _) = generate_keypair();

        let grant = make_grant(&device_key, &agent_key, &agent_did);
        let session = VcpSession::new(grant);
        let output = make_pipeline(&agent_key, &agent_did)
            .run(session, &Go2CaptureDriver).unwrap();

        assert!(output.scene_receipt.capture_receipt_ids
            .contains(&output.capture_receipt.receipt_id));
    }

    #[test]
    fn scene_receipt_has_splat_hash() {
        let (agent_key, agent_pub) = generate_keypair();
        let agent_did = did_from_pubkey(&agent_pub, "agent");
        let (device_key, _) = generate_keypair();

        let grant = make_grant(&device_key, &agent_key, &agent_did);
        let session = VcpSession::new(grant);
        let output = make_pipeline(&agent_key, &agent_did)
            .run(session, &Go2CaptureDriver).unwrap();

        assert!(!output.scene_receipt.splat_hash.is_empty());
        assert!(output.scene_receipt.splat_hash.starts_with("sha256:"));
    }

    #[test]
    fn session_tracks_three_commands() {
        let (agent_key, agent_pub) = generate_keypair();
        let agent_did = did_from_pubkey(&agent_pub, "agent");
        let (device_key, _) = generate_keypair();

        let grant = make_grant(&device_key, &agent_key, &agent_did);
        let session = VcpSession::new(grant);
        let output = make_pipeline(&agent_key, &agent_did)
            .run(session, &Go2CaptureDriver).unwrap();

        assert_eq!(output.session_receipt.commands_issued, 3);
        assert_eq!(output.session_receipt.commands_success, 3);
    }
}
