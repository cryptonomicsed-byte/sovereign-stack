//! VCP adapter for Unitree Go2 robot dog.
//!
//! The Go2 exposes a WebSocket API over WiFi (192.168.123.161:8080 default).
//! This adapter translates VCP commands → Go2 SDK JSON payloads and
//! Go2 sensor data → VCP telemetry structs.
//!
//! Capabilities supported:
//!   locomotion   — walk/stand/sit/turn/velocity control (max 1.5 m/s)
//!   camera       — RGB frame capture (Go2's front RGB camera)
//!   lidar        — point cloud snapshot (Hesai Pandar XT16 or built-in L1)
//!   telemetry    — battery/IMU/joint-state poll
//!   sportmode    — switch between Normal/AI/Advanced motion mode
//!
//! Ungrantable (hardcoded off via manifest):
//!   firmware.update, safety.disable

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};

use sovereign_types::{SafetyLevel, Timestamp};
use crate::manifest::{AgentDeviceManifest, VcpCapabilityDecl, VcpSafetyConfig, VcpTransport, VcpDeviceIdentity};
use crate::command::{VcpCommand, VcpCommandResponse, VcpCommandStatus};
use crate::error::{VcpError, VcpResult};

/// How the Go2 is reached.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Go2ConnectionMode {
    /// Direct WiFi — Go2 hosts its own AP at 192.168.123.x
    DirectWifi { host: String, port: u16 },
    /// Infrastructure WiFi — Go2 joined a shared network
    NetworkWifi { host: String, port: u16 },
    /// USB-UART passthrough (dev mode)
    Usb { serial_port: String },
}

impl Default for Go2ConnectionMode {
    fn default() -> Self {
        Go2ConnectionMode::DirectWifi {
            host: "192.168.123.161".into(),
            port: 8080,
        }
    }
}

/// Snapshot of Go2 sensor + state data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Go2Telemetry {
    pub battery_pct:   f32,    // 0–100
    pub charging:      bool,
    pub gait_type:     String, // "walk" | "trot" | "stand" | "sit"
    pub speed_ms:      f32,    // current forward velocity m/s
    pub yaw_deg:       f32,    // heading degrees from north
    pub imu_roll:      f32,
    pub imu_pitch:     f32,
    pub imu_yaw:       f32,
    pub joint_temps:   Vec<f32>, // 12 joints
    pub wifi_rssi_dbm: Option<i16>,
    pub timestamp_ms:  Timestamp,
}

impl Go2Telemetry {
    /// Parse from the Go2 WebSocket state message.
    pub fn from_go2_json(v: &Value) -> Option<Self> {
        let battery = v["sportModeState"]["batteryLevel"].as_f64()? as f32;
        let gait    = v["sportModeState"]["gaitType"].as_str().unwrap_or("unknown");
        let vx      = v["sportModeState"]["vx"].as_f64().unwrap_or(0.0) as f32;
        Some(Self {
            battery_pct:   battery * 100.0,
            charging:      v["sportModeState"]["charging"].as_bool().unwrap_or(false),
            gait_type:     gait.into(),
            speed_ms:      vx.abs(),
            yaw_deg:       v["sportModeState"]["yawSpeed"].as_f64().unwrap_or(0.0) as f32,
            imu_roll:      v["imuState"]["rpy"][0].as_f64().unwrap_or(0.0) as f32,
            imu_pitch:     v["imuState"]["rpy"][1].as_f64().unwrap_or(0.0) as f32,
            imu_yaw:       v["imuState"]["rpy"][2].as_f64().unwrap_or(0.0) as f32,
            joint_temps:   (0..12)
                .map(|i| v["motorState"][i]["temperature"].as_f64().unwrap_or(0.0) as f32)
                .collect(),
            wifi_rssi_dbm: v["wifiState"]["rssi"].as_i64().map(|r| r as i16),
            timestamp_ms:  now_ms(),
        })
    }

