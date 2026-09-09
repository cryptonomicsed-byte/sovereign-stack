use serde::{Deserialize, Serialize};
use serde_json::Value;
use crate::identity::{IdentityChain, Hash, Signature, Timestamp};

/// Canonical receipt — all three protocols feed into this format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalReceipt {
    pub receipt_id:            String,
    pub kind:                  ReceiptKind,
    pub identity:              IdentityChain,
    pub action:                ActionRecord,
    pub evidence_ids:          Vec<String>,
    pub witness_attestations:  Vec<WitnessAttestation>,
    pub throne_evaluations:    Vec<Value>,
    pub consensus_receipt:     Option<Value>,
    pub physical_attestation:  Option<Value>,
    pub timestamp:             Timestamp,
    pub previous_hash:         Hash,
    pub merkle_root:           Hash,
    pub signature:             Signature,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptKind {
    DipRoute,
    VcpSession,
    Capture,        // TSP 31020
    Scene,          // TSP 31030
    Simulation,
    Observation,
    LicenseGrant,
    Validation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionRecord {
    pub kind:   String,
    pub target: String,
    pub params: Value,
}

/// A witness node's attestation over a merkle commitment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitnessAttestation {
    pub witness_id:          String,
    pub merkle_commitment:   Hash,
    pub timestamp:           Timestamp,
    pub signature:           Signature,
}
