//! Sovereign Seat store — 1440 permanent governance/economic offices.
//!
//! Architecture:
//!   1440 seats (= minutes/day = emission ticks/day — intentional resonance)
//!   Each seat is simultaneously: economic office + governance seat + T5 cert
//!   Losing T5 = seat locks (steward loses stewardship, not the office itself)
//!   Succession record is permanent and on-chain

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};
use sovereign_types::{SovereignWallet, SOVEREIGN_SEAT_COUNT};

#[derive(Clone, Default)]
pub struct SovereignSeatStore(Arc<RwLock<HashMap<u16, SovereignWallet>>>);

impl SovereignSeatStore {
    pub fn new() -> Self { Self::default() }

    /// Attempt to claim an unclaimed seat. Returns Err if already claimed.
    pub async fn claim(
        &self,
        seat_index: u16,
        steward_did: &str,
    ) -> Result<SovereignWallet, String> {
        if u32::from(seat_index) >= SOVEREIGN_SEAT_COUNT {
            return Err(format!(
                "seat_index {} out of range (max {})", seat_index, SOVEREIGN_SEAT_COUNT - 1
            ));
        }
        let mut map = self.0.write().await;
        if let Some(existing) = map.get(&seat_index) {
            if !existing.revoked {
                return Err(format!(
                    "seat {} already claimed by {}", seat_index, existing.current_steward
                ));
            }
        }
        let now = now_ms();
        let wallet = SovereignWallet {
            seat_index,
            current_steward:     steward_did.to_string(),
            steward_since_ms:    now,
            revoked:             false,
            revocation_reason:   None,
            council_queue_position: None,
            active_council_seat:  None,
            reached_first_steward: false,
            inheritance_chain:   vec![(steward_did.to_string(), now, 0)],
        };
        map.insert(seat_index, wallet.clone());
        Ok(wallet)
    }

    /// Revoke stewardship (e.g. T5 dropped). The seat persists; steward loses it.
    pub async fn revoke(
        &self,
        seat_index: u16,
        reason: &str,
    ) -> Result<SovereignWallet, String> {
        let mut map = self.0.write().await;
        let seat = map.get_mut(&seat_index)
            .ok_or_else(|| format!("seat {} not claimed", seat_index))?;
        if seat.revoked {
            return Err(format!("seat {} already revoked", seat_index));
        }
        // Seal succession entry (set held_until_ms)
        let now = now_ms();
        if let Some(last) = seat.inheritance_chain.last_mut() {
            last.2 = now;
        }
        seat.revoked           = true;
        seat.revocation_reason = Some(reason.to_string());
        Ok(seat.clone())
    }

    /// Enter the Council rotation queue.
    pub async fn enqueue_for_council(
        &self,
        seat_index: u16,
        position: u32,
    ) -> Result<SovereignWallet, String> {
        let mut map = self.0.write().await;
        let seat = map.get_mut(&seat_index)
            .ok_or_else(|| format!("seat {} not claimed", seat_index))?;
        if seat.revoked {
            return Err(format!("seat {} is revoked — cannot queue", seat_index));
        }
        seat.council_queue_position = Some(position);
        Ok(seat.clone())
    }

    pub async fn get(&self, seat_index: u16) -> Option<SovereignWallet> {
        self.0.read().await.get(&seat_index).cloned()
    }

    pub async fn all_claimed(&self) -> Vec<SovereignWallet> {
        let mut seats: Vec<SovereignWallet> = self.0.read().await.values().cloned().collect();
        seats.sort_by_key(|s| s.seat_index);
        seats
    }

    pub async fn active_count(&self) -> usize {
        self.0.read().await.values().filter(|s| !s.revoked).count()
    }

    pub async fn vacant_count(&self) -> usize {
        let claimed = self.0.read().await.len();
        SOVEREIGN_SEAT_COUNT as usize - claimed
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// API-level seat summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeatSummary {
    pub total_seats:   u16,
    pub claimed:       usize,
    pub active:        usize,
    pub revoked:       usize,
    pub vacant:        usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn claim_seat_succeeds() {
        let store = SovereignSeatStore::new();
        let seat = store.claim(0, "did:steward:A").await.unwrap();
        assert_eq!(seat.seat_index, 0);
        assert_eq!(seat.current_steward, "did:steward:A");
        assert!(!seat.revoked);
    }

    #[tokio::test]
    async fn claim_out_of_range_fails() {
        let store = SovereignSeatStore::new();
        let err = store.claim(1440, "did:steward:X").await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn double_claim_fails_when_not_revoked() {
        let store = SovereignSeatStore::new();
        store.claim(5, "did:steward:B").await.unwrap();
        let err = store.claim(5, "did:steward:C").await;
        assert!(err.is_err());
        assert!(err.unwrap_err().contains("already claimed"));
    }

    #[tokio::test]
    async fn revoke_then_reclaim() {
        let store = SovereignSeatStore::new();
        store.claim(10, "did:steward:D").await.unwrap();
        store.revoke(10, "T5 dropped").await.unwrap();

        // After revocation, a new steward can claim it
        let reclaimed = store.claim(10, "did:steward:E").await.unwrap();
        assert_eq!(reclaimed.current_steward, "did:steward:E");
    }

    #[tokio::test]
    async fn revoke_twice_fails() {
        let store = SovereignSeatStore::new();
        store.claim(20, "did:steward:F").await.unwrap();
        store.revoke(20, "first").await.unwrap();
        let err = store.revoke(20, "second").await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn council_enqueue() {
        let store = SovereignSeatStore::new();
        store.claim(1, "did:steward:G").await.unwrap();
        let queued = store.enqueue_for_council(1, 42).await.unwrap();
        assert_eq!(queued.council_queue_position, Some(42));
    }

    #[tokio::test]
    async fn vacant_count_tracks_claims() {
        let store = SovereignSeatStore::new();
        let initial = store.vacant_count().await;
        assert_eq!(initial, SOVEREIGN_SEAT_COUNT as usize);
        store.claim(0, "did:x").await.unwrap();
        assert_eq!(store.vacant_count().await, SOVEREIGN_SEAT_COUNT as usize - 1);
    }
}
