use thiserror::Error;

#[derive(Debug, Error)]
pub enum ArpError {
    #[error("receipt hash mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },

    #[error("chain broken: receipt {receipt_id} has no previous_hash but chain is non-empty")]
    ChainBroken { receipt_id: String },

    #[error("signature invalid for receipt {receipt_id}")]
    SignatureInvalid { receipt_id: String },

    #[error("missing required field: {field}")]
    MissingField { field: String },

    #[error("unknown receipt kind: {kind}")]
    UnknownKind { kind: String },

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}
