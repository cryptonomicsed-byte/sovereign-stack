//! Off-chain governance store — indexes GrantProposals by id.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use twin_protocol::GrantProposal;

/// Thread-safe, in-memory store for governance GrantProposals.
#[derive(Clone, Default)]
pub struct GovernanceStore(Arc<RwLock<HashMap<u64, GrantProposal>>>);

impl GovernanceStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or overwrite a proposal.
    pub async fn insert(&self, p: GrantProposal) {
        self.0.write().await.insert(p.id, p);
    }

    /// Retrieve a single proposal by id.
    pub async fn get(&self, id: u64) -> Option<GrantProposal> {
        self.0.read().await.get(&id).cloned()
    }

    /// Return all proposals sorted by id ascending.
    pub async fn all(&self) -> Vec<GrantProposal> {
        let mut v: Vec<GrantProposal> = self.0.read().await.values().cloned().collect();
        v.sort_by_key(|p| p.id);
        v
    }

    /// Increment the `votes_for` bitmask by setting the next available bit.
    /// Returns the updated proposal, or `None` if the id is not found.
    pub async fn vote_for(&self, id: u64) -> Option<GrantProposal> {
        let mut guard = self.0.write().await;
        let p = guard.get_mut(&id)?;
        // Set the lowest clear bit in votes_for to record one additional vote.
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
        let p = guard.get_mut(&id)?;
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
        let p = guard
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
        let got = store.get(1).await.unwrap();
        assert_eq!(got.id, 1);
        assert_eq!(got.proposer, "0xA");
    }

    #[tokio::test]
    async fn all_sorted_by_id() {
        let store = GovernanceStore::new();
        store.insert(sample_proposal(3, 1_000_000)).await;
        store.insert(sample_proposal(1, 1_000_000)).await;
        store.insert(sample_proposal(2, 1_000_000)).await;
        let all = store.all().await;
        assert_eq!(all.iter().map(|p| p.id).collect::<Vec<_>>(), vec![1, 2, 3]);
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
        // Advance past timelock but no votes → should fail quorum check.
        let err = store.execute(1, now + GrantProposal::TIMELOCK_SECS + 1).await;
        assert!(err.is_err());
        assert!(err.unwrap_err().contains("quorum"));
    }
}
