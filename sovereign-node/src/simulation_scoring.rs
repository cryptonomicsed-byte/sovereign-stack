//! Simulation Pool 7-factor scoring.
//!
//! SimulationScore = Difficulty × Quality × Novelty × Verification
//!                 × Independence × Utility × WitnessConfidence
//!
//! Each factor ∈ [0.0, 1.0]; the composite is their product.
//! This is the economic weight for emission share — NOT the f1_score (which is
//! the PoUS simulation quality score).  The two are related but distinct.

use serde::{Deserialize, Serialize};

/// Input factors for the 7-factor simulation score.
/// All values clamped to [0.0, 1.0] before multiplication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationFactors {
    /// f1_score normalised by the current veil difficulty gate.
    /// = f1_score / current_difficulty  (clamped to 1.0 max)
    pub difficulty: f64,
    /// Raw f1_score from OSOVM evaluation.
    pub quality: f64,
    /// 1 / sqrt(prior submissions with same env_hash this epoch).
    /// = 1.0 for a novel simulation; < 1.0 for repeated envs.
    pub novelty: f64,
    /// Fraction of external verifiers that confirmed the proof.
    /// 0.0 = no external verification; 1.0 = all verifiers agreed.
    pub verification: f64,
    /// Whether this simulation is sufficiently different from other
    /// candidates in the same minute.  Computed as 1 - max_cosine_similarity.
    pub independence: f64,
    /// How directly this simulation maps to a declared utility objective.
    /// Tier-gated: T1=0.5 baseline, T2=0.65, T3=0.8, T4=0.9, T5=1.0.
    pub utility: f64,
    /// Confidence-weighted fraction of witnesses that counter-signed.
    /// = Σ(witness_confidence_weight) / num_witnesses
    pub witness_confidence: f64,
}

impl Default for SimulationFactors {
    fn default() -> Self {
        Self {
            difficulty:         1.0,
            quality:            1.0,
            novelty:            1.0,
            verification:       1.0,
            independence:       1.0,
            utility:            1.0,
            witness_confidence: 1.0,
        }
    }
}

impl SimulationFactors {
    /// Compute the composite score.
    pub fn score(&self) -> f64 {
        let d  = self.difficulty.clamp(0.0, 1.0);
        let q  = self.quality.clamp(0.0, 1.0);
        let n  = self.novelty.clamp(0.0, 1.0);
        let v  = self.verification.clamp(0.0, 1.0);
        let i  = self.independence.clamp(0.0, 1.0);
        let u  = self.utility.clamp(0.0, 1.0);
        let wc = self.witness_confidence.clamp(0.0, 1.0);
        d * q * n * v * i * u * wc
    }

    /// Novelty factor from prior submission count: 1/sqrt(n), n ≥ 1.
    pub fn novelty_from_prior_count(n: u64) -> f64 {
        1.0 / (n.max(1) as f64).sqrt()
    }

    /// Difficulty factor: f1 / current_difficulty (clamped to 1.0).
    pub fn difficulty_factor(f1_score: f64, current_difficulty: f64) -> f64 {
        if current_difficulty <= 0.0 { return 1.0; }
        (f1_score / current_difficulty).min(1.0).max(0.0)
    }

    /// Utility baseline by tier index (0=T1 … 4=T5).
    pub fn utility_for_tier(tier_index: u8) -> f64 {
        match tier_index {
            0 => 0.50,
            1 => 0.65,
            2 => 0.80,
            3 => 0.90,
            _ => 1.00,
        }
    }
}

/// Evaluated result with full breakdown.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationScoreResult {
    pub proof_id:    String,
    pub worker_did:  String,
    pub factors:     SimulationFactors,
    pub score:       f64,
    /// Emission share: this proof's fraction of the pool's per-minute tick.
    /// = score / sum_of_all_scores_this_minute  (computed by the caller)
    pub share:       f64,
    pub timestamp:   u64,
}

/// Compute emission shares across multiple scored claims in a single minute.
/// Returns (proof_id, share) pairs, normalised to sum = 1.0.
pub fn compute_emission_shares(scores: &[(String, f64)]) -> Vec<(String, f64)> {
    let total: f64 = scores.iter().map(|(_, s)| s).sum();
    if total <= 0.0 {
        // Equal split if no positive scores
        let n = scores.len() as f64;
        return scores.iter().map(|(id, _)| (id.clone(), if n > 0.0 { 1.0 / n } else { 0.0 })).collect();
    }
    scores.iter().map(|(id, s)| (id.clone(), s / total)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perfect_factors_give_1_0() {
        let f = SimulationFactors::default();
        assert!((f.score() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn zero_any_factor_gives_zero_score() {
        let mut f = SimulationFactors::default();
        f.novelty = 0.0;
        assert!((f.score() - 0.0).abs() < 1e-9);
    }

    #[test]
    fn novelty_from_prior_count_n1_is_1() {
        assert!((SimulationFactors::novelty_from_prior_count(1) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn novelty_from_prior_count_n4_is_half() {
        let n = SimulationFactors::novelty_from_prior_count(4);
        assert!((n - 0.5).abs() < 1e-9);
    }

    #[test]
    fn difficulty_factor_clamped() {
        // f1 above threshold → clamped to 1.0
        assert!((SimulationFactors::difficulty_factor(0.9, 0.7) - 1.0).abs() < 1e-6);
        // f1 = 0.5 / difficulty 0.777 ≈ 0.643
        let f = SimulationFactors::difficulty_factor(0.5, 0.777);
        assert!((f - 0.5 / 0.777).abs() < 1e-6);
    }

    #[test]
    fn utility_tier_progression() {
        assert!((SimulationFactors::utility_for_tier(0) - 0.50).abs() < 1e-9);
        assert!((SimulationFactors::utility_for_tier(4) - 1.00).abs() < 1e-9);
    }

    #[test]
    fn emission_shares_sum_to_one() {
        let scores = vec![
            ("p1".into(), 0.8),
            ("p2".into(), 0.5),
            ("p3".into(), 0.3),
        ];
        let shares = compute_emission_shares(&scores);
        let total: f64 = shares.iter().map(|(_, s)| s).sum();
        assert!((total - 1.0).abs() < 1e-9, "shares must sum to 1.0, got {total}");
    }

    #[test]
    fn emission_shares_equal_split_on_zeros() {
        let scores = vec![("p1".into(), 0.0), ("p2".into(), 0.0)];
        let shares = compute_emission_shares(&scores);
        for (_, s) in &shares {
            assert!((*s - 0.5).abs() < 1e-9);
        }
    }

    #[test]
    fn realistic_score_example() {
        let f = SimulationFactors {
            difficulty:         SimulationFactors::difficulty_factor(0.85, 0.777),
            quality:            0.85,
            novelty:            SimulationFactors::novelty_from_prior_count(1),
            verification:       0.9,
            independence:       0.8,
            utility:            SimulationFactors::utility_for_tier(2),
            witness_confidence: 0.75,
        };
        let s = f.score();
        assert!(s > 0.0 && s <= 1.0, "score {s} out of range");
        // Expected approx: 1.0 × 0.85 × 1.0 × 0.9 × 0.8 × 0.8 × 0.75 ≈ 0.367
        assert!(s > 0.3 && s < 0.5, "realistic score should be ~0.37, got {s}");
    }
}
