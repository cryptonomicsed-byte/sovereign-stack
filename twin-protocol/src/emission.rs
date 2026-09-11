//! DailyEmissionAllocator — the bridge between valid proofs and MintAuthorization.
//!
//! ═══ CANONICAL FLOW (OSOVM_CANONICAL_ARCHITECTURE.md) ═══
//!
//!  Proof Submitted
//!    → ProofEvaluation  (f1_score + proof_value)
//!    → MintEligibility Gate  (f1_score >= current_difficulty)
//!    → DailyEmissionAllocator.allocate_minute()
//!    → MintAuthorization  (passed to Sui settlement)
//!    → mint_ase()  (only Sui executes this — never called directly from proof handler)
//!
//! INVARIANTS:
//!  - 1 Àṣẹ (1_000_000 micro-Àṣẹ) per minute — FIXED FOREVER, no halving
//!  - No valid proofs in a minute → splits equally to 1,440 inheritance wallets
//!  - SimulationUTXO: (veil_id, epoch_minute, trajectory_hash) is one-time-claimable
//!  - f1_score = economic weight (emission share); proof_value = trust tier progression
//!  - Novelty decay: weight *= 1/sqrt(prior submissions with same env_hash)
//!  - Difficulty: genesis 0.777, +0.001 per 2016 blocks, ceiling 0.98
//!  - Sabbath freeze: no allocation on Saturday UTC

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

// ─── Constants ────────────────────────────────────────────────────────────────

pub const MICRO_ASE_PER_MINUTE: u64 = 1_000_000;
pub const INHERITANCE_WALLET_COUNT: u64 = 1_440;
pub const GENESIS_DIFFICULTY: f64 = 0.777;
pub const MAX_DIFFICULTY: f64 = 0.98;
pub const DIFFICULTY_STEP: f64 = 0.001;
pub const DIFFICULTY_EPOCH_BLOCKS: u64 = 2_016;
pub const MINUTES_PER_DAY: u64 = 1_440;

// ─── Proof Claim ──────────────────────────────────────────────────────────────

/// A single worker's claim on a minute's emission slot.
/// Submitted after OSOVM VEIL layer evaluates the simulation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofClaim {
    /// Unique proof identifier (UUID or hash)
    pub proof_id: String,
    /// DID of the submitting worker/agent
    pub worker_did: String,
    /// Veil challenge ID (1–777 for engineering; 1–200 for sacred via PoG)
    pub veil_id: u16,
    /// Minutes since Unix epoch (not blocks — minute granularity)
    pub epoch_minute: u64,
    /// SHA256 of quantized trajectory — the PoUS proof artifact
    pub trajectory_hash: [u8; 32],
    /// F1 score [0.0, 1.0] — economic weight for emission share
    pub f1_score: f64,
    /// ProofValue [0.0, 1.0] — trust tier progression (T1→T5), separate from f1
    pub proof_value: f64,
    /// Hash of the simulation environment (veil params + seed) — for novelty decay
    pub env_hash: [u8; 32],
}

impl ProofClaim {
    /// Derive the SimulationUTXO key for this claim.
    pub fn utxo_key(&self) -> SimulationUTXOKey {
        SimulationUTXOKey {
            veil_id: self.veil_id,
            epoch_minute: self.epoch_minute,
            trajectory_hash: self.trajectory_hash,
        }
    }
}

// ─── SimulationUTXO ───────────────────────────────────────────────────────────

/// Bitcoin UTXO analogue — (veil_id, epoch_minute, trajectory_hash) is one-time-claimable.
/// Prevents double-claiming the same simulation result across multiple emission rounds.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SimulationUTXOKey {
    pub veil_id: u16,
    pub epoch_minute: u64,
    pub trajectory_hash: [u8; 32],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationUTXO {
    pub key: SimulationUTXOKey,
    pub worker_did: String,
    pub claimed: bool,
    pub claimed_at_minute: Option<u64>,
}

impl SimulationUTXO {
    pub fn new(claim: &ProofClaim) -> Self {
        SimulationUTXO {
            key: claim.utxo_key(),
            worker_did: claim.worker_did.clone(),
            claimed: false,
            claimed_at_minute: None,
        }
    }
}

