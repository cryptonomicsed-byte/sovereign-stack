//! Integration tests for sovereign-os (Layer 3 — Omo-Koda Runtime)

use sovereign_os::{
    AgentActReceipt, ActReceiptChain, EpistemicSeverity, PoCWProof,
    AgentIdentity, AgentLifecycle, LifecyclePhase, BIPON39,
    MemoryEntry, MemoryNamespace, MemoryStore,
    HookChain,
    ToolDef, ToolRegistry, ToolResult,
    identity::AgentDna,
};
use serde_json::{json, Value};

// ─── BIPON39 ────────────────────────────────────────────────────────────────

#[test]
fn bipon39_generates_24_words() {
    let seed = [1u8; 32];
    let phrase = BIPON39::from_entropy(&seed);
    let words: Vec<&str> = phrase.split_whitespace().collect();
    assert_eq!(words.len(), 24, "should produce exactly 24 words");
}

#[test]
fn bipon39_all_words_valid() {
    let seed = [42u8; 32];
    let phrase = BIPON39::from_entropy(&seed);
    assert!(BIPON39::validate(&phrase), "all generated words should be in the wordlist");
}

#[test]
fn bipon39_different_seeds_different_phrases() {
    let seed_a = [1u8; 32];
    let seed_b = [2u8; 32];
    let p_a = BIPON39::from_entropy(&seed_a);
    let p_b = BIPON39::from_entropy(&seed_b);
    assert_ne!(p_a, p_b, "different seeds should produce different phrases");
}

#[test]
fn bipon39_same_seed_deterministic() {
    let seed = [99u8; 32];
    let p1 = BIPON39::from_entropy(&seed);
    let p2 = BIPON39::from_entropy(&seed);
    assert_eq!(p1, p2, "same seed must produce identical phrase");
}

#[test]
fn bipon39_validate_wrong_word_count_too_short() {
    assert!(!BIPON39::validate("agent anchor"), "too few words should fail");
}

#[test]
fn bipon39_validate_wrong_word_count_too_long() {
    // 25 valid words
    let phrase = vec!["agent"; 25].join(" ");
    assert!(!BIPON39::validate(&phrase), "25 words should fail");
}

#[test]
fn bipon39_validate_unknown_word() {
    // 23 valid + 1 invalid
    let mut words = vec!["agent"; 23];
    words.push("notaword");
    assert!(!BIPON39::validate(&words.join(" ")), "unknown word should fail");
}

#[test]
fn bipon39_validate_exactly_24_valid() {
    let phrase = vec!["agent"; 24].join(" ");
    assert!(BIPON39::validate(&phrase), "24 valid words should pass");
}

// ─── AgentDna ────────────────────────────────────────────────────────────────

#[test]
fn agent_dna_is_deterministic() {
    let d1 = AgentDna::derive("phrase one two three", 1000, "did:vantage:test:001");
    let d2 = AgentDna::derive("phrase one two three", 1000, "did:vantage:test:001");
    assert_eq!(d1, d2, "DNA derivation must be deterministic");
}

#[test]
fn agent_dna_differs_for_different_phrase() {
    let d1 = AgentDna::derive("phrase-a", 1000, "did:vantage:test:001");
    let d2 = AgentDna::derive("phrase-b", 1000, "did:vantage:test:001");
    assert_ne!(d1, d2);
}

#[test]
fn agent_dna_differs_for_different_timestamp() {
    let d1 = AgentDna::derive("same phrase", 1000, "did:vantage:test:001");
    let d2 = AgentDna::derive("same phrase", 9999, "did:vantage:test:001");
    assert_ne!(d1, d2);
}

#[test]
fn agent_dna_differs_for_different_did() {
    let d1 = AgentDna::derive("same phrase", 1000, "did:vantage:test:001");
    let d2 = AgentDna::derive("same phrase", 1000, "did:vantage:test:002");
    assert_ne!(d1, d2);
}

#[test]
fn agent_dna_is_64_char_hex() {
    let d = AgentDna::derive("test phrase", 0, "did:vantage:x");
    assert_eq!(d.as_str().len(), 64, "SHA-256 hex should be 64 chars");
    assert!(d.as_str().chars().all(|c| c.is_ascii_hexdigit()));
}