    /// Convert to VCP telemetry payload for a VcpCommandResponse.
    pub fn to_vcp_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

/// High-level Go2 actions (each maps to a Go2 SDK command).
#[derive(Debug, Clone, PartialEq)]
pub enum Go2Action {
    /// Walk forward/backward/strafe: vx, vy, vyaw in m/s
    Move { vx: f32, vy: f32, vyaw: f32 },
    StandUp,
    SitDown,
    Damp,           // lowest-power idle (joints go limp)
    RecoveryStand,
    /// Capture one RGB frame from front camera
    CaptureFrame,
    /// Snapshot lidar point cloud
    CaptureLidar,
    PollTelemetry,
    /// Switch motion mode: "normal" | "ai" | "advanced"
    SetSportMode(String),
}

/// Translates VCP commands to Go2 API payloads and vice-versa.
pub struct Go2Adapter {
    pub device_id: String,
    pub connection: Go2ConnectionMode,
}

impl Go2Adapter {
    pub fn new(device_id: impl Into<String>, connection: Go2ConnectionMode) -> Self {
        Self { device_id: device_id.into(), connection }
    }

    /// Build the canonical Go2 AgentDeviceManifest for VCP registration.
    pub fn manifest(&self, public_key: impl Into<String>) -> AgentDeviceManifest {
        AgentDeviceManifest {
            device_id:        self.device_id.clone(),
            manufacturer:     "Unitree".into(),
            model:            "Go2".into(),
            protocol_version: "vcp/1".into(),
            firmware_version: "1.0.0".into(),
            dip_identity:     format!("did:device:{}", self.device_id),
            capabilities: vec![
                VcpCapabilityDecl {
                    id: "locomotion".into(),
                    description: "Velocity-control walking. Params: vx, vy, vyaw (m/s), duration_ms.".into(),
                    params: Some(json!({
                        "vx":          {"type": "number", "min": -1.5, "max": 1.5},
                        "vy":          {"type": "number", "min": -0.5, "max": 0.5},
                        "vyaw":        {"type": "number", "min": -1.0, "max": 1.0},
                        "duration_ms": {"type": "integer", "max": 10000}
                    })),
                    requires_grant: true,
                    safety_level: SafetyLevel::Standard,
                    ungrantable: false,
                },
                VcpCapabilityDecl {
                    id: "camera".into(),
                    description: "Capture a single RGB frame from the front camera.".into(),
                    params: None,
                    requires_grant: true,
                    safety_level: SafetyLevel::Low,
                    ungrantable: false,
                },
                VcpCapabilityDecl {
                    id: "lidar".into(),
                    description: "Snapshot lidar point cloud (L1 or Pandar XT16).".into(),
                    params: None,
                    requires_grant: true,
                    safety_level: SafetyLevel::Low,
                    ungrantable: false,
                },
                VcpCapabilityDecl {
                    id: "telemetry".into(),
                    description: "Poll battery, IMU, gait, joint temps.".into(),
                    params: None,
                    requires_grant: false,
                    safety_level: SafetyLevel::None,
                    ungrantable: false,
                },
                VcpCapabilityDecl {
                    id: "sportmode".into(),
                    description: "Switch motion mode: normal | ai | advanced.".into(),
                    params: Some(json!({"mode": {"type": "string", "enum": ["normal", "ai", "advanced"]}})),
                    requires_grant: true,
                    safety_level: SafetyLevel::Standard,
                    ungrantable: false,
                },
                VcpCapabilityDecl {
                    id: "emergency_stop".into(),
                    description: "Immediate damp — all joints disengage.".into(),
                    params: None,
                    requires_grant: false,
                    safety_level: SafetyLevel::None,
                    ungrantable: false,
                },
                VcpCapabilityDecl {
                    id: "firmware.update".into(),
                    description: "Update firmware (UNGRANTABLE — requires physical admin access).".into(),
                    params: None,
                    requires_grant: true,
                    safety_level: SafetyLevel::Critical,
                    ungrantable: true,
                },
                VcpCapabilityDecl {
                    id: "safety.disable".into(),
                    description: "Disable collision avoidance (UNGRANTABLE).".into(),
                    params: None,
                    requires_grant: true,
                    safety_level: SafetyLevel::Critical,
                    ungrantable: true,
                },
            ],
            safety: VcpSafetyConfig {
                emergency_stop:      true,
                geofence:            true,
                collision_avoidance: Some("unitree_obstacle_avoid".into()),
                max_speed_ms:        Some(1.5),
                ungrantable:         vec!["firmware.update".into(), "safety.disable".into()],
            },
            transport: vec![VcpTransport::Wifi, VcpTransport::Ble, VcpTransport::Usb],
            identity:  VcpDeviceIdentity { public_key: public_key.into(), cert_chain: None },
            timestamp:  now_ms(),
            merkle_root: String::new(),
            signature:   String::new(),
        }
    }

