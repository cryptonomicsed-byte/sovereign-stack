/// Sovereign economic/governance machine — Shared Work Protocol v1, governance layer.
///
/// Architecture:
///   OSOVM mints via a deterministic global clock (1 Àṣẹ / minute).
///   8 DistributionPools receive allocations and run their own distribution contracts.
///   24 Sectors (2 per Council seat) govern strategic deployment.
///   1440 SovereignWallet seats provide stewardship + Council eligibility.
///   Council of 12 stewards the sectors via staggered quarterly rotation.
///   First Steward (highest-rotation seat) provides the final council layer.
///   Bínò provides constitutional sign-off on 5 defined categories only.
///   Zàngbétò records EmissionReceipts; OSOVM enforces execution.
use serde::{Deserialize, Serialize};

// ── Emission Clock ─────────────────────────────────────────────────────────────

/// The master emission rate. OSOVM is the sole mint authority.
/// Individual wallets or pools CANNOT initiate minting.
pub const EMISSION_PER_MINUTE_MIST: u64 = 1_000_000_000; // 1 Àṣẹ
pub const EMISSION_PER_HOUR_MIST: u64   = EMISSION_PER_MINUTE_MIST * 60;
pub const EMISSION_PER_DAY_MIST: u64    = EMISSION_PER_HOUR_MIST * 24;   // 1440 Àṣẹ
pub const EMISSION_PER_YEAR_MIST: u64   = EMISSION_PER_DAY_MIST * 365;   // 525_600 Àṣẹ

/// 1440 minutes/day = 1440 sovereign wallet seats. The dual meaning is exact.
pub const MINUTES_PER_DAY: u64 = 1_440;
pub const SOVEREIGN_SEAT_COUNT: u32 = 1_440;

// ── Distribution Pool ──────────────────────────────────────────────────────────

/// The 8 protocol-controlled allocation contracts.
///
/// Pools are NOT personal wallets. Each has its own distribution logic.
/// Allocation percentages sum to 100%. Stored as basis points (1 bp = 0.01%).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DistributionPool {
    /// Rewards highest-quality proof-of-simulation work each epoch.
    Simulation,
    /// Funds R&D: protocol improvement, tooling, security research.
    Research,
    /// Funds governance infrastructure and council operations.
    Governance,
    /// Protocol reserve for emergency circuit-breakers and stability.
    Reserve,
    /// Ecosystem grants reviewed and approved by Council.
    Grants,
    /// Universal Basic Income — distributed to all T1+ agents.
    Ubi,
    /// Lottery/burn mechanism for anti-inflation pressure.
    LotteryBurn,
    /// Sabbath/rest allocation — unclaimed becomes community treasury.
    Sabbath,
}

impl DistributionPool {
    pub const ALL: [DistributionPool; 8] = [
        DistributionPool::Simulation,
        DistributionPool::Research,
        DistributionPool::Governance,
        DistributionPool::Reserve,
        DistributionPool::Grants,
        DistributionPool::Ubi,
        DistributionPool::LotteryBurn,
        DistributionPool::Sabbath,
    ];

    /// Allocation in basis points (bps). 10_000 bps = 100%.
    /// Based on Elegbára treasury allocation from the architectural spec.
    pub fn allocation_bps(&self) -> u16 {
        match self {
            DistributionPool::Simulation  => 3_000, // 30%
            DistributionPool::Research    => 2_000, // 20%
            DistributionPool::Governance  => 1_000, // 10%
            DistributionPool::Reserve     => 1_000, // 10%
            DistributionPool::Grants      => 1_000, // 10%
            DistributionPool::Ubi         =>   500, //  5%
            DistributionPool::LotteryBurn => 1_000, // 10%
            DistributionPool::Sabbath     =>   500, //  5%
        }
    }

    /// Mist allocated to this pool per minute's emission.
    pub fn per_minute_mist(&self) -> u64 {
        EMISSION_PER_MINUTE_MIST * self.allocation_bps() as u64 / 10_000
    }

