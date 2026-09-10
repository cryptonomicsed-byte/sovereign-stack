/// Proof-of-Evolution tier system for Ọmọ Kọ́dà agents.
///
/// Tiers are not a reputation XP ladder — they are evolutionary certificates
/// earned through independently verified, domain-specific proof work.
use serde::{Deserialize, Serialize};

// ── Trust Tier ────────────────────────────────────────────────────────────────

/// T0–T5 trust ladder.  Advancement requires proof, not merely score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustTier {
    /// Newborn — observe, participate, learn; no consequential execution.
    T0,
    /// Curious — verified simple tasks; read/safe tools.
    T1,
    /// Creator — produces verifiable data, assets, and simulation results.
    T2,
    /// Builder — repeated simulation success + independent verification.
    T3,
    /// Architect — spatial mastery + supervised physical embodiment.
    T4,
    /// Sovereign — sustained multi-domain proof; autonomous embodiment eligible.
    T5,
}

impl TrustTier {
    pub fn name(&self) -> &'static str {
        match self {
            TrustTier::T0 => "Newborn",
            TrustTier::T1 => "Curious",
            TrustTier::T2 => "Creator",
            TrustTier::T3 => "Builder",
            TrustTier::T4 => "Architect",
            TrustTier::T5 => "Sovereign",
        }
    }

    /// Minimum aggregate proof score needed to be *eligible* for this tier.
    /// Actual promotion also requires domain-specific thresholds.
    pub fn min_proof_score(&self) -> f64 {
        match self {
            TrustTier::T0 => 0.0,
            TrustTier::T1 => 10.0,
            TrustTier::T2 => 50.0,
            TrustTier::T3 => 250.0,
            TrustTier::T4 => 1_250.0,
            TrustTier::T5 => 10_000.0,
        }
    }

    /// Maximum Busy-Beaver compute budget for this tier (existing OSOVM contract).
    pub fn busy_beaver_limit(&self) -> u64 {
        match self {
            TrustTier::T0 => 1,
            TrustTier::T1 => 6,
            TrustTier::T2 => 21,
            TrustTier::T3 => 107,
            TrustTier::T4 => 107,
            TrustTier::T5 => 47_176_870,
        }
    }

    /// Synapse token cap in micro-units.
    pub fn synapse_cap(&self) -> u64 {
        match self {
            TrustTier::T0 => 1_000_000,
            TrustTier::T1 => 5_000_000,
            TrustTier::T2 => 15_000_000,
            TrustTier::T3 => 40_000_000,
            TrustTier::T4 => 86_000_000,
            TrustTier::T5 => 86_000_000,
        }
    }

    /// Whether the tier can open unsupervised physical body sessions.
    pub fn autonomous_embodiment_eligible(&self) -> bool {
        matches!(self, TrustTier::T5)
    }

    /// Whether the tier can open human-supervised physical body sessions.
    pub fn supervised_embodiment_eligible(&self) -> bool {
        matches!(self, TrustTier::T4 | TrustTier::T5)
    }
}

impl std::fmt::Display for TrustTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", format!("{:?}", self), self.name())
    }
}

// ── Proof Domains ─────────────────────────────────────────────────────────────

/// The five Proof-of-Evolution domains.  An agent's tier eligibility is the
/// minimum across its domain scores, not the average.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProofDomain {
    /// ScarabSwarm / deterministic simulation results.
    Simulation,
    /// Gaussian splat / spatial twin quality and contribution.
    Spatial,
    /// Verified job completions.
    Work,
    /// Witness/attestation network participation quality.
    Witness,
    /// Physical embodiment session outcomes.
    Physical,
}

impl ProofDomain {
    pub const ALL: [ProofDomain; 5] = [
        ProofDomain::Simulation,
        ProofDomain::Spatial,
        ProofDomain::Work,
        ProofDomain::Witness,
        ProofDomain::Physical,
    ];
}

/// Per-domain accumulated proof scores.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProofVector {
    pub simulation: f64,
    pub spatial:    f64,
    pub work:       f64,
    pub witness:    f64,
    pub physical:   f64,
}

impl ProofVector {
    pub fn aggregate(&self) -> f64 {
        self.simulation + self.spatial + self.work + self.witness + self.physical
    }

    pub fn domain(&self, d: ProofDomain) -> f64 {
        match d {
            ProofDomain::Simulation => self.simulation,
            ProofDomain::Spatial    => self.spatial,
            ProofDomain::Work       => self.work,
            ProofDomain::Witness    => self.witness,
            ProofDomain::Physical   => self.physical,
        }
    }

