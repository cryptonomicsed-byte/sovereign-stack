/// Shared Work Protocol v1 — canonical identifiers and receipts that cross
/// all three pillars: Vantage (Society), Omo-Koda2 (Agent), OSOVM (Law).
///
/// SEP-1 (Sovereign Evidence Protocol v1):
///   Every state transition in the ecosystem produces an ActionReceipt.
///   Every tier change produces a TierTransitionReceipt.
///   Every economic settlement produces a SettlementReceipt.
///   Receipts chain via previous_receipt forming an immutable audit spine.
use serde::{Deserialize, Serialize};

// ── WorkID ────────────────────────────────────────────────────────────────────

/// Canonical cross-repo work identifier.
///
/// Format: `wk:{namespace}:{ulid_or_uuid}`
/// - namespace: "sim" | "real" | "hybrid" | "gov" | "license" | "capture" | "scene"
/// - Vantage creates WorkIDs, Omo-Koda2 executes against them,
///   OSOVM verifies them, Sui settles them.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkId(pub String);

impl WorkId {
    pub fn new(namespace: &str, id: &str) -> Self {
        Self(format!("wk:{namespace}:{id}"))
    }

    pub fn namespace(&self) -> Option<&str> {
        let parts: Vec<&str> = self.0.splitn(3, ':').collect();
        if parts.len() == 3 && parts[0] == "wk" { Some(parts[1]) } else { None }
    }

    pub fn as_str(&self) -> &str { &self.0 }

    pub fn is_simulation(&self) -> bool { self.namespace() == Some("sim") }
    pub fn is_real(&self) -> bool { self.namespace() == Some("real") }
    pub fn is_hybrid(&self) -> bool { self.namespace() == Some("hybrid") }
}

impl std::fmt::Display for WorkId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for WorkId {
    fn from(s: String) -> Self { Self(s) }
}

impl From<&str> for WorkId {
    fn from(s: &str) -> Self { Self(s.to_string()) }
}

// ── WorkKind ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkKind {
    Simulation,
    RealWorld,
    Hybrid,
    Governance,
    License,
    Capture,
    Scene,
    Delegation,
    Witness,
}

// ── ActionReceipt ─────────────────────────────────────────────────────────────

/// Universal evidence spine — every consequential state transition produces one.
///
/// Chains are linked via `previous_receipt` forming a tamper-evident log.
/// `work_id` ties the receipt back to the originating WorkID for cross-repo correlation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionReceipt {
    /// BLAKE3 hash of canonical receipt fields (excluding this field).
    pub receipt_id:       String,
    /// The work unit this receipt belongs to.
    pub work_id:          WorkId,
    /// DID of the principal who authorised the work.
    pub principal_id:     String,
    /// Agent DID or pubkey that performed the action.
    pub agent_id:         String,
    /// VCP / TSP / DIP session identifier (optional).
    pub session_id:       Option<String>,
    /// OSOVM or job execution run identifier (optional).
    pub execution_id:     Option<String>,
    /// Human-readable action name ("ScarabSwarm.run", "TSP.capture", etc.).
    pub action:           String,
    /// BLAKE3 of serialised input parameters.
    pub input_hash:       String,
    /// BLAKE3 of serialised output / result.
    pub output_hash:      String,
    /// Optional: BLAKE3 of attached evidence (video, sensor data, …).
    pub evidence_hash:    Option<String>,
    /// receipt_id of the immediately preceding receipt in the chain.
    pub previous_receipt: Option<String>,
    /// Unix timestamp millis.
    pub timestamp_ms:     u64,
    /// ed25519 hex signature by `agent_id` over all above fields.
    pub signature:        String,
}

impl ActionReceipt {
    /// Compute the receipt_id as BLAKE3 over the canonical fields.
    pub fn compute_id(
        work_id: &WorkId,
        principal_id: &str,
        agent_id: &str,
        action: &str,
        input_hash: &str,
        output_hash: &str,
        timestamp_ms: u64,
    ) -> String {
        let mut h = blake3::Hasher::new();
        h.update(work_id.as_str().as_bytes());
        h.update(principal_id.as_bytes());
        h.update(agent_id.as_bytes());
        h.update(action.as_bytes());
        h.update(input_hash.as_bytes());
        h.update(output_hash.as_bytes());
        h.update(&timestamp_ms.to_le_bytes());
        h.finalize().to_hex().to_string()
    }
}

