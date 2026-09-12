use thiserror::Error;

#[derive(Debug, Error)]
pub enum WitnessError {
    #[error("observation bundle not found: {0}")]
    BundleNotFound(String),

    #[error("signature verification failed for device {0}")]
    SignatureInvalid(String),

    #[error("VCP session not active: {0}")]
    SessionNotActive(String),

    #[error("Nostr publish failed: {0}")]
    NostrPublishFailed(String),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}
