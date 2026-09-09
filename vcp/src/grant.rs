use serde::{Deserialize, Serialize};
use serde_json::Value;
use sovereign_types::{IdentityChain, Hash, Signature, Timestamp, merkle_root, sign};
use std::collections::BTreeMap;
use crate::error::{VcpError, VcpResult};
use crate::manifest::AgentDeviceManifest;
use crate::handshake::{VcpCapabilityRequest, VcpDuration};

/// A granted capability with its enforced limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VcpGrantedCapability {
    pub id:     String,
    pub limits: Value,
}

/// A denied capability with the reason.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VcpDeniedCapability {
    pub id:     String,
    pub reason: VcpDenyReason,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VcpDenyReason {
    Ungrantable,
    InsufficientAuth,
    UnsupportedLimit,
    Unavailable,
}

/// A signed, scoped, expiring capability grant — the core VCP authorization token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VcpCapabilityGrant {
    pub grant_id:    String,
    pub request_id:  String,
    pub session_id:  String,
    pub identity:    IdentityChain,
    pub device_id:   String,
    pub capabilities: Vec<VcpGrantedCapability>,
    pub denied:       Vec<VcpDeniedCapability>,
    pub issued_at:   Timestamp,
    pub expires_at:  Timestamp,
    pub revocable:   bool,  // always true for agent grants
    pub merkle_root: Hash,
    pub signature:   Signature,  // device signs grant
}

impl VcpCapabilityGrant {
    /// Issue a grant from a device manifest against a capability request.
    pub fn issue(
        manifest:    &AgentDeviceManifest,
        request:     &VcpCapabilityRequest,
        device_private_key: &str,
    ) -> VcpResult<Self> {
        let grant_id   = format!("grant:{}", uuid::Uuid::new_v4());
        let session_id = format!("sess:{}", uuid::Uuid::new_v4());
        let issued_at  = now_ms();

        // Determine grant duration (use preferred, cap at max, enforce 24hr ceiling)
        let duration_min = request.duration.preferred_min.min(request.duration.max_min).min(1440);
        let expires_at   = issued_at + (duration_min as u64 * 60_000);

        let mut capabilities = vec![];
        let mut denied = vec![];

        for cap_id in &request.capabilities {
            match manifest.is_grantable(cap_id) {
                Err(VcpError::Ungrantable(_)) => {
                    denied.push(VcpDeniedCapability {
                        id: cap_id.clone(),
                        reason: VcpDenyReason::Ungrantable,
                    });
                }
                Err(VcpError::CapabilityNotFound(_)) => {
                    denied.push(VcpDeniedCapability {
                        id: cap_id.clone(),
                        reason: VcpDenyReason::Unavailable,
                    });
                }
                Ok(_) => {
                    let limits = default_limits(cap_id, manifest);
                    capabilities.push(VcpGrantedCapability { id: cap_id.clone(), limits });
                }
                Err(_) => {
                    denied.push(VcpDeniedCapability {
                        id: cap_id.clone(),
                        reason: VcpDenyReason::Unavailable,
                    });
                }
            }
        }

        let mut fields = BTreeMap::new();
        fields.insert("capabilities", serde_json::to_value(&capabilities)?);
        fields.insert("device_id",    Value::String(manifest.device_id.clone()));
        fields.insert("expires_at",   Value::Number(expires_at.into()));
        fields.insert("grant_id",     Value::String(grant_id.clone()));
        fields.insert("issued_at",    Value::Number(issued_at.into()));
        fields.insert("session_id",   Value::String(session_id.clone()));

        let root = merkle_root(&fields);
        let sig  = sign(&root, device_private_key)?;

        Ok(Self {
            grant_id,
            request_id:   request.request_id.clone(),
            session_id,
            identity:     request.identity.clone(),
            device_id:    manifest.device_id.clone(),
            capabilities,
            denied,
            issued_at,
            expires_at,
            revocable:    true,
            merkle_root:  root,
            signature:    sig,
        })
    }