// ── TierTransitionReceipt ─────────────────────────────────────────────────────

/// Produced by the Proof Engine when an agent advances (or regresses) tiers.
/// Must be countersigned by at least 2 independent witnesses before taking effect.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TierTransitionReceipt {
    pub receipt_id:         String,
    pub agent_id:           String,
    pub from_tier:          u8,
    pub to_tier:            u8,
    /// The ActionReceipt that triggered this evaluation.
    pub triggering_receipt: String,
    /// ProofVector snapshot at evaluation time (JSON).
    pub proof_vector_json:  String,
    /// Domain that crossed the threshold that enabled this transition.
    pub qualifying_domain:  String,
    pub timestamp_ms:       u64,
    /// Witness attestations: vec of (witness_id, signature) pairs.
    pub witness_sigs:       Vec<(String, String)>,
    pub agent_signature:    String,
}

impl TierTransitionReceipt {
    pub fn is_promotion(&self) -> bool { self.to_tier > self.from_tier }
    pub fn is_regression(&self) -> bool { self.to_tier < self.from_tier }

    /// Minimum witness count required to finalise a tier transition.
    pub fn required_witnesses(to_tier: u8) -> usize {
        match to_tier {
            0..=2 => 1,
            3 => 2,
            4 => 3,
            _ => 5, // T5 requires 5 independent witnesses
        }
    }

    pub fn is_finalised(&self, to_tier: u8) -> bool {
        self.witness_sigs.len() >= Self::required_witnesses(to_tier)
    }
}

// ── SettlementReceipt ─────────────────────────────────────────────────────────

/// Produced when Àṣẹ tokens are transferred following completed work.
///
/// Separate from the 1440/day emission and from the 3.69% tithe — those
/// are automatic flows. SettlementReceipt covers voluntary work payments.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettlementReceipt {
    pub receipt_id:       String,
    /// The work unit that was compensated.
    pub work_id:          WorkId,
    pub payer_did:        String,
    pub payee_did:        String,
    /// Amount in mist (1 Àṣẹ = 1_000_000_000 mist).
    pub amount_mist:      u64,
    /// Optional tithe component already withheld (mist).
    pub tithe_mist:       Option<u64>,
    /// Sui transaction digest if on-chain.
    pub sui_tx:           Option<String>,
    pub timestamp_ms:     u64,
    pub payer_signature:  String,
}

impl SettlementReceipt {
    /// Tithe rate: 3.69% (369 / 10_000).
    pub const TITHE_NUMERATOR: u64 = 369;
    pub const TITHE_DENOMINATOR: u64 = 10_000;

    pub fn tithe_due(amount_mist: u64) -> u64 {
        amount_mist * Self::TITHE_NUMERATOR / Self::TITHE_DENOMINATOR
    }

    pub fn net_after_tithe(amount_mist: u64) -> u64 {
        amount_mist - Self::tithe_due(amount_mist)
    }
}

// ── Emission Constants ────────────────────────────────────────────────────────
// Canonical source of truth is governance::EMISSION_PER_MINUTE_MIST.
// These re-exports allow work_id consumers to reference emission without
// importing governance directly.

/// 1 Àṣẹ per minute — the master clock rate. OSOVM is sole mint authority.
pub const EMISSION_PER_MINUTE_MIST: u64 = 1_000_000_000;
/// 1440 Àṣẹ per day (1 per minute × 1440 minutes).
pub const DAILY_ASE_EMISSION: u64 = 1_440;
/// Number of Sovereign Wallet seats (= minutes per day — the dual meaning is exact).
pub const SOVEREIGN_WALLET_COUNT: u32 = 1_440;

// ── CapabilityAdvertisement ───────────────────────────────────────────────────

/// Published by an agent on the Blockmesh capability discovery mesh.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityAdvertisement {
    pub agent_id:       String,
    pub tier:           u8,
    /// MIME-style capability strings, e.g. "scarab.sim/v1", "tsp.capture/v1".
    pub capabilities:   Vec<String>,
    pub vcp_endpoint:   Option<String>,
    pub osovm_endpoint: Option<String>,
    /// Peer list for mesh routing.
    pub peers:          Vec<String>,
    pub timestamp_ms:   u64,
    pub signature:      String,
}