// ─── Allocation Output ────────────────────────────────────────────────────────

/// Per-worker allocation within a single minute's 1 Àṣẹ.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerAllocation {
    pub worker_did: String,
    pub proof_id: String,
    pub micro_ase: u64,
    /// Weighted score used to compute the share (f1 × novelty_weight)
    pub weighted_score: f64,
}

/// Full result of allocating one minute's emission.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinuteAllocation {
    pub epoch_minute: u64,
    /// UTC seconds timestamp of the allocation (for Sabbath gate + audit)
    pub timestamp_utc: u64,
    pub total_micro_ase: u64,
    /// Worker allocations — empty when inheritance fallback is triggered
    pub workers: Vec<WorkerAllocation>,
    /// When true, 1 Àṣẹ splits equally to all 1,440 inheritance wallets
    pub inheritance_fallback: bool,
    /// micro-Àṣẹ per inheritance wallet (non-zero only on fallback)
    pub inheritance_micro_ase_per_wallet: u64,
    /// Difficulty threshold used for this minute
    pub difficulty_used: f64,
    /// Proof IDs that were rejected (f1 < difficulty)
    pub rejected_proof_ids: Vec<String>,
    /// UTXOs marked claimed this minute
    pub claimed_utxo_keys: Vec<SimulationUTXOKey>,
}

impl MinuteAllocation {
    /// Canonical receipt hash — chain each MinuteAllocation to the previous.
    /// This is the SimulationEpochChain: each epoch references prior epoch hash.
    pub fn receipt_hash(&self, prev_hash: &[u8; 32]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(prev_hash);
        hasher.update(self.epoch_minute.to_le_bytes());
        hasher.update(self.total_micro_ase.to_le_bytes());
        for w in &self.workers {
            hasher.update(w.worker_did.as_bytes());
            hasher.update(w.micro_ase.to_le_bytes());
        }
        hasher.finalize().into()
    }
}

// ─── MintAuthorization ────────────────────────────────────────────────────────

/// Output of DailyEmissionAllocator → passed to Sui settlement layer.
/// OSOVM RUNTIME signs this; only then does settlement call mint_ase().
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MintAuthorization {
    pub epoch_minute: u64,
    pub allocations: Vec<WorkerAllocation>,
    pub inheritance_fallback: bool,
    pub inheritance_micro_ase_per_wallet: u64,
    /// SHA256 of the MinuteAllocation receipt — chain anchor
    pub receipt_hash: [u8; 32],
    /// OSOVM RUNTIME signature (Ed25519, hex-encoded) — placeholder until CORE built
    pub runtime_signature: Option<String>,
}

// ─── DailyEmissionAllocator ───────────────────────────────────────────────────

/// The single bridge between proof evaluation and MintAuthorization.
/// Lives in OSOVM RUNTIME. Enforces the 1 Àṣẹ/minute ceiling.
pub struct DailyEmissionAllocator {
    /// env_hash → number of prior submissions (novelty decay counter)
    env_hash_counts: HashMap<[u8; 32], u64>,
    /// Set of claimed UTXO keys — prevents double-claiming
    claimed_utxos: HashMap<SimulationUTXOKey, SimulationUTXO>,
    /// Rolling chain hash — starts at genesis hash, updated each minute
    chain_hash: [u8; 32],
}

impl DailyEmissionAllocator {
    /// Canonical genesis chain hash — the first block anchor.
    pub const GENESIS_CHAIN_HASH: [u8; 32] = [
        0x4f, 0x53, 0x4f, 0x56, 0x4d, 0x5f, 0x47, 0x45,
        0x4e, 0x45, 0x53, 0x49, 0x53, 0x5f, 0x32, 0x30,
        0x32, 0x36, 0x30, 0x39, 0x31, 0x30, 0x5f, 0x41,
        0x53, 0x48, 0x45, 0x5f, 0x01, 0x44, 0x05, 0xa0,
    ];

    pub fn new() -> Self {
        DailyEmissionAllocator {
            env_hash_counts: HashMap::new(),
            claimed_utxos: HashMap::new(),
            chain_hash: Self::GENESIS_CHAIN_HASH,
        }
    }

