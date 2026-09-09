use serde::{Deserialize, Serialize};
use sovereign_types::{Did, Hash, Signature, Timestamp};
use crate::address::DipNetwork;

/// A DIP Identity Document — maps one canonical DID to multiple network addresses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DipIdentityDocument {
    pub did:               Did,
    pub created:           Timestamp,
    pub equivalences:      Vec<DipEquivalence>,
    pub service_endpoints: Vec<DipServiceEndpoint>,
    pub merkle_root:       Hash,
    pub signature:         Signature,  // canonical key signs all equivalences
}

impl DipIdentityDocument {
    pub fn new(did: Did, created: Timestamp) -> Self {
        Self {
            did,
            created,
            equivalences: vec![],
            service_endpoints: vec![],
            merkle_root: String::new(),
            signature: String::new(),
        }
    }

    pub fn add_equivalence(&mut self, equiv: DipEquivalence) {
        self.equivalences.push(equiv);
    }

    pub fn add_endpoint(&mut self, ep: DipServiceEndpoint) {
        self.service_endpoints.push(ep);
    }

    /// Find the address for a given network.
    pub fn address_for(&self, network: &DipNetwork) -> Option<&str> {
        self.equivalences.iter()
            .find(|e| &e.network == network)
            .map(|e| e.address.as_str())
    }
}

/// A claimed equivalence between the canonical DID and a network-specific address.
/// The foreign network key signs the canonical DID to prove ownership of both.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DipEquivalence {
    pub network:  DipNetwork,
    pub address:  String,
    /// Foreign key signature over the canonical DID string
    pub proof:    Signature,
    /// Set to true after a round-trip challenge has been verified
    pub verified: bool,
}

impl DipEquivalence {
    pub fn new(network: DipNetwork, address: impl Into<String>, proof: Signature) -> Self {
        Self { network, address: address.into(), proof, verified: false }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DipServiceEndpoint {
    pub kind:     String,   // "vantage-mcp", "a2a", "vcp-gateway"
    pub url:      String,
    pub protocol: String,   // "mcp/1", "a2a/1", "vcp/1"
}

/// In-memory identity registry — stores and resolves DID documents.
#[derive(Debug, Default)]
pub struct IdentityRegistry {
    docs: std::collections::HashMap<String, DipIdentityDocument>,
    // network_address → DID reverse index
    by_address: std::collections::HashMap<String, String>,
}

impl IdentityRegistry {
    pub fn new() -> Self { Self::default() }

    pub fn register(&mut self, doc: DipIdentityDocument) {
        for eq in &doc.equivalences {
            let key = format!("{}:{}", eq.network, eq.address);
            self.by_address.insert(key, doc.did.clone());
        }
        self.docs.insert(doc.did.clone(), doc);
    }

    pub fn resolve_did(&self, did: &str) -> Option<&DipIdentityDocument> {
        self.docs.get(did)
    }

    /// Resolve any network address to its canonical DID.
    pub fn resolve_address(&self, network: &DipNetwork, address: &str) -> Option<&str> {
        let key = format!("{}:{}", network, address);
        self.by_address.get(&key).map(|did| self.docs.get(did))
            .flatten()
            .map(|doc| doc.did.as_str())
    }
}
