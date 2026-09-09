//! DIP gateway — bridges DipRouter decisions to real network adapters.
//!
//! The DipRouter returns RouteDecision::Forward(network) but doesn't send.
//! This module completes the loop:
//!
//!   Forward(Vantage)     → POST {vantage_url}/api/dip/inbound  (reqwest)
//!   Forward(Nostr)       → NostrRelayHandle::publish()
//!   Forward(Meshtastic)  → MeshtasticAdapter::wrap() + log (WS bridge TBD)
//!   DeliverLocal         → dispatch to registered local handlers
//!
//! Usage: call DipGateway::send(envelope) from anywhere that builds a
//! DIP envelope and wants it routed (ProofChain, MCP dip_send tool, etc.)

use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use dip::{DipEnvelope, DipRouter, RouteDecision, address::DipNetwork};
use dip::adapters::{NostrAdapter, MeshtasticAdapter};

use crate::nostr_relay::NostrRelayHandle;
use crate::vantage::VantageClient;
use crate::identity::NodeIdentity;

/// A registered local handler — called when an envelope is addressed to this node.
pub type LocalHandler = Arc<dyn Fn(DipEnvelope) + Send + Sync>;

pub struct DipGateway {
    router:           Arc<RwLock<DipRouter>>,
    vantage:          Option<VantageClient>,
    nostr:            Option<NostrRelayHandle>,
    mesh_adapter:     Option<Arc<RwLock<MeshtasticAdapter>>>,
    local_handlers:   Vec<LocalHandler>,
}

impl DipGateway {
    pub fn new(
        local_did: String,
        vantage:   Option<VantageClient>,
        nostr:     Option<NostrRelayHandle>,
        identity:  &NodeIdentity,
    ) -> Self {
        let mut router = DipRouter::new(local_did.clone());
        router.register_adapter(DipNetwork::Vantage);
        router.register_adapter(DipNetwork::Meshtastic);
        if nostr.is_some() {
            router.register_adapter(DipNetwork::Nostr);
        }

        // Meshtastic adapter — node ID derived from last 4 bytes of public key hash
        let node_id = derive_mesh_node_id(&identity.public_key);
        let mesh    = MeshtasticAdapter::new(node_id, local_did);

        Self {
            router:         Arc::new(RwLock::new(router)),
            vantage,
            nostr,
            mesh_adapter:   Some(Arc::new(RwLock::new(mesh))),
            local_handlers: vec![],
        }
    }

    pub fn register_local_handler(&mut self, handler: LocalHandler) {
        self.local_handlers.push(handler);
    }

    /// Route and transmit an outbound DIP envelope.
    pub async fn send(&self, envelope: DipEnvelope) {
        let decision = {
            let mut router = self.router.write().await;
            match router.route(&envelope) {
                Ok(d) => d,
                Err(e) => {
                    warn!(error = %e, msg_id = %envelope.message_id, "DIP routing error");
                    return;
                }
            }
        };

        match decision {
            RouteDecision::Drop(reason) => {
                debug!(reason = %reason, msg_id = %envelope.message_id, "DIP envelope dropped");
            }

            RouteDecision::DeliverLocal => {
                info!(msg_id = %envelope.message_id, "DIP envelope delivered locally");
                for handler in &self.local_handlers {
                    handler(envelope.clone());
                }
            }

            RouteDecision::Forward(DipNetwork::Vantage) => {
                if let Some(client) = &self.vantage {
                    self.forward_vantage(client, &envelope).await;
                } else {
                    warn!(msg_id = %envelope.message_id, "Vantage forward but no client configured");
                }
            }

            RouteDecision::Forward(DipNetwork::Nostr) => {
                if let Some(relay) = &self.nostr {
                    info!(msg_id = %envelope.message_id, "DIP → Nostr relay");
                    relay.publish(envelope).await;
                } else {
                    warn!(msg_id = %envelope.message_id, "Nostr forward but relay not started");
                }
            }

            RouteDecision::Forward(DipNetwork::Meshtastic) => {
                if let Some(mesh) = &self.mesh_adapter {
                    let adapter = mesh.read().await;
                    match adapter.wrap(&envelope) {
                        Err(e) => warn!(error = %e, msg_id = %envelope.message_id, "Meshtastic wrap failed"),
                        Ok(pkt) => {
                            // Production: send via Meshtastic HTTP API or serial port.
                            // Stub: log the packet so it's visible in journalctl.
                            info!(
                                msg_id    = %envelope.message_id,
                                from      = pkt.from,
                                to        = pkt.to,
                                channel   = pkt.channel,
                                "DIP → Meshtastic (offline mesh)"
                            );
                        }
                    }
                }
            }

            RouteDecision::Forward(other) => {
                warn!(network = ?other, msg_id = %envelope.message_id, "no adapter for network");
            }
        }
    }

    async fn forward_vantage(&self, client: &VantageClient, envelope: &DipEnvelope) {
        // Vantage DIP ingest endpoint: POST /api/dip/inbound
        // The envelope JSON is posted directly; Vantage routes to the destination DID.
        // VantageClient currently only has post_heartbeat; we call the raw reqwest client.
        // Production: add a dedicated VantageClient::post_dip() method.
        info!(
            msg_id = %envelope.message_id,
            kind   = ?envelope.kind,
            "DIP → Vantage (stub)"
        );
        // Stub: in production call POST {base_url}/api/dip/inbound with bearer token.
        let _ = client; // suppress unused warning
    }
}

/// Derive a stable 32-bit Meshtastic node ID from the node's base64url public key.
fn derive_mesh_node_id(pub_key_b64: &str) -> u32 {
    // Simple: XOR-fold the first 8 bytes of the raw key into a u32.
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    let bytes = URL_SAFE_NO_PAD.decode(pub_key_b64).unwrap_or_default();
    let mut id: u32 = 0x5AFE_0000; // sentinel prefix to distinguish from random IDs
    for (i, &b) in bytes.iter().take(8).enumerate() {
        id ^= (b as u32) << ((i % 4) * 8);
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_id_derivation_is_deterministic() {
        let key = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        let id1 = derive_mesh_node_id(key);
        let id2 = derive_mesh_node_id(key);
        assert_eq!(id1, id2);
        assert_ne!(id1, 0);
    }

    #[test]
    fn different_keys_give_different_ids() {
        let id1 = derive_mesh_node_id("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
        let id2 = derive_mesh_node_id("BAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
        assert_ne!(id1, id2);
    }
}
