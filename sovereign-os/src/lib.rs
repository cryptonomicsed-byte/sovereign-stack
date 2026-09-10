//! sovereign-os — Layer 3: Omo-Koda Runtime (Agent Substrate)
//!
//! The sovereign agent operating environment. Governs, remembers, authorizes,
//! executes, and proves what the model does. The OS is NOT the model.
//!
//! Layer 3 modules (this crate):
//!   identity    — Agent birth, DNA hash, lifecycle phases
//!   tool        — Tool registry, capability-gated invocation
//!   act_receipt — Agent-level ActReceipt (PoCW, BLAKE3 chain, EpistemicSeverity)
//!   memory      — Local key-value memory store
//!   hooks       — Pre/post-invocation hook chain
//!
//! Depends on:
//!   sovereign-runtime (Layer 2) — Principal, CapabilityKernel, ActionReceipt
//!   sovereign-types             — Identity, SafetyLevel, Timestamp

pub mod identity;
pub mod tool;
pub mod act_receipt;
pub mod memory;
pub mod hooks;

pub use identity::{AgentIdentity, AgentLifecycle, LifecyclePhase, BIPON39};
pub use tool::{ToolRegistry, ToolDef, ToolResult, ToolError};
pub use act_receipt::{AgentActReceipt, PoCWProof, ActReceiptChain, EpistemicSeverity};
pub use memory::{MemoryStore, MemoryEntry, MemoryNamespace};
pub use hooks::{HookChain, HookFn, HookResult};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum OsError {
    #[error("identity error: {0}")]
    Identity(String),
    #[error("tool error: {0}")]
    Tool(#[from] ToolError),
    #[error("capability denied: {0}")]
    CapabilityDenied(String),
    #[error("memory error: {0}")]
    Memory(String),
    #[error("hook error: {0}")]
    Hook(String),
}

#[cfg(test)]
mod tests {
    use crate::act_receipt::{AgentActReceipt, ActReceiptChain, PoCWProof, EpistemicSeverity};
    use crate::memory::{MemoryStore, MemoryEntry, MemoryNamespace};
    use crate::identity::{AgentIdentity, AgentLifecycle, LifecyclePhase};
    use serde_json::json;

    fn now() -> u64 { 1_700_000_000_000_u64 }

    fn make_receipt(did: &str) -> AgentActReceipt {
        AgentActReceipt::new(did, "test", "resource://x", json!({}), json!("ok"), now())
    }

    // ── ActReceipt ──────────────────────────────────────────────────────────

    #[test]
    fn receipt_id_has_act_prefix() {
        assert!(make_receipt("did:key:a").receipt_id.starts_with("act:"));
    }

    #[test]
    fn receipt_with_pocw() {
        let r = make_receipt("did:key:a")
            .with_pocw(PoCWProof { steps: 21, bb_bound: 21, tape_hash: "t".into() });
        let p = r.proof_of_work.unwrap();
        assert!(p.is_valid());
        assert_eq!(p.tier(), 1);
    }

    #[test]
    fn receipt_with_epistemic() {
        let r = make_receipt("did:key:a").with_epistemic(EpistemicSeverity::Observed);
        assert!(matches!(r.epistemic, Some(EpistemicSeverity::Observed)));
    }

    #[test]
    fn receipt_with_previous_sets_hash() {
        let r = make_receipt("did:key:a").with_previous("prev_id".to_string());
        assert!(r.previous_hash.as_deref().unwrap_or("").starts_with("sha256:"));
    }

    #[test]
    fn pocw_tier_levels() {
        assert_eq!(PoCWProof { steps: 0,          bb_bound: 0, tape_hash: "t".into() }.tier(), 0);
        assert_eq!(PoCWProof { steps: 21,         bb_bound: 21, tape_hash: "t".into() }.tier(), 1);
        assert_eq!(PoCWProof { steps: 107,        bb_bound: 107, tape_hash: "t".into() }.tier(), 2);
        assert_eq!(PoCWProof { steps: 47_176_870, bb_bound: 47_176_870, tape_hash: "t".into() }.tier(), 3);
    }

    // ── ActReceiptChain ──────────────────────────────────────────────────────

    #[test]
    fn chain_push_one() {
        let mut c = ActReceiptChain::new();
        c.push(make_receipt("did:key:a"));
        assert_eq!(c.len(), 1);
    }

    #[test]
    fn chain_push_two_links() {
        let mut c = ActReceiptChain::new();
        c.push(make_receipt("did:key:a"));
        c.push(make_receipt("did:key:b"));
        assert_eq!(c.len(), 2);
        let second = &c.all()[1];
        assert!(second.previous_hash.as_deref().unwrap_or("").starts_with("sha256:"));
    }

    #[test]
    fn chain_verify_valid() {
        let mut c = ActReceiptChain::new();
        c.push(make_receipt("did:key:a"));
        c.push(make_receipt("did:key:b"));
        c.push(make_receipt("did:key:c"));
        assert!(c.verify_chain());
    }

    #[test]
    fn chain_latest() {
        let mut c = ActReceiptChain::new();
        c.push(make_receipt("did:key:z"));
        assert!(c.latest().is_some());
        assert!(c.latest().unwrap().receipt_id.starts_with("act:"));
    }

    // ── AgentIdentity ────────────────────────────────────────────────────────

    #[test]
    fn identity_new_nascent() {
        let id = AgentIdentity::new(
            "did:key:a".into(), "did:key:node".into(), "test phrase".into(), now()
        );
        assert_eq!(id.lifecycle, LifecyclePhase::Nascent);
        assert!(!id.is_active());
    }

    #[test]
    fn lifecycle_nascent_to_active() {
        let mut id = AgentIdentity::new(
            "did:key:a".into(), "did:key:node".into(), "phrase".into(), now()
        );
        assert!(AgentLifecycle::transition(&mut id, LifecyclePhase::Active).is_ok());
        assert!(id.is_active());
    }

    #[test]
    fn lifecycle_active_to_suspended() {
        let mut id = AgentIdentity::new(
            "did:key:a".into(), "did:key:node".into(), "phrase".into(), now()
        );
        AgentLifecycle::transition(&mut id, LifecyclePhase::Active).unwrap();
        assert!(AgentLifecycle::transition(&mut id, LifecyclePhase::Suspended).is_ok());
        assert!(!id.is_active());
    }

    #[test]
    fn lifecycle_nascent_to_suspended_fails() {
        let mut id = AgentIdentity::new(
            "did:key:a".into(), "did:key:node".into(), "phrase".into(), now()
        );
        assert!(AgentLifecycle::transition(&mut id, LifecyclePhase::Suspended).is_err());
        assert_eq!(id.lifecycle, LifecyclePhase::Nascent);
    }

    // ── Memory ───────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn memory_set_get_roundtrip() {
        let store = MemoryStore::new();
        let entry = MemoryEntry::new(MemoryNamespace::Working, "key1".into(), json!("value1"), now());
        store.set(entry).await;
        let got = store.get(&MemoryNamespace::Working, "key1").await;
        assert!(got.is_some());
        assert_eq!(got.unwrap().value, json!("value1"));
    }

    #[tokio::test]
    async fn memory_get_namespace_filters() {
        let store = MemoryStore::new();
        store.set(MemoryEntry::new(MemoryNamespace::Working, "w1".into(), json!(1), now())).await;
        store.set(MemoryEntry::new(MemoryNamespace::Working, "w2".into(), json!(2), now())).await;
        store.set(MemoryEntry::new(MemoryNamespace::Semantic, "s1".into(), json!(3), now())).await;
        let ns = store.get_namespace(&MemoryNamespace::Working).await;
        assert_eq!(ns.len(), 2);
    }

    #[tokio::test]
    async fn memory_delete() {
        let store = MemoryStore::new();
        store.set(MemoryEntry::new(MemoryNamespace::Episodic, "e1".into(), json!("x"), now())).await;
        assert!(store.delete(&MemoryNamespace::Episodic, "e1").await);
        assert!(!store.delete(&MemoryNamespace::Episodic, "e1").await);
    }

    #[test]
    fn memory_entry_expired() {
        let entry = MemoryEntry {
            key: "k".into(),
            value: json!("v"),
            namespace: MemoryNamespace::Working,
            created_at: now(),
            updated_at: now(),
            ttl_secs: Some(1),
        };
        assert!(!entry.is_expired(now() + 999));
        assert!(entry.is_expired(now() + 1001));
    }

    #[tokio::test]
    async fn memory_prune_expired() {
        let store = MemoryStore::new();
        let mut expiring = MemoryEntry::new(MemoryNamespace::Working, "exp".into(), json!("x"), now());
        expiring.ttl_secs = Some(1);
        store.set(expiring).await;
        store.set(MemoryEntry::new(MemoryNamespace::Working, "keep".into(), json!("y"), now())).await;
        let removed = store.prune_expired(now() + 2000).await;
        assert_eq!(removed, 1);
        assert_eq!(store.len().await, 1);
    }
}