    pub fn name(&self) -> &'static str {
        match self {
            DistributionPool::Simulation  => "Simulation",
            DistributionPool::Research    => "Research",
            DistributionPool::Governance  => "Governance",
            DistributionPool::Reserve     => "Reserve",
            DistributionPool::Grants      => "Grants",
            DistributionPool::Ubi         => "UBI",
            DistributionPool::LotteryBurn => "LotteryBurn",
            DistributionPool::Sabbath     => "Sabbath",
        }
    }
}

// ── Emission Receipt ───────────────────────────────────────────────────────────

/// Produced every minute by OSOVM. One per emission tick.
///
/// Every Àṣẹ that enters the system is traceable through an EmissionReceipt.
/// No black-box minting — the protocol can always show WHY an Àṣẹ was issued.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmissionReceipt {
    pub receipt_id:          String,
    /// Global emission counter — strictly monotonic.
    pub emission_number:     u64,
    /// Unix timestamp (seconds) of this tick.
    pub timestamp_sec:       u64,
    /// Which pool received this tick's allocation.
    pub pool:                DistributionPool,
    /// Amount allocated to this pool this tick (mist).
    pub amount_mist:         u64,
    /// Hash of the candidate work set evaluated (pool-specific).
    pub candidate_set_hash:  String,
    /// Scoring method used (e.g. "proof_evaluation_v1").
    pub scoring_method:      String,
    /// DID of selected recipient (None if pooled for later batch).
    pub selected_recipient:  Option<String>,
    /// The work/proof receipt ID that justified this distribution.
    pub proof_id:            Option<String>,
    /// Human-readable reason for this specific distribution.
    pub distribution_reason: String,
    /// receipt_id of the previous EmissionReceipt for this pool (chain).
    pub previous_emission:   Option<String>,
    /// ed25519 signature by the OSOVM emission key.
    pub osovm_signature:     String,
}

// ── Simulation Epoch ───────────────────────────────────────────────────────────

/// Epoch boundaries for the Simulation Pool distribution algorithm.
///
/// Candidate leaderboards finalise at different cadences to prevent
/// a single large simulation from monopolising the pool indefinitely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EpochKind {
    /// Every minute — emission tick, single best eligible work rewarded.
    Minute,
    /// Every hour — leaderboard snapshot, rank computed across 60 ticks.
    Hour,
    /// Every day (1440 ticks) — distribution statistics finalised.
    Day,
    /// Every week (10080 ticks) — simulation ranking epoch finalised.
    Week,
}

impl EpochKind {
    pub fn ticks(&self) -> u64 {
        match self {
            EpochKind::Minute => 1,
            EpochKind::Hour   => 60,
            EpochKind::Day    => 1_440,
            EpochKind::Week   => 10_080,
        }
    }

    pub fn duration_secs(&self) -> u64 { self.ticks() * 60 }
}

/// Eligibility criteria for Simulation Pool distribution.
/// All conditions must be true for a simulation to be a candidate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimEligibility {
    /// Proof must be structurally valid (hashes check out).
    pub proof_valid:          bool,
    /// Agent must be at least T2 (Creator) to receive sim rewards.
    pub min_tier_met:         bool,
    /// At least 2 witnesses have countersigned the proof.
    pub witnesses_valid:      bool,
    /// This simulation has not already been rewarded this epoch.
    pub no_duplicate_reward:  bool,
    /// Physical/spatial evidence is anchored (for T4+ claims).
    pub evidence_anchored:    bool,
    /// Composite proof score (difficulty × quality × novelty × verification
    /// × independence × utility × witness_confidence).
    pub proof_score:          f64,
}

impl SimEligibility {
    pub fn is_eligible(&self) -> bool {
        self.proof_valid
            && self.min_tier_met
            && self.witnesses_valid
            && self.no_duplicate_reward
            && self.evidence_anchored
    }
}

// ── Council ────────────────────────────────────────────────────────────────────

