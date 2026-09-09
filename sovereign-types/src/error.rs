use thiserror::Error;

#[derive(Debug, Error)]
pub enum SovereignError {
    #[error("identity chain incomplete: missing {0}")]
    IncompleteIdentity(&'static str),

    #[error("signature verification failed")]
    InvalidSignature,

    #[error("merkle root mismatch: expected {expected}, got {got}")]
    MerkleRootMismatch { expected: String, got: String },

    #[error("quality gate failed: f1_score {score:.3} < 0.777")]
    QualityGateFailed { score: f32 },

    #[error("capability is ungrantable: {0}")]
    Ungrantable(String),

    #[error("grant expired")]
    GrantExpired,

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("crypto error: {0}")]
    Crypto(String),
}

pub type SovereignResult<T> = Result<T, SovereignError>;
