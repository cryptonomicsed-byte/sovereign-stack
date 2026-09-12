//! Off-chain governance store — indexes GrantProposals by id.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;
use twin_protocol::GrantProposal;

/// Derived status of a governance proposal (Phase 55: Bínò veto).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalStatus {
    Active,
    Executed,
    Rejected,
    /// Constitutional veto — only Bínò council members may invoke this.
    Vetoed,
}

/// Veto record stored alongside the proposal index.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VetoRecord {
    pub proposal_id: u64,
    pub veto_by:     String,
    pub reason:      String,
    pub timestamp:   u64,
}

#[derive(Default)]
struct GovernanceInner {
    proposals: HashMap<u64, GrantProposal>,
    vetoes:    HashMap<u64, VetoRecord>,
}

/// Thread-safe, in-memory store for governance GrantProposals.
#[derive(Clone, Default)]
pub struct GovernanceStore(Arc<RwLock<GovernanceInner>>);

impl GovernanceStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    fn status_of(p: &GrantProposal, vetoed: bool) -> ProposalStatus {
        if vetoed          { return ProposalStatus::Vetoed; }
        if p.executed      { return ProposalStatus::Executed; }
        if p.rejected      { return ProposalStatus::Rejected; }
        ProposalStatus::Active
    }

    /// Insert or overwrite a proposal.
    pub async fn insert(&self, p: GrantProposal) {
        self.0.write().await.proposals.insert(p.id, p);
    }

    /// Retrieve a single proposal by id, along with its current status.
    pub async fn get(&self, id: &str) -> Option<ProposalView> {
        let id_u64: u64 = id.parse().ok()?;
        let inner = self.0.read().await;
        let p = inner.proposals.get(&id_u64)?.clone();
        let vetoed = inner.vetoes.contains_key(&id_u64);
        let status = Self::status_of(&p, vetoed);
        let veto_record = inner.vetoes.get(&id_u64).cloned();
        Some(ProposalView { proposal: p, status, veto_record })
    }

    /// Return all proposals sorted by id ascending.
    pub async fn all(&self) -> Vec<ProposalView> {
        let inner = self.0.read().await;
        let mut v: Vec<ProposalView> = inner.proposals.values().map(|p| {
            let vetoed = inner.vetoes.contains_key(&p.id);
            let status = Self::status_of(p, vetoed);
            let veto_record = inner.vetoes.get(&p.id).cloned();
            ProposalView { proposal: p.clone(), status, veto_record }
        }).collect();
        v.sort_by_key(|pv| pv.proposal.id);
        v
    }

    /// Increment the `votes_for` bitmask by setting the next available bit.
    /// Returns the updated proposal, or `None` if the id is not found.
    pub async fn vote_for(&self, id: u64) -> Option<GrantProposal> {
        let mut guard = self.0.write().await;
        let p = guard.proposals.get_mut(&id)?;
        let bit = p.votes_for.trailing_ones();
        if bit < 64 {
            p.votes_for |= 1u64 << bit;
        }
        Some(p.clone())
    }

    /// Increment the `votes_against` bitmask by setting the next available bit.
    /// Returns the updated proposal, or `None` if the id is not found.
    pub async fn vote_against(&self, id: u64) -> Option<GrantProposal> {
        let mut guard = self.0.write().await;
        let p = guard.proposals.get_mut(&id)?;
        let bit = p.votes_against.trailing_ones();
        if bit < 64 {
            p.votes_against |= 1u64 << bit;
        }
        Some(p.clone())
    }

    /// Mark a proposal as executed if `proposal.executable(now_secs)` returns true.
    /// Returns the updated proposal on success, or an error string on failure.
    pub async fn execute(&self, id: u64, now_secs: u64) -> Result<GrantProposal, String> {
        let mut guard = self.0.write().await;
        if guard.vetoes.contains_key(&id) {
            return Err(format!("proposal {} has been constitutionally vetoed", id));
        }
        let p = guard.proposals
            .get_mut(&id)
            .ok_or_else(|| format!("proposal {} not found", id))?;

        if p.executed {
            return Err(format!("proposal {} already executed", id));
        }
        if p.rejected {
            return Err(format!("proposal {} has been rejected", id));
        }
        if !p.quorum_met() {
            return Err(format!(
                "proposal {} has not reached quorum ({} of {} required votes)",
                id,
                p.votes_for_count(),
                GrantProposal::QUORUM,
            ));
        }
        if !p.timelock_elapsed(now_secs) {
            return Err(format!(
                "proposal {} timelock has not elapsed (releases at {})",
                id, p.timelock_release,
            ));
        }

        p.executed = true;
        Ok(p.clone())
    }

    /// Apply Bínò constitutional veto — only callable by council members.
    /// The proposal is permanently blocked; `execute()` will reject it afterward.
    pub async fn veto(&self, id: &str, veto_by: &str, reason: &str) {
        let id_u64: u64 = match id.parse() { Ok(n) => n, Err(_) => return };
        let record = VetoRecord {
            proposal_id: id_u64,
            veto_by:     veto_by.to_string(),
            reason:      reason.to_string(),
            timestamp:   Self::now_ms(),
        };
        self.0.write().await.vetoes.insert(id_u64, record);
    }
}

