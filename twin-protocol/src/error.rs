use thiserror::Error;

#[derive(Debug, Error)]
pub enum TspError {
    #[error("quality gate failed: f1_score {score:.3} < 0.777")]
    QualityGateFailed { score: f32 },
    #[error("twin merkle root mismatch")]
    MerkleRootMismatch,
    #[error("missing required hash: {0}")]
    MissingHash(&'static str),
    #[error("simulation must have >= 2 candidate policies, got {0}")]
    InsufficientPolicies(usize),
    #[error("selected policy {0} not found in policy list")]
    PolicyNotFound(String),
    #[error("witness count insufficient: need >= 2, got {0}")]
    InsufficientWitnesses(usize),
    #[error("hardware signature required for observation (software signature rejected)")]
    SoftwareSignatureRejected,
    #[error("duplicate twin: twin_id {0} already exists")]
    DuplicateTwin(String),
    #[error("license violation: agent does not hold simulate rights for twin {0}")]
    LicenseViolation(String),
    #[error("validation: {0}")]
    Validation(String),
    #[error("simulation engine error: {0}")]
    SimulationError(String),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("sovereign: {0}")]
    Sovereign(#[from] sovereign_types::SovereignError),
}

pub type TspResult<T> = Result<T, TspError>;
