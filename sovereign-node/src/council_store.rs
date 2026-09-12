//! Council of 12 store — seats, sectors, staggered rotation.
//!
//! Architecture:
//!   12 seats × 2 sectors = 24 sectors total
//!   Staggered rotation: cohort 0 (seats 0-2) rotates first, then 3-5, 6-8, 9-11
//!   Term duration: 91 days (quarterly)
//!   Progression: sovereign wallet → governance queue → candidate → council seat

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};
use sovereign_types::{CouncilSeat, Sector, COUNCIL_SEAT_COUNT, SECTOR_COUNT};

#[derive(Clone, Default)]
pub struct CouncilStore(Arc<RwLock<CouncilInner>>);

#[derive(Default)]
struct CouncilInner {
    seats:   HashMap<u8, CouncilSeat>,
    sectors: HashMap<u8, Sector>,
}

impl CouncilStore {
    pub fn new() -> Self { Self::default() }

    /// Initialize all 12 seats (empty — no councilors yet).
    /// Must be called once at node startup; subsequent calls are no-ops if seats exist.
    pub async fn initialize(&self) {
        let mut inner = self.0.write().await;
        if !inner.seats.is_empty() { return; }

        let now = now_ms();
        let term_end = now + CouncilSeat::TERM_DURATION_MS;

        for seat in 0..COUNCIL_SEAT_COUNT {
            let sectors = CouncilSeat::sectors_for(seat);
            inner.seats.insert(seat, CouncilSeat {
                seat_index:     seat,
                councilor_did:  format!("did:genesis:seat:{seat}"),
                sectors,
                term_start_ms:  now,
                term_end_ms:    term_end,
                terms_served:   0,
                is_first_steward: seat == 0,
            });
        }

        // Initialize 24 sectors with default domain names.
        let domains = [
            "Simulation Infrastructure",    "Physical Reality Anchoring",
            "Digital Twin Provenance",       "VCP Device Governance",
            "Àṣẹ Monetary Policy",           "Emissions & Distribution",
            "Sovereign Identity",            "Privacy & Confidentiality",
            "Agent Lifecycle",               "Agent Ethics & Alignment",
            "Physical World Economy",        "Spatial Tile Registry",
            "DIP Protocol Standards",        "Node Federation",
            "Security & Zero Trust",         "Legal & Licensing",
            "Research & R&D Funding",        "Education & Onboarding",
            "Community & Governance Health", "Constitutional Amendments",
            "Emergency & Incident Response", "International Expansion",
            "Environmental Stewardship",     "Cultural & Heritage Preservation",
        ];

        for idx in 0..SECTOR_COUNT {
            let domain = domains.get(idx as usize)
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("Sector {idx}"));
            let council_seat = idx / 2;
            inner.sectors.insert(idx, Sector {
                sector_index: idx,
                domain,
                council_seat,
                pools: vec![],
            });
        }
    }

    pub async fn seat(&self, seat_index: u8) -> Option<CouncilSeat> {
        self.0.read().await.seats.get(&seat_index).cloned()
    }

    pub async fn all_seats(&self) -> Vec<CouncilSeat> {
        let inner = self.0.read().await;
        let mut seats: Vec<CouncilSeat> = inner.seats.values().cloned().collect();
        seats.sort_by_key(|s| s.seat_index);
        seats
    }

    pub async fn all_sectors(&self) -> Vec<Sector> {
        let inner = self.0.read().await;
        let mut sectors: Vec<Sector> = inner.sectors.values().cloned().collect();
        sectors.sort_by_key(|s| s.sector_index);
        sectors
    }

    /// Rotate a seat — install new councilor, carry forward terms_served.
    pub async fn rotate(
        &self,
        seat_index: u8,
        new_councilor_did: &str,
    ) -> Result<CouncilSeat, String> {
        let mut inner = self.0.write().await;
        let seat = inner.seats.get_mut(&seat_index)
            .ok_or_else(|| format!("seat {seat_index} not found"))?;

        let now = now_ms();
        if seat.term_end_ms > now {
            return Err(format!(
                "seat {} term has not ended yet (ends at {}ms, now {}ms)",
                seat_index, seat.term_end_ms, now
            ));
        }

        seat.terms_served   += 1;
        seat.councilor_did   = new_councilor_did.to_string();
        seat.term_start_ms   = now;
        seat.term_end_ms     = now + CouncilSeat::TERM_DURATION_MS;
        // Promote to First Steward on 3rd term in seat 0
        if seat_index == 0 && seat.terms_served >= 3 {
            seat.is_first_steward = true;
        }

        Ok(seat.clone())
    }

    /// Force-rotate a seat regardless of term end (for admin/test use).
    pub async fn force_rotate(
        &self,
        seat_index: u8,
        new_councilor_did: &str,
    ) -> Result<CouncilSeat, String> {
        let mut inner = self.0.write().await;
        let seat = inner.seats.get_mut(&seat_index)
            .ok_or_else(|| format!("seat {seat_index} not found"))?;
        let now = now_ms();
        seat.terms_served   += 1;
        seat.councilor_did   = new_councilor_did.to_string();
        seat.term_start_ms   = now;
        seat.term_end_ms     = now + CouncilSeat::TERM_DURATION_MS;
        Ok(seat.clone())
    }

    /// Which seats are currently eligible for rotation?
    pub async fn rotation_eligible(&self) -> Vec<CouncilSeat> {
        let now = now_ms();
        self.0.read().await.seats.values()
            .filter(|s| s.term_end_ms <= now)
            .cloned()
            .collect()
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Summary of council state for API responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CouncilSummary {
    pub seat_count:          u8,
    pub sector_count:        u8,
    pub rotation_eligible:   u8,
    pub seats:               Vec<CouncilSeat>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn initialize_creates_12_seats_24_sectors() {
        let store = CouncilStore::new();
        store.initialize().await;
        let seats   = store.all_seats().await;
        let sectors = store.all_sectors().await;
        assert_eq!(seats.len(), 12);
        assert_eq!(sectors.len(), 24);
    }

    #[tokio::test]
    async fn initialize_idempotent() {
        let store = CouncilStore::new();
        store.initialize().await;
        store.initialize().await; // second call is a no-op
        assert_eq!(store.all_seats().await.len(), 12);
    }

    #[tokio::test]
    async fn seat_zero_is_first_steward() {
        let store = CouncilStore::new();
        store.initialize().await;
        let seat0 = store.seat(0).await.unwrap();
        assert!(seat0.is_first_steward);
    }

    #[tokio::test]
    async fn sectors_assigned_to_correct_seats() {
        let store = CouncilStore::new();
        store.initialize().await;
        let sectors = store.all_sectors().await;
        // Sector 0 and 1 → seat 0; sector 2 and 3 → seat 1; etc.
        for s in &sectors {
            assert_eq!(s.council_seat, s.sector_index / 2);
        }
    }

    #[tokio::test]
    async fn rotate_fails_before_term_end() {
        let store = CouncilStore::new();
        store.initialize().await;
        let err = store.rotate(0, "did:new:councilor").await;
        assert!(err.is_err());
        assert!(err.unwrap_err().contains("not ended yet"));
    }

    #[tokio::test]
    async fn force_rotate_succeeds_and_increments_terms() {
        let store = CouncilStore::new();
        store.initialize().await;
        let seat = store.force_rotate(5, "did:new:seat5").await.unwrap();
        assert_eq!(seat.seat_index, 5);
        assert_eq!(seat.councilor_did, "did:new:seat5");
        assert_eq!(seat.terms_served, 1);
    }

    #[tokio::test]
    async fn rotation_cohort_correct() {
        let store = CouncilStore::new();
        store.initialize().await;
        let seats = store.all_seats().await;
        // Seats 0-2 → cohort 0; 3-5 → cohort 1; etc.
        for seat in &seats {
            assert_eq!(seat.rotation_cohort(), seat.seat_index / 3);
        }
    }
}
