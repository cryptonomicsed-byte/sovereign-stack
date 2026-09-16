//! WorkClaim — generalized verification primitive for the sovereign L1.
//!
//! Replaces the GPU-specific `claimed_gpu_seconds` parameter in `is_fully_verified()`.
//! A WorkClaim carries WHAT was done, HOW to verify it, and WHO must corroborate.
//!
//! Reference: sovereign-eco-blueprint/specs/SECTOR_AGENT_DEPLOYMENT_SPEC.md
//! Hermes audit: "The verification layer, as built, verifies exactly one kind of thing."

use serde::{Deserialize, Serialize};

/// The generalized work kind — what domain produced this work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkDomain {
    /// GPU computation (training, inference, rendering, ZK, encoding).
    GpuCompute,
    /// Deterministic simulation run (ScarabSwarm, MuJoCo, VeilSim).
    Simulation,
    /// Simulation + real-world validation (sim prediction matched reality).
    SimToReal,
    /// 3D print job (slice → print → measure against sim prediction).
    PrintJob,
    /// Drone / aerial flight with telemetry.
    AerialFlight,
    /// Ground vehicle / quadruped operation.
    GroundRobot,
    /// Scientific simulation (FEA, CFD, molecular dynamics).
    SciSim,
    /// Spatial data capture (Gaussian splatting, point cloud, photogrammetry).
    SpatialCapture,
    /// Custom domain — dApp-defined, proof policy set by the contract.
    Custom(String),
}

impl WorkDomain {
    pub fn as_str(&self) -> &str {
        match self {
            Self::GpuCompute    => "gpu_compute",
            Self::Simulation    => "simulation",
            Self::SimToReal     => "sim_to_real",
            Self::PrintJob      => "print_job",
            Self::AerialFlight  => "aerial_flight",
            Self::GroundRobot   => "ground_robot",
            Self::SciSim        => "sci_sim",
            Self::SpatialCapture => "spatial_capture",
            Self::Custom(s)     => s.as_str(),
        }
    }

    /// Emission multiplier tier for the bonus ladder.
    /// Governance may override these via TOC_CONSTANTS [bonus_ladder].
    pub fn base_multiplier(&self) -> f64 {
        match self {
            Self::GpuCompute     => 1.0,
            Self::Simulation     => 1.0,
            Self::SimToReal      => 5.0,   // predicted + real-world confirmed
            Self::PrintJob       => 2.0,   // verified physical output
            Self::AerialFlight   => 3.0,   // real-world + telemetry
            Self::GroundRobot    => 3.0,
            Self::SciSim         => 2.0,
            Self::SpatialCapture => 2.0,
            Self::Custom(_)      => 1.0,
        }
    }
}

/// How many independent witnesses must corroborate this work claim.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitnessPolicy {
    /// Minimum number of independent witness receipts.
    pub min_witnesses: u8,
    /// Required witness type (e.g. "hardware", "peer_agent", "oracle", "human").
    pub witness_kind: String,
    /// Whether a Zàngbétò anchor is mandatory (always true for Dopamine minting).
    pub require_zangbeto: bool,
}

impl Default for WitnessPolicy {
    fn default() -> Self {
        Self {
            min_witnesses: 1,
            witness_kind: "hardware".into(),
            require_zangbeto: true,
        }
    }
}

/// What constitutes valid evidence for a given work domain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidencePolicy {
    /// Whether a cryptographic input commitment is required.
    pub require_input_hash: bool,
    /// Whether a cryptographic output commitment is required.
    pub require_output_hash: bool,
    /// Whether a sim-to-real measurement (e.g. calipers, telemetry) is required.
    pub require_measurement: bool,
    /// Custom policy tags for dApp-defined verification (e.g. "gcode_hash", "stl_hash").
    pub custom_tags: Vec<String>,
}

impl EvidencePolicy {
    pub fn for_gpu() -> Self {
        Self { require_input_hash: true, require_output_hash: true, require_measurement: false, custom_tags: vec![] }
    }
    pub fn for_simulation() -> Self {
        Self { require_input_hash: true, require_output_hash: true, require_measurement: false, custom_tags: vec!["sim_params_hash".into()] }
    }
    pub fn for_sim_to_real() -> Self {
        Self { require_input_hash: true, require_output_hash: true, require_measurement: true, custom_tags: vec!["prediction_hash".into(), "measurement_hash".into()] }
    }
    pub fn for_print_job() -> Self {
        Self { require_input_hash: true, require_output_hash: true, require_measurement: true, custom_tags: vec!["stl_hash".into(), "gcode_hash".into(), "slicer_params_hash".into()] }
    }
    pub fn for_aerial_flight() -> Self {
        Self { require_input_hash: false, require_output_hash: true, require_measurement: true, custom_tags: vec!["telemetry_hash".into(), "flight_log_hash".into()] }
    }
}

