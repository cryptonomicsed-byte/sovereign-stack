use serde::{Deserialize, Serialize};

/// Geographic bounding box for a Twin Asset's physical region.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TwinRegion {
    pub min_lat:   f64,
    pub min_lon:   f64,
    pub max_lat:   f64,
    pub max_lon:   f64,
    pub min_alt_m: Option<f32>,
    pub max_alt_m: Option<f32>,
    pub label:     Option<String>,
}

impl TwinRegion {
    pub fn new(min_lat: f64, min_lon: f64, max_lat: f64, max_lon: f64) -> Self {
        Self { min_lat, min_lon, max_lat, max_lon, min_alt_m: None, max_alt_m: None, label: None }
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn area_sq_deg(&self) -> f64 {
        (self.max_lat - self.min_lat).abs() * (self.max_lon - self.min_lon).abs()
    }
}
