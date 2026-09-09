use serde::{Deserialize, Serialize};
use serde_json::Value;
use sovereign_types::{IdentityChain, Signature, Timestamp, sign};
use crate::error::{VcpError, VcpResult};
use crate::grant::VcpCapabilityGrant;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum VcpCommandStatus {
    Accepted,
    Executing,
    Completed,
    Failed,
    Denied,
    GrantExpired,
}

/// A command sent from agent to device within an active VCP session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VcpCommand {
    pub cmd_id:     String,
    pub session_id: String,
    pub grant_id:   String,
    pub identity:   IdentityChain,
    pub capability: String,
    pub action:     String,
    pub params:     Value,
    pub timestamp:  Timestamp,
    pub signature:  Signature,
}

impl VcpCommand {
    pub fn new(
        grant:      &VcpCapabilityGrant,
        capability: impl Into<String>,
        action:     impl Into<String>,
        params:     Value,
        private_key: &str,
    ) -> VcpResult<Self> {
        let capability = capability.into();
        grant.covers(&capability)?;

        let cmd_id = format!("cmd:{}", uuid::Uuid::new_v4());
        let ts = now_ms();
        let data = format!("{}:{}:{}", cmd_id, capability, ts);
        let sig = sign(&data, private_key)?;

        Ok(Self {
            cmd_id,
            session_id: grant.session_id.clone(),
            grant_id:   grant.grant_id.clone(),
            identity:   grant.identity.clone(),
            capability,
            action:     action.into(),
            params,
            timestamp:  ts,
            signature:  sig,
        })
    }

    /// Validate this command against an active grant.
    pub fn validate(&self, grant: &VcpCapabilityGrant) -> VcpResult<()> {
        if grant.is_expired() {
            return Err(VcpError::GrantExpired);
        }
        grant.covers(&self.capability)?;

        // Enforce speed limit for locomotion commands
        if self.capability == "locomotion" {
            if let Some(speed) = self.params.get("speed_ms").and_then(|v| v.as_f64()) {
                if let Some(limit) = grant.covers("locomotion")?
                    .limits.get("max_speed_ms").and_then(|v| v.as_f64())
                {
                    if speed > limit {
                        return Err(VcpError::SpeedLimitExceeded {
                            requested: speed as f32,
                            granted:   limit as f32,
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

/// Device response to a command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VcpCommandResponse {
    pub cmd_id:    String,
    pub session_id: String,
    pub device_id:  String,
    pub status:     VcpCommandStatus,
    pub telemetry:  Option<Value>,
    pub error:      Option<String>,
    pub timestamp:  Timestamp,
    pub signature:  Signature,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