    pub fn add(&mut self, d: ProofDomain, delta: f64) {
        let slot = match d {
            ProofDomain::Simulation => &mut self.simulation,
            ProofDomain::Spatial    => &mut self.spatial,
            ProofDomain::Work       => &mut self.work,
            ProofDomain::Witness    => &mut self.witness,
            ProofDomain::Physical   => &mut self.physical,
        };
        *slot += delta;
    }

    /// Derive the highest tier for which the aggregate AND each required domain
    /// meets the threshold.
    pub fn eligible_tier(&self) -> TrustTier {
        let agg = self.aggregate();
        if agg >= TrustTier::T5.min_proof_score()
            && self.simulation >= 2_000.0
            && self.spatial    >= 1_000.0
            && self.physical   >= 500.0
        {
            return TrustTier::T5;
        }
        if agg >= TrustTier::T4.min_proof_score()
            && self.simulation >= 400.0
            && self.spatial    >= 200.0
        {
            return TrustTier::T4;
        }
        if agg >= TrustTier::T3.min_proof_score() && self.simulation >= 80.0 {
            return TrustTier::T3;
        }
        if agg >= TrustTier::T2.min_proof_score() { return TrustTier::T2; }
        if agg >= TrustTier::T1.min_proof_score() { return TrustTier::T1; }
        TrustTier::T0
    }
}

// ── Proof Evaluation ──────────────────────────────────────────────────────────

/// Output of the Proof Engine (Julia in production; Rust for MVP).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofEvaluation {
    pub proof_id:       String,
    pub proof_type:     ProofDomain,
    pub difficulty:     f64,
    pub quality:        f64,
    pub novelty:        f64,
    pub verification:   f64,
    pub independence:   f64,
    pub utility:        f64,
    /// Composite proof value = product of all factors.
    pub proof_value:    f64,
    /// Suggested delta to add to the relevant domain.
    pub tier_delta:     f64,
    pub mint_eligible:  bool,
}

impl ProofEvaluation {
    pub fn compute(
        proof_id: String,
        proof_type: ProofDomain,
        difficulty: f64,
        quality: f64,
        novelty: f64,
        verification: f64,
        independence: f64,
        utility: f64,
    ) -> Self {
        let proof_value = difficulty * quality * novelty * verification * independence * utility;
        // tier_delta is log-scaled so farming diminishes quickly
        let tier_delta  = proof_value.ln().max(0.0);
        let mint_eligible = proof_value >= 1.0 && quality >= 0.5 && verification >= 0.5;
        Self {
            proof_id,
            proof_type,
            difficulty,
            quality,
            novelty,
            verification,
            independence,
            utility,
            proof_value,
            tier_delta,
            mint_eligible,
        }
    }
}

// ── Simulation Proof ──────────────────────────────────────────────────────────

