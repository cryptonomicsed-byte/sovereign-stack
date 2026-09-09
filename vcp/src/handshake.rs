use serde::{Deserialize, Serialize};
use serde_json::Value;
use sovereign_types::{IdentityChain, Hash, Signature, Timestamp};
use crate::error::{VcpError, VcpResult};

/// VCP handshake states.
#[derive(Debug, Clone, PartialEq)]
pub enum HandshakeState {
    Disconnected,
    Discovered,
    Identifying,
    Authenticated,
    Negotiating,
    SessionOpen,
    SessionClosed,
    Revoked,
}

/// Step 3a: Device issues a challenge nonce.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VcpChallenge {
    pub challenge_id: String,
    pub nonce:        String,   // 32 random bytes, base64url
    pub device_id:    String,
    pub timestamp:    Timestamp,
    pub expires_at:   Timestamp,  // nonce expires in 60s
    pub signature:    Signature,  // device signs challenge
}

impl VcpChallenge {
    pub fn new(device_id: impl Into<String>, device_private_key: &str) -> Self {
        use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
        use rand::RngCore;

        let mut nonce_bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = URL_SAFE_NO_PAD.encode(nonce_bytes);

        let challenge_id = format!("challenge:{}", uuid::Uuid::new_v4());
        let now = now_ms();

        let data = format!("{}:{}:{}", challenge_id, nonce, now);
        let sig = sovereign_types::sign(&data, device_private_key)
            .unwrap_or_default();

        Self {
            challenge_id,
            nonce,
            device_id: device_id.into(),
            timestamp: now,
            expires_at: now + 60_000,
            signature: sig,
        }
    }

    pub fn is_expired(&self) -> bool {
        now_ms() > self.expires_at
    }
}

/// Step 3b: Agent responds to the challenge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VcpChallengeResponse {
    pub challenge_id: String,
    pub identity:     IdentityChain,
    /// Agent signs the challenge nonce with their principal key.
    pub nonce_sig:    Signature,
    /// URL to agent's DIP identity document.
    pub dip_identity: String,
    pub timestamp:    Timestamp,
    pub signature:    Signature,
}

impl VcpChallengeResponse {
    pub fn new(
        challenge:   &VcpChallenge,
        identity:    IdentityChain,
        dip_identity: impl Into<String>,
        agent_private_key: &str,
    ) -> VcpResult<Self> {
        if challenge.is_expired() {
            return Err(VcpError::ChallengeExpired);
        }
        let nonce_sig = sovereign_types::sign(&challenge.nonce, agent_private_key)?;
        let ts = now_ms();
        let data = format!("{}:{}:{}", challenge.challenge_id, nonce_sig, ts);
        let signature = sovereign_types::sign(&data, agent_private_key)?;

        Ok(Self {
            challenge_id: challenge.challenge_id.clone(),
            identity,
            nonce_sig,
            dip_identity: dip_identity.into(),
            timestamp: ts,
            signature,
        })
    }
}

/// Step 4a: Agent requests specific capabilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VcpCapabilityRequest {
    pub request_id:   String,
    pub identity:     IdentityChain,
    pub capabilities: Vec<String>,
    pub purpose:      String,
    pub duration:     VcpDuration,
    pub limits:       Option<Value>,
    pub timestamp:    Timestamp,
    pub signature:    Signature,
}

impl VcpCapabilityRequest {
    pub fn new(
        identity:    IdentityChain,
        capabilities: Vec<String>,
        purpose:     impl Into<String>,
        duration:    VcpDuration,
        private_key: &str,
    ) -> VcpResult<Self> {
        let request_id = format!("req:{}", uuid::Uuid::new_v4());
        let ts = now_ms();
        let data = format!("{}:{}", request_id, ts);
        let sig = sovereign_types::sign(&data, private_key)?;
        Ok(Self {
            request_id,
            identity,
            capabilities,
            purpose: purpose.into(),
            duration,
            limits: None,
            timestamp: ts,
            signature: sig,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VcpDuration {
    pub requested_min: u32,
    pub preferred_min: u32,
    pub max_min:       u32,  // max 1440 (24hr)
}

impl VcpDuration {
    pub fn minutes(min: u32) -> Self {
        Self { requested_min: min, preferred_min: min, max_min: min.min(1440) }
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

    #[test]
    fn challenge_response_roundtrip() {
        let (device_priv, _) = generate_keypair();
        let (agent_priv, _)  = generate_keypair();

        let challenge = VcpChallenge::new("unitree:go2:test", &device_priv);
        assert!(!challenge.is_expired());

        let identity = IdentityChain::new("did:p:1".into(), "did:a:1".into());
        let resp = VcpChallengeResponse::new(
            &challenge, identity, "did:vantage:agent:koda", &agent_priv
        ).unwrap();

        assert_eq!(resp.challenge_id, challenge.challenge_id);
    }
}
