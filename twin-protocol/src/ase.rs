//! Àṣẹ spatial economy — receipt-based token minting for Odù tile ownership.
//!
//! Each completed SceneReceipt (kind 31030) can mint Àṣẹ tokens on Sui.
//! Token amount = base_rate × quality_score × novelty_bonus.
//!
//! Tile economy state:
//!   staked_tokens:   tokens locked by the tile owner (earns yield from usage fees)
//!   earned_tokens:   tokens minted from captures in this tile
//!   usage_fee_pct:   percentage of captured value paid to tile owner
//!   owner_did:       DID of the current tile staker (None = unclaimed)

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

// ─── Constants ────────────────────────────────────────────────────────────────

/// Baseline mint: 1 Àṣẹ per capture (in micro-Àṣẹ; 1 Àṣẹ = 1_000_000 micro).
pub const BASE_RATE_MICRO: u64 = 1_000_000;

/// Maximum quality multiplier (3× at quality = 1.0).
pub const MAX_QUALITY_BONUS: f32 = 3.0;

/// Default usage fee percentage charged by the tile owner.
pub const DEFAULT_USAGE_FEE_PCT: f32 = 5.0;

// ─── Types ────────────────────────────────────────────────────────────────────

/// Per-tile economy state for an Odù spatial tile.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TileEconomy {
    pub tile_id:       String,
    pub owner_did:     Option<String>,
    pub staked_tokens: u64,  // units: micro-Àṣẹ (1 Àṣẹ = 1_000_000 micro)
    pub earned_tokens: u64,
    pub usage_fee_pct: f32,  // 0.0–100.0; default 5.0
    pub capture_count: u64,
    pub last_capture:  Option<u64>,  // unix ms
}

/// Request to mint Àṣẹ tokens for a completed SceneReceipt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AseMintRequest {
    pub receipt_id:  String,
    pub twin_id:     String,
    pub tile_id:     String,
    pub minter_did:  String,
    pub quality:     f32,    // 0.0–1.0 F1 quality score
    pub novelty:     f32,    // 0.0–1.0 novelty score
    pub sui_address: String, // recipient's Sui address
}

/// Result of an Àṣẹ mint operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AseMintResult {
    pub receipt_id:    String,
    pub tokens_minted: u64,         // micro-Àṣẹ
    pub owner_fee:     u64,         // portion to tile owner (0 if unclaimed)
    pub tx_digest:     Option<String>, // Sui tx digest, or None for stub
    pub stub:          bool,
}

// ─── Token calculation ────────────────────────────────────────────────────────

/// Calculate the number of micro-Àṣẹ to mint for a given capture.
///
/// Formula:
///   quality_multiplier = 1.0 + (quality × (MAX_QUALITY_BONUS − 1.0))
///   novelty_multiplier = 1.0 + novelty × 0.5
///   tokens             = BASE_RATE_MICRO × quality_multiplier × novelty_multiplier
pub fn calculate_mint_amount(req: &AseMintRequest) -> u64 {
    let base = BASE_RATE_MICRO as f64;
    let quality_multiplier = 1.0_f64 + (req.quality as f64 * (MAX_QUALITY_BONUS - 1.0) as f64);
    let novelty_multiplier = 1.0_f64 + req.novelty as f64 * 0.5_f64;
    (base * quality_multiplier * novelty_multiplier) as u64
}

/// Calculate the tile-owner fee portion for a given token amount.
///
/// `fee_pct` is in the range 0.0–100.0.
pub fn calculate_owner_fee(tokens: u64, fee_pct: f32) -> u64 {
    (tokens as f64 * fee_pct as f64 / 100.0) as u64
}

// ─── Tile economy update ──────────────────────────────────────────────────────

/// Apply a mint result to the tile's economy state.
///
/// - Increments `capture_count`.
/// - Adds `tokens_minted − owner_fee` to `earned_tokens`.
/// - Sets `last_capture` to the current unix millisecond timestamp.
pub fn update_tile_economy(economy: &mut TileEconomy, result: &AseMintResult) {
    economy.capture_count += 1;
    economy.earned_tokens += result.tokens_minted.saturating_sub(result.owner_fee);
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    economy.last_capture = Some(now_ms);
}

// ─── Mint ─────────────────────────────────────────────────────────────────────

/// Mint Àṣẹ tokens for a completed SceneReceipt.
///
/// If `sui_rpc_url` is `Some(url)` and `url` is not `"stub"`, an attempt is
/// made to call the Sui Move contract `{package}::ase::mint`.  Because the
/// package address is not yet configured in this stub implementation the call
/// falls through to the deterministic stub path.
///
/// Returns `stub: true` unless a real Sui transaction was executed.
pub async fn mint_ase(req: &AseMintRequest, sui_rpc_url: Option<&str>) -> AseMintResult {
    let tokens_minted = calculate_mint_amount(req);

    // Determine owner fee — always zero in this stub because we don't have a
    // live TileEconomy reference here; callers should use calculate_owner_fee()
    // when they have the tile context.
    let owner_fee = 0u64;

    // Decide whether to attempt a real Sui call.
    let attempt_real = sui_rpc_url
        .map(|url| !url.is_empty() && url != "stub")
        .unwrap_or(false);

    if attempt_real {
        // Real Sui path: package is not yet deployed — fall through to stub.
        // When the `ase` Move package is published, replace this section with
        // a `SuiRpcClient::new(url).move_call("ase", "mint", …)` invocation.
        tracing::warn!(
            receipt_id = %req.receipt_id,
            "Àṣẹ Sui package not yet configured — using stub mint"
        );
    }

    // Deterministic stub: derive tx_digest from receipt_id (same pattern as
    // sui_rpc::stub_mint).
    let stub_digest = stub_ase_digest(&req.receipt_id);

    AseMintResult {
        receipt_id:    req.receipt_id.clone(),
        tokens_minted,
        owner_fee,
        tx_digest:     Some(stub_digest),
        stub:          true,
    }
}

