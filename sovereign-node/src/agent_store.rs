//! Agent store — birth registry, Dopamine/Synapse balances, stake gate.
//!
//! Architecture (OSOVM ToC memory):
//!   birthEndowment: 86B Dopamine, 86M Synapse
//!   AGENT_BIRTH_FEE: 10.0 ASE (deducted from caller's wallet before birth)
//!   Stake gate: requires 10% of current Synapse to access gated capabilities
//!   Dopamine: non-transferable, 1%/day decay
//!   Synapse:  transferable, 10:1 burn from Dopamine

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};

pub const DOPAMINE_BIRTH_ENDOWMENT: u64 = 86_000_000_000; // 86B
pub const SYNAPSE_BIRTH_ENDOWMENT:  u64 =     86_000_000; // 86M
pub const AGENT_BIRTH_FEE_MICRO_ASE: u64 = 10_000_000;    // 10 ASE in micro-ASE

/// Agent tier (T1–T5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentTier {
    T1, // Observer
    T2, // Creator
    T3, // Operator
    T4, // Architect
    T5, // Steward
}

impl AgentTier {
    pub fn label(&self) -> &'static str {
        match self {
            AgentTier::T1 => "observer",
            AgentTier::T2 => "creator",
            AgentTier::T3 => "operator",
            AgentTier::T4 => "architect",
            AgentTier::T5 => "steward",
        }
    }
}

/// An agent record created at birth.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRecord {
    pub agent_id:      String,
    pub owner_did:     String,
    pub born_at_ms:    u64,
    pub tier:          AgentTier,
    pub dopamine:      u64, // birth endowment; decays 1%/day
    pub synapse:       u64, // birth endowment; transferable
    pub staked_synapse: u64,
    pub proof_count:   u32,
    pub last_decay_epoch_day: u64,
}

impl AgentRecord {
    /// Stake gate: requires 10% of CURRENT total Synapse (staked + free).
    pub fn stake_requirement(&self) -> u64 {
        let total = self.synapse + self.staked_synapse;
        (total as u128 * 10 / 100) as u64
    }

    pub fn can_stake(&self, amount: u64) -> bool {
        let req = self.stake_requirement();
        amount >= req
    }

    pub fn free_synapse(&self) -> u64 { self.synapse }
    pub fn total_synapse(&self) -> u64 { self.synapse + self.staked_synapse }
}

#[derive(Clone, Default)]
pub struct AgentStore(Arc<RwLock<HashMap<String, AgentRecord>>>);

impl AgentStore {
    pub fn new() -> Self { Self::default() }

    /// Birth a new agent. Returns Err if agent_id already registered.
    pub async fn birth(&self, agent_id: &str, owner_did: &str) -> Result<AgentRecord, String> {
        let mut map = self.0.write().await;
        if map.contains_key(agent_id) {
            return Err(format!("agent {} already exists", agent_id));
        }
        let now = now_ms();
        let epoch_day = now / (86_400 * 1_000);
        let agent = AgentRecord {
            agent_id:    agent_id.to_string(),
            owner_did:   owner_did.to_string(),
            born_at_ms:  now,
            tier:        AgentTier::T1,
            dopamine:    DOPAMINE_BIRTH_ENDOWMENT,
            synapse:     SYNAPSE_BIRTH_ENDOWMENT,
            staked_synapse: 0,
            proof_count: 0,
            last_decay_epoch_day: 0,
        };
        map.insert(agent_id.to_string(), agent.clone());
        Ok(agent)
    }

    pub async fn get(&self, agent_id: &str) -> Option<AgentRecord> {
        self.0.read().await.get(agent_id).cloned()
    }

    pub async fn all(&self) -> Vec<AgentRecord> {
        let mut agents: Vec<AgentRecord> = self.0.read().await.values().cloned().collect();
        agents.sort_by_key(|a| a.born_at_ms);
        agents
    }

    /// Stake `amount` Synapse against this agent's capabilities.
    pub async fn stake(&self, agent_id: &str, amount: u64) -> Result<AgentRecord, String> {
        let mut map = self.0.write().await;
        let a = map.get_mut(agent_id)
            .ok_or_else(|| format!("agent {} not found", agent_id))?;
        if amount > a.synapse {
            return Err(format!("insufficient free Synapse: have {}, need {}", a.synapse, amount));
        }
        a.synapse       -= amount;
        a.staked_synapse += amount;
        Ok(a.clone())
    }

    /// Unstake Synapse (release back to free balance).
    pub async fn unstake(&self, agent_id: &str, amount: u64) -> Result<AgentRecord, String> {
        let mut map = self.0.write().await;
        let a = map.get_mut(agent_id)
            .ok_or_else(|| format!("agent {} not found", agent_id))?;
        if amount > a.staked_synapse {
            return Err(format!("insufficient staked Synapse: have {}, need {}", a.staked_synapse, amount));
        }
        a.staked_synapse -= amount;
        a.synapse        += amount;
        Ok(a.clone())
    }