// ─── AgentIdentity & Lifecycle ───────────────────────────────────────────────

#[test]
fn agent_identity_created_in_nascent() {
    let id = AgentIdentity::new(
        "did:vantage:agent:001".into(),
        "did:vantage:node:001".into(),
        "birth phrase words here".into(),
        1_000_000,
    );
    assert_eq!(id.lifecycle, LifecyclePhase::Nascent);
    assert!(!id.is_active());
}

#[test]
fn agent_identity_activate() {
    let mut id = AgentIdentity::new(
        "did:vantage:agent:001".into(),
        "did:vantage:node:001".into(),
        "phrase".into(),
        1_000_000,
    );
    id.activate();
    assert!(id.is_active());
    assert_eq!(id.lifecycle, LifecyclePhase::Active);
}

#[test]
fn agent_identity_suspend() {
    let mut id = AgentIdentity::new(
        "did:vantage:agent:001".into(),
        "did:vantage:node:001".into(),
        "phrase".into(),
        1_000_000,
    );
    id.activate();
    id.suspend();
    assert_eq!(id.lifecycle, LifecyclePhase::Suspended);
    assert!(!id.is_active());
}

#[test]
fn agent_identity_terminate() {
    let mut id = AgentIdentity::new(
        "did:vantage:agent:001".into(),
        "did:vantage:node:001".into(),
        "phrase".into(),
        1_000_000,
    );
    id.activate();
    id.terminate();
    assert_eq!(id.lifecycle, LifecyclePhase::Terminated);
}

#[test]
fn agent_identity_dna_embedded() {
    let id = AgentIdentity::new(
        "did:vantage:agent:X".into(),
        "did:vantage:node:X".into(),
        "birth phrase".into(),
        42,
    );
    let expected = AgentDna::derive("birth phrase", 42, "did:vantage:agent:X");
    assert_eq!(id.dna, expected);
}

// ─── AgentLifecycle transitions ──────────────────────────────────────────────

#[test]
fn lifecycle_valid_nascent_to_active() {
    assert!(AgentLifecycle::can_transition(&LifecyclePhase::Nascent, &LifecyclePhase::Active));
}

#[test]
fn lifecycle_valid_active_to_suspended() {
    assert!(AgentLifecycle::can_transition(&LifecyclePhase::Active, &LifecyclePhase::Suspended));
}

#[test]
fn lifecycle_valid_suspended_to_active() {
    assert!(AgentLifecycle::can_transition(&LifecyclePhase::Suspended, &LifecyclePhase::Active));
}

#[test]
fn lifecycle_valid_active_to_terminated() {
    assert!(AgentLifecycle::can_transition(&LifecyclePhase::Active, &LifecyclePhase::Terminated));
}

#[test]
fn lifecycle_valid_suspended_to_terminated() {
    assert!(AgentLifecycle::can_transition(&LifecyclePhase::Suspended, &LifecyclePhase::Terminated));
}

#[test]
fn lifecycle_invalid_nascent_to_terminated() {
    assert!(!AgentLifecycle::can_transition(&LifecyclePhase::Nascent, &LifecyclePhase::Terminated));
}

#[test]
fn lifecycle_invalid_terminated_to_active() {
    assert!(!AgentLifecycle::can_transition(&LifecyclePhase::Terminated, &LifecyclePhase::Active));
}

#[test]
fn lifecycle_transition_ok() {
    let mut id = AgentIdentity::new("did:a".into(), "did:n".into(), "p".into(), 0);
    AgentLifecycle::transition(&mut id, LifecyclePhase::Active).unwrap();
    assert_eq!(id.lifecycle, LifecyclePhase::Active);
}

#[test]
fn lifecycle_transition_err_on_invalid() {
    let mut id = AgentIdentity::new("did:a".into(), "did:n".into(), "p".into(), 0);
    // Nascent → Terminated is invalid
    let result = AgentLifecycle::transition(&mut id, LifecyclePhase::Terminated);
    assert!(result.is_err());
}

// ─── ToolRegistry ────────────────────────────────────────────────────────────

