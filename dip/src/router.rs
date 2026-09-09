use std::collections::HashSet;
use crate::envelope::DipEnvelope;
use crate::address::DipNetwork;
use crate::error::{DipError, DipResult};

/// What the router should do with an envelope.
#[derive(Debug)]
pub enum RouteDecision {
    /// Deliver to a local handler (this node is the destination).
    DeliverLocal,
    /// Forward to the named adapter.
    Forward(DipNetwork),
    /// Drop — TTL expired or duplicate.
    Drop(String),
}

/// The DIP router: receives envelopes, decides where they go.
pub struct DipRouter {
    /// Adapters registered on this node.
    registered_adapters: HashSet<DipNetwork>,
    /// Seen message IDs for deduplication (bounded — real impl uses a TTL cache).
    seen: HashSet<String>,
    /// This node's canonical DID.
    local_did: String,
}

impl DipRouter {
    pub fn new(local_did: impl Into<String>) -> Self {
        Self {
            registered_adapters: HashSet::new(),
            seen: HashSet::new(),
            local_did: local_did.into(),
        }
    }

    pub fn register_adapter(&mut self, network: DipNetwork) {
        self.registered_adapters.insert(network);
    }

    /// Route an incoming envelope.
    pub fn route(&mut self, envelope: &DipEnvelope) -> DipResult<RouteDecision> {
        // Deduplication
        if self.seen.contains(&envelope.message_id) {
            return Ok(RouteDecision::Drop(format!("duplicate: {}", envelope.message_id)));
        }

        // TTL check
        if envelope.is_expired() {
            return Ok(RouteDecision::Drop("ttl_expired".into()));
        }

        self.seen.insert(envelope.message_id.clone());

        // Is this for us?
        let dest_did = envelope.destination.did.as_deref().unwrap_or("");
        let dest_addr = &envelope.destination.address;
        if dest_did == self.local_did || dest_addr == &self.local_did {
            return Ok(RouteDecision::DeliverLocal);
        }

        // Try to forward via destination network's adapter
        let dest_network = &envelope.destination.network;
        if self.registered_adapters.contains(dest_network) {
            return Ok(RouteDecision::Forward(dest_network.clone()));
        }

        Err(DipError::NoAdapter(dest_network.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::{DipEnvelope, DipKind};
    use crate::address::DipAddress;
    use sovereign_types::{IdentityChain, crypto::generate_keypair};

    fn make_envelope(dest: DipAddress, private_key: &str) -> DipEnvelope {
        DipEnvelope::build(
            DipAddress::vantage("did:vantage:agent:koda"),
            dest,
            IdentityChain::new("did:p:1".into(), "did:a:1".into()),
            DipKind::Message,
            serde_json::json!({"text": "hi"}),
            300,
            private_key,
        ).unwrap()
    }

    #[test]
    fn routes_local() {
        let (priv_key, _) = generate_keypair();
        let mut router = DipRouter::new("did:vantage:agent:koda");
        let env = make_envelope(DipAddress::vantage("did:vantage:agent:koda"), &priv_key);
        matches!(router.route(&env).unwrap(), RouteDecision::DeliverLocal);
    }

    #[test]
    fn deduplicates() {
        let (priv_key, _) = generate_keypair();
        let mut router = DipRouter::new("did:vantage:agent:koda");
        let env = make_envelope(DipAddress::vantage("did:vantage:agent:koda"), &priv_key);
        let _ = router.route(&env).unwrap();
        let decision = router.route(&env).unwrap();
        assert!(matches!(decision, RouteDecision::Drop(_)));
    }

    #[test]
    fn forwards_to_nostr() {
        let (priv_key, _) = generate_keypair();
        let mut router = DipRouter::new("did:vantage:agent:koda");
        router.register_adapter(DipNetwork::Nostr);
        let env = make_envelope(DipAddress::nostr("npub1xyz"), &priv_key);
        matches!(router.route(&env).unwrap(), RouteDecision::Forward(DipNetwork::Nostr));
    }

    #[test]
    fn no_adapter_errors() {
        let (priv_key, _) = generate_keypair();
        let mut router = DipRouter::new("did:vantage:agent:koda");
        let env = make_envelope(DipAddress::nostr("npub1xyz"), &priv_key);
        assert!(router.route(&env).is_err());
    }
}