    /// Novelty decay weight — penalizes repeated env_hash submissions.
    /// count = prior times this exact env_hash was seen.
    /// Returns 1.0 on first submission, 1/sqrt(n) thereafter.
    pub fn novelty_weight(prior_count: u64) -> f64 {
        if prior_count == 0 {
            1.0
        } else {
            1.0 / (prior_count as f64).sqrt()
        }
    }

    /// Dynamic difficulty — genesis 0.777, increases +0.001 per 2016-block epoch, ceiling 0.98.
    /// epoch_minute used as block proxy (1 block = 1 minute in PoUS).
    pub fn current_difficulty(epoch_minute: u64) -> f64 {
        let epochs_elapsed = epoch_minute / DIFFICULTY_EPOCH_BLOCKS;
        let adjusted = GENESIS_DIFFICULTY + (epochs_elapsed as f64 * DIFFICULTY_STEP);
        adjusted.min(MAX_DIFFICULTY)
    }

    /// Returns true if today (UTC from timestamp_utc) is Saturday — Sabbath freeze.
    pub fn is_sabbath(timestamp_utc: u64) -> bool {
        // Day of week from Unix epoch: epoch day 0 = Thursday
        // Thursday=0, Friday=1, Saturday=2, Sunday=3, Monday=4, Tuesday=5, Wednesday=6
        let day_of_week = (timestamp_utc / 86_400 + 4) % 7;
        day_of_week == 6 // Saturday
    }

