//! OSOVM Token-of-Compute (ToC) GPU contribution pool.
//!
//! Architecture (from sovereign memory):
//!   GPU = Dopamine token  (total supply cap: 86B micro-units)
//!   Synapse = agent slice (cap: 86M; earned by burning 10 GPU)
//!   Decay: 1%/day applied to Synapse balances (not GPU)
//!   Èṣù tithe: 3.69% of each GPU mint goes to the protocol tithe wallet.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};

/// Micro-unit constants.
pub const GPU_SUPPLY_CAP:      u64 = 86_000_000_000_000_000; // 86B × 10^9 micro-GPU
pub const SYNAPSE_SUPPLY_CAP:  u64 =     86_000_000_000_000; // 86M × 10^9 micro-SYN
pub const GPU_PER_SYNAPSE_BURN: u64 = 10;     // 10 micro-GPU → 1 micro-Synapse (scaled)
pub const ESHU_TITHE_BPS:       u32 = 369;    // 3.69% expressed as basis points
pub const DECAY_BPS_PER_DAY:    u32 = 100;    // 1.00% per day = 100 bps

/// A single GPU contribution record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuContribution {
    pub contribution_id: String,
    pub contributor_did: String,
    pub device_id:       String,
    pub compute_units:   u64,
    pub proof_hash:      String,
    pub gpu_minted:      u64,  // micro-GPU credited to contributor
    pub eshu_tithe:      u64,  // micro-GPU withheld as protocol tithe
    pub timestamp:       u64,
}

/// Aggregate pool state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuPoolState {
    pub total_compute_units:  u64,
    pub total_gpu_minted:     u64,
    pub total_eshu_tithe:     u64,
    pub synapse_minted:       u64,
    pub contribution_count:   usize,
    pub decay_bps_per_day:    u32,
    pub eshu_tithe_bps:       u32,
    pub last_decay_epoch_day: u64,
}

#[derive(Clone, Default)]
pub struct GpuPool {
    inner: Arc<RwLock<GpuPoolInner>>,
}

#[derive(Default)]
struct GpuPoolInner {
    contributions: Vec<GpuContribution>,
    /// micro-GPU balance per contributor DID (after tithe).
    gpu_balances:  HashMap<String, u64>,
    /// micro-Synapse balance per DID.
    syn_balances:  HashMap<String, u64>,
    total_compute_units: u64,
    total_gpu_minted:    u64,
    total_eshu_tithe:    u64,
    synapse_minted:      u64,
    last_decay_epoch_day: u64,
}

impl GpuPool {
    pub fn new() -> Self { Self::default() }

    /// Record a GPU contribution and mint GPU tokens.
    ///
    /// `compute_units` is a raw measure (e.g. FLOP-hours × scale).
    /// Returns the contribution record (including how many micro-GPU were minted).
    pub async fn contribute(
        &self,
        contributor_did: &str,
        device_id:       &str,
        compute_units:   u64,
        proof_hash:      &str,
    ) -> GpuContribution {
        let mut inner = self.inner.write().await;

        let gpu_gross = compute_units; // 1 compute-unit → 1 micro-GPU (simplest mint ratio)
        let eshu_tithe = (gpu_gross as u128 * ESHU_TITHE_BPS as u128 / 10_000) as u64;
        let gpu_net    = gpu_gross.saturating_sub(eshu_tithe);

        *inner.gpu_balances.entry(contributor_did.to_string()).or_default() += gpu_net;
        inner.total_compute_units += compute_units;
        inner.total_gpu_minted    += gpu_gross;
        inner.total_eshu_tithe    += eshu_tithe;

        let contribution_id = format!("gpu:{}", uuid::Uuid::new_v4());
        let now = now_ms();

        let record = GpuContribution {
            contribution_id: contribution_id.clone(),
            contributor_did: contributor_did.to_string(),
            device_id:       device_id.to_string(),
            compute_units,
            proof_hash:      proof_hash.to_string(),
            gpu_minted:      gpu_net,
            eshu_tithe,
            timestamp:       now,
        };
        inner.contributions.push(record.clone());
        record
    }

    /// Burn `gpu_amount` micro-GPU from `did` to mint Synapse tokens (10:1 ratio).
    /// Returns `(synapses_minted, remaining_gpu_balance)` or Err if insufficient balance.
    pub async fn burn_for_synapse(
        &self,
        did: &str,
        gpu_amount: u64,
    ) -> Result<(u64, u64), String> {
        let mut inner = self.inner.write().await;
        let bal = inner.gpu_balances.entry(did.to_string()).or_default();
        if *bal < gpu_amount {
            return Err(format!("insufficient GPU balance: have {bal}, need {gpu_amount}"));
        }
        *bal -= gpu_amount;
        let remaining = *bal;

        let synapses = gpu_amount / GPU_PER_SYNAPSE_BURN;
        *inner.syn_balances.entry(did.to_string()).or_default() += synapses;
        inner.synapse_minted += synapses;

        Ok((synapses, remaining))
    }

