use thiserror::Error;

#[derive(Debug, Error)]
pub enum SwarmError {
    #[error("twin not found: {0}")]
    TwinNotFound(String),

    #[error("no feasible policy found after {0} trajectories")]
    NoFeasiblePolicy(u32),

    #[error("VCP session required but not available")]
    VcpSessionRequired,

    #[error("simulation engine error: {0}")]
    EngineError(String),

    #[error("proof verification failed: {0}")]
    ProofInvalid(String),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}