    /// Core allocation: given all eligible proof claims for epoch_minute,
    /// compute per-worker micro-Àṣẹ allocations and return MintAuthorization.
    ///
    /// Rules:
    ///  1. Sabbath (Saturday UTC) → zero allocation, return empty authorization
    ///  2. Claims with f1_score < current_difficulty → rejected
    ///  3. Claims referencing already-claimed UTXOs → rejected
    ///  4. Eligible: weighted_score = f1_score × novelty_weight(prior_count)
    ///  5. Share = (weighted_score / total_weighted) × MICRO_ASE_PER_MINUTE
    ///  6. No eligible claims → inheritance fallback (1 Àṣẹ ÷ 1440 wallets)
    pub fn allocate_minute(
        &mut self,
        epoch_minute: u64,
        timestamp_utc: u64,
        claims: &[ProofClaim],
    ) -> (MinuteAllocation, MintAuthorization) {
        let difficulty = Self::current_difficulty(epoch_minute);
        let mut rejected_ids: Vec<String> = Vec::new();

        // Sabbath freeze
        if Self::is_sabbath(timestamp_utc) {
            let alloc = MinuteAllocation {
                epoch_minute,
                timestamp_utc,
                total_micro_ase: 0,
                workers: vec![],
                inheritance_fallback: false,
                inheritance_micro_ase_per_wallet: 0,
                difficulty_used: difficulty,
                rejected_proof_ids: claims.iter().map(|c| c.proof_id.clone()).collect(),
                claimed_utxo_keys: vec![],
            };
            let auth = MintAuthorization {
                epoch_minute,
                allocations: vec![],
                inheritance_fallback: false,
                inheritance_micro_ase_per_wallet: 0,
                receipt_hash: alloc.receipt_hash(&self.chain_hash),
                runtime_signature: None,
            };
            return (alloc, auth);
        }

        // Filter and score eligible claims
        let mut eligible: Vec<(&ProofClaim, f64)> = Vec::new();
        for claim in claims {
            // Reject: below difficulty
            if claim.f1_score < difficulty {
                rejected_ids.push(claim.proof_id.clone());
                continue;
            }
            // Reject: UTXO already claimed
            let key = claim.utxo_key();
            if self.claimed_utxos.get(&key).map(|u| u.claimed).unwrap_or(false) {
                rejected_ids.push(claim.proof_id.clone());
                continue;
            }
            let prior_count = *self.env_hash_counts.get(&claim.env_hash).unwrap_or(&0);
            let novelty = Self::novelty_weight(prior_count);
            let weighted = claim.f1_score * novelty;
            eligible.push((claim, weighted));
        }

        // Inheritance fallback when no eligible claims
        if eligible.is_empty() {
            let per_wallet = MICRO_ASE_PER_MINUTE / INHERITANCE_WALLET_COUNT;
            let alloc = MinuteAllocation {
                epoch_minute,
                timestamp_utc,
                total_micro_ase: MICRO_ASE_PER_MINUTE,
                workers: vec![],
                inheritance_fallback: true,
                inheritance_micro_ase_per_wallet: per_wallet,
                difficulty_used: difficulty,
                rejected_proof_ids: rejected_ids,
                claimed_utxo_keys: vec![],
            };
            let receipt = alloc.receipt_hash(&self.chain_hash);
            self.chain_hash = receipt;
            let auth = MintAuthorization {
                epoch_minute,
                allocations: vec![],
                inheritance_fallback: true,
                inheritance_micro_ase_per_wallet: per_wallet,
                receipt_hash: receipt,
                runtime_signature: None,
            };
            return (alloc, auth);
        }

        // Compute proportional allocations
        let total_weighted: f64 = eligible.iter().map(|(_, w)| w).sum();
        let mut workers: Vec<WorkerAllocation> = Vec::new();
        let mut claimed_keys: Vec<SimulationUTXOKey> = Vec::new();
        let mut distributed: u64 = 0;

        for (i, (claim, weighted)) in eligible.iter().enumerate() {
            // Last worker gets the remainder to avoid rounding loss
            let share = if i == eligible.len() - 1 {
                MICRO_ASE_PER_MINUTE - distributed
            } else {
                ((weighted / total_weighted) * MICRO_ASE_PER_MINUTE as f64) as u64
            };

            workers.push(WorkerAllocation {
                worker_did: claim.worker_did.clone(),
                proof_id: claim.proof_id.clone(),
                micro_ase: share,
                weighted_score: *weighted,
            });

            // Mark UTXO claimed
            let key = claim.utxo_key();
            let mut utxo = SimulationUTXO::new(claim);
            utxo.claimed = true;
            utxo.claimed_at_minute = Some(epoch_minute);
            claimed_keys.push(key.clone());
            self.claimed_utxos.insert(key, utxo);

            // Update novelty counter
            *self.env_hash_counts.entry(claim.env_hash).or_insert(0) += 1;

            distributed += share;
        }

        let alloc = MinuteAllocation {
            epoch_minute,
            timestamp_utc,
            total_micro_ase: MICRO_ASE_PER_MINUTE,
            workers: workers.clone(),
            inheritance_fallback: false,
            inheritance_micro_ase_per_wallet: 0,
            difficulty_used: difficulty,
            rejected_proof_ids: rejected_ids,
            claimed_utxo_keys: claimed_keys,
        };

        let receipt = alloc.receipt_hash(&self.chain_hash);
        self.chain_hash = receipt;

        let auth = MintAuthorization {
            epoch_minute,
            allocations: workers,
            inheritance_fallback: false,
            inheritance_micro_ase_per_wallet: 0,
            receipt_hash: receipt,
            runtime_signature: None,
        };

        (alloc, auth)
    }

    /// Current chain tip hash — the SimulationEpochChain anchor.
    pub fn chain_tip(&self) -> &[u8; 32] {
        &self.chain_hash
    }

    /// How many unique env_hashes have been seen (novelty decay state).
    pub fn env_hash_count(&self) -> usize {
        self.env_hash_counts.len()
    }

    /// Total UTXOs claimed since allocator was created.
    pub fn utxos_claimed(&self) -> usize {
        self.claimed_utxos.len()
    }
}

impl Default for DailyEmissionAllocator {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_claim(proof_id: &str, worker: &str, veil: u16, minute: u64, f1: f64) -> ProofClaim {
        let mut env_hash = [0u8; 32];
        env_hash[0] = veil as u8;
        env_hash[1] = (minute & 0xff) as u8;
        let mut traj_hash = [0u8; 32];
        traj_hash[0] = proof_id.as_bytes()[0];
        ProofClaim {
            proof_id: proof_id.to_string(),
            worker_did: worker.to_string(),
            veil_id: veil,
            epoch_minute: minute,
            trajectory_hash: traj_hash,
            f1_score: f1,
            proof_value: f1 * 0.9,
            env_hash,
        }
    }

