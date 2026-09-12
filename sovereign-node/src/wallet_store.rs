//! Sovereign wallet store — per-DID micro-Àṣẹ balance ledger.
//!
//! In-memory for the MVP. Production: Sui on-chain balance + local cache.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletEntry {
    pub did:              String,
    /// Balance in micro-Àṣẹ (1 Àṣẹ = 1_000_000 µÀṣẹ).
    pub balance_micro_ase: u64,
    /// Total credited across all time (monotonic).
    pub total_credited:   u64,
    /// Total debited across all time (monotonic).
    pub total_debited:    u64,
    /// Unix ms of last update.
    pub updated_at:       u64,
}

impl WalletEntry {
    fn new(did: String) -> Self {
        Self {
            did,
            balance_micro_ase: 0,
            total_credited:    0,
            total_debited:     0,
            updated_at:        now_ms(),
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[derive(Clone, Default)]
pub struct WalletStore(Arc<RwLock<HashMap<String, WalletEntry>>>);

impl WalletStore {
    pub fn new() -> Self { Self::default() }

    /// Credit `amount` µÀṣẹ to `did`, creating the wallet if it doesn't exist.
    /// Returns the new balance.
    pub async fn credit(&self, did: &str, amount: u64) -> u64 {
        let mut map = self.0.write().await;
        let entry = map.entry(did.to_string()).or_insert_with(|| WalletEntry::new(did.to_string()));
        entry.balance_micro_ase = entry.balance_micro_ase.saturating_add(amount);
        entry.total_credited    = entry.total_credited.saturating_add(amount);
        entry.updated_at        = now_ms();
        entry.balance_micro_ase
    }

    /// Debit `amount` µÀṣẹ from `did`.  Returns `Err` if insufficient balance.
    pub async fn debit(&self, did: &str, amount: u64) -> Result<u64, String> {
        let mut map = self.0.write().await;
        let entry = map.entry(did.to_string()).or_insert_with(|| WalletEntry::new(did.to_string()));
        if entry.balance_micro_ase < amount {
            return Err(format!(
                "insufficient balance: have {} µÀṣẹ, need {}",
                entry.balance_micro_ase, amount
            ));
        }
        entry.balance_micro_ase -= amount;
        entry.total_debited     = entry.total_debited.saturating_add(amount);
        entry.updated_at        = now_ms();
        Ok(entry.balance_micro_ase)
    }

    /// Get wallet by DID.
    pub async fn get(&self, did: &str) -> Option<WalletEntry> {
        self.0.read().await.get(did).cloned()
    }

    /// Return all wallets sorted by balance descending.
    pub async fn all(&self) -> Vec<WalletEntry> {
        let mut wallets: Vec<WalletEntry> = self.0.read().await.values().cloned().collect();
        wallets.sort_by(|a, b| b.balance_micro_ase.cmp(&a.balance_micro_ase));
        wallets
    }

    /// Total µÀṣẹ in circulation across all wallets.
    pub async fn total_supply(&self) -> u64 {
        self.0.read().await.values().map(|w| w.balance_micro_ase).sum()
    }

    /// Return the balance for a DID (0 if not found).
    pub async fn balance(&self, did: &str) -> u64 {
        self.0.read().await.get(did).map(|w| w.balance_micro_ase).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn credit_and_get() {
        let store = WalletStore::new();
        let bal = store.credit("did:node:alice", 500_000).await;
        assert_eq!(bal, 500_000);
        let entry = store.get("did:node:alice").await.unwrap();
        assert_eq!(entry.balance_micro_ase, 500_000);
        assert_eq!(entry.total_credited, 500_000);
    }

    #[tokio::test]
    async fn debit_success() {
        let store = WalletStore::new();
        store.credit("did:node:bob", 1_000_000).await;
        let bal = store.debit("did:node:bob", 300_000).await.unwrap();
        assert_eq!(bal, 700_000);
    }

    #[tokio::test]
    async fn debit_insufficient() {
        let store = WalletStore::new();
        store.credit("did:node:charlie", 100).await;
        let err = store.debit("did:node:charlie", 200).await;
        assert!(err.is_err());
        assert!(err.unwrap_err().contains("insufficient balance"));
    }

    #[tokio::test]
    async fn total_supply() {
        let store = WalletStore::new();
        store.credit("did:node:a", 1_000_000).await;
        store.credit("did:node:b", 2_000_000).await;
        assert_eq!(store.total_supply().await, 3_000_000);
    }

    #[tokio::test]
    async fn all_sorted_by_balance_desc() {
        let store = WalletStore::new();
        store.credit("did:node:low",  100_000).await;
        store.credit("did:node:high", 999_000).await;
        store.credit("did:node:mid",  500_000).await;
        let all = store.all().await;
        assert_eq!(all[0].did, "did:node:high");
        assert_eq!(all[2].did, "did:node:low");
    }
}