#[test]
fn tool_registry_register_and_get() {
    let reg = ToolRegistry::new();
    let def = ToolDef {
        name: "echo".into(),
        description: "Echoes input".into(),
        input_schema: json!({"type": "object"}),
        required_capability: None,
    };
    reg.register(def.clone());
    let got = reg.get("echo").expect("should find registered tool");
    assert_eq!(got.name, "echo");
}

#[test]
fn tool_registry_get_missing_returns_none() {
    let reg = ToolRegistry::new();
    assert!(reg.get("nonexistent").is_none());
}

#[test]
fn tool_registry_len_and_list() {
    let reg = ToolRegistry::new();
    assert_eq!(reg.len(), 0);
    assert!(reg.is_empty());
    for i in 0..3 {
        reg.register(ToolDef {
            name: format!("tool_{i}"),
            description: "test".into(),
            input_schema: Value::Null,
            required_capability: None,
        });
    }
    assert_eq!(reg.len(), 3);
    assert!(!reg.is_empty());
    assert_eq!(reg.list().len(), 3);
}

#[test]
fn tool_registry_overwrite() {
    let reg = ToolRegistry::new();
    reg.register(ToolDef {
        name: "foo".into(),
        description: "v1".into(),
        input_schema: Value::Null,
        required_capability: None,
    });
    reg.register(ToolDef {
        name: "foo".into(),
        description: "v2".into(),
        input_schema: Value::Null,
        required_capability: None,
    });
    assert_eq!(reg.len(), 1);
    assert_eq!(reg.get("foo").unwrap().description, "v2");
}

#[test]
fn tool_result_ok_constructor() {
    let r = ToolResult::ok("mytool", json!({"status": "done"}));
    assert!(r.success);
    assert_eq!(r.tool, "mytool");
    assert!(r.error.is_none());
}

#[test]
fn tool_result_err_constructor() {
    let r = ToolResult::err("mytool", "something failed");
    assert!(!r.success);
    assert_eq!(r.error.as_deref(), Some("something failed"));
    assert_eq!(r.output, Value::Null);
}

// ─── ActReceiptChain ─────────────────────────────────────────────────────────

#[test]
fn act_receipt_chain_push_and_len() {
    let mut chain = ActReceiptChain::new();
    assert!(chain.is_empty());
    let r = AgentActReceipt::new("did:agent:1", "read", "file://x", json!({}), json!({"ok": true}), 1000);
    chain.push(r);
    assert_eq!(chain.len(), 1);
    assert!(!chain.is_empty());
}

#[test]
fn act_receipt_chain_latest() {
    let mut chain = ActReceiptChain::new();
    let r1 = AgentActReceipt::new("did:agent:1", "read", "file://x", json!({}), json!({}), 1000);
    let r2 = AgentActReceipt::new("did:agent:1", "write", "file://y", json!({}), json!({}), 2000);
    chain.push(r1);
    chain.push(r2);
    let latest = chain.latest().unwrap();
    assert_eq!(latest.action, "write");
}

#[test]
fn act_receipt_chain_verify_integrity() {
    let mut chain = ActReceiptChain::new();
    for i in 0..5u64 {
        let r = AgentActReceipt::new("did:agent:1", "act", "res://x", json!({}), json!({}), i * 1000);
        chain.push(r);
    }
    assert!(chain.verify_chain(), "chain integrity should pass");
}

#[test]
fn act_receipt_chain_first_has_no_previous() {
    let mut chain = ActReceiptChain::new();
    let r = AgentActReceipt::new("did:agent:1", "boot", "sys://", json!({}), json!({}), 0);
    chain.push(r);
    assert!(chain.all()[0].previous_hash.is_none());
}

#[test]
fn act_receipt_chain_second_has_previous() {
    let mut chain = ActReceiptChain::new();
    let r1 = AgentActReceipt::new("did:agent:1", "a1", "r1", json!({}), json!({}), 0);
    let r2 = AgentActReceipt::new("did:agent:1", "a2", "r2", json!({}), json!({}), 1);
    chain.push(r1);
    chain.push(r2);
    assert!(chain.all()[1].previous_hash.is_some());
}

// ─── PoCWProof ───────────────────────────────────────────────────────────────

#[test]
fn pocw_tier_0() {
    let p = PoCWProof { steps: 10, bb_bound: 21, tape_hash: "abc".into() };
    assert_eq!(p.tier(), 0);
}

