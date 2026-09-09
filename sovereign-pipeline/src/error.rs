use thiserror::Error;

#[derive(Debug, Error)]
pub enum PipelineError {
    #[error("vcp error: {0}")]
    Vcp(#[from] vcp::VcpError),
    #[error("tsp error: {0}")]
    Tsp(#[from] twin_protocol::TspError),
    #[error("sovereign error: {0}")]
    Sovereign(#[from] sovereign_types::SovereignError),
    #[error("serialization: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("capture failed: {0}")]
    CaptureFailed(String),
    #[error("insufficient data for scene assembly: {0}")]
    InsufficientData(String),
}

pub type PipelineResult<T> = Result<T, PipelineError>;