    /// Apply daily 1% decay to all Synapse balances. Safe to call once per epoch-day.
    /// Returns the total micro-Synapse burned by decay.
    pub async fn apply_daily_decay(&self, epoch_day: u64) -> u64 {
        let mut inner = self.inner.write().await;
        if inner.last_decay_epoch_day >= epoch_day {
            return 0; // already decayed this day
        }
        inner.last_decay_epoch_day = epoch_day;

        let mut total_decayed: u64 = 0;
        for bal in inner.syn_balances.values_mut() {
            let decay = (*bal as u128 * DECAY_BPS_PER_DAY as u128 / 10_000) as u64;
            *bal = bal.saturating_sub(decay);
            total_decayed += decay;
        }
        total_decayed
    }

    pub async fn state(&self) -> GpuPoolState {
        let inner = self.inner.read().await;
        GpuPoolState {
            total_compute_units:  inner.total_compute_units,
            total_gpu_minted:     inner.total_gpu_minted,
            total_eshu_tithe:     inner.total_eshu_tithe,
            synapse_minted:       inner.synapse_minted,
            contribution_count:   inner.contributions.len(),
            decay_bps_per_day:    DECAY_BPS_PER_DAY,
            eshu_tithe_bps:       ESHU_TITHE_BPS,
            last_decay_epoch_day: inner.last_decay_epoch_day,
        }
    }

    pub async fn gpu_balance(&self, did: &str) -> u64 {
        *self.inner.read().await.gpu_balances.get(did).unwrap_or(&0)
    }

    pub async fn synapse_balance(&self, did: &str) -> u64 {
        *self.inner.read().await.syn_balances.get(did).unwrap_or(&0)
    }

    pub async fn contributions(&self) -> Vec<GpuContribution> {
        self.inner.read().await.contributions.clone()
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
    async fn contribute_mints_gpu_minus_eshu_tithe() {
        let pool = GpuPool::new();
        let contrib = pool.contribute("did:worker:A", "gpu:device:1", 1_000, "hash:aaa").await;
        let expected_tithe = (1_000u128 * ESHU_TITHE_BPS as u128 / 10_000) as u64;
        assert_eq!(contrib.eshu_tithe, expected_tithe);
        assert_eq!(contrib.gpu_minted, 1_000 - expected_tithe);
        let bal = pool.gpu_balance("did:worker:A").await;
        assert_eq!(bal, contrib.gpu_minted);
    }

    #[tokio::test]
    async fn burn_for_synapse_deducts_gpu() {
        let pool = GpuPool::new();
        pool.contribute("did:worker:B", "gpu:device:2", 100, "hash:bbb").await;
        let initial = pool.gpu_balance("did:worker:B").await;
        let burn = initial / 2;
        let (synapses, remaining) = pool.burn_for_synapse("did:worker:B", burn).await.unwrap();
        assert_eq!(synapses, burn / GPU_PER_SYNAPSE_BURN);
        assert_eq!(remaining, initial - burn);
        assert_eq!(pool.synapse_balance("did:worker:B").await, synapses);
    }

    #[tokio::test]
    async fn burn_fails_on_insufficient_balance() {
        let pool = GpuPool::new();
        let err = pool.burn_for_synapse("did:worker:nobody", 9999).await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn daily_decay_reduces_synapse_balance() {
        let pool = GpuPool::new();
        pool.contribute("did:worker:C", "gpu:3", 10_000, "hash:ccc").await;
        let gpu = pool.gpu_balance("did:worker:C").await;
        pool.burn_for_synapse("did:worker:C", gpu).await.unwrap();
        let before = pool.synapse_balance("did:worker:C").await;
        let decayed = pool.apply_daily_decay(1).await;
        let after = pool.synapse_balance("did:worker:C").await;
        assert!(decayed > 0, "decay should burn some synapses");
        assert!(after < before, "balance should decrease after decay");
    }

    #[tokio::test]
    async fn daily_decay_idempotent_same_epoch_day() {
        let pool = GpuPool::new();
        pool.contribute("did:worker:D", "gpu:4", 10_000, "hash:ddd").await;
        let gpu = pool.gpu_balance("did:worker:D").await;
        pool.burn_for_synapse("did:worker:D", gpu).await.unwrap();
        let d1 = pool.apply_daily_decay(5).await;
        let d2 = pool.apply_daily_decay(5).await; // same day — no-op
        assert!(d1 > 0);
        assert_eq!(d2, 0, "second decay on same epoch_day must be 0");
    }

    #[tokio::test]
    async fn pool_state_reflects_contributions() {
        let pool = GpuPool::new();
        pool.contribute("did:worker:E", "gpu:5", 500, "hash:eee").await;
        let s = pool.state().await;
        assert_eq!(s.contribution_count, 1);
        assert_eq!(s.total_compute_units, 500);
        assert!(s.total_gpu_minted > 0);
        assert!(s.total_eshu_tithe > 0);
        assert_eq!(s.eshu_tithe_bps, ESHU_TITHE_BPS);
    }
}
