//! The universal per-action receipt — the SEP-1 evidence spine.
//!
//! Contract: `sovereign-stack/CANONICAL_PILLAR_CONTRACT.md` § SEP-1 requires
//! every state transition to produce an `ActionReceipt` carrying:
//!
//! ```text
//! receipt_id   — BLAKE3 of canonical fields
//! work_id      — wk:{namespace}:{ulid}
//! principal_id — DID of the authorising principal
//! agent_id     — DID of the executing agent
//! action       — human-readable action name
//! input_hash   — BLAKE3 of serialised input
//! output_hash  — BLAKE3 of serialised output
//! timestamp_ms — Unix milliseconds
//! signature    — ed25519 over all above fields
//! ```
//!
//! Plus agent-level fields from the Omo-Koda lineage: `proof_of_work`,
//! `epistemic_severity`, `plane`, `previous_hash`.
//!
//! Before this revision the type was missing `work_id`, `input_hash`,
//! `output_hash` and `signature` entirely, derived `receipt_id` from the
//! principal rather than hashing canonical fields, hashed its chain link with
//! SHA-256 while documenting BLAKE3, and `to_canonical()` silently discarded
//! `previous_hash` and emitted an empty signature.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sovereign_types::identity::{Hash, Timestamp};
use sovereign_types::proof::{blake3_hex, blake3_of_json, HASH_PREFIX};
use sovereign_types::receipt::CanonicalReceipt;
use sovereign_types::work_id::{ActionReceipt as Sep1ActionReceipt, WorkId};

use crate::capability::CapabilityAction;
use crate::evidence::EvidenceBundle;
use crate::principal::Principal;

/// Sentinel `previous_hash` for the first receipt in a chain.
///
/// `CanonicalReceipt::previous_hash` is not an `Option`, so genesis needs an
/// explicit marker rather than an empty string — an empty string is
/// indistinguishable from "the field was dropped", which is exactly the bug
/// this constant exists to prevent.
pub const GENESIS_PREVIOUS_HASH: &str = "blake3:genesis";

// ── Re-exported shared proof primitives ───────────────────────────────────────
//
// These used to be defined here AND in sovereign-os under the same names. They
// now live in sovereign-types (the shared layer) and are re-exported so existing
// `crate::receipt::PoCWProof` paths keep working.
pub use sovereign_types::proof::{Confidence, EnsembleDisagreement, PoCWProof, SimPlane};

// ── Action enums ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionOutcome {
    Success,
    Partial,
    Failure,
    Denied,
}

impl ActionOutcome {
    /// Whether this receipt represents a completed (non-refused) action.
    pub fn is_conclusive(&self) -> bool {
        matches!(self, ActionOutcome::Success | ActionOutcome::Failure | ActionOutcome::Partial)
    }
}

// ── The receipt ───────────────────────────────────────────────────────────────

/// The universal per-action receipt — emitted by every consequential operation
/// in every repo in the ecosystem.
///
/// Machine-level fields (L2): action, resource, principal_id, capability_id,
/// outcome, evidence.
/// Agent-level fields (L3):   proof_of_work, epistemic_severity, plane,
/// previous_hash.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionReceipt {
    // ── SEP-1 core identity ────────────────────────────────────────────────
    /// BLAKE3 over the canonical fields. Derived, never caller-supplied.
    pub receipt_id: String,
    /// The cross-repo work unit this receipt belongs to.
    pub work_id: WorkId,
    pub principal_id: String,
    pub agent_id: String,
    pub session_id: String,
    pub execution_id: String,

    // ── SEP-1 action ──────────────────────────────────────────────────────
    pub action: CapabilityAction,
    pub resource: String,
    pub capability_id: Option<String>,
    pub params: Value,
    pub result: Value,
    pub outcome: ActionOutcome,
    pub error: Option<String>,

    // ── SEP-1 hashes ──────────────────────────────────────────────────────
    /// BLAKE3 of the serialised input parameters.
    pub input_hash: String,
    /// BLAKE3 of the serialised output / result.
    pub output_hash: String,

    // ── evidence ──────────────────────────────────────────────────────────
    pub evidence_root: Hash,
    pub evidence_count: usize,

    // ── timing ────────────────────────────────────────────────────────────
    pub started_at: Timestamp,
    pub completed_at: Timestamp,

    // ── agent-level proof fields (Omo-Koda ActReceipt lineage) ────────────
    /// BLAKE3 hash of the previous receipt — chain integrity.
    pub previous_hash: Option<String>,
    /// Proof of Cognitive Work — required for act tier elevation.
    pub proof_of_work: Option<PoCWProof>,
    /// How strongly the agent believes the claim (Omo-Koda concept).
    pub epistemic_severity: Option<Confidence>,
    /// Multi-model wisdom-ensemble disagreement (distinct from the above).
    pub ensemble_disagreement: Option<EnsembleDisagreement>,
    /// Sim-to-real verification plane. Defaults to Physical (unverified).
    pub plane: SimPlane,

    // ── SEP-1 signature ───────────────────────────────────────────────────
    /// ed25519 signature over the canonical signing payload. Empty until signed.
    pub signature: String,
}

