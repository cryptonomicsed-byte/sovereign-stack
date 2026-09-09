use serde::{Deserialize, Serialize};
use sovereign_types::Did;

/// Which decentralized network is being addressed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DipNetwork {
    Vantage,
    Nostr,
    A2A,
    Mcp,
    Meshtastic,
    Freenet,
    Libp2p,
}

impl std::fmt::Display for DipNetwork {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Vantage    => "vantage",
            Self::Nostr      => "nostr",
            Self::A2A        => "a2a",
            Self::Mcp        => "mcp",
            Self::Meshtastic => "meshtastic",
            Self::Freenet    => "freenet",
            Self::Libp2p     => "libp2p",
        };
        write!(f, "{}", s)
    }
}

/// A network-specific address, optionally resolved to a canonical DID.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DipAddress {
    pub network: DipNetwork,
    /// Network-specific address string (npub1..., !deadbeef, agent@host, etc.)
    pub address: String,
    /// Resolved canonical DID, if known
    pub did:     Option<Did>,
}

impl DipAddress {
    pub fn vantage(did: impl Into<String>) -> Self {
        let did = did.into();
        Self { network: DipNetwork::Vantage, address: did.clone(), did: Some(did) }
    }

    pub fn nostr(npub: impl Into<String>) -> Self {
        Self { network: DipNetwork::Nostr, address: npub.into(), did: None }
    }

    pub fn meshtastic(node_id: impl Into<String>) -> Self {
        Self { network: DipNetwork::Meshtastic, address: node_id.into(), did: None }
    }
}

/// A routing hop — filled in by routers as the envelope transits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DipHop {
    pub node:       DipAddress,
    pub adapter:    DipNetwork,
    pub timestamp:  sovereign_types::Timestamp,
    pub latency_ms: Option<u32>,
}
