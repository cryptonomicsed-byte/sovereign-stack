//! DIP ↔ Meshtastic adapter — offline mesh routing.
//!
//! DIP-over-Meshtastic uses portnum=PRIVATE_APP (256) so standard Meshtastic
//! nodes forward the packet without inspecting it.  The DIP envelope is
//! base64url-encoded and placed in the `decoded.payload` field.
//!
//! Node ID mapping:
//!   Meshtastic uses 32-bit node IDs.  This adapter maintains a local
//!   DID → node_id table (`did_table`).  Unknown senders are keyed by
//!   node_id only; the full DID is recovered from the envelope body.
//!
//! Channel selection:
//!   DipKind::Receipt / Evidence → channel 1 (encrypted, private)
//!   All other kinds             → channel 0 (primary, default)
//!
//! Transport note:
//!   Meshtastic provides HTTP, serial, and BLE interfaces.  This adapter
//!   handles the packet encoding/decoding only.  Wire transport is injected
//!   by the caller (see `MeshtasticTransport` trait).

use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

use crate::address::{DipAddress, DipNetwork};
use crate::envelope::{DipEnvelope, DipKind};
use crate::error::{DipError, DipResult};

/// A Meshtastic packet (simplified JSON representation from the HTTP API).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshPacket {
    /// Sending node's 32-bit ID (decimal).
    pub from:      u32,
    /// Receiving node's 32-bit ID (0xFFFFFFFF = broadcast).
    pub to:        u32,
    pub id:        u32,
    pub channel:   u8,
    pub decoded:   MeshDecoded,
    pub hop_limit: u8,
    pub rx_time:   Option<u64>,
    pub rx_snr:    Option<f32>,
    pub rx_rssi:   Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshDecoded {
    /// 256 = PRIVATE_APP
    pub portnum: u16,
    /// Base64url-encoded DIP envelope JSON.
    pub payload: String,
    pub bitfield: u32,
}

impl MeshPacket {
    /// Broadcast packet ID — all Meshtastic nodes in range will receive it.
    pub const BROADCAST_ID: u32 = 0xFFFF_FFFF;

    pub fn is_dip_packet(&self) -> bool {
        self.decoded.portnum == 256
    }

    pub fn to_dip_envelope(&self) -> DipResult<DipEnvelope> {
        if !self.is_dip_packet() {
            return Err(DipError::RoutingFailed(
                format!("not a DIP packet (portnum={})", self.decoded.portnum)
            ));
        }
        let bytes = URL_SAFE_NO_PAD
            .decode(&self.decoded.payload)
            .map_err(|e| DipError::RoutingFailed(format!("base64 decode: {e}")))?;
        serde_json::from_slice(&bytes).map_err(DipError::Serialization)
    }
}

/// A trait for actually sending/receiving Meshtastic packets.
/// Implementations: HTTP, serial (UART), BLE.
/// The adapter itself is transport-agnostic.
pub trait MeshtasticTransport: Send + Sync {
    fn send_packet(&self, packet: &MeshPacket) -> DipResult<()>;
}

/// Stub transport that collects sent packets in memory (for testing).
#[derive(Debug, Default)]
pub struct InMemoryTransport {
    pub sent: std::sync::Mutex<Vec<MeshPacket>>,
}

impl MeshtasticTransport for InMemoryTransport {
    fn send_packet(&self, packet: &MeshPacket) -> DipResult<()> {
        self.sent.lock().unwrap().push(packet.clone());
        Ok(())
    }
}

/// The Meshtastic DIP adapter.
pub struct MeshtasticAdapter {
    /// This node's Meshtastic 32-bit ID.
    pub local_node_id: u32,
    /// DID of this node (filled into envelope origin).
    pub local_did:     String,
    /// DID → Meshtastic node_id table for known peers.
    pub did_table:     HashMap<String, u32>,
}

