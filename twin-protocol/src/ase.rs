//! Àṣẹ token economy — mint engine and Éṣù-Elegbára routing.
//!
//! ═══ CANONICAL ARCHITECTURE (LOCKED 2026-09-10) ═══
//!
//! THREE DISTINCT FLOWS — do NOT conflate:
//!
//! 1. DAILY EMISSION (1,440 Àṣẹ/day, FIXED FOREVER — no halving):
//!    OSOVM mints 1 Àṣẹ/minute → main Éṣù wallet
//!    → Éṣù-Elegbára router distributes to 8 sub-wallets
//!    Each minute's token: allocated by F9 score to winning valid sim
//!    No valid sim that minute → splits to 1,440 inheritance wallets
//!    Annual supply: 525,600 Àṣẹ/year, forever.
//!    STATUS: SPEC (orchestrator not yet built)
//!
//! 2. UNIVERSAL TRANSACTION TITHE (every flow, always, 3.69% — LOCKED):
//!    Every mint, payment, settlement → 3.69% Éṣù tithe
//!    → same Éṣù-Elegbára router → same 8 sub-wallets
//!    Do NOT change to 7.77%. AIO context only.
//!    STATUS: BUILT — elegbara_router.move (2/2 tests pass)
//!
//!    Elegbára 8 sub-wallets (basis points, sum = 10,000):
//!      VeilSim       30%  · R&D           20%  · Governance  10%
//!      Reserve       10%  · Lottery       10%  · Grants      10%
//!      UBI            5%  · Sabbath Rsv    5%
//!
//! 3. 24-SECTOR FUNDING SPLIT (sector inflows only — donations/investments/hardware):
//!    50% Treasury · 25% Inheritance · 15% Council · 10% Executor
//!    Applies when a sector wallet (6 houses × 4 categories) receives inflow.
//!    NOT applied to daily emission or per-transaction tithe.
//!    STATUS: SPEC (24-sector treasuries not yet built)
//!
//! INVARIANT COMMONS (in every split path):
//!    3.69% Éṣù tithe  +  11.11% inheritance fraction

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

// ─── Constants ────────────────────────────────────────────────────────────────

/// Baseline mint: 1 Àṣẹ per capture (in micro-Àṣẹ; 1 Àṣẹ = 1_000_000 micro).
/// NOTE: This is a per-receipt dev approximation.
/// Canonical spec uses 1,440 Àṣẹ/day allocated by F9 score (Open Decision C above).
pub const BASE_RATE_MICRO: u64 = 1_000_000;

/// Maximum quality multiplier (3× at quality = 1.0).
pub const MAX_QUALITY_BONUS: f32 = 3.0;

/// Default usage fee percentage charged by the tile owner.
pub const DEFAULT_USAGE_FEE_PCT: f32 = 5.0;

/// Éṣù tithe rate: 3.69% — LOCKED by owner. Do NOT change to 7.77%.
/// Applied to every mint and settlement. Routes entirely through Éṣù-Elegbára router.
pub const ESHU_TITHE_BPS: u64 = 369; // basis points

/// Daily emission: 1,440 Àṣẹ/day (one per minute) — FIXED FOREVER, no halving.
/// Annual supply: 525,600 Àṣẹ/year.
pub const DAILY_EMISSION_MICRO: u64 = 1_440 * 1_000_000;

/// 24-sector funding split ratios: [treasury, inheritance, council, executor]
/// Applies ONLY to 24-sector wallet inflows (donations, investments, hardware fees, embodiment).
/// Does NOT apply to daily emission or per-transaction tithe.
pub const SECTOR_SPLIT: [u64; 4] = [50, 25, 15, 10];

/// Manumission threshold: agent earns autonomy at 100 USDC.
pub const MANUMISSION_TARGET_USDC: u64 = 100;

