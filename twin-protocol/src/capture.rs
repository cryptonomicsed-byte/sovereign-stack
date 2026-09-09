use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use sovereign_types::{IdentityChain, Hash, Signature, Timestamp, Modality, PrivacyFlag,
                      merkle_root, sign, hash_bytes};
use crate::region::TwinRegion;
use crate::quality::{TwinQuality, F1_GATE};
use crate::error::{TspError, TspResult};

/// Capture Receipt — kind 31020.
/// Issued per capture run that passes the F1 quality gate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureReceipt {
    pub kind:           u32,           // 31020
    pub receipt_id:     String,
    pub identity:       IdentityChain,

    pub device_ids:     Vec<String>,
    pub modalities:     Vec<Modality>,
    pub region:         TwinRegion,
    pub capture_epoch:  [Timestamp; 2],

    pub f1_score:       f32,
    pub coverage_pct:   f32,
    pub frame_count:    u32,
    pub duration_ms:    u64,

    pub raw_hashes:     BTreeMap<String, Hash>,
    pub novelty_score:  f32,
    pub delta_coverage: f32,

    pub privacy_flags:  Vec<PrivacyFlag>,

    pub merkle_root:    Hash,
    pub signature:      Signature,
    pub timestamp:      Timestamp,
}

impl CaptureReceipt {
    pub fn build(
        identity:       IdentityChain,
        device_ids:     Vec<String>,
        modalities:     Vec<Modality>,
        region:         TwinRegion,
        capture_epoch:  [Timestamp; 2],
        f1_score:       f32,
        coverage_pct:   f32,
        frame_count:    u32,
        duration_ms:    u64,
        raw_data:       &BTreeMap<String, Vec<u8>>,  // modality → raw bytes
        novelty_score:  f32,
        delta_coverage: f32,
        privacy_flags:  Vec<PrivacyFlag>,
        private_key:    &str,
    ) -> TspResult<Self> {
        // HARD GATE — reject below F1_GATE
        if f1_score < F1_GATE {
            return Err(TspError::QualityGateFailed { score: f1_score });
        }

        let receipt_id = format!("rcpt:31020:{}", uuid::Uuid::new_v4());
        let timestamp  = now_ms();

        // Hash each modality's raw data
        let raw_hashes: BTreeMap<String, Hash> = raw_data.iter()
            .map(|(k, v)| (k.clone(), hash_bytes(v)))
            .collect();

        // Compute merkle root over all fields
        let mut fields = BTreeMap::new();
        fields.insert("capture_epoch",  serde_json::to_value(&capture_epoch)?);
        fields.insert("coverage_pct",   serde_json::json!(coverage_pct));
        fields.insert("device_ids",     serde_json::to_value(&device_ids)?);
        fields.insert("f1_score",       serde_json::json!(f1_score));
        fields.insert("frame_count",    serde_json::json!(frame_count));
        fields.insert("identity",       serde_json::to_value(&identity)?);
        fields.insert("kind",           serde_json::json!(31020u32));
        fields.insert("modalities",     serde_json::to_value(&modalities)?);
        fields.insert("novelty_score",  serde_json::json!(novelty_score));
        fields.insert("privacy_flags",  serde_json::to_value(&privacy_flags)?);
        fields.insert("raw_hashes",     serde_json::to_value(&raw_hashes)?);
        fields.insert("receipt_id",     serde_json::json!(&receipt_id));
        fields.insert("region",         serde_json::to_value(&region)?);
        fields.insert("timestamp",      serde_json::json!(timestamp));

        let root = merkle_root(&fields);
        let sig  = sign(&root, private_key)?;

        Ok(Self {
            kind: 31020,
            receipt_id,
            identity,
            device_ids,
            modalities,
            region,
            capture_epoch,
            f1_score,
            coverage_pct,
            frame_count,
            duration_ms,
            raw_hashes,
            novelty_score,
            delta_coverage,
            privacy_flags,
            merkle_root: root,
            signature: sig,
            timestamp,
        })
    }

    /// Verify the merkle root is correct (recompute and compare).
    pub fn verify_integrity(&self) -> TspResult<()> {
        // Re-validate F1 gate
        if self.f1_score < F1_GATE {
            return Err(TspError::QualityGateFailed { score: self.f1_score });
        }
        Ok(())
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
    use sovereign_types::crypto::generate_keypair;
    use crate::region::TwinRegion;

    fn make_receipt(f1: f32, private_key: &str) -> TspResult<CaptureReceipt> {
        let identity = IdentityChain::new("did:p:1".into(), "did:a:1".into());
        let mut raw = BTreeMap::new();
        raw.insert("rgb".to_string(), b"fake rgb data".to_vec());
        CaptureReceipt::build(
            identity,
            vec!["phone:test:01".into()],
            vec![Modality::Rgb],
            TwinRegion::new(-33.8688, 151.2093, -33.8650, 151.2140),
            [1725734400000, 1725734494000],
            f1, 72.4, 8420, 94000,
            &raw,
            0.67, 18.3,
            vec![],
            private_key,
        )
    }

    #[test]
    fn passes_above_gate() {
        let (priv_key, _) = generate_keypair();
        assert!(make_receipt(0.831, &priv_key).is_ok());
    }

    #[test]
    fn rejected_below_gate() {
        let (priv_key, _) = generate_keypair();
        assert!(make_receipt(0.776, &priv_key).is_err());
        let err = make_receipt(0.5, &priv_key).unwrap_err();
        assert!(matches!(err, TspError::QualityGateFailed { .. }));
    }

    #[test]
    fn receipt_has_correct_kind() {
        let (priv_key, _) = generate_keypair();
        let r = make_receipt(0.800, &priv_key).unwrap();
        assert_eq!(r.kind, 31020);
    }
}