impl ActionReceipt {
    /// Build the canonical signing payload — every field covered by the
    /// signature, in a fixed order.
    ///
    /// `signature` itself is excluded (a signature cannot cover itself), and
    /// `receipt_id` IS included so the signature commits to the hash.
    fn signing_payload(&self) -> String {
        let mut s = String::new();
        s.push_str(&self.receipt_id);
        s.push('|');
        s.push_str(self.work_id.as_str());
        s.push('|');
        s.push_str(&self.principal_id);
        s.push('|');
        s.push_str(&self.agent_id);
        s.push('|');
        s.push_str(&self.session_id);
        s.push('|');
        s.push_str(&self.execution_id);
        s.push('|');
        s.push_str(&format!("{:?}", self.action));
        s.push('|');
        s.push_str(&self.resource);
        s.push('|');
        s.push_str(self.capability_id.as_deref().unwrap_or(""));
        s.push('|');
        s.push_str(&self.input_hash);
        s.push('|');
        s.push_str(&self.output_hash);
        s.push('|');
        s.push_str(&self.evidence_root);
        s.push('|');
        s.push_str(&format!("{:?}", self.outcome));
        s.push('|');
        s.push_str(&self.started_at.to_string());
        s.push('|');
        s.push_str(&self.completed_at.to_string());
        s
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        principal: &Principal,
        action: CapabilityAction,
        resource: impl Into<String>,
        capability_id: Option<String>,
        params: Value,
        result: Value,
        outcome: ActionOutcome,
        evidence: &EvidenceBundle,
        started_at: Timestamp,
        completed_at: Timestamp,
    ) -> Self {
        let work_id = WorkId::new("real", &uuid::Uuid::new_v4().to_string());
        let input_hash = blake3_of_json(&params);
        let output_hash = blake3_of_json(&result);

        Self {
            receipt_id: Sep1ActionReceipt::compute_id(
                &work_id,
                principal.principal_id(),
                principal.agent_id(),
                &format!("{:?}", action),
                &input_hash,
                &output_hash,
                completed_at,
            ),
            work_id,
            principal_id: principal.principal_id().to_string(),
            agent_id: principal.agent_id().to_string(),
            session_id: principal.chain.session_id.clone(),
            execution_id: principal.chain.execution_id.clone(),
            action,
            resource: resource.into(),
            capability_id,
            params,
            result,
            outcome,
            error: None,
            input_hash,
            output_hash,
            evidence_root: evidence.merkle_root(),
            evidence_count: evidence.len(),
            started_at,
            completed_at,
            previous_hash: None,
            proof_of_work: None,
            epistemic_severity: None,
            ensemble_disagreement: None,
            plane: SimPlane::Physical,
            signature: String::new(),
        }
    }

