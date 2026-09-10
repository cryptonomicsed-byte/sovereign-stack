use serde::{Deserialize, Serialize};
use sovereign_types::identity::{Did, AgentDid, IdentityChain, SafetyLevel};

/// Hardware attestation from a physical device key.
/// The actual signing algorithm is abstracted via HardwareKeyProvider.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HardwareAttestation {
    pub device_did:   String,
    pub hw_pubkey:    String,   // hex-encoded
    pub attestation:  String,   // base64url(sig over principal_id||nonce)
    pub nonce:        String,
    pub provider:     String,   // e.g. "atecc608b", "tpm2", "se050", "software"
}

/// The full runtime identity of a Principal —
/// extends IdentityChain with hardware + network bindings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Principal {
    pub chain:          IdentityChain,

    // Network bindings
    pub nostr_pubkey:   Option<String>,   // hex-encoded 32-byte key
    pub sui_address:    Option<String>,   // 0x-prefixed
    pub ip_root_id:     Option<String>,   // kind:31900 event id

    // Hardware binding
    pub device_did:     Option<String>,
    pub hw_attestation: Option<HardwareAttestation>,

    // Safety ceiling — capability grants cannot exceed this
    pub safety_level:   SafetyLevel,
}

impl Principal {
    /// Minimal principal — DID only, no hardware binding.
    pub fn from_did(principal_id: Did, agent_id: AgentDid) -> Self {
        Self {
            chain:          IdentityChain::new(principal_id, agent_id),
            nostr_pubkey:   None,
            sui_address:    None,
            ip_root_id:     None,
            device_did:     None,
            hw_attestation: None,
            safety_level:   SafetyLevel::Standard,
        }
    }

    pub fn with_nostr(mut self, pubkey: impl Into<String>) -> Self {
        self.nostr_pubkey = Some(pubkey.into());
        self
    }

    pub fn with_sui(mut self, address: impl Into<String>) -> Self {
        self.sui_address = Some(address.into());
        self
    }

    pub fn with_ip_root(mut self, event_id: impl Into<String>) -> Self {
        self.ip_root_id = Some(event_id.into());
        self
    }

    pub fn with_device(mut self, device_did: impl Into<String>, attestation: HardwareAttestation) -> Self {
        self.device_did     = Some(device_did.into());
        self.hw_attestation = Some(attestation);
        self
    }

    pub fn with_safety(mut self, level: SafetyLevel) -> Self {
        self.safety_level = level;
        self
    }

    pub fn principal_id(&self) -> &str { &self.chain.principal_id }
    pub fn agent_id(&self)     -> &str { &self.chain.agent_id }
    pub fn receipt_id(&self)   -> &str { &self.chain.receipt_id }

    pub fn is_hardware_bound(&self) -> bool {
        self.hw_attestation.is_some()
    }

    pub fn validate(&self) -> Result<(), crate::RuntimeError> {
        self.chain.validate().map_err(|e| crate::RuntimeError::Identity(e.to_string()))?;
        if let Some(ref nostr) = self.nostr_pubkey {
            if nostr.len() != 64 {
                return Err(crate::RuntimeError::InvalidPrincipal("nostr_pubkey must be 64-char hex"));
            }
        }
        if let Some(ref sui) = self.sui_address {
            if !sui.starts_with("0x") {
                return Err(crate::RuntimeError::InvalidPrincipal("sui_address must start with 0x"));
            }
        }
        Ok(())
    }
}