    #[test]
    fn test_genesis_difficulty() {
        assert_eq!(DailyEmissionAllocator::current_difficulty(0), GENESIS_DIFFICULTY);
    }

    #[test]
    fn test_difficulty_increases_with_epochs() {
        let d0 = DailyEmissionAllocator::current_difficulty(0);
        let d1 = DailyEmissionAllocator::current_difficulty(DIFFICULTY_EPOCH_BLOCKS);
        assert!((d1 - d0 - DIFFICULTY_STEP).abs() < 1e-10);
    }

    #[test]
    fn test_difficulty_ceiling() {
        let d = DailyEmissionAllocator::current_difficulty(u64::MAX / 2);
        assert_eq!(d, MAX_DIFFICULTY);
    }

    #[test]
    fn test_novelty_weight_first_submission() {
        assert_eq!(DailyEmissionAllocator::novelty_weight(0), 1.0);
    }

    #[test]
    fn test_novelty_weight_decay() {
        let w4 = DailyEmissionAllocator::novelty_weight(4);
        assert!((w4 - 0.5).abs() < 1e-10, "1/sqrt(4) = 0.5");
    }

    #[test]
    fn test_inheritance_fallback_when_no_claims() {
        let mut alloc = DailyEmissionAllocator::new();
        let (minute_alloc, auth) = alloc.allocate_minute(0, 1_000_000, &[]);
        assert!(minute_alloc.inheritance_fallback);
        assert_eq!(minute_alloc.total_micro_ase, MICRO_ASE_PER_MINUTE);
        assert_eq!(
            minute_alloc.inheritance_micro_ase_per_wallet,
            MICRO_ASE_PER_MINUTE / INHERITANCE_WALLET_COUNT
        );
        assert!(auth.inheritance_fallback);
    }

    #[test]
    fn test_inheritance_fallback_when_all_below_difficulty() {
        let mut alloc = DailyEmissionAllocator::new();
        let claims = vec![make_claim("p1", "did:worker:1", 1, 0, 0.3)];
        let (minute_alloc, _) = alloc.allocate_minute(0, 1_000_000, &claims);
        assert!(minute_alloc.inheritance_fallback);
        assert_eq!(minute_alloc.rejected_proof_ids, vec!["p1"]);
    }

    #[test]
    fn test_single_winner_gets_full_emission() {
        let mut alloc = DailyEmissionAllocator::new();
        let claims = vec![make_claim("p1", "did:worker:1", 1, 0, 0.9)];
        let (minute_alloc, auth) = alloc.allocate_minute(0, 1_000_000, &claims);
        assert!(!minute_alloc.inheritance_fallback);
        assert_eq!(minute_alloc.workers.len(), 1);
        assert_eq!(minute_alloc.workers[0].micro_ase, MICRO_ASE_PER_MINUTE);
        assert_eq!(auth.allocations[0].micro_ase, MICRO_ASE_PER_MINUTE);
    }

    #[test]
    fn test_two_equal_workers_split_evenly() {
        let mut alloc = DailyEmissionAllocator::new();
        // Different env_hashes so novelty is 1.0 for both
        let mut c1 = make_claim("p1", "did:worker:1", 1, 0, 0.9);
        let mut c2 = make_claim("p2", "did:worker:2", 2, 0, 0.9);
        c1.env_hash[31] = 0xAA;
        c2.env_hash[31] = 0xBB;
        c1.trajectory_hash[31] = 0x01;
        c2.trajectory_hash[31] = 0x02;
        let (minute_alloc, _) = alloc.allocate_minute(0, 1_000_000, &[c1, c2]);
        assert_eq!(minute_alloc.workers.len(), 2);
        let total: u64 = minute_alloc.workers.iter().map(|w| w.micro_ase).sum();
        assert_eq!(total, MICRO_ASE_PER_MINUTE);
        // Each should get ~500_000 (within rounding)
        for w in &minute_alloc.workers {
            assert!(w.micro_ase >= 499_000 && w.micro_ase <= 501_000);
        }
    }