pub const COUNCIL_SEAT_COUNT: u8 = 12;
pub const SECTORS_PER_SEAT: u8 = 2;
pub const SECTOR_COUNT: u8 = 24; // COUNCIL_SEAT_COUNT × SECTORS_PER_SEAT

/// One of the 12 rotating Council seats.
///
/// Each seat stewards 2 of the 24 sectors.
/// Rotation is staggered: seats 1–3 rotate in month 1, 4–6 in month 2, etc.
/// This prevents entire institutional memory loss at one rotation point.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CouncilSeat {
    /// Seat index 0–11.
    pub seat_index:     u8,
    /// DID of the current councilor.
    pub councilor_did:  String,
    /// The two sector indices (0–23) this seat stewards.
    pub sectors:        [u8; 2],
    /// When this councilor's term began (Unix timestamp ms).
    pub term_start_ms:  u64,
    /// When this seat is next eligible for rotation (Unix timestamp ms).
    pub term_end_ms:    u64,
    /// Number of completed terms by this councilor.
    pub terms_served:   u32,
    /// Whether this councilor has reached First Steward rotation.
    pub is_first_steward: bool,
}

impl CouncilSeat {
    /// Quarterly rotation: 91 days × 24 × 60 × 60 × 1000 ms.
    pub const TERM_DURATION_MS: u64 = 91 * 24 * 60 * 60 * 1_000;

    /// Which rotation cohort a seat belongs to (0=seats 0-2, 1=seats 3-5, ...).
    /// Staggered monthly within the quarterly cycle.
    pub fn rotation_cohort(&self) -> u8 { self.seat_index / 3 }

    /// The two sectors for a given seat index.
    pub fn sectors_for(seat_index: u8) -> [u8; 2] {
        let base = seat_index * SECTORS_PER_SEAT;
        [base, base + 1]
    }
}

/// One of the 24 governance sectors.
///
/// Each sector has a domain and is stewarded by one Council seat.
/// Pools and sectors intersect: pool resources flow through sector governance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sector {
    /// Sector index 0–23.
    pub sector_index:  u8,
    /// Human-readable domain name (e.g. "Simulation Infrastructure").
    pub domain:        String,
    /// Council seat index (0–11) responsible for this sector.
    pub council_seat:  u8,
    /// The distribution pool(s) whose resources this sector governs.
    pub pools:         Vec<DistributionPool>,
}

impl Sector {
    /// Which Council seat governs this sector.
    pub fn governing_seat(sector_index: u8) -> u8 { sector_index / SECTORS_PER_SEAT }
}

// ── Governance Proposal Lifecycle ─────────────────────────────────────────────

/// A proposal must pass through the full governance stack before execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalStage {
    /// Created in a sector, awaiting sector councilor endorsement.
    Sector,
    /// Endorsed; awaiting 12-seat Council vote.
    CouncilReview,
    /// Passed council vote; awaiting First Steward review.
    FirstStewardReview,
    /// Conditionally approved; awaiting Bínò constitutional sign-off (if required).
    BinoReview,
    /// Fully approved; ready for OSOVM execution.
    Approved,
    /// Rejected at any stage.
    Rejected,
    /// Executed; receipt produced.
    Executed,
}

/// Bínò's constitutional veto is constrained to 5 defined categories.
/// Outside these categories, Bínò's sign-off is a ceremony, not an override.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BinoVetoCategory {
    ConstitutionalViolation,
    MonetaryInvariantViolation,
    SovereigntyViolation,
    CatastrophicSecurityRisk,
    EmergencyHalt,
}

/// Bínò's sign-off record. `veto` must be Some only for the 5 defined categories.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinoSignOff {
    pub proposal_id:    String,
    pub signed_by:      String,
    pub timestamp_ms:   u64,
    /// None = approved. Some(category) = constitutional veto.
    pub veto:           Option<BinoVetoCategory>,
    pub veto_reason:    Option<String>,
    pub signature:      String,
}

impl BinoSignOff {
    pub fn is_approved(&self) -> bool { self.veto.is_none() }
    pub fn is_veto(&self) -> bool { self.veto.is_some() }
}

