use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A bundle of sensor readings captured during a physical execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservationBundle {
    pub bundle_id:   String,
    pub device_id:   String,
    pub session_id:  String,
    pub readings:    Vec<SensorReading>,
    pub duration_ms: u64,
    pub hash:        String,
    pub captured_at: DateTime<Utc>,
}

impl ObservationBundle {
    pub fn compute_hash(&self) -> String {
        let data: String = self.readings.iter()
            .map(|r| format!("{}:{}:{}", r.channel, r.value, r.timestamp_ms))
            .collect::<Vec<_>>()
            .join("|");
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut h = DefaultHasher::new();
        data.hash(&mut h);
        format!("{:016x}", h.finish())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorReading {
    pub channel:      String,
    pub value:        f64,
    pub unit:         String,
    pub timestamp_ms: u64,
    pub quality:      f32,
}

/// A single high-level physical observation (summary of a bundle).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhysicalObservation {
    pub obs_id:      String,
    pub bundle_id:   String,
    pub agent_id:    String,
    pub description: String,
    pub outcome:     String,
    pub confidence:  f32,
    pub timestamp:   DateTime<Utc>,
}
