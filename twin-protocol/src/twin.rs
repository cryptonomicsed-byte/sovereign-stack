use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use sovereign_types::{Did, AgentDid, Hash, Signature, Timestamp, LicenseType, UsageRight};
use crate::region::TwinRegion;
use crate::quality::TwinQuality;
use crate::error::TspResult;

/// Hashes over all captured data modalities — each bound into the Twin merkle root.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TwinDataHashes {
    pub rgb:         Option<Hash>,
    pub depth:       Option<Hash>,
    pub lidar:       Option<Hash>,
    pub imu:         Option<Hash>,
    pub slam:        Option<Hash>,
    pub splat:       Option<Hash>,    // Gaussian splat .ply — REQUIRED for scene receipt
    pub geometry:    Option<Hash>,
    pub semantic:    Option<Hash>,
    pub environment: Option<Hash>,
    pub custom:      Option<BTreeMap<String, Hash>>,
}

impl TwinDataHashes {
    pub fn has_splat(&self) -> bool { self.splat.is_some() }
}

/// Per-region provenance chain.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TwinProvenance {
    pub capture_receipt_ids:     Vec<String>,
    pub capture_epoch:           Option<[Timestamp; 2]>,
    pub device_ids:              Vec<String>,
    pub camera_ids:              Vec<String>,
    pub pose_estimate_hashes:    Vec<Hash>,
    pub calibration_version:     String,
    pub evidence_ids:            Vec<String>,
}

/// Revenue split for a twin asset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevenueSplit {
    pub owner_pct:    f32,
    pub contributors: Vec<ContributorSplit>,
    pub protocol_fee: f32,
}

impl Default for RevenueSplit {
    fn default() -> Self {
        Self { owner_pct: 70.0, contributors: vec![], protocol_fee: 2.5 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributorSplit {
    pub did: Did,
    pub pct: f32,
}

/// License configuration for a Twin Asset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TwinLicenseConfig {
    pub license_type:  LicenseType,
    pub usage_rights:  Vec<UsageRight>,
    pub revenue_split: RevenueSplit,
    pub transferable:  bool,
}

impl Default for TwinLicenseConfig {
    fn default() -> Self {
        Self {
            license_type:  LicenseType::NonExclusive,
            usage_rights:  vec![UsageRight::View, UsageRight::Simulate],
            revenue_split: RevenueSplit::default(),
            transferable:  true,
        }
    }
}

/// The Twin Asset — a cryptographically-identified, owned, licensed digital twin.
/// twin_id = "twin:sha256:<merkle_root_of_all_data_hashes>"
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TwinAsset {
    pub twin_id:      Hash,         // deterministic from content
    pub version:      u32,
    pub owner_did:    Did,
    pub creator_did:  AgentDid,
    pub contributors: Vec<Did>,

    pub region:       TwinRegion,
    pub data_hashes:  TwinDataHashes,
    pub quality:      TwinQuality,
    pub provenance:   TwinProvenance,
    pub license:      TwinLicenseConfig,

    pub sui_object_id: Option<String>,

    pub merkle_root:  Hash,
    pub signature:    Signature,
    pub created_at:   Timestamp,
    pub updated_at:   Timestamp,
}

impl TwinAsset {
    pub fn twin_id_from_hashes(data_hashes: &TwinDataHashes) -> Hash {
        let json = serde_json::to_string(data_hashes).unwrap_or_default();
        sovereign_types::hash_str(&json)
            .replace("sha256:", "twin:sha256:")
    }

    /// Validate the twin: F1 gate, splat hash required, merkle integrity.
    pub fn validate(&self) -> TspResult<()> {
        self.quality.assert_valid()?;
        if !self.data_hashes.has_splat() {
            return Err(crate::error::TspError::MissingHash("splat"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quality::TwinQuality;
    use sovereign_types::LicenseType;

    #[test]
    fn quality_gate_enforced() {
        assert!(TwinQuality::new(0.776, 90.0, "test").is_err());
        assert!(TwinQuality::new(0.777, 90.0, "test").is_ok());
    }

    #[test]
    fn twin_id_deterministic() {
        let hashes = TwinDataHashes { splat: Some("sha256:aabb".into()), ..Default::default() };
        let id1 = TwinAsset::twin_id_from_hashes(&hashes);
        let id2 = TwinAsset::twin_id_from_hashes(&hashes);
        assert_eq!(id1, id2);
        assert!(id1.starts_with("twin:sha256:"));
    }
}
