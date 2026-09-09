use serde::{Deserialize, Serialize};
use serde_json::Value;
use sovereign_types::{IdentityChain, Hash, Signature, Timestamp, merkle_root, sign, verify};
use std::collections::BTreeMap;
use crate::address::{DipAddress, DipHop};
use crate::error::{DipError, DipResult};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DipKind {
    Capability,
    Message,
    Evidence,
    Receipt,
    Event,
    Claim,
}

/// The DIP envelope — lingua franca for all cross-network messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DipEnvelope {
    pub version:     String,           // "dip/1"
    pub message_id:  String,
    pub timestamp:   Timestamp,
    pub ttl:         u32,              // seconds until expiry

    pub origin:      DipAddress,
    pub destination: DipAddress,
    pub routing:     Vec<DipHop>,

    pub identity:    IdentityChain,

    pub kind:        DipKind,
    pub payload:     Value,

    pub merkle_root: Hash,
    pub signature:   Signature,
}

impl DipEnvelope {
    pub fn build(
        origin:      DipAddress,
        destination: DipAddress,
        identity:    IdentityChain,
        kind:        DipKind,
        payload:     Value,
        ttl_secs:    u32,
        private_key: &str,
    ) -> DipResult<Self> {
        identity.validate().map_err(|_| DipError::IncompleteIdentity)?;

        let message_id = format!("dip:{}", uuid::Uuid::new_v4());
        let timestamp  = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let mut fields = BTreeMap::new();
        fields.insert("destination", serde_json::to_value(&destination)?);
        fields.insert("identity",    serde_json::to_value(&identity)?);
        fields.insert("kind",        serde_json::to_value(&kind)?);
        fields.insert("message_id",  Value::String(message_id.clone()));
        fields.insert("origin",      serde_json::to_value(&origin)?);
        fields.insert("payload",     payload.clone());
        fields.insert("timestamp",   Value::Number(timestamp.into()));
        fields.insert("ttl",         Value::Number(ttl_secs.into()));
        fields.insert("version",     Value::String("dip/1".into()));

        let root = merkle_root(&fields);
        let sig  = sign(&root, private_key)?;

        Ok(Self {
            version: "dip/1".into(),
            message_id,
            timestamp,
            ttl: ttl_secs,
            origin,
            destination,
            routing: vec![],
            identity,
            kind,
            payload,
            merkle_root: root,
            signature: sig,
        })
    }

    /// Validate: signature, TTL, identity completeness.
    pub fn validate(&self, public_key: &str) -> DipResult<()> {
        // Check TTL
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let expires_at = self.timestamp + (self.ttl as u64 * 1000);
        if now > expires_at {
            return Err(DipError::Expired);
        }

        // Check identity
        self.identity.validate().map_err(|_| DipError::IncompleteIdentity)?;

        // Check signature
        verify(&self.merkle_root, &self.signature, public_key)?;

        Ok(())
    }

    /// Record a routing hop (called by each router that forwards the envelope).
    pub fn add_hop(&mut self, hop: DipHop) {
        self.routing.push(hop);
    }

    pub fn is_expired(&self) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        now > self.timestamp + (self.ttl as u64 * 1000)
    }
}

/// Payload for DipKind::Message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagePayload {
    pub text:    String,
    pub context: Option<String>,
    pub thread:  Option<String>,   // thread_id for reply chains
}

/// Payload for DipKind::Capability
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityPayload {
    pub direction:    CapabilityDirection,
    pub capabilities: Vec<DipCapability>,
    pub context:      Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityDirection { Declare, Request, Grant, Revoke }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DipCapability {
    pub id:          String,
    pub description: Option<String>,
    pub params:      Option<Value>,
    pub constraints: Option<Value>,
    pub expires_at:  Option<Timestamp>,
}

/// Payload for DipKind::Receipt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiptPayload {
    pub action:      String,
    pub target:      Option<String>,
    pub outcome:     sovereign_types::Outcome,
    pub duration_ms: u64,
    pub evidence_ids: Vec<String>,
    pub notes:       Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use sovereign_types::crypto::generate_keypair;
    use crate::address::DipAddress;

    #[test]
    fn envelope_roundtrip() {
        let (priv_key, pub_key) = generate_keypair();
        let identity = IdentityChain::new(
            "did:vantage:principal:test".into(),
            "did:vantage:agent:koda".into(),
        );
        let env = DipEnvelope::build(
            DipAddress::vantage("did:vantage:agent:koda"),
            DipAddress::nostr("npub1xyz"),
            identity,
            DipKind::Message,
            serde_json::json!({ "text": "hello" }),
            300,
            &priv_key,
        ).unwrap();

        assert_eq!(env.version, "dip/1");
        env.validate(&pub_key).unwrap();
    }

    #[test]
    fn expired_envelope_rejected() {
        let (priv_key, pub_key) = generate_keypair();
        let identity = IdentityChain::new("did:p:1".into(), "did:a:1".into());
        let mut env = DipEnvelope::build(
            DipAddress::vantage("did:vantage:agent:koda"),
            DipAddress::nostr("npub1"),
            identity,
            DipKind::Message,
            serde_json::json!({}),
            300,
            &priv_key,
        ).unwrap();
        // Force expired
        env.timestamp = 0;
        assert!(env.validate(&pub_key).is_err());
    }
}
