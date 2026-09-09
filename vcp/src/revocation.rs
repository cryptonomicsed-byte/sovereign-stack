use serde::{Deserialize, Serialize};
use sovereign_types::{IdentityChain, Signature, Timestamp, sign};
use crate::error::VcpResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VcpRevocationReason {
    UserRequested,
    GrantExpired,
    SecurityIncident,
    PolicyViolation,
    PrincipalRevoked,
}

/// A signed revocation — principal can send this to terminate any session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VcpRevocation {
    pub revocation_id: String,
    pub grant_id:      String,
    pub session_id:    String,
    pub identity:      IdentityChain,
    pub reason:        VcpRevocationReason,
    pub timestamp:     Timestamp,
    /// If true, broadcast via Meshtastic to reach offline devices.
    pub propagate:     bool,
    pub signature:     Signature,
}

impl VcpRevocation {
    pub fn new(
        grant_id:   impl Into<String>,
        session_id: impl Into<String>,
        identity:   IdentityChain,
        reason:     VcpRevocationReason,
        propagate:  bool,
        private_key: &str,
    ) -> VcpResult<Self> {
        let revocation_id = format!("rev:{}", uuid::Uuid::new_v4());
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let data = format!("{}:{}", revocation_id, ts);
        let sig = sign(&data, private_key)?;

        Ok(Self {
            revocation_id,
            grant_id:   grant_id.into(),
            session_id: session_id.into(),
            identity,
            reason,
            timestamp:  ts,
            propagate,
            signature:  sig,
        })
    }
}