// ─── Éṣù-Elegbára Router (8 sub-wallets) ─────────────────────────────────────
//
// From elegbara_router.move (215 lines, 2/2 tests pass, generic over Coin<T>).
// The router NEVER mints — it only routes USDC/stablecoin flows.
// Basis points sum to 10,000.

pub const ELEGBARA_VEILSIM_BPS:        u64 = 3_000; // 30%
pub const ELEGBARA_RD_BPS:             u64 = 2_000; // 20%
pub const ELEGBARA_GOVERNANCE_BPS:     u64 = 1_000; // 10%
pub const ELEGBARA_RESERVE_BPS:        u64 = 1_000; // 10%
pub const ELEGBARA_LOTTERY_BPS:        u64 = 1_000; // 10%
pub const ELEGBARA_GRANTS_BPS:         u64 = 1_000; // 10%
pub const ELEGBARA_UBI_BPS:            u64 =   500; //  5%
// Sabbath Reserve = remainder to avoid rounding dust
pub const ELEGBARA_SABBATH_RESERVE_BPS: u64 = 500; //  5%

/// Result of routing the 3.69% tithe through the Éṣù-Elegbára router.
/// Maps to the 8 isolated sub-wallets in elegbara_router.move.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ElegbaraRouterSplit {
    pub tithe_total:     u64,  // 3.69% of original amount (= sum of all buckets)
    pub veilsim:         u64,  // 30% → VeilSim execution budget
    pub r_and_d:         u64,  // 20% → Research & development
    pub governance:      u64,  // 10% → On-chain governance
    pub reserve:         u64,  // 10% → Emergency reserve
    pub lottery:         u64,  // 10% → Lottery prize pool
    pub grants:          u64,  // 10% → Community grants
    pub ubi:             u64,  //  5% → Universal basic income pool
    pub sabbath_reserve: u64,  //  5% → Sabbath / rounding dust reserve
}

/// Compute the Éṣù-Elegbára split for a given tithe amount.
/// `tithe_amount` should already be 3.69% of the original flow.
pub fn elegbara_route(tithe_amount: u64) -> ElegbaraRouterSplit {
    let veilsim         = tithe_amount * ELEGBARA_VEILSIM_BPS        / 10_000;
    let r_and_d         = tithe_amount * ELEGBARA_RD_BPS             / 10_000;
    let governance      = tithe_amount * ELEGBARA_GOVERNANCE_BPS     / 10_000;
    let reserve         = tithe_amount * ELEGBARA_RESERVE_BPS        / 10_000;
    let lottery         = tithe_amount * ELEGBARA_LOTTERY_BPS        / 10_000;
    let grants          = tithe_amount * ELEGBARA_GRANTS_BPS         / 10_000;
    let ubi             = tithe_amount * ELEGBARA_UBI_BPS            / 10_000;
    let distributed     = veilsim + r_and_d + governance + reserve
                        + lottery + grants + ubi;
    let sabbath_reserve = tithe_amount.saturating_sub(distributed);
    ElegbaraRouterSplit {
        tithe_total: tithe_amount,
        veilsim, r_and_d, governance, reserve, lottery, grants, ubi, sabbath_reserve,
    }
}

/// Result of the 24-sector funding split (50/25/15/10).
/// Applied to sector wallet inflows — NOT to daily emission or tithe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectorFundingSplit {
    pub total:       u64,
    pub treasury:    u64,  // 50% — work rewards pool
    pub inheritance: u64,  // 25% — 1,440 inheritance wallets
    pub council:     u64,  // 15% — 13-member council
    pub executor:    u64,  // 10% — embodied executor / tail
}

/// Apply the 24-sector funding split (50/25/15/10) to a sector inflow amount.
/// Use this for donations, investments, hardware fees, and embodiment offerings.
/// Do NOT use for daily emission or per-transaction tithe — those go through Elegbára.
pub fn sector_funding_split(amount: u64) -> SectorFundingSplit {
    let treasury    = amount * SECTOR_SPLIT[0] / 100;
    let inheritance = amount * SECTOR_SPLIT[1] / 100;
    let council     = amount * SECTOR_SPLIT[2] / 100;
    let executor    = amount.saturating_sub(treasury + inheritance + council);
    SectorFundingSplit { total: amount, treasury, inheritance, council, executor }
}

