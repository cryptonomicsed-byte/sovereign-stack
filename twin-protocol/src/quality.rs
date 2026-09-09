use serde::{Deserialize, Serialize};
use crate::error::{TspError, TspResult};

pub const F1_GATE: f32 = 0.777;

/// Quality metrics for a Twin Asset or capture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TwinQuality {
    /// F1 score over reconstruction quality. MUST be >= F1_GATE (0.777).
    pub f1_score:              f32,
    pub coverage_pct:          f32,
    pub reconstruction_engine: String,
    pub frame_count:           u32,
    pub gaussian_count:        Option<u32>,
    pub validated:             bool,
}

impl TwinQuality {
    pub fn new(f1_score: f32, coverage_pct: f32, engine: impl Into<String>) -> TspResult<Self> {
        if f1_score < F1_GATE {
            return Err(TspError::QualityGateFailed { score: f1_score });
        }
        Ok(Self {
            f1_score,
            coverage_pct,
            reconstruction_engine: engine.into(),
            frame_count:    0,
            gaussian_count: None,
            validated:      false,
        })
    }

    pub fn with_frames(mut self, frame_count: u32) -> Self {
        self.frame_count = frame_count;
        self
    }

    pub fn with_gaussians(mut self, count: u32) -> Self {
        self.gaussian_count = Some(count);
        self
    }

    /// Re-validate the F1 gate (use when deserializing untrusted data).
    pub fn assert_valid(&self) -> TspResult<()> {
        if self.f1_score < F1_GATE {
            return Err(TspError::QualityGateFailed { score: self.f1_score });
        }
        Ok(())
    }
}