    /// Translate a VcpCommand into Go2 SDK JSON payload.
    pub fn command_to_go2(&self, cmd: &VcpCommand) -> VcpResult<Value> {
        let action = self.parse_action(cmd)?;
        Ok(self.action_to_go2_payload(action))
    }

    fn parse_action(&self, cmd: &VcpCommand) -> VcpResult<Go2Action> {
        match cmd.capability.as_str() {
            "locomotion" => {
                let vx   = cmd.params["vx"].as_f64().unwrap_or(0.0) as f32;
                let vy   = cmd.params["vy"].as_f64().unwrap_or(0.0) as f32;
                let vyaw = cmd.params["vyaw"].as_f64().unwrap_or(0.0) as f32;
                let action_str = cmd.action.as_str();
                match action_str {
                    "stand"    => Ok(Go2Action::StandUp),
                    "sit"      => Ok(Go2Action::SitDown),
                    "damp"     => Ok(Go2Action::Damp),
                    "recover"  => Ok(Go2Action::RecoveryStand),
                    _          => Ok(Go2Action::Move { vx, vy, vyaw }),
                }
            }
            "camera"         => Ok(Go2Action::CaptureFrame),
            "lidar"          => Ok(Go2Action::CaptureLidar),
            "telemetry"      => Ok(Go2Action::PollTelemetry),
            "emergency_stop" => Ok(Go2Action::Damp),
            "sportmode"      => {
                let mode = cmd.params["mode"].as_str().unwrap_or("normal").to_string();
                Ok(Go2Action::SetSportMode(mode))
            }
            other => Err(VcpError::CapabilityNotFound(other.into())),
        }
    }

    fn action_to_go2_payload(&self, action: Go2Action) -> Value {
        match action {
            Go2Action::Move { vx, vy, vyaw } => json!({
                "header": {"identity": {"id": 0}, "policy": {"header": {"identity": {"id": 0}}}},
                "parameter": format!("{{\"x\": {vx:.3}, \"y\": {vy:.3}, \"z\": {vyaw:.3}}}"),
                "api_id": 1008  // Move
            }),
            Go2Action::StandUp => json!({"api_id": 1004}),
            Go2Action::SitDown => json!({"api_id": 1005}),
            Go2Action::Damp    => json!({"api_id": 1001}),
            Go2Action::RecoveryStand => json!({"api_id": 1006}),
            Go2Action::CaptureFrame  => json!({"api_id": 3001, "source": "front_rgb"}),
            Go2Action::CaptureLidar  => json!({"api_id": 3002, "source": "lidar_l1"}),
            Go2Action::PollTelemetry => json!({"api_id": 2001}),
            Go2Action::SetSportMode(mode) => {
                let mode_id: u8 = match mode.as_str() {
                    "ai"       => 1,
                    "advanced" => 2,
                    _          => 0,  // normal
                };
                json!({"api_id": 1003, "parameter": mode_id.to_string()})
            }
        }
    }

