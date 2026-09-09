use thiserror::Error;

#[derive(Debug, Error)]
pub enum VcpError {
    #[error("capability is ungrantable: {0}")]
    Ungrantable(String),
    #[error("capability not in manifest: {0}")]
    CapabilityNotFound(String),
    #[error("grant expired")]
    GrantExpired,
    #[error("grant does not cover capability: {0}")]
    GrantMismatch(String),
    #[error("speed limit exceeded: requested {requested:.2} > granted {granted:.2}")]
    SpeedLimitExceeded { requested: f32, granted: f32 },
    #[error("handshake failed at step {step}: {reason}")]
    HandshakeFailed { step: &'static str, reason: String },
    #[error("challenge expired")]
    ChallengeExpired,
    #[error("invalid signature")]
    InvalidSignature,
    #[error("session not found: {0}")]
    SessionNotFound(String),
    #[error("serialization: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("sovereign: {0}")]
    Sovereign(#[from] sovereign_types::SovereignError),
}

pub type VcpResult<T> = Result<T, VcpError>;
