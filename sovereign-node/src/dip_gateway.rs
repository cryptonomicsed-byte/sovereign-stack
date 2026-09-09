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
use dip::adapters::{MeshtasticAdapter, MeshPacket};

use crate::nostr_relay::NostrRelayHandle;
use crate::vantage::VantageClient;
use crate::identity::NodeIdentity;
use crate::config::MeshtasticSection;

/// A registered local handler — called when an envelope is addressed to this node.
pub type LocalHandler = Arc<dyn Fn(DipEnvelope) + Send + Sync>;

pub struct DipGateway {
    router:           Arc<RwLock<DipRouter>>,
    vantage:          Option<VantageClient>,
    nostr:            Option<NostrRelayHandle>,
    mesh_adapter:     Option<Arc<RwLock<MeshtasticAdapter>>>,
    mesh_http:        Option<String>,   // device_url for HTTP toRadio bridge
    http_client:      reqwest::Client,
    local_handlers:   Vec<LocalHandler>,
}

impl DipGateway {
    pub fn new(
        local_did:  String,
        vantage:    Option<VantageClient>,
        nostr:      Option<NostrRelayHandle>,
        identity:   &NodeIdentity,
        meshtastic: Option<&MeshtasticSection>,
    ) -> Self {
        let mut router = DipRouter::new(local_did.clone());
        router.register_adapter(DipNetwork::Vantage);
        router.register_adapter(DipNetwork::Meshtastic);
        if nostr.is_some() {
            router.register_adapter(DipNetwork::Nostr);
        }

        let node_id = derive_mesh_node_id(&identity.public_key);
        let mesh    = MeshtasticAdapter::new(node_id, local_did);

        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap_or_default();

        Self {
            router:         Arc::new(RwLock::new(router)),
            vantage,
            nostr,
            mesh_adapter:   Some(Arc::new(RwLock::new(mesh))),
            mesh_http:      meshtastic.map(|m| m.device_url.clone()),
            http_client,
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
                            info!(
                                msg_id  = %envelope.message_id,
                                from    = pkt.from,
                                to      = pkt.to,
                                channel = pkt.channel,
                                "DIP → Meshtastic"
                            );
                            if let Some(device_url) = &self.mesh_http {
                                self.forward_meshtastic(device_url, &pkt).await;
                            } else {
                                debug!(msg_id = %envelope.message_id, "Meshtastic HTTP bridge not configured — logged only");
                            }
                        }
                    }
                }
            }

            RouteDecision::Forward(other) => {
                warn!(network = ?other, msg_id = %envelope.message_id, "no adapter for network");
            }
        }
    }

    /// Spawn a background task that polls `{device_url}/api/v1/fromRadio` every 200 ms
    /// and forwards any received DIP envelopes to `inbound_tx`.
    pub fn spawn_mesh_inbound(
        &self,
        device_url: &str,
        inbound_tx: tokio::sync::mpsc::Sender<DipEnvelope>,
    ) {
        let Some(mesh) = self.mesh_adapter.clone() else {
            warn!("spawn_mesh_inbound called but no mesh adapter available");
            return;
        };

        let url    = format!("{device_url}/api/v1/fromRadio");
        let client = self.http_client.clone();

        tokio::spawn(async move {
            info!(url = %url, "Meshtastic inbound poll started");
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(200));

            loop {
                interval.tick().await;

                let resp = match client.get(&url).send().await {
                    Ok(r)  => r,
                    Err(e) => {
                        warn!(error = %e, "Meshtastic fromRadio poll error — retrying in 2s");
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        continue;
                    }
                };

                if !resp.status().is_success() {
                    debug!(status = %resp.status(), "Meshtastic fromRadio non-2xx");
                    continue;
                }

                let pkt: dip::adapters::MeshPacket = match resp.json().await {
                    Ok(p)  => p,
                    Err(_) => continue, // empty body or non-DIP packet — skip silently
                };

                let mut adapter = mesh.write().await;
                match adapter.unwrap(&pkt) {
                    Ok(envelope) => {
                        info!(
                            msg_id = %envelope.message_id,
                            kind   = ?envelope.kind,
                            from   = pkt.from,
                            "inbound DIP envelope via Meshtastic"
                        );
                        let _ = inbound_tx.send(envelope).await;
                    }
                    Err(_) => {} // non-DIP or invalid — skip silently
                }
            }
        });
    }

    async fn forward_vantage(&self, client: &VantageClient, envelope: &DipEnvelope) {
        info!(msg_id = %envelope.message_id, kind = ?envelope.kind, "DIP → Vantage");
        client.post_dip(envelope).await;
    }

    /// POST the mesh packet to the Meshtastic device HTTP API.
    /// Endpoint: POST {device_url}/api/v1/toRadio
    /// Body: JSON-encoded MeshPacket (Meshtastic firmware accepts JSON via HTTP API).
    async fn forward_meshtastic(&self, device_url: &str, pkt: &MeshPacket) {
        let url = format!("{device_url}/api/v1/toRadio");
        match self.http_client.post(&url).json(pkt).send().await {
            Ok(resp) if resp.status().is_success() => {
                debug!(to = pkt.to, channel = pkt.channel, "Meshtastic toRadio OK");
            }
            Ok(resp) => {
                warn!(
                    to      = pkt.to,
                    status  = %resp.status(),
                    "Meshtastic toRadio non-2xx"
                );
            }
            Err(e) => {
                warn!(error = %e, "Meshtastic toRadio POST failed");
            }
        }
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
