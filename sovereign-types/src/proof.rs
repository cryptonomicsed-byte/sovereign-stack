//! Shared proof primitives — the types that must be identical across layers.
//!
//! These live in `sovereign-types` (the bottom of the dependency graph) because
//! both `sovereign-runtime` (L2) and `sovereign-os` (L3) need them, and
//! `sovereign-os` depends on `sovereign-runtime` — so a shared type cannot live
//! in either without creating a cycle or a duplicate.
//!
//! Prior to this module both crates defined their own `PoCWProof` and
//! `EpistemicSeverity`. The `PoCWProof` copies had identical fields but
//! different methods; the `EpistemicSeverity` copies were **different concepts
//! under the same name**, which is worse — a serialized `epistemic_severity`
//! field meant two incompatible things depending on which crate wrote it.
//!
//! Resolution (per `OSOVM_CANONICAL_ARCHITECTURE.md` receipt convergence table):
//!   * `epistemic_severity` is the Omo-Koda concept → `Confidence`
//!     (Observed / Inferred / Speculative / ModelOutput).
//!   * the multi-model ensemble disagreement signal → `EnsembleDisagreement`
//!     (Unanimous / Strong / Moderate / Severe), a **separate field**.

use serde::{Deserialize, Serialize};

// ── Hashing ───────────────────────────────────────────────────────────────────
//
// Canonical contract (CANONICAL_PILLAR_CONTRACT.md, SEP-1):
//   receipt_id  — BLAKE3 of canonical fields
//   input_hash  — BLAKE3 of serialised input
//   output_hash — BLAKE3 of serialised output
//
// Before this module, `sovereign-runtime` and `sovereign-os` hashed their
// receipt chains with SHA-256 while their doc comments claimed BLAKE3 — the
// `blake3` crate was only a dependency of `sovereign-types`. These helpers make
// the primitive explicit and single-sourced.

/// Canonical hash prefix for content-addressed values.
pub const HASH_PREFIX: &str = "blake3:";

/// BLAKE3 over `bytes`, returned as `blake3:<hex>` — the canonical form.
pub fn blake3_hex(bytes: &[u8]) -> String {
    format!("{}{}", HASH_PREFIX, blake3::hash(bytes).to_hex())
}

/// BLAKE3 over `bytes`, returned as bare lowercase hex (no prefix).
///
/// Used where a bare digest is required — notably `ActionReceipt::receipt_id`,
/// which SEP-1 specifies as a bare BLAKE3 hex string.
pub fn blake3_hex_raw(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

/// BLAKE3 over a `serde`-serialisable value.
///
/// Falls back to hashing the `Display` of `serde_json::Error` if serialisation
/// fails, so a malformed value still yields a deterministic (if unhelpful)
/// digest rather than a panic. Callers that need to detect that should
/// serialise themselves.
pub fn blake3_of_json<T: Serialize + ?Sized>(value: &T) -> String {
    match serde_json::to_vec(value) {
        Ok(bytes) => blake3_hex(&bytes),
        Err(e) => blake3_hex(format!("<unserialisable:{e}>").as_bytes()),
    }
}

// ── Proof of Cognitive Work ───────────────────────────────────────────────────

/// Proof of Cognitive Work — Busy Beaver grounded computational proof.
///
/// The claim is: this many Turing Machine steps were actually executed, which
/// is at least the Busy Beaver bound for the claimed tier, and the tape at halt
/// is committed by hash so the claim is checkable without re-running.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PoCWProof {
    /// Turing Machine steps actually executed (>= `bb_bound` for a valid proof).
    pub steps: u64,
    /// BB bound claimed: 1, 6, 21, 107, or 47_176_870.
    pub bb_bound: u64,
    /// Content hash of the TM tape at halt (see [`blake3_hex`]).
    pub tape_hash: String,
}

impl PoCWProof {
    pub fn new(steps: u64, bb_bound: u64, tape_hash: impl Into<String>) -> Self {
        Self { steps, bb_bound, tape_hash: tape_hash.into() }
    }

    /// A proof is valid iff the executed steps meet the claimed bound and the
    /// tape was actually committed.
    pub fn is_valid(&self) -> bool {
        self.steps >= self.bb_bound && !self.tape_hash.is_empty()
    }

    /// Minimum steps required to support the given act tier.
    pub fn min_for_tier(tier: u8) -> u64 {
        match tier {
            0 => 0,
            1 => 21,           // BB(3)
            2 => 107,          // BB(4)
            _ => 47_176_870,   // BB(5)
        }
    }

    /// The tier this proof's step count actually supports (0..=3).
    pub fn tier(&self) -> u8 {
        if self.steps >= 47_176_870 {
            3
        } else if self.steps >= 107 {
            2
        } else if self.steps >= 21 {
            1
        } else {
            0
        }
    }
}

// ── Epistemic severity (Omo-Koda lineage) ─────────────────────────────────────

/// How strongly the agent believes the claim it is making.
///
/// This is the canonical `epistemic_severity` field. It answers
/// *"on what basis does the agent assert this?"* — NOT *"how much did a
/// multi-model ensemble disagree?"* (that is [`EnsembleDisagreement`]).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// Agent observed the thing directly — highest confidence.
    Observed,
    /// Agent inferred it from evidence — moderate confidence.
    Inferred,
    /// Agent speculated or extrapolated — low confidence.
    Speculative,
    /// Agent is relaying model output; epistemic trust is deferred to the model.
    ModelOutput,
}

impl Confidence {
    /// Ordinal strength for comparisons (higher = stronger basis).
    pub fn strength(&self) -> u8 {
        match self {
            Confidence::Observed => 3,
            Confidence::Inferred => 2,
            Confidence::ModelOutput => 1,
            Confidence::Speculative => 0,
        }
    }