impl MeshtasticAdapter {
    pub fn new(local_node_id: u32, local_did: impl Into<String>) -> Self {
        Self {
            local_node_id,
            local_did: local_did.into(),
            did_table: HashMap::new(),
        }
    }

    /// Register a peer DID ↔ node_id mapping learned from a received packet.
    pub fn learn_peer(&mut self, did: impl Into<String>, node_id: u32) {
        self.did_table.insert(did.into(), node_id);
    }

    /// Resolve a DID to a Meshtastic node ID.
    /// Returns broadcast if unknown (flood the mesh — recipient filters by DID).
    pub fn resolve_node_id(&self, did: &str) -> u32 {
        self.did_table.get(did).copied().unwrap_or(MeshPacket::BROADCAST_ID)
    }

    /// DipKind → Meshtastic channel index.
    fn channel_for(kind: &DipKind) -> u8 {
        match kind {
            DipKind::Receipt | DipKind::Evidence => 1,  // encrypted/private channel
            _ => 0,
        }
    }

    /// Wrap a DIP envelope in a MeshPacket ready to transmit.
    pub fn wrap(&self, envelope: &DipEnvelope) -> DipResult<MeshPacket> {
        let dest_did  = envelope.destination.did.as_deref()
            .unwrap_or(&envelope.destination.address);
        let to        = self.resolve_node_id(dest_did);
        let channel   = Self::channel_for(&envelope.kind);

        let payload_bytes = serde_json::to_vec(envelope)?;
        let payload       = URL_SAFE_NO_PAD.encode(&payload_bytes);

        let packet_id = (envelope.timestamp & 0xFFFF_FFFF) as u32;

        Ok(MeshPacket {
            from:      self.local_node_id,
            to,
            id:        packet_id,
            channel,
            decoded:   MeshDecoded { portnum: 256, payload, bitfield: 0 },
            hop_limit: 3,
            rx_time:   None,
            rx_snr:    None,
            rx_rssi:   None,
        })
    }

    /// Unwrap a received MeshPacket into a DIP envelope, and learn the sender's node_id.
    pub fn unwrap(&mut self, packet: &MeshPacket) -> DipResult<DipEnvelope> {
        let envelope = packet.to_dip_envelope()?;

        // Learn the peer's DID ↔ node_id so future replies are unicast
        if let Some(origin_did) = &envelope.origin.did {
            self.did_table.insert(origin_did.clone(), packet.from);
        }

        // Discard packets not addressed to us (if unicast)
        if packet.to != MeshPacket::BROADCAST_ID && packet.to != self.local_node_id {
            return Err(DipError::RoutingFailed(
                format!("packet to={} is not for us (node_id={})", packet.to, self.local_node_id)
            ));
        }

        Ok(envelope)
    }