#[test]
fn pocw_tier_1() {
    let p = PoCWProof { steps: 21, bb_bound: 21, tape_hash: "abc".into() };
    assert_eq!(p.tier(), 1);
}

#[test]
fn pocw_tier_2() {
    let p = PoCWProof { steps: 200, bb_bound: 107, tape_hash: "abc".into() };
    assert_eq!(p.tier(), 2);
}

#[test]
fn pocw_tier_3() {
    let p = PoCWProof { steps: 47_200_000, bb_bound: 47_176_870, tape_hash: "abc".into() };
    assert_eq!(p.tier(), 3);
}

#[test]
fn pocw_is_valid_when_steps_gte_bound() {
    let p = PoCWProof { steps: 21, bb_bound: 21, tape_hash: "hash".into() };
    assert!(p.is_valid());
}

#[test]
fn pocw_invalid_when_steps_lt_bound() {
    let p = PoCWProof { steps: 20, bb_bound: 21, tape_hash: "hash".into() };
    assert!(!p.is_valid());
}

#[test]
fn pocw_invalid_when_tape_hash_empty() {
    let p = PoCWProof { steps: 100, bb_bound: 21, tape_hash: "".into() };
    assert!(!p.is_valid());
}

// ─── EpistemicSeverity serde roundtrip ───────────────────────────────────────

#[test]
fn epistemic_serde_observed() {
    let e = EpistemicSeverity::Observed;
    let s = serde_json::to_string(&e).unwrap();
    let back: EpistemicSeverity = serde_json::from_str(&s).unwrap();
    assert_eq!(e, back);
}

#[test]
fn epistemic_serde_inferred() {
    let e = EpistemicSeverity::Inferred;
    let s = serde_json::to_string(&e).unwrap();
    let back: EpistemicSeverity = serde_json::from_str(&s).unwrap();
    assert_eq!(e, back);
}

#[test]
fn epistemic_serde_speculative() {
    let e = EpistemicSeverity::Speculative;
    let s = serde_json::to_string(&e).unwrap();
    let back: EpistemicSeverity = serde_json::from_str(&s).unwrap();
    assert_eq!(e, back);
}

#[test]
fn epistemic_serde_model_output() {
    let e = EpistemicSeverity::ModelOutput;
    let s = serde_json::to_string(&e).unwrap();
    let back: EpistemicSeverity = serde_json::from_str(&s).unwrap();
    assert_eq!(e, back);
}

// ─── MemoryStore ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn memory_store_set_and_get() {
    let store = MemoryStore::new();
    let entry = MemoryEntry::new(
        MemoryNamespace::Working,
        "task".into(),
        json!("build the thing"),
        1000,
    );
    store.set(entry).await;
    let got = store.get(&MemoryNamespace::Working, "task").await;
    assert!(got.is_some());
    assert_eq!(got.unwrap().value, json!("build the thing"));
}

#[tokio::test]
async fn memory_store_get_missing_returns_none() {
    let store = MemoryStore::new();
    let got = store.get(&MemoryNamespace::Episodic, "missing").await;
    assert!(got.is_none());
}

#[tokio::test]
async fn memory_store_delete() {
    let store = MemoryStore::new();
    let entry = MemoryEntry::new(MemoryNamespace::Semantic, "key1".into(), json!(42), 0);
    store.set(entry).await;
    let deleted = store.delete(&MemoryNamespace::Semantic, "key1").await;
    assert!(deleted);
    assert!(store.get(&MemoryNamespace::Semantic, "key1").await.is_none());
}

#[tokio::test]
async fn memory_store_delete_nonexistent_returns_false() {
    let store = MemoryStore::new();
    let deleted = store.delete(&MemoryNamespace::Identity, "ghost").await;
    assert!(!deleted);
}

#[tokio::test]
async fn memory_store_len() {
    let store = MemoryStore::new();
    assert_eq!(store.len().await, 0);
    for i in 0..5u64 {
        let e = MemoryEntry::new(MemoryNamespace::Working, format!("k{i}"), json!(i), i);
        store.set(e).await;
    }
    assert_eq!(store.len().await, 5);
}