    /// Apply 1%/day Dopamine decay to this agent. Safe to call repeatedly (idempotent per day).
    pub async fn apply_decay(&self, agent_id: &str, epoch_day: u64) -> u64 {
        let mut map = self.0.write().await;
        let Some(a) = map.get_mut(agent_id) else { return 0 };
        if a.last_decay_epoch_day >= epoch_day { return 0; }
        a.last_decay_epoch_day = epoch_day;
        let decay = (a.dopamine as u128 * 100 / 10_000) as u64; // 1% = 100 bps
        a.dopamine = a.dopamine.saturating_sub(decay);
        decay
    }

    /// Promote agent tier (by proof count threshold).
    pub async fn try_promote(&self, agent_id: &str) -> Option<AgentTier> {
        let mut map = self.0.write().await;
        let a = map.get_mut(agent_id)?;
        let new_tier = match a.tier {
            AgentTier::T1 if a.proof_count >= 1  => Some(AgentTier::T2),
            AgentTier::T2 if a.proof_count >= 10 => Some(AgentTier::T3),
            AgentTier::T3 if a.proof_count >= 50 => Some(AgentTier::T4),
            AgentTier::T4 if a.proof_count >= 200 => Some(AgentTier::T5),
            _ => None,
        };
        if let Some(t) = new_tier {
            a.tier = t;
        }
        new_tier
    }

    /// Increment proof count (called on successful proof submission).
    pub async fn increment_proofs(&self, agent_id: &str) {
        let mut map = self.0.write().await;
        if let Some(a) = map.get_mut(agent_id) {
            a.proof_count += 1;
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn birth_gives_full_endowment() {
        let store = AgentStore::new();
        let agent = store.birth("agent:001", "did:owner:A").await.unwrap();
        assert_eq!(agent.dopamine, DOPAMINE_BIRTH_ENDOWMENT);
        assert_eq!(agent.synapse,  SYNAPSE_BIRTH_ENDOWMENT);
        assert_eq!(agent.tier,     AgentTier::T1);
    }

    #[tokio::test]
    async fn duplicate_birth_fails() {
        let store = AgentStore::new();
        store.birth("agent:002", "did:owner:B").await.unwrap();
        let err = store.birth("agent:002", "did:owner:B").await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn stake_and_unstake_roundtrip() {
        let store = AgentStore::new();
        store.birth("agent:003", "did:owner:C").await.unwrap();
        let after_stake = store.stake("agent:003", 1_000_000).await.unwrap();
        assert_eq!(after_stake.staked_synapse, 1_000_000);
        assert_eq!(after_stake.synapse, SYNAPSE_BIRTH_ENDOWMENT - 1_000_000);

        let after_unstake = store.unstake("agent:003", 1_000_000).await.unwrap();
        assert_eq!(after_unstake.synapse, SYNAPSE_BIRTH_ENDOWMENT);
        assert_eq!(after_unstake.staked_synapse, 0);
    }

    #[tokio::test]
    async fn stake_fails_on_insufficient_balance() {
        let store = AgentStore::new();
        store.birth("agent:004", "did:owner:D").await.unwrap();
        let err = store.stake("agent:004", SYNAPSE_BIRTH_ENDOWMENT + 1).await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn dopamine_decay_applies_once_per_day() {
        let store = AgentStore::new();
        store.birth("agent:005", "did:owner:E").await.unwrap();
        let d1 = store.apply_decay("agent:005", 100).await;
        let d2 = store.apply_decay("agent:005", 100).await; // same day
        assert!(d1 > 0);
        assert_eq!(d2, 0, "decay should be idempotent within the same epoch_day");
        let agent = store.get("agent:005").await.unwrap();
        assert!(agent.dopamine < DOPAMINE_BIRTH_ENDOWMENT);
    }

    #[tokio::test]
    async fn tier_promotion_by_proof_count() {
        let store = AgentStore::new();
        store.birth("agent:006", "did:owner:F").await.unwrap();
        assert_eq!(store.get("agent:006").await.unwrap().tier, AgentTier::T1);

        store.increment_proofs("agent:006").await;
        let promoted = store.try_promote("agent:006").await;
        assert_eq!(promoted, Some(AgentTier::T2));
        assert_eq!(store.get("agent:006").await.unwrap().tier, AgentTier::T2);
    }

    #[tokio::test]
    async fn stake_gate_requirement_is_ten_pct() {
        let store = AgentStore::new();
        store.birth("agent:007", "did:owner:G").await.unwrap();
        let a = store.get("agent:007").await.unwrap();
        let req = a.stake_requirement();
        let expected = (SYNAPSE_BIRTH_ENDOWMENT as u128 * 10 / 100) as u64;
        assert_eq!(req, expected);
    }
}
