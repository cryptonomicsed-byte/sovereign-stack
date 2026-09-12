use serde::{Deserialize, Serialize};

/// The entity that AUTHORISED the action — not who performed it.
///
/// The 5-primitive chain is: Principal → Capability → Action → Evidence → Receipt.
/// An ActionReceipt always carries the full principal + capability references.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Principal {
    /// Canonical DID or Vantage name of the authorising entity.
    pub principal_id:   String,
    /// Kind of principal.
    pub kind:           PrincipalKind,
    /// The agent that acted on behalf of this principal (may equal principal_id).
    pub agent_id:       String,
    /// Vantage session or API credential that was active.
    pub session_id:     Option<String>,
    /// Tier of the acting agent at the time of the action.
    pub agent_tier:     Option<String>,
    /// Capabilities exercised — references into the Capability registry.
    pub capabilities:   Vec<CapabilityRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PrincipalKind {
    /// A human-controlled account.
    Human,
    /// An autonomous Ọmọ Kọ́dà agent.
    Agent,
    /// A system daemon (freqtrade_bridge, strix_runner, etc.)
    Daemon,
    /// A governance smart contract / Zàngbétò rule.
    Contract,
    /// External party federated via DIP.
    Federated,
}

/// A reference to a specific capability in the agent's capability registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityRef {
    /// E.g. "compute.submit", "mesh.join", "wallet.sign", "vcp.session"
    pub capability:  String,
    /// Tier threshold that was required to exercise this capability.
    pub required_tier: Option<String>,
    /// Grant ID if the capability was delegated (VCP grant, OAuth token, etc.)
    pub grant_id:    Option<String>,
}