/// Compute the 3.69% tithe on an amount and return the full Elegbára split.
/// Returns (net_after_tithe, ElegbaraRouterSplit).
pub fn eshu_tithe(amount_micro: u64) -> (u64, ElegbaraRouterSplit) {
    let tithe = amount_micro * ESHU_TITHE_BPS / 10_000;
    let net   = amount_micro.saturating_sub(tithe);
    (net, elegbara_route(tithe))
}

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

/// Request to mint Àṣẹ tokens for a completed proof or capture receipt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AseMintRequest {
    pub receipt_id:  String,
    pub twin_id:     String,
    pub tile_id:     String,
    pub minter_did:  String,
    pub quality:     f32,    // 0.0–1.0 (F9/F1 quality score)
    pub novelty:     f32,    // 0.0–1.0 novelty score
    pub sui_address: String, // recipient's Sui address
}

/// Result of an Àṣẹ mint operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AseMintResult {
    pub receipt_id:    String,
    pub tokens_minted: u64,               // gross micro-Àṣẹ (pre-tithe)
    pub net_minted:    u64,               // micro-Àṣẹ after 3.69% Éṣù tithe
    pub owner_fee:     u64,               // portion to tile owner (0 if unclaimed)
    pub elegbara:      ElegbaraRouterSplit, // full 8-bucket routing of the tithe
    pub tx_digest:     Option<String>,    // Sui tx digest, or None for stub
    pub stub:          bool,
}

// ─── Token calculation ────────────────────────────────────────────────────────

/// Calculate the number of micro-Àṣẹ to mint for a given proof/capture.
///
/// This is the dev approximation for Open Decision C (per-receipt vs per-minute).
/// Canonical spec: each minute's 1 Àṣẹ allocates to the highest-F9 sim.
///
/// Formula:
///   quality_multiplier = 1.0 + (quality × (MAX_QUALITY_BONUS − 1.0))
///   novelty_multiplier = 1.0 + novelty × 0.5
///   tokens             = BASE_RATE_MICRO × quality_multiplier × novelty_multiplier
pub fn calculate_mint_amount(req: &AseMintRequest) -> u64 {
    let base               = BASE_RATE_MICRO as f64;
    let quality_multiplier = 1.0 + req.quality as f64 * (MAX_QUALITY_BONUS - 1.0) as f64;
    let novelty_multiplier = 1.0 + req.novelty as f64 * 0.5;
    (base * quality_multiplier * novelty_multiplier) as u64
}

/// Calculate the tile-owner fee portion for a given token amount.
/// `fee_pct` is in the range 0.0–100.0.
pub fn calculate_owner_fee(tokens: u64, fee_pct: f32) -> u64 {
    (tokens as f64 * fee_pct as f64 / 100.0) as u64
}

// ─── Tile economy update ──────────────────────────────────────────────────────

/// Apply a mint result to the tile's economy state.
pub fn update_tile_economy(economy: &mut TileEconomy, result: &AseMintResult) {
    economy.capture_count += 1;
    economy.earned_tokens += result.net_minted.saturating_sub(result.owner_fee);
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    economy.last_capture = Some(now_ms);
}

// ─── Mint ─────────────────────────────────────────────────────────────────────

