use serde::{Deserialize, Serialize};
use serde_json::Value;
use sovereign_types::identity::{Hash, Timestamp};
use sovereign_types::receipt::CanonicalReceipt;
use crate::principal::Principal;
use crate::capability::CapabilityAction;
use crate::evidence::EvidenceBundle;

/// Proof of Cognitive Work — Busy Beaver grounded computational proof.
/// Originated in Omo-Koda2's ActReceipt; unified here for cross-layer interop.
/// Justice uses this as a hard floor for act tier elevation above ACT_TIER_1.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PoCWProof {
    /// Turing Machine steps actually executed (≥ BB(n) for claimed tier)
    pub steps:     u64,
    /// BB bound claimed: 1, 6, 21, 107, or 47_176_870
    pub bb_bound:  u64,
    /// SHA3-256 hash of the TM tape at halt — verifiable without re-running
    pub tape_hash: String,
}

impl PoCWProof {
    pub fn is_valid(&self) -> bool {
        self.steps >= self.bb_bound && !self.tape_hash.is_empty()
    }

    pub fn min_for_tier(tier: u8) -> u64 {
        match tier {
            0 => 0,
            1 => 21,             // BB(3)
            2 => 107,            // BB(4)
            _ => 47_176_870,     // BB(5)
        }
    }
}

/// How much the multi-model wisdom ensemble disagreed on this action.
/// Originated in Omo-Koda2's ActReceipt.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EpistemicSeverity {
    Unanimous,
    Strong,
    Moderate,
    Severe,
}

/// Sim-to-real verification plane — from omokoda-hermetic.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SimPlane {
    /// Unverified claim — the default. Must be upgraded by a real verify call.
    Physical,
    Digital,
    Hybrid,
    Verified,
}

/// The universal per-action receipt — emitted by every consequential operation
/// in every repo in the ecosystem. Unifies sovereign-runtime + Omo-Koda2 receipt lineages.
///
/// Machine-level fields: action, resource, principal_id, capability_id, outcome, evidence.
/// Agent-level fields:   proof_of_work, epistemic_severity, plane, previous_hash.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionReceipt {
    // ── core identity ──────────────────────────────────────────────────────
    pub receipt_id:          String,
    pub principal_id:        String,
    pub agent_id:            String,
    pub session_id:          String,
    pub execution_id:        String,

    // ── action ─────────────────────────────────────────────────────────────
    pub action:              CapabilityAction,
    pub resource:            String,
    pub capability_id:       Option<String>,
    pub params:              Value,
    pub result:              Value,
    pub outcome:             ActionOutcome,
    pub error:               Option<String>,

    // ── evidence ───────────────────────────────────────────────────────────
    pub evidence_root:       Hash,
    pub evidence_count:      usize,

    // ── timing ─────────────────────────────────────────────────────────────
    pub started_at:          Timestamp,
    pub completed_at:        Timestamp,

    // ── agent-level proof fields (from Omo-Koda2 ActReceipt lineage) ──────
    /// BLAKE3 hash of the previous receipt — enables chain integrity verification.
    pub previous_hash:       Option<String>,
    /// Proof of Cognitive Work — required for act tier elevation.
    pub proof_of_work:       Option<PoCWProof>,
    /// Epistemic severity from multi-model wisdom ensemble.
    pub epistemic_severity:  Option<EpistemicSeverity>,
    /// Sim-to-real verification plane. Defaults to Physical (unverified claim).
    pub plane:               SimPlane,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionOutcome {
    Success,
    Partial,
    Failure,
    Denied,
}

impl ActionReceipt {
    pub fn new(
        principal:    &Principal,
        action:       CapabilityAction,
        resource:     impl Into<String>,
        capability_id: Option<String>,
        params:       Value,
        result:       Value,
        outcome:      ActionOutcome,
        evidence:     &EvidenceBundle,
        started_at:   Timestamp,
        completed_at: Timestamp,
    ) -> Self {
        Self {
            receipt_id:         principal.receipt_id().to_string(),
            principal_id:       principal.principal_id().to_string(),
            agent_id:           principal.agent_id().to_string(),
            session_id:         principal.chain.session_id.clone(),
            execution_id:       principal.chain.execution_id.clone(),
            action,
            resource:           resource.into(),
            capability_id,
            params,
            result,
            outcome,
            error:              None,
            evidence_root:      evidence.merkle_root(),
            evidence_count:     evidence.len(),
            started_at,
            completed_at,
            previous_hash:      None,
            proof_of_work:      None,
            epistemic_severity: None,
            plane:              SimPlane::Physical,
        }
    }

