//! Agent-level ActReceipt — Layer 3 complement to Layer 2 ActionReceipt.
//!
//! Layer 2 (`sovereign-runtime`) proves: **Principal did Action on Resource.**
//! Layer 3 (`sovereign-os`) proves: **Agent reasoned about it** — with PoCW and
//! a hash chain linking successive receipts, so no single receipt can be forged
//! without recomputing the chain from genesis.
//!
//! ## History
//!
//! This module previously defined its own `PoCWProof` and `EpistemicSeverity`,
//! duplicating `sovereign-runtime`'s. The `PoCWProof` copies were merely
//! redundant; the `EpistemicSeverity` copies were **different concepts sharing a
//! name** (agent confidence here, ensemble disagreement there), which meant a
//! serialised `epistemic_severity` field carried two incompatible meanings.
//!
//! Both types now live in `sovereign-types::proof` and are re-exported here.
//! `EpistemicSeverity` is kept as an alias for [`Confidence`] because that is the
//! concept this layer always meant — preserving every existing call site.

use serde::{Deserialize, Serialize};
use sovereign_types::identity::Timestamp;
use sovereign_types::proof::{blake3_hex, Confidence};

/// Re-exported so `crate::act_receipt::{PoCWProof, EpistemicSeverity}` keeps
/// working. `EpistemicSeverity` here has always meant agent confidence, which is
/// now canonically named [`Confidence`].
pub use sovereign_types::proof::{EnsembleDisagreement, PoCWProof, SimPlane};
pub use sovereign_types::proof::Confidence as EpistemicSeverity;

/// One agent-level ActReceipt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentActReceipt {
    pub receipt_id: String,
    pub agent_did: String,
    pub action: String,
    pub resource: String,
    pub params: serde_json::Value,
    pub result: serde_json::Value,
    pub created_at: Timestamp,
    pub proof_of_work: Option<PoCWProof>,
    pub epistemic: Option<Confidence>,
    /// `blake3:<hex>` of the previous receipt_id — forms a tamper-evident chain.
    pub previous_hash: Option<String>,
    /// The Layer 2 ActionReceipt ID this ActReceipt corresponds to (if any).
    pub action_receipt_id: Option<String>,
    /// ed25519 signature over the receipt's canonical payload. Empty until signed.
    pub signature: String,
}

impl AgentActReceipt {
    pub fn new(
        agent_did: impl Into<String>,
        action: impl Into<String>,
        resource: impl Into<String>,
        params: serde_json::Value,
        result: serde_json::Value,
        now: Timestamp,
    ) -> Self {
        let id = uuid::Uuid::new_v4().to_string();
        Self {
            receipt_id: format!("act:{id}"),
            agent_did: agent_did.into(),
            action: action.into(),
            resource: resource.into(),
            params,
            result,
            created_at: now,
            proof_of_work: None,
            epistemic: None,
            previous_hash: None,
            action_receipt_id: None,
            signature: String::new(),
        }
    }

    pub fn with_pocw(mut self, proof: PoCWProof) -> Self {
        self.proof_of_work = Some(proof);
        self
    }

    pub fn with_epistemic(mut self, e: Confidence) -> Self {
        self.epistemic = Some(e);
        self
    }

    pub fn with_previous(mut self, prev_id: String) -> Self {
        self.previous_hash = Some(blake3_hex(prev_id.as_bytes()));
        self
    }

    pub fn with_action_receipt(mut self, id: String) -> Self {
        self.action_receipt_id = Some(id);
        self
    }

    /// Canonical signing payload — deterministic field order, `signature` excluded.
    fn signing_payload(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}",
            self.receipt_id,
            self.agent_did,
            self.action,
            self.resource,
            self.created_at,
            self.previous_hash.as_deref().unwrap_or("")
        )
    }

    /// Sign with an ed25519 private key (base64url).
    pub fn sign(&mut self, private_key_b64: &str) -> Result<(), sovereign_types::SovereignError> {
        let payload = self.signing_payload();
        self.signature = sovereign_types::crypto::sign(&payload, private_key_b64)?;
        Ok(())
    }

    pub fn is_signed(&self) -> bool {
        !self.signature.is_empty()
    }

    /// True if the attached PoCW supports the given act tier.
    pub fn meets_pocw_floor(&self, tier: u8) -> bool {
        if tier == 0 {
            return true;
        }
        match &self.proof_of_work {
            None => false,
            Some(p) => p.steps >= PoCWProof::min_for_tier(tier) && p.is_valid(),
        }
    }
}

/// An append-only chain of ActReceipts — each links to the previous via
/// `previous_hash` (BLAKE3).
#[derive(Debug, Default, Clone)]
pub struct ActReceiptChain {
    receipts: Vec<AgentActReceipt>,
}

impl ActReceiptChain {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, mut receipt: AgentActReceipt) -> &AgentActReceipt {
        if let Some(prev) = self.receipts.last() {
            receipt = receipt.with_previous(prev.receipt_id.clone());
        }
        self.receipts.push(receipt);
        self.receipts.last().unwrap()
    }

    pub fn len(&self) -> usize {
        self.receipts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.receipts.is_empty()
    }

    pub fn latest(&self) -> Option<&AgentActReceipt> {
        self.receipts.last()
    }

    pub fn all(&self) -> &[AgentActReceipt] {
        &self.receipts
    }

    /// Verify chain integrity — every receipt's `previous_hash` matches the
    /// BLAKE3 of the prior receipt's id.
    ///
    /// Note the first receipt legitimately has no `previous_hash` (genesis), so
    /// this only examines adjacent pairs.
    pub fn verify_chain(&self) -> bool {
        for window in self.receipts.windows(2) {
            let (prev, curr) = (&window[0], &window[1]);
            let expected = blake3_hex(prev.receipt_id.as_bytes());
            if curr.previous_hash.as_deref() != Some(expected.as_str()) {
                return false;
            }
        }
        true
    }

    /// True if every receipt in the chain carries a signature.
    pub fn all_signed(&self) -> bool {
        !self.receipts.is_empty() && self.receipts.iter().all(|r| r.is_signed())
    }
}
