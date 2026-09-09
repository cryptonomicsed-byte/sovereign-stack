//! DIP ↔ Nostr adapter.
//!
//! DIP-over-Nostr uses NIP-01 event kinds:
//!   kind 20000–29999  ephemeral   → DipKind::Message, Capability, Event, Claim
//!   kind 30000–39999  replaceable → DipKind::Receipt, Evidence
//!
//! DIP envelope is serialised into the Nostr event `content` field as JSON.
//! Routing metadata lives in tags:
//!   ["dip", "v1"]                   — marks as DIP envelope
//!   ["dip-kind", "<kind>"]           — DipKind string
//!   ["dip-origin", "<did>"]          — sender DID
//!   ["dip-dest", "<did>"]            — destination DID
//!   ["dip-msg-id", "<message_id>"]   — for deduplication

use crate::envelope::{DipEnvelope, DipKind};
use crate::address::{DipAddress, DipNetwork};
use crate::error::{DipError, DipResult};

pub fn dip_kind_to_nostr_kind(kind: &DipKind) -> u16 {
    match kind {
        DipKind::Message    => 20001,
        DipKind::Capability => 20002,
        DipKind::Event      => 20003,
        DipKind::Claim      => 20004,
        DipKind::Receipt    => 30001,
        DipKind::Evidence   => 30002,
    }
}

pub fn nostr_kind_to_dip_kind(kind: u16) -> Option<DipKind> {
    match kind {
        20001 => Some(DipKind::Message),
        20002 => Some(DipKind::Capability),
        20003 => Some(DipKind::Event),
        20004 => Some(DipKind::Claim),
        30001 => Some(DipKind::Receipt),
        30002 => Some(DipKind::Evidence),
        _     => None,
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NostrEvent {
    pub id:         String,
    pub pubkey:     String,
    pub created_at: u64,
    pub kind:       u16,
    pub tags:       Vec<Vec<String>>,
    pub content:    String,
    pub sig:        String,
}

impl NostrEvent {
    pub fn to_dip_envelope(&self) -> DipResult<DipEnvelope> {
        let is_dip = self.tags.iter()
            .any(|t| t.len() >= 2 && t[0] == "dip" && t[1] == "v1");
        if !is_dip {
            return Err(DipError::RoutingFailed("not a DIP envelope (missing dip tag)".into()));
        }
        serde_json::from_str(&self.content).map_err(DipError::Serialization)
    }

    pub fn from_dip_envelope(envelope: &DipEnvelope, nostr_pubkey: &str, nostr_sig: &str) -> DipResult<Self> {
        let nostr_kind = dip_kind_to_nostr_kind(&envelope.kind);
        let content    = serde_json::to_string(envelope)?;
        let origin_did = envelope.origin.did.as_deref().unwrap_or(&envelope.origin.address);
        let dest_did   = envelope.destination.did.as_deref().unwrap_or(&envelope.destination.address);

        let tags = vec![
            vec!["dip".into(),        "v1".into()],
            vec!["dip-kind".into(),   format!("{:?}", envelope.kind).to_lowercase()],
            vec!["dip-origin".into(), origin_did.to_string()],
            vec!["dip-dest".into(),   dest_did.to_string()],
            vec!["dip-msg-id".into(), envelope.message_id.clone()],
            vec!["p".into(),          nostr_pubkey.to_string()],
        ];

        Ok(Self {
            id:         envelope.message_id.clone(),
            pubkey:     nostr_pubkey.into(),
            created_at: envelope.timestamp / 1000,
            kind:       nostr_kind,
            tags,
            content,
            sig:        nostr_sig.into(),
        })
    }
}

pub struct NostrAdapter {
    pub relay_url:     String,
    pub nostr_pubkey:  String,
    pub nostr_privkey: String,
}

impl NostrAdapter {
    pub fn new(relay_url: impl Into<String>, pubkey: impl Into<String>, privkey: impl Into<String>) -> Self {
        Self { relay_url: relay_url.into(), nostr_pubkey: pubkey.into(), nostr_privkey: privkey.into() }
    }

    pub fn wrap(&self, envelope: &DipEnvelope) -> DipResult<NostrEvent> {
        let sig = format!("nostr_sig:{}", &self.nostr_pubkey[..8.min(self.nostr_pubkey.len())]);
        NostrEvent::from_dip_envelope(envelope, &self.nostr_pubkey, &sig)
    }

    pub fn unwrap(&self, event: &NostrEvent) -> DipResult<DipEnvelope> {
        event.to_dip_envelope()
    }

    pub fn resolve_npub(&self, npub: &str, did: Option<String>) -> DipAddress {
        DipAddress { network: DipNetwork::Nostr, address: npub.into(), did }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::DipEnvelope;
    use crate::address::DipAddress;
    use sovereign_types::{IdentityChain, crypto::generate_keypair};

    fn make_envelope(kind: DipKind, priv_key: &str) -> DipEnvelope {
        DipEnvelope::build(
            DipAddress::vantage("did:vantage:agent:koda"),
            DipAddress::nostr("npub1xyz"),
            IdentityChain::new("did:p:1".into(), "did:a:koda".into()),
            kind,
            serde_json::json!({"text": "hello"}),
            300,
            priv_key,
        ).unwrap()
    }

    #[test]
    fn wrap_unwrap_roundtrip() {
        let (priv_key, _) = generate_keypair();
        let adapter  = NostrAdapter::new("wss://relay.damus.io", "npub1vantage", "nsec1test");
        let envelope = make_envelope(DipKind::Message, &priv_key);
        let event    = adapter.wrap(&envelope).unwrap();

        assert!(event.tags.iter().any(|t| t[0] == "dip" && t[1] == "v1"));
        assert_eq!(event.kind, 20001);

        let recovered = adapter.unwrap(&event).unwrap();
        assert_eq!(recovered.message_id, envelope.message_id);
    }

    #[test]
    fn receipt_uses_replaceable_kind() {
        let (priv_key, _) = generate_keypair();
        let adapter = NostrAdapter::new("wss://relay.damus.io", "npub1", "nsec1");
        let event   = adapter.wrap(&make_envelope(DipKind::Receipt, &priv_key)).unwrap();
        assert!(event.kind >= 30000, "receipts must use replaceable Nostr kinds");
    }

    #[test]
    fn non_dip_event_rejected() {
        let adapter = NostrAdapter::new("wss://relay.damus.io", "npub1", "nsec1");
        let plain   = NostrEvent { id: "abc".into(), pubkey: "npub1".into(), created_at: 0,
                                   kind: 1, tags: vec![], content: "hello".into(), sig: "sig".into() };
        assert!(adapter.unwrap(&plain).is_err());
    }
}
