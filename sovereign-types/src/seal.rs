//! AccessProvider / Seal trait — capability-gated encryption interface.
//!
//! "Seal" = encrypt content so only capability-holders can decrypt it.
//! Mirrors the Sui Seal primitive but is backend-agnostic: implementations
//! can target Sui Seal, age, or a local symmetric key store.
//!
//! Hermetic rule: private memory ALWAYS goes through AccessProvider.
//! Public state uses StorageProvider directly.

use serde::{Deserialize, Serialize};

/// Opaque encrypted blob (backend chooses encoding — callers treat as bytes).
pub type SealedPayload = Vec<u8>;

/// Identifies a seal policy object on the backing system.
/// For Sui Seal this is the Move object ID; for local this is a UUID.
pub type PolicyObjectId = String;

/// DID of the principal who holds the decryption capability.
pub type PrincipalDid = String;

/// Result type for seal operations.
pub type SealResult<T> = Result<T, SealError>;

#[derive(Debug, thiserror::Error)]
pub enum SealError {
    #[error("access denied: caller {0} does not hold capability for policy {1}")]
    AccessDenied(PrincipalDid, PolicyObjectId),
    #[error("policy {0} not found")]
    PolicyNotFound(PolicyObjectId),
    #[error("backend unavailable: {0}")]
    BackendUnavailable(String),
    #[error("decryption failed: {0}")]
    DecryptionFailed(String),
    #[error("encryption failed: {0}")]
    EncryptionFailed(String),
    #[error("capability expired")]
    CapabilityExpired,
}

/// Access control entry: one principal, one set of allowed operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessEntry {
    pub principal: PrincipalDid,
    pub capabilities: Vec<SealCapability>,
    /// Optional Unix timestamp after which this entry is revoked.
    pub expires_at: Option<u64>,
}

/// Operations a capability-holder may perform on sealed content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SealCapability {
    /// Decrypt the payload.
    Decrypt,
    /// Re-encrypt with a different policy (re-seal).
    Reseal,
    /// Grant the Decrypt capability to a third party.
    Delegate,
    /// Revoke an existing grant (owner-only in practice).
    Revoke,
}

/// The policy object that governs who can access a sealed blob.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SealPolicy {
    pub id: PolicyObjectId,
    /// Agent or principal that created (owns) this policy.
    pub owner: PrincipalDid,
    pub entries: Vec<AccessEntry>,
    pub created_at: u64,
}

impl SealPolicy {
    pub fn can(&self, principal: &str, cap: &SealCapability, now_unix: u64) -> bool {
        self.entries.iter().any(|e| {
            e.principal == principal
                && e.capabilities.contains(cap)
                && e.expires_at.map_or(true, |exp| now_unix < exp)
        })
    }
}

/// Receipt proving a seal or unseal event occurred.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SealReceipt {
    pub policy_id: PolicyObjectId,
    pub operation: SealOperation,
    pub principal: PrincipalDid,
    pub payload_cid: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SealOperation {
    Seal,
    Unseal,
    Reseal,
    DelegateGrant,
    Revoke,
}

/// Abstract interface all seal backends must implement.
pub trait AccessProvider: Send + Sync {
    /// Encrypt `plaintext` under `policy`, returning the sealed blob.
    fn seal(
        &self,
        plaintext: &[u8],
        policy: &SealPolicy,
        caller: &PrincipalDid,
    ) -> SealResult<(SealedPayload, SealReceipt)>;

    /// Decrypt `ciphertext` if `caller` holds Decrypt capability.
    fn unseal(
        &self,
        ciphertext: &SealedPayload,
        policy: &SealPolicy,
        caller: &PrincipalDid,
    ) -> SealResult<(Vec<u8>, SealReceipt)>;

    /// Check whether `caller` holds `capability` at `now`.
    fn check(
        &self,
        policy: &SealPolicy,
        caller: &PrincipalDid,
        capability: &SealCapability,
        now_unix: u64,
    ) -> bool {
        policy.can(caller, capability, now_unix)
    }

    /// Create and persist a new policy, returning its canonical ID.
    fn create_policy(
        &self,
        owner: PrincipalDid,
        initial_entries: Vec<AccessEntry>,
    ) -> SealResult<SealPolicy>;

    /// Look up an existing policy by ID.
    fn get_policy(&self, policy_id: &PolicyObjectId) -> SealResult<SealPolicy>;

    /// Grant an additional access entry to an existing policy.
    fn grant(
        &self,
        policy_id: &PolicyObjectId,
        entry: AccessEntry,
        caller: &PrincipalDid,
    ) -> SealResult<()>;

    /// Revoke all capabilities for `revokee` from a policy.
    fn revoke(
        &self,
        policy_id: &PolicyObjectId,
        revokee: &PrincipalDid,
        caller: &PrincipalDid,
    ) -> SealResult<()>;

    /// Backend name for logging and routing decisions.
    fn backend_name(&self) -> &'static str;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_can_check_expiry() {
        let policy = SealPolicy {
            id: "pol-1".into(),
            owner: "did:p:owner".into(),
            entries: vec![
                AccessEntry {
                    principal: "did:p:alice".into(),
                    capabilities: vec![SealCapability::Decrypt],
                    expires_at: Some(1_000),
                },
            ],
            created_at: 0,
        };
        assert!(policy.can("did:p:alice", &SealCapability::Decrypt, 999));
        assert!(!policy.can("did:p:alice", &SealCapability::Decrypt, 1_000));
        assert!(!policy.can("did:p:bob", &SealCapability::Decrypt, 0));
    }

    #[test]
    fn policy_no_expiry_always_valid() {
        let policy = SealPolicy {
            id: "pol-2".into(),
            owner: "did:p:owner".into(),
            entries: vec![AccessEntry {
                principal: "did:p:carol".into(),
                capabilities: vec![SealCapability::Decrypt, SealCapability::Delegate],
                expires_at: None,
            }],
            created_at: 0,
        };
        assert!(policy.can("did:p:carol", &SealCapability::Delegate, u64::MAX));
    }
}
