use serde::{Deserialize, Serialize};
use serde_json::Value;
use sovereign_types::identity::{Hash, Timestamp};

/// A single piece of evidence attached to an execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub evidence_id:  String,
    pub kind:         EvidenceKind,
    pub content_hash: Hash,          // sha256:<hex>
    pub uri:          Option<String>,
    pub metadata:     Value,
    pub captured_at:  Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    SensorCapture,
    GaussianSplat,
    SlumMap,
    ProofHash,
    WitnessSignature,
    PolicyEvaluation,
    SimulationTrace,
    AudioCapture,
    VideoCapture,
    Custom,
}

/// A typed collection of evidence gathered during one execution.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EvidenceBundle {
    pub items: Vec<Evidence>,
}

impl EvidenceBundle {
    pub fn new() -> Self { Self::default() }

    pub fn push(&mut self, e: Evidence) { self.items.push(e); }

    pub fn ids(&self) -> Vec<String> {
        self.items.iter().map(|e| e.evidence_id.clone()).collect()
    }

    /// Content-addresses the bundle: sha256 of all evidence_id + content_hash pairs.
    pub fn merkle_root(&self) -> Hash {
        use sha2::{Sha256, Digest};
        let mut h = Sha256::new();
        for e in &self.items {
            h.update(e.evidence_id.as_bytes());
            h.update(e.content_hash.as_bytes());
        }
        format!("sha256:{}", hex::encode(h.finalize()))
    }

    pub fn is_empty(&self) -> bool { self.items.is_empty() }
    pub fn len(&self)     -> usize { self.items.len() }
}