    pub fn is_expired(&self) -> bool {
        now_ms() > self.expires_at
    }

    /// Check if this grant covers a specific capability.
    pub fn covers(&self, capability_id: &str) -> VcpResult<&VcpGrantedCapability> {
        if self.is_expired() {
            return Err(VcpError::GrantExpired);
        }
        self.capabilities.iter()
            .find(|c| c.id == capability_id)
            .ok_or_else(|| VcpError::GrantMismatch(capability_id.to_string()))
    }
}

fn default_limits(cap_id: &str, manifest: &AgentDeviceManifest) -> Value {
    match cap_id {
        "locomotion" => serde_json::json!({
            "max_speed_ms": manifest.safety.max_speed_ms.unwrap_or(1.5),
            "geofence_enabled": manifest.safety.geofence,
            "collision_avoidance": "required"
        }),
        "camera.read" => serde_json::json!({
            "max_resolution": "720p",
            "max_fps": 15
        }),
        "telemetry.read" => serde_json::json!({
            "max_hz": 10
        }),
        _ => serde_json::json!({}),
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use sovereign_types::crypto::generate_keypair;
    use crate::manifest::{VcpCapabilityDecl, VcpSafetyConfig, VcpTransport, VcpDeviceIdentity};
    use sovereign_types::SafetyLevel;

    fn make_manifest(device_priv: &str) -> AgentDeviceManifest {
        AgentDeviceManifest {
            device_id:        "test:device:01".into(),
            manufacturer:     "Test".into(),
            model:            "T1".into(),
            protocol_version: "vcp/1".into(),
            firmware_version: "1.0".into(),
            dip_identity:     "did:device:test:01".into(),
            capabilities:     vec![
                VcpCapabilityDecl {
                    id: "locomotion".into(), description: "walk".into(),
                    params: None, requires_grant: true,
                    safety_level: SafetyLevel::Standard, ungrantable: false,
                },
                VcpCapabilityDecl {
                    id: "firmware.update".into(), description: "fw".into(),
                    params: None, requires_grant: true,
                    safety_level: SafetyLevel::Critical, ungrantable: true,
                },
                VcpCapabilityDecl {
                    id: "emergency_stop".into(), description: "stop".into(),
                    params: None, requires_grant: false,
                    safety_level: SafetyLevel::None, ungrantable: false,
                },
            ],
            safety: VcpSafetyConfig {
                emergency_stop: true, geofence: true,
                collision_avoidance: Some("required".into()),
                max_speed_ms: Some(1.5),
                ungrantable: vec!["firmware.update".into()],
            },
            transport:   vec![VcpTransport::Wifi],
            identity:    VcpDeviceIdentity { public_key: "base64url:test".into(), cert_chain: None },
            timestamp:   0, merkle_root: String::new(), signature: String::new(),
        }
    }

    #[test]
    fn grant_locomotion_deny_firmware() {
        let (device_priv, _) = generate_keypair();
        let (agent_priv, _)  = generate_keypair();
        let manifest  = make_manifest(&device_priv);
        let identity  = IdentityChain::new("did:p:1".into(), "did:a:1".into());
        let request   = VcpCapabilityRequest::new(
            identity,
            vec!["locomotion".into(), "firmware.update".into()],
            "test scan",
            VcpDuration::minutes(15),
            &agent_priv,
        ).unwrap();

        let grant = VcpCapabilityGrant::issue(&manifest, &request, &device_priv).unwrap();

        assert_eq!(grant.capabilities.len(), 1);
        assert_eq!(grant.capabilities[0].id, "locomotion");
        assert_eq!(grant.denied.len(), 1);
        assert!(matches!(grant.denied[0].reason, VcpDenyReason::Ungrantable));

        assert!(grant.covers("locomotion").is_ok());
        assert!(grant.covers("firmware.update").is_err());
    }
}
