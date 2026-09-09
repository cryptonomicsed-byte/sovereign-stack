use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Canonical DID — root identity across all protocol representations.
/// Format: did:vantage:principal:<sha256hex(pubkey)>
pub type Did = String;
pub type AgentDid = String;
pub type DeviceDid = String;
pub type WitnessDid = String;

/// Unix milliseconds
pub type Timestamp = u64;

/// sha256:<hex> — content-addressed hash
pub type Hash = String;

/// base64url(ed25519_sig) — always over the struct's merkle root
pub type Signature = String;

/// The 5 canonical primitives — travel together on every consequential event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IdentityChain {
    pub principal_id: Did,
    pub agent_id:     AgentDid,
    pub session_id:   String,
    pub execution_id: String,
    pub receipt_id:   String,
}

impl IdentityChain {
    pub fn new(principal_id: Did, agent_id: AgentDid) -> Self {
        Self {
            principal_id,
            agent_id,
            session_id:   format!("sess:{}", Uuid::new_v4()),
            execution_id: format!("exec:{}", Uuid::new_v4()),
            receipt_id:   format!("rcpt:{}", Uuid::new_v4()),
        }
    }

    pub fn with_session(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = session_id.into();
        self
    }

    pub fn validate(&self) -> Result<(), crate::SovereignError> {
        if self.principal_id.is_empty() {
            return Err(crate::SovereignError::IncompleteIdentity("principal_id"));
        }
        if self.agent_id.is_empty() {
            return Err(crate::SovereignError::IncompleteIdentity("agent_id"));
        }
        if self.session_id.is_empty() {
            return Err(crate::SovereignError::IncompleteIdentity("session_id"));
        }
        Ok(())
    }
}

/// A DIP network identifier — which protocol network is being addressed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Network {
    Vantage,
    Nostr,
    A2A,
    Mcp,
    Meshtastic,
    Freenet,
    Libp2p,
}

/// Universal outcome for any action.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Success,
    Partial,
    Failure,
    Pending,
    Inconclusive,
}

/// TSP modality types for capture data.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Modality {
    Rgb,
    Depth,
    Lidar,
    Imu,
    Slam,
    Thermal,
    Acoustic,
}

/// TSP privacy flags for captured data.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyFlag {
    FacesDetected,
    LicensePlatesDetected,
    PersonalDataPresent,
}

/// TSP twin usage rights.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum UsageRight {
    View,
    Simulate,
    Annotate,
    Derive,
    Distribute,
    Commercial,
}

/// TSP twin license types.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LicenseType {
    Exclusive,
    NonExclusive,
    OpenAccess,
    ResearchOnly,
}

pub const F1_QUALITY_GATE: f32 = 0.777;

/// VCP safety levels for capabilities.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum SafetyLevel {
    None,
    Low,
    Standard,
    Elevated,
    Critical,
}
