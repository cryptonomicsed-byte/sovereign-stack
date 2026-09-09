use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use sovereign_types::{IdentityChain, Hash, Signature, Timestamp, LicenseType, merkle_root, sign};
use crate::capture::CaptureReceipt;
use crate::quality::F1_GATE;
use crate::error::{TspError, TspResult};
use crate::twin::TwinAsset;

/// Scene Receipt — kind 31030.
/// Issued when a full Twin Asset is assembled from capture receipts.
/// This IS the IP Root creation event — anchors to Sui.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneReceipt {
    pub kind:                   u32,   // 31030
    pub receipt_id:             String,
    pub twin_id:                Hash,
    pub version:                u32,
    pub identity:               IdentityChain,

    pub capture_receipt_ids:    Vec<String>,

    pub reconstruction_engine:  String,
    pub splat_hash:             Hash,    // REQUIRED
    pub geometry_hash:          Option<Hash>,
    pub semantic_hash:          Option<Hash>,

    pub f1_score:               f32,
    pub coverage_pct:           f32,
    pub gaussian_count:         Option<u32>,

    pub sui_object_id:          Option<String>,
    pub ip_root_tx:             Option<String>,

    pub license_type:           LicenseType,

    pub merkle_root:            Hash,
    pub signature:              Signature,
    pub timestamp:              Timestamp,
}

impl SceneReceipt {
    pub fn build(
        identity:    IdentityChain,
        twin:        &TwinAsset,
        captures:    &[&CaptureReceipt],
        engine:      impl Into<String>,
        private_key: &str,
    ) -> TspResult<Self> {
        // Splat hash required
        let splat_hash = twin.data_hashes.splat.clone()
            .ok_or(TspError::MissingHash("splat"))?;

        // F1 gate on assembled scene
        if twin.quality.f1_score < F1_GATE {
            return Err(TspError::QualityGateFailed { score: twin.quality.f1_score });
        }

        let receipt_id = format!("rcpt:31030:{}", uuid::Uuid::new_v4());
        let timestamp  = now_ms();
        let capture_receipt_ids: Vec<String> = captures.iter()
            .map(|c| c.receipt_id.clone())
            .collect();

        let mut fields = BTreeMap::new();
        fields.insert("capture_receipt_ids", serde_json::to_value(&capture_receipt_ids)?);
        fields.insert("coverage_pct",         serde_json::json!(twin.quality.coverage_pct));
        fields.insert("f1_score",             serde_json::json!(twin.quality.f1_score));
        fields.insert("identity",             serde_json::to_value(&identity)?);
        fields.insert("kind",                 serde_json::json!(31030u32));
        fields.insert("receipt_id",           serde_json::json!(&receipt_id));
        fields.insert("splat_hash",           serde_json::json!(&splat_hash));
        fields.insert("timestamp",            serde_json::json!(timestamp));
        fields.insert("twin_id",              serde_json::json!(&twin.twin_id));
        fields.insert("version",              serde_json::json!(twin.version));

        let root = merkle_root(&fields);
        let sig  = sign(&root, private_key)?;

        Ok(Self {
            kind: 31030,
            receipt_id,
            twin_id:                 twin.twin_id.clone(),
            version:                 twin.version,
            identity,
            capture_receipt_ids,
            reconstruction_engine:   engine.into(),
            splat_hash,
            geometry_hash:           twin.data_hashes.geometry.clone(),
            semantic_hash:           twin.data_hashes.semantic.clone(),
            f1_score:                twin.quality.f1_score,
            coverage_pct:            twin.quality.coverage_pct,
            gaussian_count:          twin.quality.gaussian_count,
            sui_object_id:           None,  // filled after Sui tx
            ip_root_tx:              None,
            license_type:            twin.license.license_type.clone(),
            merkle_root:             root,
            signature:               sig,
            timestamp,
        })
    }

    pub fn with_sui(mut self, object_id: impl Into<String>, tx: impl Into<String>) -> Self {
        self.sui_object_id = Some(object_id.into());
        self.ip_root_tx    = Some(tx.into());
        self
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