    /// Build a DipAddress for a Meshtastic peer.
    /// The address is the node_id in hex (e.g. "!aabbccdd").
    pub fn mesh_address(node_id: u32, did: Option<String>) -> DipAddress {
        DipAddress {
            network: DipNetwork::Meshtastic,
            address: format!("!{:08x}", node_id),
            did,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::DipEnvelope;
    use crate::address::DipAddress;
    use sovereign_types::{IdentityChain, crypto::generate_keypair};

    fn make_adapter() -> MeshtasticAdapter {
        MeshtasticAdapter::new(0xDEAD_BEEF, "did:vantage:node:local")
    }

    fn make_envelope(priv_key: &str) -> DipEnvelope {
        DipEnvelope::build(
            DipAddress { network: DipNetwork::Meshtastic, address: "!deadbeef".into(), did: Some("did:vantage:node:local".into()) },
            DipAddress { network: DipNetwork::Meshtastic, address: "!cafebabe".into(), did: Some("did:vantage:node:peer".into()) },
            IdentityChain::new("did:p:1".into(), "did:a:node".into()),
            DipKind::Message,
            serde_json::json!({"text": "offline ping"}),
            300,
            priv_key,
        ).unwrap()
    }

    #[test]
    fn wrap_produces_private_app_portnum() {
        let (priv_key, _) = generate_keypair();
        let adapter  = make_adapter();
        let envelope = make_envelope(&priv_key);
        let packet   = adapter.wrap(&envelope).unwrap();

        assert_eq!(packet.decoded.portnum, 256);
        assert_eq!(packet.from, 0xDEAD_BEEF);
        assert_eq!(packet.channel, 0);  // DipKind::Message → channel 0
    }

    #[test]
    fn receipt_routes_to_encrypted_channel() {
        let (priv_key, _) = generate_keypair();
        let mut adapter = make_adapter();
        adapter.learn_peer("did:vantage:node:peer", 0xCAFE_BABE);

        let envelope = DipEnvelope::build(
            DipAddress { network: DipNetwork::Meshtastic, address: "!deadbeef".into(), did: Some("did:vantage:node:local".into()) },
            DipAddress { network: DipNetwork::Meshtastic, address: "!cafebabe".into(), did: Some("did:vantage:node:peer".into()) },
            IdentityChain::new("did:p:1".into(), "did:a:node".into()),
            DipKind::Receipt,
            serde_json::json!({"receipt_id": "r:123"}),
            300,
            &priv_key,
        ).unwrap();

        let packet = adapter.wrap(&envelope).unwrap();
        assert_eq!(packet.channel, 1);           // Receipt → encrypted channel
        assert_eq!(packet.to, 0xCAFE_BABE);      // Known peer → unicast, not broadcast
    }

    #[test]
    fn unknown_dest_broadcasts() {
        let (priv_key, _) = generate_keypair();
        let adapter  = make_adapter();
        let envelope = make_envelope(&priv_key);
        let packet   = adapter.wrap(&envelope).unwrap();

        assert_eq!(packet.to, MeshPacket::BROADCAST_ID);
    }

    #[test]
    fn roundtrip_wrap_unwrap() {
        let (priv_key, _) = generate_keypair();
        let mut adapter  = make_adapter();
        let original     = make_envelope(&priv_key);
        let packet       = adapter.wrap(&original).unwrap();

        // Simulate receiving this packet as a broadcast addressed to us
        let mut recv_packet = packet.clone();
        recv_packet.from = 0xCAFE_BABE;
        recv_packet.to   = MeshPacket::BROADCAST_ID;

        let recovered = adapter.unwrap(&recv_packet).unwrap();
        assert_eq!(recovered.message_id, original.message_id);
    }

    #[test]
    fn learns_sender_did_on_unwrap() {
        let (priv_key, _) = generate_keypair();
        let mut adapter  = make_adapter();
        let envelope     = make_envelope(&priv_key);
        let mut packet   = adapter.wrap(&envelope).unwrap();
        packet.from      = 0x1234_5678;
        packet.to        = MeshPacket::BROADCAST_ID;

        adapter.unwrap(&packet).unwrap();

        // Should have learned did:vantage:node:local → 0x12345678
        assert_eq!(
            adapter.did_table.get("did:vantage:node:local"),
            Some(&0x1234_5678)
        );
    }

    #[test]
    fn non_dip_packet_rejected() {
        let packet = MeshPacket {
            from: 1, to: 2, id: 0, channel: 0,
            decoded: MeshDecoded { portnum: 1, payload: String::new(), bitfield: 0 },
            hop_limit: 3, rx_time: None, rx_snr: None, rx_rssi: None,
        };
        assert!(packet.to_dip_envelope().is_err());
    }

    #[test]
    fn mesh_address_formats_hex() {
        let addr = MeshtasticAdapter::mesh_address(0xDEAD_BEEF, None);
        assert_eq!(addr.address, "!deadbeef");
        assert!(matches!(addr.network, DipNetwork::Meshtastic));
    }
}