/// Mint Àṣẹ tokens for a completed SceneReceipt or proof.
///
/// Applies the 3.69% Éṣù tithe and routes it through the Elegbára 8-bucket split.
/// Tile owner fee is drawn from net_minted (post-tithe).
pub async fn mint_ase(
    req:         &AseMintRequest,
    sui_rpc_url: Option<&str>,
    tile:        Option<&TileEconomy>,
) -> AseMintResult {
    let tokens_minted = calculate_mint_amount(req);
    let (net_minted, elegbara) = eshu_tithe(tokens_minted);

    let owner_fee = tile
        .filter(|t| t.owner_did.is_some())
        .map(|t| calculate_owner_fee(net_minted, t.usage_fee_pct))
        .unwrap_or(0);

    let attempt_real = sui_rpc_url
        .map(|url| !url.is_empty() && url != "stub")
        .unwrap_or(false);

    if attempt_real {
        // Real Sui path: ase.move package not yet deployed.
        // Replace with SuiRpcClient::new(url).move_call("ase", "mint", …) when live.
        tracing::warn!(receipt_id = %req.receipt_id, "Àṣẹ Sui package not configured — stub mint");
    }

    AseMintResult {
        receipt_id: req.receipt_id.clone(),
        tokens_minted,
        net_minted,
        owner_fee,
        elegbara,
        tx_digest: Some(stub_ase_digest(&req.receipt_id)),
        stub:      true,
    }
}

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
            tile_id:     "odu:00".to_string(),
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
        assert!(tokens > BASE_RATE_MICRO);
        assert!(tokens < MAX_QUALITY_BONUS as u64 * BASE_RATE_MICRO * 2);
    }

    #[test]
    fn test_perfect_capture_max_tokens() {
        let req = make_req("receipt-perfect", 1.0, 1.0);
        let tokens = calculate_mint_amount(&req);
        // 1_000_000 × 3.0 × 1.5 = 4_500_000
        let expected = (BASE_RATE_MICRO as f64 * MAX_QUALITY_BONUS as f64 * 1.5) as u64;
        assert_eq!(tokens, expected);
    }

    #[test]
    fn test_zero_quality_gives_base() {
        let req = make_req("receipt-zero", 0.0, 0.0);
        assert_eq!(calculate_mint_amount(&req), BASE_RATE_MICRO);
    }

    #[test]
    fn test_owner_fee_calculation() {
        assert_eq!(calculate_owner_fee(1_000_000, DEFAULT_USAGE_FEE_PCT), 50_000);
    }

    #[test]
    fn test_update_tile_economy() {
        let mut economy = TileEconomy {
            tile_id: "odu:37".to_string(), usage_fee_pct: DEFAULT_USAGE_FEE_PCT,
            ..Default::default()
        };
        let result = AseMintResult {
            receipt_id:    "receipt-update".to_string(),
            tokens_minted: 2_000_000,
            net_minted:    1_926_200,
            owner_fee:     96_310,
            elegbara:      ElegbaraRouterSplit::default(),
            tx_digest:     Some("ase_tx:stub".to_string()),
            stub:          true,
        };
        update_tile_economy(&mut economy, &result);
        assert_eq!(economy.capture_count, 1);
        assert_eq!(economy.earned_tokens, result.net_minted - result.owner_fee);
        assert!(economy.last_capture.is_some());
    }

    #[tokio::test]
    async fn test_mint_stub_is_deterministic_for_same_receipt() {
        let req = make_req("receipt-det-1", 0.7, 0.3);
        let r1 = mint_ase(&req, None, None).await;
        let r2 = mint_ase(&req, None, None).await;
        assert_eq!(r1.tx_digest, r2.tx_digest);
        assert_eq!(r1.tokens_minted, r2.tokens_minted);
        assert!(r1.stub && r2.stub);
    }

    #[tokio::test]
    async fn test_mint_stub_differs_per_receipt() {
        let ra = mint_ase(&make_req("receipt-A", 0.5, 0.5), None, None).await;
        let rb = mint_ase(&make_req("receipt-B", 0.5, 0.5), None, None).await;
        assert_ne!(ra.tx_digest, rb.tx_digest);
    }

    #[tokio::test]
    async fn test_mint_applies_eshu_tithe() {
        let result = mint_ase(&make_req("receipt-tithe", 1.0, 1.0), None, None).await;
        let expected_tithe = result.tokens_minted * ESHU_TITHE_BPS / 10_000;
        assert_eq!(result.elegbara.tithe_total, expected_tithe);
        assert_eq!(result.net_minted, result.tokens_minted - expected_tithe);
    }

    #[tokio::test]
    async fn test_mint_with_claimed_tile_charges_owner_fee() {
        let tile = TileEconomy {
            tile_id: "odu:00".into(), owner_did: Some("did:vantage:owner:1".into()),
            usage_fee_pct: DEFAULT_USAGE_FEE_PCT, ..Default::default()
        };
        let result = mint_ase(&make_req("receipt-owner", 1.0, 1.0), None, Some(&tile)).await;
        assert!(result.owner_fee > 0);
        assert_eq!(result.owner_fee, calculate_owner_fee(result.net_minted, DEFAULT_USAGE_FEE_PCT));
    }

    #[test]
    fn test_elegbara_route_sums_to_tithe() {
        // All 8 buckets must sum to the tithe amount (no dust lost)
        let split = elegbara_route(10_000);
        let total = split.veilsim + split.r_and_d + split.governance + split.reserve
                  + split.lottery + split.grants + split.ubi + split.sabbath_reserve;
        assert_eq!(total, split.tithe_total);
    }

    #[test]
    fn test_elegbara_route_bucket_proportions() {
        let split = elegbara_route(10_000);
        assert_eq!(split.veilsim,      3_000, "VeilSim 30%");
        assert_eq!(split.r_and_d,      2_000, "R&D 20%");
        assert_eq!(split.governance,   1_000, "Governance 10%");
        assert_eq!(split.reserve,      1_000, "Reserve 10%");
        assert_eq!(split.lottery,      1_000, "Lottery 10%");
        assert_eq!(split.grants,       1_000, "Grants 10%");
        assert_eq!(split.ubi,            500, "UBI 5%");
        assert_eq!(split.sabbath_reserve, 500, "Sabbath Reserve 5%");
    }

    #[test]
    fn test_eshu_tithe_369() {
        let (net, split) = eshu_tithe(10_000);
        assert_eq!(split.tithe_total, 369, "3.69% of 10_000");
        assert_eq!(net, 10_000 - 369,      "net after tithe");
        assert_eq!(split.lottery,      36, "10% of tithe → lottery");
        assert_eq!(split.veilsim,      110, "~30% of tithe → VeilSim"); // 369*30/100 = 110
    }

    #[test]
    fn test_daily_emission_constant() {
        assert_eq!(DAILY_EMISSION_MICRO, 1_440_000_000, "1440 Àṣẹ × 1_000_000 micro");
        // Annual: 1440 × 365 = 525,600 Àṣẹ/year, fixed forever
        assert_eq!(DAILY_EMISSION_MICRO * 365, 525_600 * 1_000_000);
    }

    #[test]
    fn test_sector_funding_split_proportions() {
        let split = sector_funding_split(100_000_000);
        assert_eq!(split.treasury,    50_000_000, "50% treasury");
        assert_eq!(split.inheritance, 25_000_000, "25% inheritance");
        assert_eq!(split.council,     15_000_000, "15% council");
        assert_eq!(split.executor,    10_000_000, "10% executor");
        assert_eq!(
            split.treasury + split.inheritance + split.council + split.executor,
            split.total
        );
    }

    #[test]
    fn test_daily_emission_elegbara_routing() {
        // 1440 Àṣẹ/day routes through Elegbára: first take tithe, then route
        let (net, elegbara) = eshu_tithe(DAILY_EMISSION_MICRO);
        assert!(net < DAILY_EMISSION_MICRO);
        // VeilSim gets 30% of the tithe — biggest single bucket
        assert!(elegbara.veilsim > elegbara.r_and_d);
        assert_eq!(elegbara.tithe_total,
            DAILY_EMISSION_MICRO * ESHU_TITHE_BPS / 10_000);
    }
}