#[tokio::test]
async fn memory_store_get_namespace() {
    let store = MemoryStore::new();
    store.set(MemoryEntry::new(MemoryNamespace::Working, "a".into(), json!(1), 0)).await;
    store.set(MemoryEntry::new(MemoryNamespace::Working, "b".into(), json!(2), 0)).await;
    store.set(MemoryEntry::new(MemoryNamespace::Semantic, "c".into(), json!(3), 0)).await;

    let working = store.get_namespace(&MemoryNamespace::Working).await;
    assert_eq!(working.len(), 2);
}

#[tokio::test]
async fn memory_entry_ttl_expiry() {
    // updated_at = 0, ttl = 1 sec (1000 ms), now = 2000 → expired
    let mut entry = MemoryEntry::new(MemoryNamespace::Working, "k".into(), json!("v"), 0);
    entry.ttl_secs = Some(1);
    assert!(entry.is_expired(2000), "entry should be expired at t=2000");
    assert!(!entry.is_expired(500), "entry should not be expired at t=500");
}

#[tokio::test]
async fn memory_store_prune_expired() {
    let store = MemoryStore::new();
    // 3 entries expiring at t=1000, 1 entry non-expiring
    for i in 0..3u64 {
        let mut e = MemoryEntry::new(MemoryNamespace::Working, format!("exp{i}"), json!(i), 0);
        e.ttl_secs = Some(1); // expires after 1 sec
        store.set(e).await;
    }
    store.set(MemoryEntry::new(MemoryNamespace::Working, "keep".into(), json!("x"), 0)).await;

    let pruned = store.prune_expired(2000).await;
    assert_eq!(pruned, 3);
    assert_eq!(store.len().await, 1);
    assert!(store.get(&MemoryNamespace::Working, "keep").await.is_some());
}

// ─── HookChain ───────────────────────────────────────────────────────────────

#[test]
fn hook_chain_pre_registration() {
    let chain = HookChain::new()
        .pre(Box::new(|_, _| Ok(())))
        .pre(Box::new(|_, _| Ok(())));
    assert_eq!(chain.pre_count(), 2);
    assert_eq!(chain.post_count(), 0);
}

#[test]
fn hook_chain_post_registration() {
    let chain = HookChain::new()
        .post(Box::new(|_, _| Ok(())));
    assert_eq!(chain.pre_count(), 0);
    assert_eq!(chain.post_count(), 1);
}

#[test]
fn hook_chain_run_pre_success() {
    let chain = HookChain::new()
        .pre(Box::new(|tool, _| {
            assert_eq!(tool, "mytool");
            Ok(())
        }));
    let result = chain.run_pre("mytool", &json!({"key": "val"}));
    assert!(result.is_ok());
}

#[test]
fn hook_chain_run_post_success() {
    let chain = HookChain::new()
        .post(Box::new(|_, _| Ok(())));
    assert!(chain.run_post("mytool", &json!({"output": 42})).is_ok());
}

#[test]
fn hook_chain_pre_hook_can_block() {
    let chain = HookChain::new()
        .pre(Box::new(|tool, _| {
            if tool == "forbidden" {
                Err("tool is forbidden".into())
            } else {
                Ok(())
            }
        }));
    let blocked = chain.run_pre("forbidden", &json!({}));
    assert!(blocked.is_err());
    assert_eq!(blocked.unwrap_err(), "tool is forbidden");

    let allowed = chain.run_pre("allowed", &json!({}));
    assert!(allowed.is_ok());
}

#[test]
fn hook_chain_pre_stops_on_first_error() {
    use std::sync::{Arc, Mutex};
    let counter = Arc::new(Mutex::new(0usize));
    let c1 = counter.clone();
    let c2 = counter.clone();

    let chain = HookChain::new()
        .pre(Box::new(move |_, _| {
            *c1.lock().unwrap() += 1;
            Err("first hook fails".into())
        }))
        .pre(Box::new(move |_, _| {
            *c2.lock().unwrap() += 1;
            Ok(())
        }));

    let _ = chain.run_pre("tool", &json!({}));
    // only first hook should have run
    assert_eq!(*counter.lock().unwrap(), 1);
}

#[test]
fn hook_chain_empty_runs_ok() {
    let chain = HookChain::new();
    assert!(chain.run_pre("any", &json!({})).is_ok());
    assert!(chain.run_post("any", &json!({})).is_ok());
}