    pub fn denied(
        principal: &Principal,
        action: CapabilityAction,
        resource: impl Into<String>,
        reason: impl Into<String>,
        now: Timestamp,
    ) -> Self {
        let work_id = WorkId::new("real", &uuid::Uuid::new_v4().to_string());
        let input_hash = blake3_of_json(&Value::Null);
        let output_hash = blake3_of_json(&Value::Null);

        Self {
            receipt_id: Sep1ActionReceipt::compute_id(
                &work_id,
                principal.principal_id(),
                principal.agent_id(),
                &format!("{:?}", action),
                &input_hash,
                &output_hash,
                now,
            ),
            work_id,
            principal_id: principal.principal_id().to_string(),
            agent_id: principal.agent_id().to_string(),
            session_id: principal.chain.session_id.clone(),
            execution_id: principal.chain.execution_id.clone(),
            action,
            resource: resource.into(),
            capability_id: None,
            params: Value::Null,
            result: Value::Null,
            outcome: ActionOutcome::Denied,
            error: Some(reason.into()),
            input_hash,
            output_hash,
            evidence_root: format!("{}empty", HASH_PREFIX),
            evidence_count: 0,
            started_at: now,
            completed_at: now,
            previous_hash: None,
            proof_of_work: None,
            epistemic_severity: None,
            ensemble_disagreement: None,
            plane: SimPlane::Physical,
            signature: String::new(),
        }
    }

    // ── Builders ──────────────────────────────────────────────────────────

    /// Attach the originating WorkID (overrides the auto-generated one).
    pub fn with_work_id(mut self, work_id: WorkId) -> Self {
        self.work_id = work_id;
        self.recompute_id()
    }

    /// Chain this receipt to a previous one via BLAKE3 (Omo-Koda lineage).
    pub fn with_previous(mut self, prev_receipt_id: impl Into<String>) -> Self {
        self.previous_hash = Some(blake3_hex(prev_receipt_id.into().as_bytes()));
        self
    }

    /// Attach Proof of Cognitive Work for act tier elevation.
    pub fn with_pocw(mut self, proof: PoCWProof) -> Self {
        self.proof_of_work = Some(proof);
        self
    }

    /// Attach the agent's confidence in the claim (Omo-Koda `epistemic_severity`).
    pub fn with_confidence(mut self, confidence: Confidence) -> Self {
        self.epistemic_severity = Some(confidence);
        self
    }

    /// Attach the multi-model ensemble disagreement signal.
    pub fn with_ensemble_disagreement(mut self, d: EnsembleDisagreement) -> Self {
        self.ensemble_disagreement = Some(d);
        self
    }

    /// Upgrade the plane after a real verify call returns Verified.
    /// Callers must have obtained Verified from a real verification — this
    /// setter does not itself verify anything.
    pub fn with_plane(mut self, plane: SimPlane) -> Self {
        self.plane = plane;
        self
    }

    /// Sign the receipt with an ed25519 private key (base64url).
    ///
    /// Returns `Err` if the key is malformed. On success `signature` is
    /// populated and [`Self::is_signed`] becomes true.
    pub fn sign(&mut self, private_key_b64: &str) -> Result<(), sovereign_types::SovereignError> {
        let payload = self.signing_payload();
        self.signature = sovereign_types::crypto::sign(&payload, private_key_b64)?;
        Ok(())
    }

    /// Recompute `receipt_id` from the current canonical fields.
    ///
    /// Must be called after mutating any field that participates in the ID
    /// (currently `work_id`), otherwise the ID no longer matches its contents.
    fn recompute_id(mut self) -> Self {
        self.receipt_id = Sep1ActionReceipt::compute_id(
            &self.work_id,
            &self.principal_id,
            &self.agent_id,
            &format!("{:?}", self.action),
            &self.input_hash,
            &self.output_hash,
            self.completed_at,
        );
        self
    }

    // ── Predicates ────────────────────────────────────────────────────────

    pub fn is_signed(&self) -> bool {
        !self.signature.is_empty()
    }

    /// True if PoCW meets the floor for the given act tier.
    pub fn meets_pocw_floor(&self, tier: u8) -> bool {
        if tier == 0 {
            return true;
        }
        match &self.proof_of_work {
            None => false,
            Some(proof) => proof.steps >= PoCWProof::min_for_tier(tier) && proof.is_valid(),
        }
    }