/// The generalized work claim — replaces `claimed_gpu_seconds` as the gate input.
///
/// Callers assemble this struct and pass it to `is_fully_verified(claim)`.
/// The verifier checks each requirement based on `domain`, `evidence`, and `witness_policy`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkClaim {
    /// Unique ID for this claim (UUID or deterministic hash).
    pub claim_id: String,
    /// Agent submitting the claim.
    pub agent_id: String,
    /// Device that performed the work (GPU, printer, drone, robot).
    pub device_id: String,

    /// What domain produced this work.
    pub domain: WorkDomain,

    /// Hash of the work inputs (model, STL, sim config, waypoints…).
    pub input_commitment: Option<String>,
    /// Hash of the work outputs (weights, measurements, telemetry, print).
    pub output_commitment: Option<String>,
    /// Domain-specific measurement (e.g. print dimension check, GPS track).
    pub measurement_commitment: Option<String>,

    /// Evidence requirements for this claim.
    pub evidence: EvidencePolicy,
    /// Witness corroboration requirements.
    pub witness_policy: WitnessPolicy,

    /// How much work was done — units are domain-specific.
    ///   GpuCompute:    GPU-seconds
    ///   Simulation:    sim-seconds (wall-clock × parallelism)
    ///   PrintJob:      filament-grams × print-seconds
    ///   AerialFlight:  flight-seconds
    ///   GroundRobot:   operation-seconds
    pub claimed_quantity: f64,
    /// Human-readable unit of claimed_quantity (e.g. "gpu_seconds", "grams_filament").
    pub quantity_unit: String,

    /// Witness receipt IDs (from independent corroborators).
    pub witness_receipts: Vec<String>,
    /// Zàngbétò anchor (required for Dopamine minting).
    pub zangbeto_anchor: Option<String>,

    /// Governance-configurable bonus multiplier override. None = use domain default.
    pub bonus_multiplier_override: Option<f64>,

    pub created_at: u64,
}

impl WorkClaim {
    /// True iff all evidence and witness requirements are satisfied.
    pub fn is_fully_verified(&self) -> bool {
        // Input commitment check
        if self.evidence.require_input_hash && self.input_commitment.is_none() {
            return false;
        }
        // Output commitment check
        if self.evidence.require_output_hash && self.output_commitment.is_none() {
            return false;
        }
        // Real-world measurement check
        if self.evidence.require_measurement && self.measurement_commitment.is_none() {
            return false;
        }
        // Quantity must be positive
        if self.claimed_quantity <= 0.0 {
            return false;
        }
        // Witness count
        if self.witness_receipts.len() < self.witness_policy.min_witnesses as usize {
            return false;
        }
        // Zàngbétò anchor
        if self.witness_policy.require_zangbeto && self.zangbeto_anchor.is_none() {
            return false;
        }
        true
    }

    /// Effective emission multiplier (override > domain default).
    pub fn effective_multiplier(&self) -> f64 {
        self.bonus_multiplier_override.unwrap_or_else(|| self.domain.base_multiplier())
    }

    /// Compute score — quantity × effective multiplier, used for Dopamine allocation.
    pub fn compute_score(&self) -> f64 {
        if self.is_fully_verified() {
            self.claimed_quantity * self.effective_multiplier()
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_claim(domain: WorkDomain) -> WorkClaim {
        WorkClaim {
            claim_id: "claim-1".into(),
            agent_id: "agent-1".into(),
            device_id: "gpu-01".into(),
            input_commitment: Some("abc".into()),
            output_commitment: Some("def".into()),
            measurement_commitment: None,
            evidence: EvidencePolicy::for_gpu(),
            witness_policy: WitnessPolicy::default(),
            claimed_quantity: 3600.0,
            quantity_unit: "gpu_seconds".into(),
            witness_receipts: vec!["w-1".into()],
            zangbeto_anchor: Some("z-1".into()),
            bonus_multiplier_override: None,
            created_at: 0,
            domain,
        }
    }

    #[test]
    fn gpu_claim_fully_verified() {
        let claim = base_claim(WorkDomain::GpuCompute);
        assert!(claim.is_fully_verified());
        assert_eq!(claim.effective_multiplier(), 1.0);
        assert_eq!(claim.compute_score(), 3600.0);
    }

    #[test]
    fn sim_to_real_requires_measurement() {
        let mut claim = base_claim(WorkDomain::SimToReal);
        claim.evidence = EvidencePolicy::for_sim_to_real();
        // Missing measurement → not verified
        assert!(!claim.is_fully_verified());
        // Add measurement → verified, 5x multiplier
        claim.measurement_commitment = Some("meas-hash".into());
        assert!(claim.is_fully_verified());
        assert_eq!(claim.effective_multiplier(), 5.0);
        assert_eq!(claim.compute_score(), 3600.0 * 5.0);
    }

    #[test]
    fn print_job_requires_measurement() {
        let mut claim = base_claim(WorkDomain::PrintJob);
        claim.evidence = EvidencePolicy::for_print_job();
        assert!(!claim.is_fully_verified()); // no measurement
        claim.measurement_commitment = Some("caliper-hash".into());
        assert!(claim.is_fully_verified());
        assert_eq!(claim.effective_multiplier(), 2.0);
    }

    #[test]
    fn zero_quantity_fails() {
        let mut claim = base_claim(WorkDomain::GpuCompute);
        claim.claimed_quantity = 0.0;
        assert!(!claim.is_fully_verified());
    }

    #[test]
    fn missing_witness_fails() {
        let mut claim = base_claim(WorkDomain::GpuCompute);
        claim.witness_receipts = vec![];
        assert!(!claim.is_fully_verified());
    }

    #[test]
    fn missing_zangbeto_fails() {
        let mut claim = base_claim(WorkDomain::GpuCompute);
        claim.zangbeto_anchor = None;
        assert!(!claim.is_fully_verified());
    }

    #[test]
    fn governance_override_multiplier() {
        let mut claim = base_claim(WorkDomain::SimToReal);
        claim.measurement_commitment = Some("meas".into());
        claim.evidence = EvidencePolicy::for_sim_to_real();
        claim.bonus_multiplier_override = Some(10.0);
        assert_eq!(claim.effective_multiplier(), 10.0);
    }
}
