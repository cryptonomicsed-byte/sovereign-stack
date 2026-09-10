use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PowerSource {
    Mains,
    Battery,
    Solar,
    Poe,            // Power-over-Ethernet
    Usb,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum PowerState {
    /// Full compute — all sensors, GPU, networking active
    FullPower,
    /// Normal operation — sensors active, GPU on demand
    Normal,
    /// Conservation — background tasks paused, CPU throttled
    Conservation,
    /// Low battery — critical functions only
    LowBattery,
    /// Suspend — waiting for wake event
    Suspend,
    /// Hibernation — state persisted to disk, minimal draw
    Hibernate,
}

impl PowerState {
    pub fn allows_gpu(&self)     -> bool { matches!(self, Self::FullPower | Self::Normal) }
    pub fn allows_camera(&self)  -> bool { !matches!(self, Self::Suspend | Self::Hibernate) }
    pub fn allows_sensors(&self) -> bool { !matches!(self, Self::Hibernate) }
    pub fn allows_network(&self) -> bool { !matches!(self, Self::Hibernate) }
}

/// Observed power snapshot from the platform.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PowerSnapshot {
    pub state:            PowerState,
    pub source:           PowerSource,
    pub battery_pct:      Option<u8>,     // 0–100
    pub charge_rate_mw:   Option<i32>,    // positive = charging, negative = discharging
    pub temperature_c:    Option<f32>,
}

impl PowerSnapshot {
    pub fn is_critical(&self) -> bool {
        matches!(self.state, PowerState::LowBattery)
        || self.battery_pct.map(|p| p < 10).unwrap_or(false)
    }
}

/// Policy table — maps PowerState to per-subsystem permission.
pub struct PowerPolicy;

impl PowerPolicy {
    pub fn allows_capture(snap: &PowerSnapshot) -> bool {
        snap.state.allows_camera() && snap.state.allows_sensors()
    }

    pub fn allows_gaussian_splat(snap: &PowerSnapshot) -> bool {
        snap.state.allows_gpu() && !snap.is_critical()
    }

    pub fn allows_swarm_broadcast(snap: &PowerSnapshot) -> bool {
        snap.state.allows_network()
    }

    pub fn allows_embodiment(snap: &PowerSnapshot) -> bool {
        // Never move if battery is critically low
        !snap.is_critical() && snap.state.allows_sensors()
    }
}
