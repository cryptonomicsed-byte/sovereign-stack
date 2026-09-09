use thiserror::Error;

#[derive(Debug, Error)]
pub enum DipError {
    #[error("envelope expired (ttl elapsed)")]
    Expired,
    #[error("duplicate message_id: {0}")]
    Duplicate(String),
    #[error("invalid signature")]
    InvalidSignature,
    #[error("no adapter for network: {0:?}")]
    NoAdapter(crate::address::DipNetwork),
    #[error("identity not found: {0}")]
    IdentityNotFound(String),
    #[error("equivalence verification failed for {network} address {address}")]
    EquivalenceFailed { network: String, address: String },
    #[error("routing failed: {0}")]
    RoutingFailed(String),
    #[error("incomplete identity chain")]
    IncompleteIdentity,
    #[error("serialization: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("sovereign: {0}")]
    Sovereign(#[from] sovereign_types::SovereignError),
}

pub type DipResult<T> = Result<T, DipError>;
