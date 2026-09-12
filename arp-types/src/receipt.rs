use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::principal::Principal;

/// The canonical action receipt — one format to rule them all.
///
/// This is the ARP v1 envelope.  Every consequential operation in the
/// sovereign ecosystem emits an ActionReceipt.  Kind-specific data lives in
/// `payload` so that the envelope structure is always parseable, even when
/// the payload schema evolves.
///
/// Hash chain: `hash()` over canonical JSON → stored as `previous_hash` in
/// the NEXT receipt from this agent.  Breaks in the chain are evidence of
/// tampering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionReceipt {
    pub receipt_id:    Uuid,
    pub kind:          ReceiptKind,
    /// Sub-type for "custom" kind, or additional qualifier.
    pub kind_ext:      Option<String>,

    // ── 5-primitive identity chain ────────────────────────────────────────────
    pub principal:     Principal,

    // ── what happened ─────────────────────────────────────────────────────────
    pub action:        ActionSpec,

    // ── evidence ──────────────────────────────────────────────────────────────
    pub evidence_ids:          Vec<String>,
    pub witness_attestations:  Vec<WitnessAttestation>,
    pub throne_evaluations:    Vec<ThroneEvaluation>,

    // ── settlement ────────────────────────────────────────────────────────────
    pub consensus_receipt:    Option<ConsensusReceipt>,
    pub physical_attestation: Option<PhysicalAttestation>,
    /// Set when Zàngbétò records this receipt on OSOVM.
    pub zangbeto_anchor:      Option<String>,
    /// Nostr event ID if published.
    pub nostr_event_id:       Option<String>,

    // ── chain ─────────────────────────────────────────────────────────────────
    pub timestamp:       i64,           // Unix seconds
    pub execution_id:    Option<Uuid>,  // links to a batch or job
    /// SHA-256 hex of the previous ActionReceipt from this agent.  None = genesis.
    pub previous_hash:   Option<String>,

    // ── auth ──────────────────────────────────────────────────────────────────
    /// Ed25519 signature by principal.agent_id over canonical JSON.
    pub signature:       String,
}

impl ActionReceipt {
    /// SHA-256 over canonical JSON fields (deterministic, omits `signature`).
    pub fn hash(&self) -> String {
        let canonical = serde_json::json!({
            "receipt_id":   self.receipt_id,
            "kind":         self.kind,
            "principal_id": self.principal.principal_id,
            "agent_id":     self.principal.agent_id,
            "action":       self.action,
            "timestamp":    self.timestamp,
            "previous_hash": self.previous_hash,
        });
        let mut h = Sha256::new();
        h.update(canonical.to_string().as_bytes());
        hex::encode(h.finalize())
    }

    /// Verify the hash chain: this receipt's `previous_hash` must equal
    /// `prev.hash()`.
    pub fn chain_valid(&self, prev: &ActionReceipt) -> bool {
        self.previous_hash.as_deref() == Some(&prev.hash())
    }
}

// ── kind discriminant ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptKind {
    Compute,
    VcpSession,
    Emission,
    TwinCapture,
    TwinScene,
    Simulation,
    Governance,
    Economic,
    AgentLifecycle,
    Witness,
    MeshEvent,
    Custom,
}

// ── action spec ───────────────────────────────────────────────────────────────

/// What was done — the verb, target, and parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionSpec {
    /// Verb: "submit_job", "join_mesh", "grant_capability", "emit_ase", etc.
    pub kind:    String,
    /// The entity acted upon (job ID, agent ID, device ID, resource ID, …)
    pub target:  String,
    /// Outcome: "success" | "failure" | "partial" | "revoked"
    pub outcome: String,
    /// Kind-specific parameters — preserved verbatim for audit.
    pub params:  serde_json::Value,
}

// ── evidence types ────────────────────────────────────────────────────────────

/// Reference to an external evidence artefact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceRef {
    pub id:       String,
    pub kind:     String,   // "telemetry", "log", "image", "simulation_output", …
    pub uri:      Option<String>,
    pub hash:     Option<String>,
}

/// A signed observation from a Witness node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitnessAttestation {
    pub witness_id:  String,
    pub attested_at: DateTime<Utc>,
    pub observation: String,
    pub outcome_hash: String,
    pub signature:   String,
}

/// Independent evaluation from one of the Twelve Thrones.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThroneEvaluation {
    pub throne_id:   String,
    pub model:       String,   // e.g. "claude-sonnet-4-6", "gpt-4o", …
    pub verdict:     String,   // "confirmed" | "disputed" | "insufficient_evidence"
    pub confidence:  f64,
    pub rationale:   String,
    pub evaluated_at: DateTime<Utc>,
    pub signature:   String,
}

// ── settlement types ──────────────────────────────────────────────────────────

/// Twelve Thrones consensus outcome for this receipt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusReceipt {
    pub quorum:          u8,   // N of 12 thrones that participated
    pub threshold:       u8,   // minimum required for consensus
    pub consensus:       bool,
    pub combined_hash:   String,
    pub settled_at:      DateTime<Utc>,
}

/// Physical world observation that confirms/falsifies the action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhysicalAttestation {
    pub device_id:       String,
    pub observed_at:     DateTime<Utc>,
    pub f1_score:        Option<f64>,   // sim-vs-reality quality score
    pub trajectory_hash: Option<String>,
    pub nostr_kind:      Option<u32>,   // 31020 (capture) or 31030 (scene)
    pub signature:       String,
}
