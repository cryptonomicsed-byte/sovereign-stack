use serde::{Deserialize, Serialize};
use crate::device::HardwareTier;
use crate::power::PowerSnapshot;
use crate::safety::{SafetyCommand, SafetyVerdict};

/// A single sensor connected to this embodiment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorDescriptor {
    pub sensor_id:  String,
    pub kind:       SensorKind,
    pub model:      String,
    pub active:     bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SensorKind {
    Camera,
    Lidar,
    Imu,
    Microphone,
    Gps,
    Temperature,
    Pressure,
    Midi,
    Gpio,
    Ultrasonic,
    Tactile,
}

/// The physical embodiment of a sovereign agent on a specific hardware tier.
/// This is the machine-facing half of the Sovereign Runtime.
/// Omo-Koda interacts with the agent via intents; Embodiment Manager
/// translates those intents into physical actions that pass through the safety hierarchy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbodimentDescriptor {
    pub embodiment_id:  String,
    pub device_did:     String,
    pub tier:           HardwareTier,
    pub model:          String,
    pub sensors:        Vec<SensorDescriptor>,
    pub has_actuators:  bool,
    pub has_gpu:        bool,
    pub has_npu:        bool,
}

impl EmbodimentDescriptor {
    pub fn spatial_capable(&self) -> bool {
        self.has_gpu || self.has_npu
    }

    pub fn sensor_by_kind(&self, kind: &SensorKind) -> Option<&SensorDescriptor> {
        self.sensors.iter().find(|s| &s.kind == kind && s.active)
    }
}

/// The Embodiment Manager trait — the interface between Sovereign Runtime and the platform.
/// Each hardware target (Linux, RTOS, MCU) provides its own implementation.
pub trait EmbodimentManager: Send + Sync {
    /// Current embodiment descriptor (sensors, tier, capabilities).
    fn descriptor(&self) -> &EmbodimentDescriptor;

    /// Current power state from the platform.
    fn power_snapshot(&self) -> PowerSnapshot;

    /// Submit a physical command to the actuator layer.
    /// The implementation MUST check SafetyVerdict before executing.
    fn actuate(&self, cmd: SafetyCommand, verdict: SafetyVerdict) -> Result<(), EmbodimentError>;

    /// Read the current value from a sensor.
    fn read_sensor(&self, sensor_id: &str) -> Result<serde_json::Value, EmbodimentError>;

    /// True if the embodiment is currently in motion.
    fn is_in_motion(&self) -> bool;
}

/// Stub embodiment for dev/test — never executes physical commands.
pub struct StubEmbodiment {
    pub descriptor: EmbodimentDescriptor,
}

impl StubEmbodiment {
    pub fn portable(device_did: impl Into<String>) -> Self {
        Self {
            descriptor: EmbodimentDescriptor {
                embodiment_id: format!("emb:{}", uuid::Uuid::new_v4()),
                device_did:    device_did.into(),
                tier:          HardwareTier::Portable,
                model:         "stub-portable".into(),
                sensors:       vec![
                    SensorDescriptor {
                        sensor_id: "cam:0".into(),
                        kind:      SensorKind::Camera,
                        model:     "stub-cam".into(),
                        active:    true,
                    },
                ],
                has_actuators: false,
                has_gpu:       false,
                has_npu:       false,
            },
        }
    }
}

impl EmbodimentManager for StubEmbodiment {
    fn descriptor(&self) -> &EmbodimentDescriptor { &self.descriptor }

    fn power_snapshot(&self) -> PowerSnapshot {
        PowerSnapshot {
            state:          crate::power::PowerState::Normal,
            source:         crate::power::PowerSource::Mains,
            battery_pct:    Some(100),
            charge_rate_mw: Some(0),
            temperature_c:  Some(35.0),
        }
    }

    fn actuate(&self, _cmd: SafetyCommand, verdict: SafetyVerdict) -> Result<(), EmbodimentError> {
        match verdict {
            SafetyVerdict::Clear | SafetyVerdict::Conditional { .. } => {
                // stub: log but don't execute
                Ok(())
            }
            SafetyVerdict::Blocked { reason } | SafetyVerdict::Halt { reason } => {
                Err(EmbodimentError::CommandBlocked(reason))
            }
        }
    }

    fn read_sensor(&self, sensor_id: &str) -> Result<serde_json::Value, EmbodimentError> {
        if self.descriptor.sensors.iter().any(|s| s.sensor_id == sensor_id) {
            Ok(serde_json::json!({ "stub": true, "sensor_id": sensor_id }))
        } else {
            Err(EmbodimentError::SensorNotFound(sensor_id.into()))
        }
    }

    fn is_in_motion(&self) -> bool { false }
}

#[derive(Debug, thiserror::Error)]
pub enum EmbodimentError {
    #[error("sensor not found: {0}")]
    SensorNotFound(String),
    #[error("command blocked by safety supervisor: {0}")]
    CommandBlocked(String),
    #[error("actuator error: {0}")]
    ActuatorError(String),
    #[error("power insufficient: {0}")]
    InsufficientPower(String),
}
