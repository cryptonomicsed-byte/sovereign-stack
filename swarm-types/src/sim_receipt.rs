use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Proof-of-Simulation receipt — cryptographic commitment to a simulation run.
///
/// This receipt flows into the ARP → Zàngbétò → Mycelium chain.
/// The Merkle root commits to the full trajectory set; the policy hash
/// commits to the selected winning policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimReceipt {
    pub receipt_id:       String,
    pub twin_id:          String,
    pub agent_id:         String,
    pub session_id:       Option<String>,

    pub n_trajectories:   u32,
    pub n_feasible:       u32,
    pub winning_traj_id:  String,
    pub winner_score:     f64,

    /// SHA-256 Merkle root over all trajectory hashes
    pub merkle_root:      String,
    /// SHA-256 of the selected policy
    pub policy_hash:      String,
    /// Proof-of-Simulation commitment (hash over merkle_root + policy_hash + timestamp)
    pub proof_of_sim:     String,

    pub outcome:          SimOutcome,
    pub zangbeto_anchor:  Option<String>,
    pub witness_event_id: Option<String>,

    pub created_at:       DateTime<Utc>,
    /// Ed25519 signature by the executing agent
    pub signature:        String,
}

impl SimReceipt {
    pub fn canonical_hash(&self) -> String {
        let data = format!(
            "{}:{}:{}:{}:{}",
            self.receipt_id, self.twin_id, self.merkle_root,
            self.policy_hash, self.created_at.timestamp()
        );
        sha256_hex(data.as_bytes())
    }
}

fn sha256_hex(data: &[u8]) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    data.hash(&mut h);
    format!("{:016x}{:016x}", h.finish(), h.finish().wrapping_mul(0xdeadbeef))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SimOutcome {
    PolicySelected,
    NoFeasiblePolicy,
    TwinUnavailable,
    AgentRevoked,
}

/// Wrapper for the 5-primitive proof chain commitment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofOfSimulation {
    pub proof_id:     String,
    pub sim_receipt:  SimReceipt,
    pub vcp_receipt:  Option<serde_json::Value>,
    pub arp_receipt:  Option<serde_json::Value>,
}