/// Combined view of a proposal with its computed status.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProposalView {
    #[serde(flatten)]
    pub proposal:     GrantProposal,
    pub status:       ProposalStatus,
    pub veto_record:  Option<VetoRecord>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_proposal(id: u64, now_secs: u64) -> GrantProposal {
        GrantProposal {
            id,
            proposer: "0xA".into(),
            recipient: "0xB".into(),
            amount_micro_ase: 1_000_000,
            purpose: "test grant".into(),
            veil_id: 0,
            votes_for: 0,
            votes_against: 0,
            created_at: now_secs * 1000,
            timelock_release: now_secs + GrantProposal::TIMELOCK_SECS,
            executed: false,
            rejected: false,
        }
    }

    #[tokio::test]
    async fn insert_and_get() {
        let store = GovernanceStore::new();
        let p = sample_proposal(1, 1_000_000);
        store.insert(p.clone()).await;
        let got = store.get("1").await.unwrap();
        assert_eq!(got.proposal.id, 1);
        assert_eq!(got.proposal.proposer, "0xA");
        assert_eq!(got.status, ProposalStatus::Active);
    }

    #[tokio::test]
    async fn all_sorted_by_id() {
        let store = GovernanceStore::new();
        store.insert(sample_proposal(3, 1_000_000)).await;
        store.insert(sample_proposal(1, 1_000_000)).await;
        store.insert(sample_proposal(2, 1_000_000)).await;
        let all = store.all().await;
        assert_eq!(all.iter().map(|pv| pv.proposal.id).collect::<Vec<_>>(), vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn vote_for_increments() {
        let store = GovernanceStore::new();
        store.insert(sample_proposal(1, 1_000_000)).await;
        let updated = store.vote_for(1).await.unwrap();
        assert_eq!(updated.votes_for_count(), 1);
    }

    #[tokio::test]
    async fn execute_fails_without_quorum() {
        let store = GovernanceStore::new();
        let now = 1_000_000u64;
        store.insert(sample_proposal(1, now)).await;
        let err = store.execute(1, now + GrantProposal::TIMELOCK_SECS + 1).await;
        assert!(err.is_err());
        assert!(err.unwrap_err().contains("quorum"));
    }

    #[tokio::test]
    async fn veto_blocks_execution() {
        let store = GovernanceStore::new();
        let now = 1_000_000u64;
        // Create proposal with enough votes and past timelock
        let mut p = sample_proposal(10, now);
        // Simulate quorum: set enough votes_for bits
        for i in 0..GrantProposal::QUORUM {
            p.votes_for |= 1u64 << i;
        }
        p.timelock_release = now; // already elapsed
        store.insert(p).await;

        // Veto before execution
        store.veto("10", "did:council:bino", "constitutional violation").await;
        let view = store.get("10").await.unwrap();
        assert_eq!(view.status, ProposalStatus::Vetoed);

        let err = store.execute(10, now + 1).await;
        assert!(err.is_err());
        assert!(err.unwrap_err().contains("vetoed"));
    }

    #[tokio::test]
    async fn get_nonexistent_returns_none() {
        let store = GovernanceStore::new();
        assert!(store.get("9999").await.is_none());
    }
}