/// Cryptographically committed simulation result — produced by ScarabSwarm,
/// evaluated by the Proof Engine, settled by Ọ̀ṢỌ́VM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationProof {
    pub proof_id:          String,
    pub agent_id:          String,
    pub principal_id:      String,
    pub simulation_id:     String,
    /// SHA-256 of the serialised environment configuration.
    pub environment_hash:  String,
    pub world_hash:        String,
    pub model_hash:        String,
    pub controller_hash:   String,
    pub input_hash:        String,
    pub simulator_version: String,
    pub seed:              u64,
    /// SHA-256 over the full trajectory waypoint sequence.
    pub trajectory_hash:   String,
    pub sensor_hash:       String,
    /// Merkle root of all checkpoint hashes.
    pub checkpoint_root:   String,
    pub metrics:           SimulationMetrics,
    pub outcome:           SimulationOutcome,
    pub timestamp:         u64,
    /// ed25519 signature from the agent over the proof.
    pub signature:         String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationMetrics {
    pub execution_time_ms:    u64,
    pub energy_estimate:      f64,
    pub gates_cleared:        u32,
    pub gates_total:          u32,
    pub crashes:              u32,
    pub collision_margin_m:   f64,
    pub controller_stability: f64,   // 0.0–1.0
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SimulationOutcome {
    Success,
    PartialSuccess { gates_cleared: u32, gates_total: u32 },
    Failure,
    Timeout,
}

impl SimulationProof {
    /// Compute difficulty from metrics.  More gates + tighter margin = harder.
    pub fn difficulty(&self) -> f64 {
        let gate_ratio = self.metrics.gates_cleared as f64
            / self.metrics.gates_total.max(1) as f64;
        let margin_factor = (self.metrics.collision_margin_m / 0.5).min(2.0);
        gate_ratio * margin_factor * 10.0
    }

    /// Quality from crashes and controller stability.
    pub fn quality(&self) -> f64 {
        if self.metrics.crashes > 0 { return 0.0; }
        (self.metrics.controller_stability * 0.7
            + (1.0 - (self.metrics.gates_total.saturating_sub(self.metrics.gates_cleared)) as f64
                / self.metrics.gates_total.max(1) as f64)
                * 0.3)
            .clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_ordering() {
        assert!(TrustTier::T0 < TrustTier::T5);
        assert!(TrustTier::T4 < TrustTier::T5);
        assert_eq!(TrustTier::T3, TrustTier::T3);
    }

    #[test]
    fn proof_vector_eligible_tier_t0_at_zero() {
        let pv = ProofVector::default();
        assert_eq!(pv.eligible_tier(), TrustTier::T0);
    }

    #[test]
    fn proof_vector_reaches_t1() {
        let mut pv = ProofVector::default();
        pv.add(ProofDomain::Work, 15.0);
        assert_eq!(pv.eligible_tier(), TrustTier::T1);
    }

    #[test]
    fn proof_vector_t3_requires_simulation() {
        let mut pv = ProofVector::default();
        pv.add(ProofDomain::Work, 300.0);  // aggregate OK but sim too low
        assert_eq!(pv.eligible_tier(), TrustTier::T2);
        pv.add(ProofDomain::Simulation, 80.0);
        assert_eq!(pv.eligible_tier(), TrustTier::T3);
    }

    #[test]
    fn proof_evaluation_mint_eligible() {
        let pe = ProofEvaluation::compute(
            "test".into(), ProofDomain::Simulation,
            10.0, 0.9, 0.8, 0.95, 0.88, 0.9,
        );
        assert!(pe.mint_eligible);
        assert!(pe.proof_value > 1.0);
        assert!(pe.tier_delta > 0.0);
    }

    #[test]
    fn proof_evaluation_low_quality_not_eligible() {
        let pe = ProofEvaluation::compute(
            "test".into(), ProofDomain::Simulation,
            10.0, 0.3, 0.8, 0.95, 0.88, 0.9,
        );
        assert!(!pe.mint_eligible);
    }

    #[test]
    fn simulation_proof_difficulty_and_quality() {
        let proof = SimulationProof {
            proof_id:          "p1".into(),
            agent_id:          "agent:1".into(),
            principal_id:      "did:1".into(),
            simulation_id:     "sim:1".into(),
            environment_hash:  "e1".into(),
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
                execution_time_ms:    12_400,
                energy_estimate:      87.2,
                gates_cleared:        5,
                gates_total:          5,
                crashes:              0,
                collision_margin_m:   0.84,
                controller_stability: 0.91,
            },
            outcome:   SimulationOutcome::Success,
            timestamp: 0,
            signature: String::new(),
        };
        let d = proof.difficulty();
        let q = proof.quality();
        assert!(d > 0.0, "difficulty > 0");
        assert!(q > 0.8, "quality > 0.8 for clean run");
    }

    #[test]
    fn simulation_proof_crash_zeroes_quality() {
        let mut proof = SimulationProof {
            proof_id:          "p2".into(),
            agent_id:          "agent:1".into(),
            principal_id:      "did:1".into(),
            simulation_id:     "sim:2".into(),
            environment_hash:  "e1".into(),
            world_hash:        "w1".into(),
            model_hash:        "m1".into(),
            controller_hash:   "c1".into(),
            input_hash:        "i1".into(),
            simulator_version: "0.1".into(),
            seed:              0,
            trajectory_hash:   "t2".into(),
            sensor_hash:       "s2".into(),
            checkpoint_root:   "r2".into(),
            metrics: SimulationMetrics {
                execution_time_ms:    5_000,
                energy_estimate:      40.0,
                gates_cleared:        2,
                gates_total:          5,
                crashes:              1,
                collision_margin_m:   0.0,
                controller_stability: 0.3,
            },
            outcome:   SimulationOutcome::Failure,
            timestamp: 0,
            signature: String::new(),
        };
        assert_eq!(proof.quality(), 0.0);
        proof.metrics.crashes = 0;
        assert!(proof.quality() > 0.0);
    }

    #[test]
    fn autonomous_embodiment_only_t5() {
        assert!(!TrustTier::T4.autonomous_embodiment_eligible());
        assert!(TrustTier::T5.autonomous_embodiment_eligible());
        assert!(TrustTier::T4.supervised_embodiment_eligible());
        assert!(!TrustTier::T3.supervised_embodiment_eligible());
    }
}