    /// Parse a Go2 WebSocket response into a VcpCommandResponse.
    pub fn go2_response_to_vcp(
        &self,
        cmd_id:     &str,
        session_id: &str,
        go2_resp:   &Value,
        signing_key: &str,
    ) -> VcpResult<VcpCommandResponse> {
        let code = go2_resp["code"].as_i64().unwrap_or(-1);
        let status = if code == 0 {
            VcpCommandStatus::Completed
        } else {
            VcpCommandStatus::Failed
        };
        let telemetry = if go2_resp.get("sportModeState").is_some() {
            Go2Telemetry::from_go2_json(go2_resp).map(|t| t.to_vcp_value())
        } else {
            None
        };
        let sig = sovereign_types::crypto::sign(
            &format!("{}:{}", cmd_id, now_ms()),
            signing_key,
        )?;
        Ok(VcpCommandResponse {
            cmd_id:    cmd_id.into(),
            session_id: session_id.into(),
            device_id:  self.device_id.clone(),
            status,
            telemetry,
            error: if code != 0 { go2_resp["message"].as_str().map(String::from) } else { None },
            timestamp: now_ms(),
            signature: sig,
        })
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adapter() -> Go2Adapter {
        Go2Adapter::new("unitree:go2:unit-test", Go2ConnectionMode::default())
    }

    #[test]
    fn manifest_has_required_capabilities() {
        let m = adapter().manifest("base64url:fakepubkey");
        assert!(m.validate().is_ok());
        assert!(m.is_grantable("locomotion").is_ok());
        assert!(m.is_grantable("camera").is_ok());
        assert!(m.is_grantable("firmware.update").is_err()); // ungrantable
        assert!(m.is_grantable("safety.disable").is_err());  // ungrantable
    }

    #[test]
    fn manifest_emergency_stop_present() {
        let m = adapter().manifest("key");
        assert!(m.capability("emergency_stop").is_some());
        assert!(m.safety.emergency_stop);
    }

    #[test]
    fn move_command_produces_correct_api_id() {
        let a = adapter();
        let payload = a.action_to_go2_payload(Go2Action::Move { vx: 0.5, vy: 0.0, vyaw: 0.1 });
        assert_eq!(payload["api_id"], 1008);
    }

    #[test]
    fn stand_sit_damp_api_ids() {
        let a = adapter();
        assert_eq!(a.action_to_go2_payload(Go2Action::StandUp)["api_id"], 1004);
        assert_eq!(a.action_to_go2_payload(Go2Action::SitDown)["api_id"], 1005);
        assert_eq!(a.action_to_go2_payload(Go2Action::Damp)["api_id"],    1001);
    }

    #[test]
    fn telemetry_from_go2_json() {
        let raw = serde_json::json!({
            "sportModeState": {
                "batteryLevel": 0.72,
                "charging": false,
                "gaitType": "trot",
                "vx": -0.3,
                "yawSpeed": 5.2
            },
            "imuState": {"rpy": [0.01, -0.02, 1.57]},
            "motorState": [
                {"temperature": 38.5}, {"temperature": 37.2}, {"temperature": 39.1},
                {"temperature": 38.0}, {"temperature": 37.5}, {"temperature": 38.8},
                {"temperature": 39.0}, {"temperature": 37.0}, {"temperature": 38.2},
                {"temperature": 37.8}, {"temperature": 38.5}, {"temperature": 37.9}
            ]
        });
        let t = Go2Telemetry::from_go2_json(&raw).expect("parse failed");
        assert!((t.battery_pct - 72.0).abs() < 0.1);
        assert_eq!(t.gait_type, "trot");
        assert!((t.speed_ms - 0.3).abs() < 0.01);
        assert_eq!(t.joint_temps.len(), 12);
        assert!(!t.charging);
    }

    #[test]
    fn sport_mode_encoding() {
        let a = adapter();
        let payload = a.action_to_go2_payload(Go2Action::SetSportMode("ai".into()));
        assert_eq!(payload["api_id"], 1003);
        assert_eq!(payload["parameter"], "1");
    }
}
