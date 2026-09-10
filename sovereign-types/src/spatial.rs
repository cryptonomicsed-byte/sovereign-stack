/// Spatial/Gaussian proof types and Reality Transfer Score.
///
/// GaussianProof: quality-gated commitment to a spatial reconstruction.
/// RealityTransferScore: measures how well a simulation policy transferred to
/// the physical world (sim ↔ real divergence).
use serde::{Deserialize, Serialize};

// ── Gaussian Proof ────────────────────────────────────────────────────────────

/// Quality assessment of a Gaussian splat / spatial twin submission.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GaussianQualityMetrics {
    /// Fraction of the scan volume that was captured (0.0–1.0).
    pub capture_completeness: f64,
    /// Mean reprojection error of camera poses (lower = better; good < 0.5 px).
    pub pose_quality:         f64,   // 0.0 = perfect, 1.0 = terrible
    /// Geometric consistency score from point-cloud cross-validation (0.0–1.0).
    pub geometric_consistency: f64,
    /// Photometric PSNR-style quality (0.0–1.0 normalised).
    pub photometric_quality:  f64,
    /// Novel-view synthesis quality (LPIPS-style, 0.0–1.0).
    pub novel_view_quality:   f64,
    /// Semantic coverage: how many labelled categories are present (0.0–1.0).
    pub semantic_accuracy:    f64,
    /// Area covered in square metres.
    pub area_m2:              f64,
    /// Number of independent witness validations.
    pub witness_count:        u32,
}

impl GaussianQualityMetrics {
    /// Aggregate quality in [0, 1].
    pub fn aggregate(&self) -> f64 {
        let pose_score = 1.0 - self.pose_quality.clamp(0.0, 1.0);
        (self.capture_completeness * 0.20
            + pose_score             * 0.20
            + self.geometric_consistency * 0.20
            + self.photometric_quality   * 0.15
            + self.novel_view_quality    * 0.15
            + self.semantic_accuracy     * 0.10)
            .clamp(0.0, 1.0)
    }

    /// Novelty factor: larger areas earn proportionally (log-scaled to prevent
    /// trivial farming of tiny captures).
    pub fn area_novelty_factor(&self) -> f64 {
        (1.0 + self.area_m2.max(0.0).ln().max(0.0) / 10.0).min(3.0)
    }

    /// Witness bonus: each independent witness adds marginal trust.
    pub fn witness_bonus(&self) -> f64 {
        (1.0 + self.witness_count as f64 * 0.1).min(2.0)
    }
}

/// Cryptographically committed Gaussian/spatial reconstruction proof.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GaussianProof {
    pub proof_id:          String,
    pub agent_id:          String,
    pub principal_id:      String,
    /// SHA-256 of the raw capture session (images + depth + IMU).
    pub capture_hash:      String,
    /// SHA-256 of the Gaussian splat output (e.g. .ply or .splat blob).
    pub splat_hash:        String,
    /// Walrus blob ID (or IPFS CID) where the actual data lives.
    pub storage_ref:       Option<String>,
    /// Odù tile this capture belongs to (e.g. "odu:a3").
    pub odu_tile:          Option<String>,
    pub quality:           GaussianQualityMetrics,
    pub timestamp:         u64,
    /// ed25519 signature from the capturing agent.
    pub signature:         String,
}

impl GaussianProof {
    /// Proof difficulty scales with area and witness count.
    pub fn difficulty(&self) -> f64 {
        let q = self.quality.aggregate();
        if q < 0.3 { return 0.0; }  // too low quality to count
        q * self.quality.area_novelty_factor() * 5.0
    }

    pub fn quality_score(&self) -> f64 {
        self.quality.aggregate()
    }
}

// ── Reality Transfer Score ────────────────────────────────────────────────────

/// Measures how faithfully a simulation policy transferred to the physical
/// world.  Higher = the agent learned something real, not just sim-optimal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RealityTransferScore {
    pub session_id:            String,
    pub sim_proof_id:          Option<String>,
    pub flight_receipt_id:     String,
    /// Positional accuracy: 1.0 - normalised position error.
    pub position_accuracy:     f64,
    /// Orientation accuracy.
    pub orientation_accuracy:  f64,
    /// Altitude tracking accuracy.
    pub altitude_accuracy:     f64,
    /// Energy prediction accuracy (sim predicted vs real consumed).
    pub energy_accuracy:       f64,
    /// Collision margin match (sim margin vs real margin).
    pub collision_accuracy:    f64,
    /// Mission completion rate (physical / simulated gates).
    pub mission_transfer:      f64,
    /// Composite RTS: geometric mean of all components.
    pub rts:                   f64,
    /// Whether the RTS clears the minimum threshold for Physical proof credit.
    pub physical_proof_eligible: bool,
}

