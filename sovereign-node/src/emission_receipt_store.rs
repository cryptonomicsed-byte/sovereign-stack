//! Emission receipt chain — every Àṣẹ distribution produces an EmissionReceipt.
//!
//! The receipts form a per-pool hash chain: each receipt includes the
//! `receipt_id` of the previous receipt for the same pool, ensuring the
//! entire emission history is auditable without a block explorer.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use sha2::{Sha256, Digest};
use sovereign_types::{EmissionReceipt, DistributionPool};

#[derive(Clone, Default)]
pub struct EmissionReceiptStore {
    inner: Arc<RwLock<EmissionInner>>,
}

#[derive(Default)]
struct EmissionInner {
    /// All receipts in insertion order.
    receipts: Vec<EmissionReceipt>,
    /// Index by receipt_id.
    by_id: HashMap<String, usize>,
    /// Most recent receipt_id per pool (for chaining).
    chain_head: HashMap<String, String>,
    /// Global emission counter.
    emission_number: u64,
}

fn pool_key(pool: &DistributionPool) -> String {
    pool.name().to_string()
}

fn compute_receipt_id(emission_number: u64, pool: &DistributionPool, timestamp_sec: u64) -> String {
    let mut h = Sha256::new();
    h.update(emission_number.to_le_bytes());
    h.update(pool.name().as_bytes());
    h.update(timestamp_sec.to_le_bytes());
    format!("emit:{}", hex::encode(h.finalize()))
}

impl EmissionReceiptStore {
    pub fn new() -> Self { Self::default() }

    /// Record one distribution tick. Returns the new EmissionReceipt.
    pub async fn record(
        &self,
        pool:                DistributionPool,
        amount_mist:         u64,
        candidate_set_hash:  String,
        scoring_method:      String,
        selected_recipient:  Option<String>,
        proof_id:            Option<String>,
        distribution_reason: String,
    ) -> EmissionReceipt {
        let mut inner = self.inner.write().await;
        let emission_number = {
            inner.emission_number += 1;
            inner.emission_number
        };

        let now = now_secs();
        let receipt_id = compute_receipt_id(emission_number, &pool, now);
        let key = pool_key(&pool);
        let previous_emission = inner.chain_head.get(&key).cloned();

        let receipt = EmissionReceipt {
            receipt_id: receipt_id.clone(),
            emission_number,
            timestamp_sec: now,
            pool,
            amount_mist,
            candidate_set_hash,
            scoring_method,
            selected_recipient,
            proof_id,
            distribution_reason,
            previous_emission,
            osovm_signature: "stub:unsigned".to_string(),
        };

        inner.chain_head.insert(key, receipt_id.clone());
        let idx = inner.receipts.len();
        inner.by_id.insert(receipt_id, idx);
        inner.receipts.push(receipt.clone());
        receipt
    }

    pub async fn get(&self, id: &str) -> Option<EmissionReceipt> {
        let inner = self.inner.read().await;
        let idx = *inner.by_id.get(id)?;
        inner.receipts.get(idx).cloned()
    }

    /// Return all receipts for a given pool, ordered by emission_number.
    pub async fn by_pool(&self, pool: &DistributionPool) -> Vec<EmissionReceipt> {
        let key = pool_key(pool);
        self.inner.read().await.receipts.iter()
            .filter(|r| pool_key(&r.pool) == key)
            .cloned().collect()
    }

    pub async fn all(&self) -> Vec<EmissionReceipt> {
        self.inner.read().await.receipts.clone()
    }

    pub async fn emission_number(&self) -> u64 {
        self.inner.read().await.emission_number
    }

    /// Chain head: latest receipt_id for this pool (for next chaining).
    pub async fn chain_head(&self, pool: &DistributionPool) -> Option<String> {
        self.inner.read().await.chain_head.get(&pool_key(pool)).cloned()
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn record_increments_emission_number() {
        let store = EmissionReceiptStore::new();
        let r1 = store.record(
            DistributionPool::Simulation, 1_000_000,
            "hash:a".into(), "proof_evaluation_v1".into(),
            None, None, "per-minute sim pool".into(),
        ).await;
        let r2 = store.record(
            DistributionPool::Research, 500_000,
            "hash:b".into(), "research_v1".into(),
            Some("did:worker:x".into()), Some("proof:123".into()), "research".into(),
        ).await;
        assert_eq!(r1.emission_number, 1);
        assert_eq!(r2.emission_number, 2);
        assert_eq!(store.emission_number().await, 2);
    }

    #[tokio::test]
    async fn chain_links_per_pool() {
        let store = EmissionReceiptStore::new();
        let r1 = store.record(
            DistributionPool::Simulation, 1_000_000,
            "h1".into(), "v1".into(), None, None, "tick 1".into(),
        ).await;
        let r2 = store.record(
            DistributionPool::Simulation, 1_000_000,
            "h2".into(), "v1".into(), None, None, "tick 2".into(),
        ).await;
        // r1 has no previous; r2 should chain back to r1
        assert!(r1.previous_emission.is_none());
        assert_eq!(r2.previous_emission.as_deref(), Some(r1.receipt_id.as_str()));
    }

    #[tokio::test]
    async fn different_pools_have_independent_chains() {
        let store = EmissionReceiptStore::new();
        let sim = store.record(
            DistributionPool::Simulation, 1_000_000,
            "hs".into(), "v1".into(), None, None, "sim".into(),
        ).await;
        let res = store.record(
            DistributionPool::Research, 500_000,
            "hr".into(), "v1".into(), None, None, "research".into(),
        ).await;
        // Both are first in their respective chains
        assert!(sim.previous_emission.is_none());
        assert!(res.previous_emission.is_none());
    }

    #[tokio::test]
    async fn get_by_id_round_trips() {
        let store = EmissionReceiptStore::new();
        let r = store.record(
            DistributionPool::Governance, 100,
            "hg".into(), "gov".into(), None, None, "governance tick".into(),
        ).await;
        let fetched = store.get(&r.receipt_id).await.unwrap();
        assert_eq!(fetched.emission_number, r.emission_number);
        assert_eq!(fetched.amount_mist, 100);
    }

    #[tokio::test]
    async fn by_pool_filters_correctly() {
        let store = EmissionReceiptStore::new();
        store.record(DistributionPool::Simulation, 1, "h".into(), "v".into(), None, None, "s".into()).await;
        store.record(DistributionPool::Research,   2, "h".into(), "v".into(), None, None, "r".into()).await;
        store.record(DistributionPool::Simulation, 3, "h".into(), "v".into(), None, None, "s2".into()).await;
        let sim = store.by_pool(&DistributionPool::Simulation).await;
        assert_eq!(sim.len(), 2);
        let res = store.by_pool(&DistributionPool::Research).await;
        assert_eq!(res.len(), 1);
    }
}
