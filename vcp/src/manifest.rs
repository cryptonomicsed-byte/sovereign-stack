use serde::{Deserialize, Serialize};
use serde_json::Value;
use sovereign_types::{DeviceDid, Hash, Signature, Timestamp, SafetyLevel};
use crate::error::{VcpError, VcpResult};

/// Device-side capability declaration in the Agent Device Manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VcpCapabilityDecl {
    pub id:             String,
    pub description:    String,
    pub params:         Option<Value>,
    pub requires_grant: bool,
    pub safety_level:   SafetyLevel,
    /// If true, this capability can NEVER be granted — hardcoded off.
    pub ungrantable:    bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VcpSafetyConfig {
    pub emergency_stop:      bool,
    pub geofence:            bool,
    pub collision_avoidance: Option<String>,
    pub max_speed_ms:        Option<f32>,
    /// Capabilities that can never be granted (enforced at parse time).
    pub ungrantable:         Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum VcpTransport { Ble, Wifi, Usb, Nfc, Meshtastic, Cellular }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VcpDeviceIdentity {
    pub public_key:  String,   // ed25519 base64url
    pub cert_chain:  Option<Vec<String>>,
}

/// The Agent Device Manifest — broadcast by a VCP-compatible device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDeviceManifest {
    pub device_id:         String,
    pub manufacturer:      String,
    pub model:             String,
    pub protocol_version:  String,     // "vcp/1"
    pub firmware_version:  String,
    pub dip_identity:      DeviceDid,
    pub capabilities:      Vec<VcpCapabilityDecl>,
    pub safety:            VcpSafetyConfig,
    pub transport:         Vec<VcpTransport>,
    pub identity:          VcpDeviceIdentity,
    pub timestamp:         Timestamp,
    pub merkle_root:       Hash,
    pub signature:         Signature,   // device signs manifest
}

impl AgentDeviceManifest {
    /// Validate the manifest: safety rules, ungrantable constraints.
    pub fn validate(&self) -> VcpResult<()> {
        // emergency_stop required on any mobile device
        if !self.safety.emergency_stop {
            return Err(VcpError::HandshakeFailed {
                step: "manifest_validation",
                reason: "emergency_stop must be true".into(),
            });
        }

        // Verify ungrantable capabilities are correctly marked in the decl list
        for cap in &self.capabilities {
            if self.safety.ungrantable.contains(&cap.id) && !cap.ungrantable {
                return Err(VcpError::HandshakeFailed {
                    step: "manifest_validation",
                    reason: format!("capability {} is in ungrantable list but not marked ungrantable", cap.id),
                });
            }
        }

        Ok(())
    }

    /// Find a capability declaration by ID.
    pub fn capability(&self, id: &str) -> Option<&VcpCapabilityDecl> {
        self.capabilities.iter().find(|c| c.id == id)
    }

    /// Check if a capability ID is grantable.
    pub fn is_grantable(&self, id: &str) -> VcpResult<()> {
        let cap = self.capability(id)
            .ok_or_else(|| VcpError::CapabilityNotFound(id.to_string()))?;
        if cap.ungrantable || self.safety.ungrantable.contains(&id.to_string()) {
            return Err(VcpError::Ungrantable(id.to_string()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_manifest() -> AgentDeviceManifest {
        AgentDeviceManifest {
            device_id:        "unitree:go2:test".into(),
            manufacturer:     "Unitree".into(),
            model:            "Go2".into(),
            protocol_version: "vcp/1".into(),
            firmware_version: "1.0.0".into(),
            dip_identity:     "did:device:unitree:go2:test".into(),
            capabilities:     vec![
                VcpCapabilityDecl {
                    id: "locomotion".into(),
                    description: "Walk".into(),
                    params: None,
                    requires_grant: true,
                    safety_level: SafetyLevel::Standard,
                    ungrantable: false,
                },
                VcpCapabilityDecl {
                    id: "firmware.update".into(),
                    description: "Update firmware".into(),
                    params: None,
                    requires_grant: true,
                    safety_level: SafetyLevel::Critical,
                    ungrantable: true,
                },
                VcpCapabilityDecl {
                    id: "emergency_stop".into(),
                    description: "Stop".into(),
                    params: None,
                    requires_grant: false,
                    safety_level: SafetyLevel::None,
                    ungrantable: false,
                },
            ],
            safety: VcpSafetyConfig {
                emergency_stop:      true,
                geofence:            true,
                collision_avoidance: Some("required".into()),
                max_speed_ms:        Some(1.5),
                ungrantable:         vec!["firmware.update".into(), "safety.disable".into()],
            },
            transport:    vec![VcpTransport::Wifi, VcpTransport::Ble],
            identity:     VcpDeviceIdentity { public_key: "base64url:test".into(), cert_chain: None },
            timestamp:    0,
            merkle_root:  String::new(),
            signature:    String::new(),
        }
    }

    #[test]
    fn valid_manifest_passes() {
        assert!(test_manifest().validate().is_ok());
    }

    #[test]
    fn ungrantable_blocked() {
        let m = test_manifest();
        assert!(m.is_grantable("firmware.update").is_err());
        assert!(m.is_grantable("locomotion").is_ok());
    }

    #[test]
    fn missing_emergency_stop_rejected() {
        let mut m = test_manifest();
        m.safety.emergency_stop = false;
        assert!(m.validate().is_err());
    }
}
