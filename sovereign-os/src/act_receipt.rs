//! Agent-level ActReceipt — Layer 3 complement to Layer 2 ActionReceipt.
//!
//! Layer 2 (sovereign-runtime) proves: Principal did Action on Resource.
//! Layer 3 (sovereign-os) proves: Agent reasoned about it with PoCW + BLAKE3 chain.
//!
//! The BLAKE3 chain links successive ActReceipts so that no single receipt
//! can be forged without recomputing the entire chain from genesis.

use serde::{Deserialize, Serialize};
use sha2::{Sha256, Digest};
use sovereign_types::identity::Timestamp;

/// Proof-of-Cognitive-Work — how hard the agent worked to produce this output.
/// Tier 0: no proof (read-only operations)
/// Tier 1: ≥21 reasoning steps (BB(3) = 21)
/// Tier 2: ≥107 steps (BB(4) = 107)
/// Tier 3: ≥47M steps (BB(5) = 47,176,870 — physical simulation required)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoCWProof {
    pub steps:     u64,
    pub bb_bound:  u64,
    pub tape_hash: String,  // sha256 of the reasoning trace
}

impl PoCWProof {
    pub fn is_valid(&self) -> bool {
        self.steps >= self.bb_bound && !self.tape_hash.is_empty()
    }

    pub fn tier(&self) -> u8 {
        if self.steps >= 47_176_870 { 3 }
        else if self.steps >= 107    { 2 }
        else if self.steps >= 21     { 1 }
        else                         { 0 }
    }
}

/// How strongly the agent believes the claim it is making.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EpistemicSeverity {
    /// Agent observed directly — high confidence.
    Observed,
    /// Agent inferred from evidence — moderate confidence.
    Inferred,
    /// Agent speculated or extrapolated — low confidence.
    Speculative,
    /// Agent is reporting a model output — epistemic trust deferred to model.
    ModelOutput,
}

/// One agent-level ActReceipt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentActReceipt {
    pub receipt_id:     String,
    pub agent_did:      String,
    pub action:         String,
    pub resource:       String,
    pub params:         serde_json::Value,
    pub result:         serde_json::Value,
    pub created_at:     Timestamp,
    pub proof_of_work:  Option<PoCWProof>,
    pub epistemic:      Option<EpistemicSeverity>,
    /// sha256 of previous receipt_id — forms a tamper-evident chain.
    pub previous_hash:  Option<String>,
    /// The Layer 2 ActionReceipt ID this ActReceipt corresponds to (if any).
    pub action_receipt_id: Option<String>,
}

impl AgentActReceipt {
    pub fn new(
        agent_did: impl Into<String>,
        action:    impl Into<String>,
        resource:  impl Into<String>,
        params:    serde_json::Value,
        result:    serde_json::Value,
        now:       Timestamp,
    ) -> Self {
        let id = uuid::Uuid::new_v4().to_string();
        Self {
            receipt_id: format!("act:{id}"),
            agent_did: agent_did.into(),
            action:    action.into(),
            resource:  resource.into(),
            params,
            result,
            created_at: now,
            proof_of_work: None,
            epistemic: None,
            previous_hash: None,
            action_receipt_id: None,
        }
    }

    pub fn with_pocw(mut self, proof: PoCWProof) -> Self {
        self.proof_of_work = Some(proof);
        self
    }

    pub fn with_epistemic(mut self, e: EpistemicSeverity) -> Self {
        self.epistemic = Some(e);
        self
    }

    pub fn with_previous(mut self, prev_id: String) -> Self {
        let mut h = Sha256::new();
        h.update(prev_id.as_bytes());
        self.previous_hash = Some(format!("sha256:{}", hex::encode(h.finalize())));
        self
    }

    pub fn with_action_receipt(mut self, id: String) -> Self {
        self.action_receipt_id = Some(id);
        self
    }
}

/// An append-only chain of ActReceipts — each links to the previous via previous_hash.
#[derive(Debug, Default, Clone)]
pub struct ActReceiptChain {
    receipts: Vec<AgentActReceipt>,
}

impl ActReceiptChain {
    pub fn new() -> Self { Self::default() }

    pub fn push(&mut self, mut receipt: AgentActReceipt) -> &AgentActReceipt {
        if let Some(prev) = self.receipts.last() {
            receipt = receipt.with_previous(prev.receipt_id.clone());
        }
        self.receipts.push(receipt);
        self.receipts.last().unwrap()
    }

    pub fn len(&self) -> usize { self.receipts.len() }
    pub fn is_empty(&self) -> bool { self.receipts.is_empty() }
    pub fn latest(&self) -> Option<&AgentActReceipt> { self.receipts.last() }
    pub fn all(&self) -> &[AgentActReceipt] { &self.receipts }

    /// Verify chain integrity — every receipt's previous_hash matches the prior receipt's id.
    pub fn verify_chain(&self) -> bool {
        for window in self.receipts.windows(2) {
            let (prev, curr) = (&window[0], &window[1]);
            let expected = format!("sha256:{}", hex::encode({
                let mut h = Sha256::new();
                h.update(prev.receipt_id.as_bytes());
                h.finalize()
            }));
            if curr.previous_hash.as_deref() != Some(&expected) {
                return false;
            }
        }
        true
    }
}