/// Derive a deterministic stub tx digest from a receipt ID.
///
/// Uses the same hashing approach as `sui_rpc::stub_mint` so that the same
/// receipt always produces the same digest, and different receipts produce
/// different digests.
fn stub_ase_digest(receipt_id: &str) -> String {
    format!("ase_tx:{}", &sovereign_types::hash_str(receipt_id)[7..23])
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_req(receipt_id: &str, quality: f32, novelty: f32) -> AseMintRequest {
        AseMintRequest {
            receipt_id:  receipt_id.to_string(),
            twin_id:     "twin:sha256:test".to_string(),
            tile_id:     "odu:0,0".to_string(),
            minter_did:  "did:vantage:minter".to_string(),
            quality,
            novelty,
            sui_address: "0xSuiAddress".to_string(),
        }
    }

    #[test]
    fn test_calculate_mint_amount_baseline() {
        let req = make_req("receipt-baseline", 0.5, 0.5);
        let tokens = calculate_mint_amount(&req);
        // quality_multiplier = 1.0 + 0.5 * 2.0 = 2.0
        // novelty_multiplier = 1.0 + 0.5 * 0.5 = 1.25
        // expected = 1_000_000 * 2.0 * 1.25 = 2_500_000
        assert!(
            tokens > BASE_RATE_MICRO && tokens < (MAX_QUALITY_BONUS as u64 * BASE_RATE_MICRO * 2),
            "Expected tokens between 1× and 3× BASE_RATE, got {tokens}"
        );
    }

    #[test]
    fn test_perfect_capture_max_tokens() {
        let req = make_req("receipt-perfect", 1.0, 1.0);
        let tokens = calculate_mint_amount(&req);
        // quality_multiplier = 1.0 + 1.0 * 2.0 = 3.0  (= MAX_QUALITY_BONUS)
        // novelty_multiplier = 1.0 + 1.0 * 0.5 = 1.5
        // expected = 1_000_000 * 3.0 * 1.5 = 4_500_000
        let expected = (BASE_RATE_MICRO as f64
            * MAX_QUALITY_BONUS as f64
            * 1.5_f64) as u64;
        assert_eq!(tokens, expected, "Perfect capture should yield MAX_QUALITY_BONUS × 1.5 × BASE");
    }

    #[test]
    fn test_zero_quality_gives_base() {
        let req = make_req("receipt-zero", 0.0, 0.0);
        let tokens = calculate_mint_amount(&req);
        // quality_multiplier = 1.0, novelty_multiplier = 1.0 → exactly BASE
        assert_eq!(
            tokens, BASE_RATE_MICRO,
            "Zero quality+novelty must yield exactly BASE_RATE_MICRO"
        );
    }

    #[test]
    fn test_owner_fee_calculation() {
        let fee = calculate_owner_fee(1_000_000, DEFAULT_USAGE_FEE_PCT);
        assert_eq!(fee, 50_000, "5% of 1_000_000 should be 50_000");
    }

    #[test]
    fn test_update_tile_economy() {
        let mut economy = TileEconomy {
            tile_id:       "odu:3,7".to_string(),
            usage_fee_pct: DEFAULT_USAGE_FEE_PCT,
            ..Default::default()
        };
        let result = AseMintResult {
            receipt_id:    "receipt-update".to_string(),
            tokens_minted: 2_000_000,
            owner_fee:     100_000,
            tx_digest:     Some("ase_tx:stub".to_string()),
            stub:          true,
        };
        update_tile_economy(&mut economy, &result);
        assert_eq!(economy.capture_count, 1);
        assert_eq!(
            economy.earned_tokens,
            2_000_000 - 100_000,
            "earned_tokens should be tokens_minted minus owner_fee"
        );
        assert!(economy.last_capture.is_some());
    }

    #[tokio::test]
    async fn test_mint_stub_is_deterministic_for_same_receipt() {
        let req = make_req("receipt-det-1", 0.7, 0.3);
        let r1 = mint_ase(&req, None).await;
        let r2 = mint_ase(&req, None).await;
        assert_eq!(r1.tx_digest, r2.tx_digest, "Same receipt_id must produce same stub digest");
        assert_eq!(r1.tokens_minted, r2.tokens_minted);
        assert!(r1.stub && r2.stub);
    }

    #[tokio::test]
    async fn test_mint_stub_differs_per_receipt() {
        let req_a = make_req("receipt-A", 0.5, 0.5);
        let req_b = make_req("receipt-B", 0.5, 0.5);
        let ra = mint_ase(&req_a, None).await;
        let rb = mint_ase(&req_b, None).await;
        assert_ne!(
            ra.tx_digest, rb.tx_digest,
            "Different receipt_ids must produce different stub digests"
        );
    }
}
