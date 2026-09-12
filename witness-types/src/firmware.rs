use serde::{Deserialize, Serialize};

/// Manifest for Witness firmware running on M5Stack-class hardware.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirmwareManifest {
    pub firmware_id:  String,
    pub kind:         FirmwareKind,
    pub version:      String,
    pub device_class: String,
    pub capabilities: Vec<String>,
    pub vcp_enabled:  bool,
    pub nostr_pubkey: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FirmwareKind {
    /// M5Stack-class companion (Level 2 hardware)
    Companion,
    /// Agent Tag (Level 1 — secure element)
    AgentTag,
    /// Gateway terminal (Level 3)
    Gateway,
    /// Ground robot controller
    RobotController,
    /// Drone autopilot bridge
    DroneAutopilot,
}