// SovereignWallet is defined in governance.rs — see governance::SovereignWallet.
// Re-exported from lib.rs. Seats do NOT mint; the global clock mints.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_id_namespace() {
        let wid = WorkId::new("sim", "test-ulid-001");
        assert_eq!(wid.namespace(), Some("sim"));
        assert!(wid.is_simulation());
        assert!(!wid.is_real());
        assert_eq!(wid.as_str(), "wk:sim:test-ulid-001");
    }

    #[test]
    fn work_id_from_str() {
        let wid: WorkId = "wk:real:job-42".into();
        assert!(wid.is_real());
        assert!(!wid.is_simulation());
    }

    #[test]
    fn action_receipt_compute_id_deterministic() {
        let wid = WorkId::new("sim", "u001");
        let id1 = ActionReceipt::compute_id(&wid, "did:p", "did:a", "run", "ih", "oh", 1_000);
        let id2 = ActionReceipt::compute_id(&wid, "did:p", "did:a", "run", "ih", "oh", 1_000);
        assert_eq!(id1, id2);
    }

    #[test]
    fn action_receipt_id_changes_with_timestamp() {
        let wid = WorkId::new("sim", "u001");
        let id1 = ActionReceipt::compute_id(&wid, "p", "a", "run", "i", "o", 1_000);
        let id2 = ActionReceipt::compute_id(&wid, "p", "a", "run", "i", "o", 2_000);
        assert_ne!(id1, id2);
    }

    #[test]
    fn tier_transition_required_witnesses() {
        assert_eq!(TierTransitionReceipt::required_witnesses(1), 1);
        assert_eq!(TierTransitionReceipt::required_witnesses(3), 2);
        assert_eq!(TierTransitionReceipt::required_witnesses(4), 3);
        assert_eq!(TierTransitionReceipt::required_witnesses(5), 5);
    }

    #[test]
    fn tier_transition_not_finalised_without_sigs() {
        let r = TierTransitionReceipt {
            receipt_id:         "r".into(),
            agent_id:           "a".into(),
            from_tier:          2,
            to_tier:            3,
            triggering_receipt: "t".into(),
            proof_vector_json:  "{}".into(),
            qualifying_domain:  "simulation".into(),
            timestamp_ms:       0,
            witness_sigs:       vec![],
            agent_signature:    "s".into(),
        };
        assert!(!r.is_finalised(3));
        // 2 witnesses → finalised
        let r2 = TierTransitionReceipt { witness_sigs: vec![
            ("w1".into(), "s1".into()),
            ("w2".into(), "s2".into()),
        ], ..r };
        assert!(r2.is_finalised(3));
    }

    #[test]
    fn settlement_tithe_calculation() {
        // 1_000_000 mist payment → 36_900 mist tithe (3.69%)
        let payment: u64 = 1_000_000;
        let tithe = SettlementReceipt::tithe_due(payment);
        assert_eq!(tithe, 36_900); // 1_000_000 * 369 / 10_000 = 36_900
        let net = SettlementReceipt::net_after_tithe(payment);
        assert_eq!(net, payment - tithe);
    }

    #[test]
    fn emission_per_minute_times_day_equals_daily() {
        // 1 Àṣẹ/min × 1440 min/day = 1440 Àṣẹ/day.
        let daily_from_minute = EMISSION_PER_MINUTE_MIST * DAILY_ASE_EMISSION;
        assert_eq!(daily_from_minute, 1_440 * 1_000_000_000);
    }

    #[test]
    fn sovereign_seat_count_matches_daily_minutes() {
        assert_eq!(SOVEREIGN_WALLET_COUNT, 1_440);
    }

    #[test]
    fn capability_advertisement_fields() {
        let ad = CapabilityAdvertisement {
            agent_id:       "did:key:a".into(),
            tier:           3,
            capabilities:   vec!["scarab.sim/v1".into(), "tsp.capture/v1".into()],
            vcp_endpoint:   Some("ws://localhost:9090".into()),
            osovm_endpoint: Some("http://localhost:7780".into()),
            peers:          vec![],
            timestamp_ms:   1_000,
            signature:      "sig".into(),
        };
        assert_eq!(ad.capabilities.len(), 2);
        assert_eq!(ad.tier, 3);
    }
}