    #[test]
    fn test_utxo_double_claim_rejected() {
        let mut alloc = DailyEmissionAllocator::new();
        let claim = make_claim("p1", "did:worker:1", 1, 0, 0.9);
        let (_, _) = alloc.allocate_minute(0, 1_000_000, &[claim.clone()]);
        // Submit same proof again next minute
        let (minute2, _) = alloc.allocate_minute(1, 1_000_060, &[claim]);
        assert!(minute2.inheritance_fallback, "double-claimed UTXO must be rejected → fallback");
    }

    #[test]
    fn test_novelty_decay_reduces_share() {
        let mut alloc = DailyEmissionAllocator::new();
        // Two workers with same env_hash (repeated submission)
        let mut c1 = make_claim("p1", "did:worker:1", 1, 0, 0.9);
        let mut c2 = make_claim("p2", "did:worker:2", 1, 1, 0.9);
        // same env_hash
        c1.env_hash = [0x42u8; 32];
        c2.env_hash = [0x42u8; 32];
        // different trajectory hashes (different UTXOs)
        c1.trajectory_hash[31] = 0x01;
        c2.trajectory_hash[31] = 0x02;
        // minute 0: c1 submitted fresh → novelty=1.0
        alloc.allocate_minute(0, 1_000_000, &[c1]);
        // minute 1: c2 submitted same env_hash → novelty=1/sqrt(1)=1.0 for count=1
        let (m2, _) = alloc.allocate_minute(1, 1_000_060, &[c2]);
        // c2 should still win but with novelty weight applied
        assert!(!m2.inheritance_fallback);
        assert_eq!(m2.workers[0].weighted_score, 0.9 * (1.0_f64 / 1.0_f64.sqrt()));
    }

    #[test]
    fn test_sabbath_freeze() {
        let mut alloc = DailyEmissionAllocator::new();
        // 2026-09-12 is a Saturday — Unix timestamp 1757721600
        let saturday_utc: u64 = 1_757_721_600;
        assert!(DailyEmissionAllocator::is_sabbath(saturday_utc));
        let claims = vec![make_claim("p1", "did:worker:1", 1, 1_000_000, 0.99)];
        let (minute_alloc, auth) = alloc.allocate_minute(1_000_000, saturday_utc, &claims);
        assert_eq!(minute_alloc.total_micro_ase, 0);
        assert!(!minute_alloc.inheritance_fallback);
        assert!(auth.allocations.is_empty());
    }

    #[test]
    fn test_emission_chain_advances() {
        let mut alloc = DailyEmissionAllocator::new();
        let tip0 = *alloc.chain_tip();
        let claims = vec![make_claim("p1", "did:worker:1", 1, 0, 0.9)];
        alloc.allocate_minute(0, 1_000_000, &claims);
        let tip1 = *alloc.chain_tip();
        assert_ne!(tip0, tip1, "chain hash must advance after each allocation");
    }

    #[test]
    fn test_total_daily_emission_ceiling() {
        let mut alloc = DailyEmissionAllocator::new();
        let mut total: u64 = 0;
        // Simulate 1440 minutes (one full day)
        for minute in 0..MINUTES_PER_DAY {
            let mut traj = [0u8; 32];
            traj[0] = (minute & 0xff) as u8;
            traj[1] = ((minute >> 8) & 0xff) as u8;
            let mut env = [0u8; 32];
            env[0] = minute as u8;
            let claim = ProofClaim {
                proof_id: format!("p{}", minute),
                worker_did: "did:worker:daily".to_string(),
                veil_id: 1,
                epoch_minute: minute,
                trajectory_hash: traj,
                f1_score: 0.9,
                proof_value: 0.8,
                env_hash: env,
            };
            let (m, _) = alloc.allocate_minute(minute, 1_000_000 + minute * 60, &[claim]);
            total += m.total_micro_ase;
        }
        assert_eq!(total, MICRO_ASE_PER_MINUTE * MINUTES_PER_DAY);
    }
}