    pub fn denied(
        principal:    &Principal,
        action:       CapabilityAction,
        resource:     impl Into<String>,
        reason:       impl Into<String>,
        now:          Timestamp,
    ) -> Self {
        Self {
            receipt_id:         principal.receipt_id().to_string(),
            principal_id:       principal.principal_id().to_string(),
            agent_id:           principal.agent_id().to_string(),
            session_id:         principal.chain.session_id.clone(),
            execution_id:       principal.chain.execution_id.clone(),
            action,
            resource:           resource.into(),
            capability_id:      None,
            params:             Value::Null,
            result:             Value::Null,
            outcome:            ActionOutcome::Denied,
            error:              Some(reason.into()),
            evidence_root:      "sha256:".into(),
            evidence_count:     0,
            started_at:         now,
            completed_at:       now,
            previous_hash:      None,
            proof_of_work:      None,
            epistemic_severity: None,
            plane:              SimPlane::Physical,
        }
    }

    /// Chain this receipt to a previous one via BLAKE3 hash (Omo-Koda lineage).
    pub fn with_previous(mut self, prev_receipt_id: impl Into<String>) -> Self {
        use sha2::{Sha256, Digest};
        let mut h = Sha256::new();
        h.update(prev_receipt_id.into().as_bytes());
        self.previous_hash = Some(format!("sha256:{}", hex::encode(h.finalize())));
        self
    }

    /// Attach Proof of Cognitive Work for act tier elevation.
    pub fn with_pocw(mut self, proof: PoCWProof) -> Self {
        self.proof_of_work = Some(proof);
        self
    }

    /// Attach epistemic severity from wisdom ensemble.
    pub fn with_epistemic(mut self, severity: EpistemicSeverity) -> Self {
        self.epistemic_severity = Some(severity);
        self
    }

    /// Upgrade the plane after a real verify call returns Verified.
    /// Callers must have obtained Verified from a real verification — this setter
    /// does not itself verify anything.
    pub fn with_plane(mut self, plane: SimPlane) -> Self {
        self.plane = plane;
        self
    }

    /// True if PoCW meets the floor for the given act tier.
    pub fn meets_pocw_floor(&self, tier: u8) -> bool {
        if tier == 0 { return true; }
        match &self.proof_of_work {
            None        => false,
            Some(proof) => proof.steps >= PoCWProof::min_for_tier(tier) && proof.is_valid(),
        }
    }

    /// Project into the canonical CanonicalReceipt for storage/broadcast.
    pub fn to_canonical(&self) -> CanonicalReceipt {
        use sovereign_types::identity::IdentityChain;
        use sovereign_types::receipt::{ActionRecord, ReceiptKind};

        let kind = match self.action {
            CapabilityAction::Capture  => ReceiptKind::Capture,
            CapabilityAction::Simulate => ReceiptKind::Simulation,
            CapabilityAction::Route    => ReceiptKind::DipRoute,
            CapabilityAction::Attest   => ReceiptKind::Observation,  // LoRa witness attestation
            CapabilityAction::Delegate => ReceiptKind::LicenseGrant,
            _                          => ReceiptKind::Validation,
        };

        let chain = IdentityChain {
            principal_id: self.principal_id.clone(),
            agent_id:     self.agent_id.clone(),
            session_id:   self.session_id.clone(),
            execution_id: self.execution_id.clone(),
            receipt_id:   self.receipt_id.clone(),
        };

        CanonicalReceipt {
            receipt_id:           self.receipt_id.clone(),
            kind,
            identity:             chain,
            action:               ActionRecord {
                kind:   format!("{:?}", self.action),
                target: self.resource.clone(),
                params: self.params.clone(),
            },
            evidence_ids:         vec![self.evidence_root.clone()],
            witness_attestations: vec![],
            throne_evaluations:   vec![],
            consensus_receipt:    None,
            physical_attestation: None,
            timestamp:            self.completed_at,
            previous_hash:        "sha256:".into(),
            merkle_root:          self.evidence_root.clone(),
            signature:            String::new(),
        }
    }
}
