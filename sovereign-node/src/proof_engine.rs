/// Proof-of-Evolution engine (Rust MVP — Julia in production).
///
/// Evaluates SimulationProof → ProofEvaluation.
/// Anti-farming: novelty decays with repeated environment_hash.
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use sovereign_types::{
    GaussianProof, ProofDomain, ProofEvaluation, RealityTransferScore, SimulationProof,
};

// ── Novelty tracker ───────────────────────────────────────────────────────────

/// Tracks how many times each environment_hash has been submitted.
/// The same environment submitted repeatedly yields diminishing novelty.
#[derive(Clone, Default)]
pub struct NoveltyLedger(Arc<RwLock<HashMap<String, u32>>>);

impl NoveltyLedger {
    pub fn new() -> Self { Self::default() }

    /// Record a new submission and return the novelty score (1.0 for first, decaying).
    pub async fn record(&self, env_hash: &str) -> f64 {
        let mut map = self.0.write().await;
        let count = map.entry(env_hash.to_string()).or_insert(0);
        *count += 1;
        novelty_from_count(*count)
    }

    pub async fn count(&self, env_hash: &str) -> u32 {
        *self.0.read().await.get(env_hash).unwrap_or(&0)
    }
}

/// Novelty curve: first submission = 1.0, tenth = ~0.35, hundredth = ~0.14.
fn novelty_from_count(n: u32) -> f64 {
    1.0 / (n as f64).sqrt()
}

// ── Engine ────────────────────────────────────────────────────────────────────

pub struct ProofEngine {
    novelty: NoveltyLedger,
}

impl ProofEngine {
    pub fn new() -> Self {
        Self { novelty: NoveltyLedger::new() }
    }

    pub fn with_novelty(novelty: NoveltyLedger) -> Self {
        Self { novelty }
    }

    /// Evaluate a SimulationProof and return a ProofEvaluation.
    pub async fn evaluate_simulation(
        &self,
        proof: &SimulationProof,
    ) -> Result<ProofEvaluation, String> {
        let difficulty   = proof.difficulty().max(0.0);
        let quality      = proof.quality();
        let novelty      = self.novelty.record(&proof.environment_hash).await;
        let verification = verification_score(proof);
        let independence = 0.8;   // stub — real impl checks witness chain
        let utility      = utility_score(proof);

        Ok(ProofEvaluation::compute(
            proof.proof_id.clone(),
            ProofDomain::Simulation,
            difficulty,
            quality,
            novelty,
            verification,
            independence,
            utility,
        ))
    }

    /// Evaluate a GaussianProof and return a Spatial-domain ProofEvaluation.
    pub async fn evaluate_gaussian(
        &self,
        proof: &GaussianProof,
    ) -> Result<ProofEvaluation, String> {
        if proof.quality.aggregate() < 0.3 {
            return Err("gaussian quality below minimum threshold (0.3)".into());
        }
        let difficulty   = proof.difficulty().max(0.0);
        let quality      = proof.quality_score();
        let novelty      = self.novelty.record(&proof.splat_hash).await;
        let verification = if proof.signature.is_empty() { 0.6 } else { 0.9 };
        let independence = (1.0 + proof.quality.witness_count as f64 * 0.15).min(1.0);
        let utility      = proof.quality.area_novelty_factor() / 3.0;

        Ok(ProofEvaluation::compute(
            proof.proof_id.clone(),
            ProofDomain::Spatial,
            difficulty,
            quality,
            novelty,
            verification,
            independence,
            utility,
        ))
    }

    /// Evaluate a RealityTransferScore into a Physical-domain ProofEvaluation.
    /// Only eligible if rts.physical_proof_eligible is true.
    pub async fn evaluate_physical(
        &self,
        rts: &RealityTransferScore,
    ) -> Result<ProofEvaluation, String> {
        if !rts.physical_proof_eligible {
            return Err(format!(
                "RTS {:.3} below minimum {:.3} — physical proof not earned",
                rts.rts, RealityTransferScore::MIN_RTS_FOR_PROOF
            ));
        }

        let difficulty   = rts.rts * rts.mission_transfer * 20.0;
        let quality      = rts.rts;
        let novelty      = self.novelty.record(&rts.flight_receipt_id).await;
        let verification = if rts.sim_proof_id.is_some() { 0.95 } else { 0.70 };
        let independence = 0.85;
        let utility      = rts.mission_transfer;

        Ok(ProofEvaluation::compute(
            rts.flight_receipt_id.clone(),
            ProofDomain::Physical,
            difficulty,
            quality,
            novelty,
            verification,
            independence,
            utility,
        ))
    }
}