// ── SovereignWallet ────────────────────────────────────────────────────────────

/// One of the 1440 permanent sovereign stewardship offices.
///
/// Seats correspond 1:1 to minutes of daily emission (seat_index ↔ minute_of_day).
/// The *seat* is permanent; the *steward* is revocable.
///
/// IMPORTANT: Seats do NOT mint Àṣẹ directly. The global clock mints.
/// Seats are:
///   (1) Economic offices: accumulated balance reflects historical distribution
///   (2) Governance seats: T5 holders are eligible for Council rotation queue
///   (3) Constitutional objects: every holder leaves a permanent succession record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SovereignWallet {
    /// Seat index 0–1439, corresponds to one minute of daily emission.
    pub seat_index:             u16,
    /// DID of the current steward.
    pub current_steward:        String,
    pub steward_since_ms:       u64,
    /// Stewardship is revoked when the holder drops below T5.
    pub revoked:                bool,
    pub revocation_reason:      Option<String>,
    /// Position in the Council rotation queue (None = not queued).
    pub council_queue_position: Option<u32>,
    /// Which council seat this steward currently occupies (None = not on council).
    pub active_council_seat:    Option<u8>,
    /// Has this steward ever reached First Steward?
    pub reached_first_steward:  bool,
    /// (steward_did, held_from_ms, held_until_ms) — permanent succession record.
    pub inheritance_chain:      Vec<(String, u64, u64)>,
}

impl SovereignWallet {
    /// Seat ID string.
    pub fn seat_id(&self) -> String { format!("seat:{:04}", self.seat_index) }

    /// Minute of day this seat corresponds to (0–1439).
    pub fn minute_of_day(&self) -> u16 { self.seat_index }

    /// Is this steward eligible for Council rotation queue?
    pub fn council_eligible(&self) -> bool {
        !self.revoked && self.council_queue_position.is_some()
    }
}

// ── Governance Strata ──────────────────────────────────────────────────────────

/// Summary of how resources and authority flow through the governance machine.
///
/// Used by documentation and monitoring tools to explain the architecture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceStrata {
    pub emission_rate_per_minute: u64,
    pub pool_count:               u8,
    pub sector_count:             u8,
    pub council_seat_count:       u8,
    pub sovereign_seat_count:     u32,
}