impl RealityTransferScore {
    pub const MIN_RTS_FOR_PROOF: f64 = 0.6;

    pub fn compute(
        session_id: String,
        sim_proof_id: Option<String>,
        flight_receipt_id: String,
        position_accuracy:    f64,
        orientation_accuracy: f64,
        altitude_accuracy:    f64,
        energy_accuracy:      f64,
        collision_accuracy:   f64,
        mission_transfer:     f64,
    ) -> Self {
        let components = [
            position_accuracy,
            orientation_accuracy,
            altitude_accuracy,
            energy_accuracy,
            collision_accuracy,
            mission_transfer,
        ];
        // Geometric mean — one bad component drags the whole score down.
        let product: f64 = components.iter().copied().map(|v| v.clamp(0.001, 1.0)).product();
        let rts = product.powf(1.0 / components.len() as f64);
        let physical_proof_eligible = rts >= Self::MIN_RTS_FOR_PROOF;
        Self {
            session_id,
            sim_proof_id,
            flight_receipt_id,
            position_accuracy,
            orientation_accuracy,
            altitude_accuracy,
            energy_accuracy,
            collision_accuracy,
            mission_transfer,
            rts,
            physical_proof_eligible,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gaussian_aggregate_quality() {
        let m = GaussianQualityMetrics {
            capture_completeness:  0.95,
            pose_quality:          0.05,
            geometric_consistency: 0.90,
            photometric_quality:   0.88,
            novel_view_quality:    0.85,
            semantic_accuracy:     0.80,
            area_m2:               120.0,
            witness_count:         3,
        };
        let q = m.aggregate();
        assert!(q > 0.85, "high-quality scan should score > 0.85, got {q}");
    }

    #[test]
    fn gaussian_poor_quality_low_score() {
        let m = GaussianQualityMetrics {
            capture_completeness:  0.30,
            pose_quality:          0.80,   // bad poses
            geometric_consistency: 0.20,
            photometric_quality:   0.25,
            novel_view_quality:    0.15,
            semantic_accuracy:     0.10,
            area_m2:               5.0,
            witness_count:         0,
        };
        assert!(m.aggregate() < 0.4);
    }

    #[test]
    fn gaussian_proof_difficulty_requires_quality() {
        let make_proof = |q: f64| GaussianProof {
            proof_id:      "p1".into(),
            agent_id:      "a1".into(),
            principal_id:  "did:1".into(),
            capture_hash:  "c1".into(),
            splat_hash:    "s1".into(),
            storage_ref:   None,
            odu_tile:      None,
            quality: GaussianQualityMetrics {
                capture_completeness:  q,
                pose_quality:          1.0 - q,
                geometric_consistency: q,
                photometric_quality:   q,
                novel_view_quality:    q,
                semantic_accuracy:     q,
                area_m2:               50.0,
                witness_count:         1,
            },
            timestamp: 0,
            signature: String::new(),
        };
        let poor   = make_proof(0.1);
        let medium = make_proof(0.6);
        let good   = make_proof(0.9);
        assert_eq!(poor.difficulty(), 0.0, "poor quality gets no difficulty credit");
        assert!(medium.difficulty() > 0.0);
        assert!(good.difficulty()   > medium.difficulty());
    }

    #[test]
    fn rts_perfect_transfer() {
        let rts = RealityTransferScore::compute(
            "s1".into(), Some("sim:1".into()), "rcpt:1".into(),
            0.95, 0.93, 0.97, 0.88, 0.90, 0.95,
        );
        assert!(rts.rts > 0.9);
        assert!(rts.physical_proof_eligible);
    }

    #[test]
    fn rts_poor_position_drags_score() {
        let rts = RealityTransferScore::compute(
            "s2".into(), None, "rcpt:2".into(),
            0.05, 0.90, 0.90, 0.90, 0.90, 0.90,
        );
        assert!(rts.rts < RealityTransferScore::MIN_RTS_FOR_PROOF);
        assert!(!rts.physical_proof_eligible);
    }

    #[test]
    fn rts_borderline_case() {
        // All components exactly at 0.6 → RTS = 0.6 → eligible
        let rts = RealityTransferScore::compute(
            "s3".into(), None, "rcpt:3".into(),
            0.6, 0.6, 0.6, 0.6, 0.6, 0.6,
        );
        assert!((rts.rts - 0.6).abs() < 1e-9);
        assert!(rts.physical_proof_eligible);
    }
}