/// Verification score based on trajectory/checkpoint hashes being present.
fn verification_score(proof: &SimulationProof) -> f64 {
    let mut score = 0.0_f64;
    if !proof.trajectory_hash.is_empty() { score += 0.3; }
    if !proof.checkpoint_root.is_empty() { score += 0.3; }
    if !proof.sensor_hash.is_empty()     { score += 0.2; }
    if !proof.signature.is_empty()       { score += 0.2; }
    score
}

/// Utility: reward complete gate clearance and stable controller.
fn utility_score(proof: &SimulationProof) -> f64 {
    let gate_pct = proof.metrics.gates_cleared as f64
        / proof.metrics.gates_total.max(1) as f64;
    (gate_pct * 0.6 + proof.metrics.controller_stability * 0.4).clamp(0.0, 1.0)
}

impl Default for ProofEngine {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sovereign_types::{SimulationMetrics, SimulationOutcome};

    fn make_proof(env: &str, crashes: u32) -> SimulationProof {
        SimulationProof {
            proof_id:          "p1".into(),
            agent_id:          "a1".into(),
            principal_id:      "did:1".into(),
            simulation_id:     "sim:1".into(),
            environment_hash:  env.into(),
            world_hash:        "w1".into(),
            model_hash:        "m1".into(),
            controller_hash:   "c1".into(),
            input_hash:        "i1".into(),
            simulator_version: "0.1".into(),
            seed:              42,
            trajectory_hash:   "t1".into(),
            sensor_hash:       "s1".into(),
            checkpoint_root:   "r1".into(),
            metrics: SimulationMetrics {
                execution_time_ms:    12_000,
                energy_estimate:      80.0,
                gates_cleared:        5,
                gates_total:          5,
                crashes,
                collision_margin_m:   0.8,
                controller_stability: 0.9,
            },
            outcome:   if crashes == 0 {
                SimulationOutcome::Success
            } else {
                SimulationOutcome::Failure
            },
            timestamp: 0,
            signature: "sig".into(),
        }
    }

    #[tokio::test]
    async fn clean_run_is_mint_eligible() {
        let engine = ProofEngine::new();
        let pe = engine.evaluate_simulation(&make_proof("env:alpha", 0)).await.unwrap();
        assert!(pe.mint_eligible);
        assert!(pe.proof_value > 0.0);
    }

    #[tokio::test]
    async fn crashed_run_not_eligible() {
        let engine = ProofEngine::new();
        let pe = engine.evaluate_simulation(&make_proof("env:alpha", 1)).await.unwrap();
        assert!(!pe.mint_eligible);
    }

    #[tokio::test]
    async fn repeated_env_novelty_decays() {
        let engine = ProofEngine::new();
        let p1 = engine.evaluate_simulation(&make_proof("env:repeat", 0)).await.unwrap();
        let p2 = engine.evaluate_simulation(&make_proof("env:repeat", 0)).await.unwrap();
        let p3 = engine.evaluate_simulation(&make_proof("env:repeat", 0)).await.unwrap();
        assert!(p1.novelty > p2.novelty, "second submission less novel");
        assert!(p2.novelty > p3.novelty, "third even less novel");
    }

    #[tokio::test]
    async fn different_envs_keep_novelty() {
        let engine = ProofEngine::new();
        let pa = engine.evaluate_simulation(&make_proof("env:a", 0)).await.unwrap();
        let pb = engine.evaluate_simulation(&make_proof("env:b", 0)).await.unwrap();
        assert_eq!(pa.novelty, pb.novelty, "fresh envs get full novelty");
    }

    #[test]
    fn novelty_curve_shape() {
        assert_eq!(novelty_from_count(1), 1.0);
        assert!(novelty_from_count(4) < 1.0);
        assert!(novelty_from_count(4) > novelty_from_count(9));
        assert!(novelty_from_count(100) < 0.2);
    }
}
