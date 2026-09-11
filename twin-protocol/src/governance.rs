//! Rust mirror of governance.move GrantProposal — for off-chain indexing.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrantProposal {
    pub id: u64,
    pub proposer: String,       // Sui address
    pub recipient: String,      // Sui address
    pub amount_micro_ase: u64,
    pub purpose: String,
    pub veil_id: u64,           // 0 = open grant
    pub votes_for: u64,
    pub votes_against: u64,
    pub created_at: u64,
    pub timelock_release: u64,
    pub executed: bool,
    pub rejected: bool,
}

impl GrantProposal {
    pub const QUORUM: u64 = 5;
    pub const TIMELOCK_SECS: u64 = 259_200;
    pub const MAX_AMOUNT_MICRO_ASE: u64 = 100_000_000_000;

    pub fn votes_for_count(&self) -> u32 {
        self.votes_for.count_ones() as u32
    }

    pub fn votes_against_count(&self) -> u32 {
        self.votes_against.count_ones() as u32
    }

    pub fn quorum_met(&self) -> bool {
        self.votes_for_count() as u64 >= Self::QUORUM
    }

    pub fn timelock_elapsed(&self, now_secs: u64) -> bool {
        now_secs >= self.timelock_release
    }

    pub fn executable(&self, now_secs: u64) -> bool {
        !self.executed && !self.rejected && self.quorum_met() && self.timelock_elapsed(now_secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> GrantProposal {
        GrantProposal {
            id: 1,
            proposer: "0xA".into(),
            recipient: "0xB".into(),
            amount_micro_ase: 1_000_000_000,
            purpose: "fund veil 42".into(),
            veil_id: 42,
            votes_for: 0b11111,
            votes_against: 0,
            created_at: 1000,
            timelock_release: 1000 + 259_200,
            executed: false,
            rejected: false,
        }
    }

    #[test]
    fn five_votes_meets_quorum() {
        let g = sample();
        assert!(g.quorum_met());
    }

    #[test]
    fn four_votes_not_quorum() {
        let mut g = sample();
        g.votes_for = 0b1111;
        assert!(!g.quorum_met());
    }

    #[test]
    fn executable_after_timelock() {
        let g = sample();
        assert!(g.executable(1000 + 259_200));
        assert!(!g.executable(1000 + 259_199));
    }
}