    /// Verify that this receipt satisfies every SEP-1 obligation.
    ///
    /// This is the enforcement gate: a consequential action whose receipt fails
    /// this check has violated the protocol, which the canonical architecture
    /// classifies as a P0 bug.
    pub fn is_sep1_conformant(&self) -> bool {
        // Every required field present and non-empty.
        if self.receipt_id.is_empty()
            || self.work_id.as_str().is_empty()
            || self.principal_id.is_empty()
            || self.agent_id.is_empty()
            || self.action_string().is_empty()
            || self.input_hash.is_empty()
            || self.output_hash.is_empty()
        {
            return false;
        }

        // work_id must be well-formed: wk:{namespace}:{id}
        if self.work_id.namespace().is_none() {
            return false;
        }

        // The payload hashes must actually describe the payloads.
        //
        // Without this, swapping `result` while leaving `output_hash` (and thus
        // a matching `receipt_id`) intact would still pass — the id binds the
        // hashes, but nothing bound the hashes to the payloads.
        if self.input_hash != blake3_of_json(&self.params) {
            return false;
        }
        if self.output_hash != blake3_of_json(&self.result) {
            return false;
        }

        // receipt_id must actually be the BLAKE3 of the canonical fields,
        // not a value copied from elsewhere.
        let expected = Sep1ActionReceipt::compute_id(
            &self.work_id,
            &self.principal_id,
            &self.agent_id,
            &self.action_string(),
            &self.input_hash,
            &self.output_hash,
            self.completed_at,
        );
        if self.receipt_id != expected {
            return false;
        }

        // A denial carries no signature requirement beyond field presence
        // (there is no result to attest). Everything else must be signed.
        if self.outcome != ActionOutcome::Denied && !self.is_signed() {
            return false;
        }

        true
    }

    fn action_string(&self) -> String {
        format!("{:?}", self.action)
    }

    /// Project into the canonical `CanonicalReceipt` for storage/broadcast.
    ///
    /// Carries `previous_hash` and `signature` through rather than dropping
    /// them — the previous revision emitted `"sha256:"` and `""` here, which
    /// broke the SRP-1 chain on every round trip.
    pub fn to_canonical(&self) -> CanonicalReceipt {
        use sovereign_types::identity::IdentityChain;
        use sovereign_types::receipt::{ActionRecord, ReceiptKind};

        let kind = match self.action {
            CapabilityAction::Capture => ReceiptKind::Capture,
            CapabilityAction::Simulate => ReceiptKind::Simulation,
            CapabilityAction::Route => ReceiptKind::DipRoute,
            CapabilityAction::Attest => ReceiptKind::Observation,
            CapabilityAction::Delegate => ReceiptKind::LicenseGrant,
            _ => ReceiptKind::Validation,
        };

        let chain = IdentityChain {
            principal_id: self.principal_id.clone(),
            agent_id: self.agent_id.clone(),
            session_id: self.session_id.clone(),
            execution_id: self.execution_id.clone(),
            receipt_id: self.receipt_id.clone(),
        };

        CanonicalReceipt {
            receipt_id: self.receipt_id.clone(),
            kind,
            identity: chain,
            action: ActionRecord {
                kind: self.action_string(),
                target: self.resource.clone(),
                params: self.params.clone(),
            },
            // Preserve the real root; do not fabricate a list from a count.
            evidence_ids: if self.evidence_count == 0 {
                vec![]
            } else {
                vec![self.evidence_root.clone()]
            },
            witness_attestations: vec![],
            throne_evaluations: vec![],
            consensus_receipt: None,
            physical_attestation: None,
            timestamp: self.completed_at,
            previous_hash: self
                .previous_hash
                .clone()
                .unwrap_or_else(|| GENESIS_PREVIOUS_HASH.to_string()),
            merkle_root: self.evidence_root.clone(),
            signature: self.signature.clone(),
        }
    }
}
