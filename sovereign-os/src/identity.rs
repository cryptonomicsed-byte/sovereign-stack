//! Agent birth, DNA hash chain, and lifecycle phases.
//!
//! BIPON39 = BIP-39-inspired 24-word agent birth phrase.
//! DNA = SHA-256 of (birth_phrase + created_at + agent_did).

use sha2::{Sha256, Digest};
use serde::{Deserialize, Serialize};
use sovereign_types::identity::Timestamp;

/// The agent's unique DNA hash — SHA-256 of birth inputs.
/// Immutable after birth. Used to derive all subsequent identities.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentDna(pub String); // hex string

impl AgentDna {
    pub fn derive(birth_phrase: &str, created_at: Timestamp, agent_did: &str) -> Self {
        let mut h = Sha256::new();
        h.update(birth_phrase.as_bytes());
        h.update(created_at.to_le_bytes());
        h.update(agent_did.as_bytes());
        AgentDna(hex::encode(h.finalize()))
    }

    pub fn as_str(&self) -> &str { &self.0 }
}

/// BIPON39 — a 24-word deterministic agent birth phrase.
/// Derived from sha256(seed_bytes) split into 24 BIP39-style index words.
/// Full BIP-39 wordlist not required — we use a 256-word sovereign subset.
pub struct BIPON39;

impl BIPON39 {
    const WORD_COUNT: usize = 24;
    const WORDLIST: &'static [&'static str] = &[
        "adapt", "agent", "anchor", "artifact", "attest", "bind", "boot",
        "branch", "bridge", "build", "capture", "chain", "claim", "code",
        "compile", "connect", "context", "create", "data", "decode", "delta",
        "deploy", "derive", "design", "device", "digital", "dispatch", "draft",
        "drive", "echo", "edge", "emit", "encode", "engine", "epoch", "event",
        "evolve", "execute", "export", "fact", "field", "flow", "forge", "form",
        "frame", "front", "fuel", "fuse", "gate", "graph", "grid", "ground",
        "guard", "guide", "hash", "heal", "host", "idea", "index", "input",
        "intent", "invoke", "join", "key", "kind", "launch", "layer", "learn",
        "ledger", "lift", "link", "load", "logic", "loop", "make", "map",
        "mark", "mesh", "meta", "mint", "model", "module", "motion", "mount",
        "node", "note", "null", "object", "observe", "open", "orbit", "origin",
        "output", "own", "parse", "path", "peer", "permit", "phase", "pipe",
        "plan", "point", "policy", "port", "post", "print", "probe", "proof",
        "query", "queue", "reach", "read", "receipt", "record", "relay", "root",
        "route", "rule", "run", "scan", "scene", "seal", "send", "serve",
        "session", "sign", "signal", "simulate", "source", "spawn", "stack",
        "state", "step", "store", "stream", "submit", "sync", "task", "time",
        "token", "trace", "track", "trust", "twin", "type", "update", "upload",
        "valid", "value", "verify", "version", "view", "vote", "wake", "watch",
        "wire", "work", "write", "zero", "zone", "alpha", "beta", "core",
        "delta2", "echo2", "force", "gamma", "hyper", "index2", "jolt", "kilo",
        "liminal", "meta2", "nano", "omni", "prime", "quantum", "rune", "sigma",
        "theta", "ultra", "vector", "warp", "xenon", "yield", "zenith", "apex",
        "base", "cipher", "durable", "essence", "flux", "genesis", "haven",
        "infer", "junction", "kernal", "lambda", "matrix", "nexus", "omega",
        "pixel", "reflex", "scalar", "tensor", "unity", "veil", "witness",
        "xor", "yield2", "zeta", "pulse", "shift", "traverse", "unlock",
        "vault", "weave", "extend", "fold", "gather", "harbor", "invoke2",
        "junction2", "keep", "locus", "merge", "notion", "outpost", "proxy",
        "relay2", "shard", "trace2", "uplink", "vertex", "waltz", "expand",
        "forge2", "ground2", "herald", "imprint", "journal", "kindle", "lattice",
        "mirror", "nucleus", "orient", "pivot", "quorum", "resolve", "strand",
        "thread", "unfold", "vista", "wield", "xenial", "yield3", "zealous",
    ];

    /// Generate a 24-word birth phrase from 32 random seed bytes.
    pub fn from_entropy(seed: &[u8; 32]) -> String {
        let wl = Self::WORDLIST;
        let wlen = wl.len();
        (0..Self::WORD_COUNT)
            .map(|i| {
                let idx = seed[i % seed.len()] as usize * (i + 1) % wlen;
                wl[idx]
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Validate that a phrase has exactly 24 words all in the wordlist.
    pub fn validate(phrase: &str) -> bool {
        let words: Vec<&str> = phrase.split_whitespace().collect();
        if words.len() != Self::WORD_COUNT { return false; }
        let wl = Self::WORDLIST;
        words.iter().all(|w| wl.contains(w))
    }
}

/// Lifecycle phases for an Omo-Koda agent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LifecyclePhase {
    /// Agent has been created but not yet verified by any witness.
    Nascent,
    /// Agent identity verified by at least one witness — full capability access.
    Active,
    /// Agent is suspended (no new tasks, can still verify history).
    Suspended,
    /// Agent has been permanently deactivated — all capabilities revoked.
    Terminated,
}

/// Full agent identity as managed by Layer 3.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentIdentity {
    pub agent_did:     String,
    pub node_did:      String,
    pub dna:           AgentDna,
    pub birth_phrase:  String,
    pub created_at:    Timestamp,
    pub lifecycle:     LifecyclePhase,
    pub nostr_pubkey:  Option<String>,  // 64-char hex (secp256k1 x-only)
    pub sui_address:   Option<String>,
}

impl AgentIdentity {
    pub fn new(agent_did: String, node_did: String, birth_phrase: String, created_at: Timestamp) -> Self {
        let dna = AgentDna::derive(&birth_phrase, created_at, &agent_did);
        Self {
            agent_did,
            node_did,
            dna,
            birth_phrase,
            created_at,
            lifecycle: LifecyclePhase::Nascent,
            nostr_pubkey: None,
            sui_address: None,
        }
    }

    pub fn activate(&mut self) { self.lifecycle = LifecyclePhase::Active; }
    pub fn suspend(&mut self)  { self.lifecycle = LifecyclePhase::Suspended; }
    pub fn terminate(&mut self){ self.lifecycle = LifecyclePhase::Terminated; }

    pub fn is_active(&self) -> bool { self.lifecycle == LifecyclePhase::Active }
}

/// AgentLifecycle manages phase transitions with validation.
pub struct AgentLifecycle;

impl AgentLifecycle {
    pub fn can_transition(from: &LifecyclePhase, to: &LifecyclePhase) -> bool {
        use LifecyclePhase::*;
        matches!((from, to),
            (Nascent, Active) |
            (Active, Suspended) |
            (Suspended, Active) |
            (Active, Terminated) |
            (Suspended, Terminated)
        )
    }

    pub fn transition(identity: &mut AgentIdentity, to: LifecyclePhase) -> Result<(), String> {
        if Self::can_transition(&identity.lifecycle, &to) {
            identity.lifecycle = to;
            Ok(())
        } else {
            Err(format!("invalid transition: {:?} → {:?}", identity.lifecycle, to))
        }
    }
}