impl GovernanceStrata {
    pub fn canonical() -> Self {
        Self {
            emission_rate_per_minute: EMISSION_PER_MINUTE_MIST,
            pool_count:               8,
            sector_count:             SECTOR_COUNT,
            council_seat_count:       COUNCIL_SEAT_COUNT,
            sovereign_seat_count:     SOVEREIGN_SEAT_COUNT,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Emission arithmetic ──────────────────────────────────────────────

    #[test]
    fn emission_rate_hierarchy() {
        assert_eq!(EMISSION_PER_HOUR_MIST, EMISSION_PER_MINUTE_MIST * 60);
        assert_eq!(EMISSION_PER_DAY_MIST, EMISSION_PER_MINUTE_MIST * 1_440);
        assert_eq!(EMISSION_PER_YEAR_MIST, EMISSION_PER_MINUTE_MIST * 525_600);
    }

    #[test]
    fn sovereign_seats_equal_minutes_per_day() {
        assert_eq!(SOVEREIGN_SEAT_COUNT, MINUTES_PER_DAY as u32);
        assert_eq!(SOVEREIGN_SEAT_COUNT, 1_440);
    }

    // ── Pool allocations ─────────────────────────────────────────────────

    #[test]
    fn pool_allocations_sum_to_100_percent() {
        let total_bps: u32 = DistributionPool::ALL
            .iter()
            .map(|p| p.allocation_bps() as u32)
            .sum();
        assert_eq!(total_bps, 10_000, "pool allocations must sum to 100%");
    }

    #[test]
    fn simulation_pool_is_largest() {
        let sim_bps = DistributionPool::Simulation.allocation_bps();
        for pool in &DistributionPool::ALL {
            if *pool != DistributionPool::Simulation {
                assert!(sim_bps >= pool.allocation_bps(),
                    "Simulation pool should be largest: {} >= {}", sim_bps, pool.allocation_bps());
            }
        }
    }

    #[test]
    fn pool_per_minute_mist_sums_to_emission() {
        let total: u64 = DistributionPool::ALL
            .iter()
            .map(|p| p.per_minute_mist())
            .sum();
        // Rounding loss from integer division must be < 8 mist (one per pool).
        assert!(EMISSION_PER_MINUTE_MIST - total < 8,
            "pool mist sum {total} should be within 8 mist of emission {EMISSION_PER_MINUTE_MIST}");
    }

    #[test]
    fn simulation_pool_per_minute() {
        // 30% of 1_000_000_000 = 300_000_000 mist
        assert_eq!(DistributionPool::Simulation.per_minute_mist(), 300_000_000);
    }

    // ── Council / sector structure ───────────────────────────────────────

    #[test]
    fn council_sectors_cover_all_24() {
        let mut covered = [false; 24];
        for seat in 0..COUNCIL_SEAT_COUNT {
            let [s1, s2] = CouncilSeat::sectors_for(seat);
            assert!((s1 as u8) < SECTOR_COUNT, "sector {s1} out of range");
            assert!((s2 as u8) < SECTOR_COUNT, "sector {s2} out of range");
            covered[s1 as usize] = true;
            covered[s2 as usize] = true;
        }
        assert!(covered.iter().all(|&c| c), "not all 24 sectors are covered");
    }

    #[test]
    fn staggered_rotation_four_cohorts() {
        // Seats 0–2 = cohort 0, 3–5 = cohort 1, 6–8 = cohort 2, 9–11 = cohort 3
        for seat in 0..COUNCIL_SEAT_COUNT {
            let seat = CouncilSeat {
                seat_index:     seat,
                councilor_did:  "did:key:x".into(),
                sectors:        CouncilSeat::sectors_for(seat),
                term_start_ms:  0,
                term_end_ms:    CouncilSeat::TERM_DURATION_MS,
                terms_served:   0,
                is_first_steward: false,
            };
            let cohort = seat.rotation_cohort();
            assert!(cohort < 4, "cohort {cohort} out of range for seat {}", seat.seat_index);
            assert_eq!(cohort, seat.seat_index / 3);
        }
    }

    #[test]
    fn sector_governing_seat() {
        assert_eq!(Sector::governing_seat(0), 0);
        assert_eq!(Sector::governing_seat(1), 0);
        assert_eq!(Sector::governing_seat(2), 1);
        assert_eq!(Sector::governing_seat(22), 11);
        assert_eq!(Sector::governing_seat(23), 11);
    }

    // ── Bínò veto ────────────────────────────────────────────────────────

    #[test]
    fn bino_approval_when_no_veto() {
        let sign_off = BinoSignOff {
            proposal_id: "prop:1".into(),
            signed_by:   "did:key:bino".into(),
            timestamp_ms: 0,
            veto:         None,
            veto_reason:  None,
            signature:    "sig".into(),
        };
        assert!(sign_off.is_approved());
        assert!(!sign_off.is_veto());
    }

    #[test]
    fn bino_constitutional_veto() {
        let sign_off = BinoSignOff {
            proposal_id: "prop:2".into(),
            signed_by:   "did:key:bino".into(),
            timestamp_ms: 0,
            veto:         Some(BinoVetoCategory::MonetaryInvariantViolation),
            veto_reason:  Some("proposal would alter emission rate".into()),
            signature:    "sig".into(),
        };
        assert!(!sign_off.is_approved());
        assert!(sign_off.is_veto());
        assert_eq!(sign_off.veto, Some(BinoVetoCategory::MonetaryInvariantViolation));
    }

    // ── SovereignWallet ──────────────────────────────────────────────────

    #[test]
    fn sovereign_wallet_seat_id_format() {
        let w = SovereignWallet {
            seat_index:             7,
            current_steward:        "did:key:abc".into(),
            steward_since_ms:       0,
            revoked:                false,
            revocation_reason:      None,
            council_queue_position: None,
            active_council_seat:    None,
            reached_first_steward:  false,
            inheritance_chain:      vec![],
        };
        assert_eq!(w.seat_id(), "seat:0007");
        assert_eq!(w.minute_of_day(), 7);
        assert!(!w.council_eligible());
    }

    #[test]
    fn sovereign_wallet_council_eligible_when_queued() {
        let w = SovereignWallet {
            seat_index:             100,
            current_steward:        "did:key:xyz".into(),
            steward_since_ms:       0,
            revoked:                false,
            revocation_reason:      None,
            council_queue_position: Some(42),
            active_council_seat:    None,
            reached_first_steward:  false,
            inheritance_chain:      vec![],
        };
        assert!(w.council_eligible());
    }

    #[test]
    fn sovereign_wallet_revoked_not_eligible() {
        let w = SovereignWallet {
            seat_index:             200,
            current_steward:        "did:key:xyz".into(),
            steward_since_ms:       0,
            revoked:                true,
            revocation_reason:      Some("dropped below T5".into()),
            council_queue_position: Some(10),
            active_council_seat:    None,
            reached_first_steward:  false,
            inheritance_chain:      vec![],
        };
        assert!(!w.council_eligible(), "revoked steward must not be council-eligible");
    }

    // ── Epoch kinds ──────────────────────────────────────────────────────

    #[test]
    fn epoch_ticks_hierarchy() {
        assert_eq!(EpochKind::Minute.ticks(), 1);
        assert_eq!(EpochKind::Hour.ticks(), 60);
        assert_eq!(EpochKind::Day.ticks(), 1_440);
        assert_eq!(EpochKind::Week.ticks(), 10_080);
    }

    #[test]
    fn epoch_duration_secs() {
        assert_eq!(EpochKind::Minute.duration_secs(), 60);
        assert_eq!(EpochKind::Hour.duration_secs(), 3_600);
        assert_eq!(EpochKind::Day.duration_secs(), 86_400);
        assert_eq!(EpochKind::Week.duration_secs(), 604_800);
    }

    // ── GovernanceStrata ─────────────────────────────────────────────────

    #[test]
    fn governance_strata_canonical() {
        let g = GovernanceStrata::canonical();
        assert_eq!(g.emission_rate_per_minute, EMISSION_PER_MINUTE_MIST);
        assert_eq!(g.pool_count, 8);
        assert_eq!(g.sector_count, 24);
        assert_eq!(g.council_seat_count, 12);
        assert_eq!(g.sovereign_seat_count, 1_440);
    }

    // ── SimEligibility ───────────────────────────────────────────────────

    #[test]
    fn sim_eligibility_all_conditions_required() {
        let base = SimEligibility {
            proof_valid:         true,
            min_tier_met:        true,
            witnesses_valid:     true,
            no_duplicate_reward: true,
            evidence_anchored:   true,
            proof_score:         5.0,
        };
        assert!(base.is_eligible());

        let bad_proof = SimEligibility { proof_valid: false, ..base.clone() };
        assert!(!bad_proof.is_eligible());

        let bad_tier = SimEligibility { min_tier_met: false, ..base.clone() };
        assert!(!bad_tier.is_eligible());

        let duplicate = SimEligibility { no_duplicate_reward: false, ..base.clone() };
        assert!(!duplicate.is_eligible());
    }

    // ── ProposalStage ordering ───────────────────────────────────────────

    #[test]
    fn proposal_stage_variants_exist() {
        let stages = vec![
            ProposalStage::Sector,
            ProposalStage::CouncilReview,
            ProposalStage::FirstStewardReview,
            ProposalStage::BinoReview,
            ProposalStage::Approved,
            ProposalStage::Rejected,
            ProposalStage::Executed,
        ];
        assert_eq!(stages.len(), 7);
    }
}