    /// Whether this claim basis is strong enough to carry an economic action.
    pub fn is_economically_assertable(&self) -> bool {
        self.strength() >= Confidence::Inferred.strength()
    }
}

// ── Ensemble disagreement (wisdom-ensemble lineage) ───────────────────────────

/// How much a multi-model wisdom ensemble disagreed on this action.
///
/// Formerly named `EpistemicSeverity` in `sovereign-runtime`, which collided
/// with the Omo-Koda concept above. Kept as a distinct field because it answers
/// a different question and both signals are genuinely wanted.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EnsembleDisagreement {
    /// All models agreed.
    Unanimous,
    /// Broad agreement with minor divergence.
    Strong,
    /// Meaningful split.
    Moderate,
    /// Models substantially disagreed — treat the output with suspicion.
    Severe,
}

impl EnsembleDisagreement {
    /// Whether the disagreement level should block an economic action.
    pub fn is_blocking(&self) -> bool {
        matches!(self, EnsembleDisagreement::Severe)
    }
}

// ── Sim-to-real plane ─────────────────────────────────────────────────────────

/// Sim-to-real verification plane — from `omokoda-hermetic`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SimPlane {
    /// Unverified claim — the default. Must be upgraded by a real verify call.
    #[default]
    Physical,
    Digital,
    Hybrid,
    Verified,
}

impl SimPlane {
    /// Only `Verified` may be used to support a physical-world proof claim.
    pub fn is_verified(&self) -> bool {
        matches!(self, SimPlane::Verified)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blake3_hex_is_prefixed_and_stable() {
        let a = blake3_hex(b"hello");
        let b = blake3_hex(b"hello");
        assert_eq!(a, b);
        assert!(a.starts_with("blake3:"));
        // BLAKE3 of "hello" — well-known vector.
        assert_eq!(
            blake3_hex_raw(b"hello"),
            "ea8f163db38682925e4491c5e58d4bb3506ef8c14eb78a86e908c5624a67200f"
        );
    }

    #[test]
    fn blake3_raw_has_no_prefix() {
        let raw = blake3_hex_raw(b"x");
        assert!(!raw.starts_with("blake3:"));
        assert_eq!(raw.len(), 64);
    }

    #[test]
    fn blake3_of_json_is_stable_and_order_sensitive() {
        let a = blake3_of_json(&serde_json::json!({"a": 1, "b": 2}));
        let b = blake3_of_json(&serde_json::json!({"a": 1, "b": 2}));
        let c = blake3_of_json(&serde_json::json!({"a": 1, "b": 3}));
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn pocw_valid_only_when_steps_meet_bound_and_tape_present() {
        assert!(PoCWProof::new(21, 21, "h").is_valid());
        assert!(!PoCWProof::new(20, 21, "h").is_valid());
        assert!(!PoCWProof::new(21, 21, "").is_valid());
    }

    #[test]
    fn pocw_min_for_tier_matches_bb_table() {
        assert_eq!(PoCWProof::min_for_tier(0), 0);
        assert_eq!(PoCWProof::min_for_tier(1), 21);
        assert_eq!(PoCWProof::min_for_tier(2), 107);
        assert_eq!(PoCWProof::min_for_tier(3), 47_176_870);
    }

    #[test]
    fn pocw_tier_derives_from_steps_consistent_with_min_for_tier() {
        // The two directions must agree: min_for_tier(t) <= steps < min_for_tier(t+1)
        // implies tier() == t.
        for tier in 0u8..=3 {
            let steps = PoCWProof::min_for_tier(tier);
            let p = PoCWProof::new(steps, steps, "h");
            assert_eq!(p.tier(), tier, "tier mismatch at steps={steps}");
        }
    }

    #[test]
    fn confidence_strength_ordering() {
        assert!(Confidence::Observed.strength() > Confidence::Inferred.strength());
        assert!(Confidence::Inferred.strength() > Confidence::ModelOutput.strength());
        assert!(Confidence::ModelOutput.strength() > Confidence::Speculative.strength());
    }

    #[test]
    fn speculative_and_model_output_are_not_economically_assertable() {
        assert!(Confidence::Observed.is_economically_assertable());
        assert!(Confidence::Inferred.is_economically_assertable());
        assert!(!Confidence::Speculative.is_economically_assertable());
        assert!(!Confidence::ModelOutput.is_economically_assertable());
    }

    #[test]
    fn only_severe_disagreement_is_blocking() {
        assert!(EnsembleDisagreement::Severe.is_blocking());
        assert!(!EnsembleDisagreement::Moderate.is_blocking());
        assert!(!EnsembleDisagreement::Strong.is_blocking());
        assert!(!EnsembleDisagreement::Unanimous.is_blocking());
    }

    #[test]
    fn plane_defaults_to_unverified_physical() {
        assert_eq!(SimPlane::default(), SimPlane::Physical);
        assert!(!SimPlane::default().is_verified());
        assert!(SimPlane::Verified.is_verified());
    }

    #[test]
    fn confidence_and_ensemble_disagreement_are_distinct_types() {
        // Regression guard for the original name collision: a serialised
        // epistemic_severity must never deserialise into the ensemble concept.
        let c = serde_json::to_string(&Confidence::Observed).unwrap();
        assert_eq!(c, "\"observed\"");
        let e = serde_json::to_string(&EnsembleDisagreement::Severe).unwrap();
        assert_eq!(e, "\"severe\"");
        // "observed" is not a valid EnsembleDisagreement — proves they are
        // genuinely disjoint, not aliases.
        assert!(serde_json::from_str::<EnsembleDisagreement>("\"observed\"").is_err());
        assert!(serde_json::from_str::<Confidence>("\"severe\"").is_err());
    }
}
