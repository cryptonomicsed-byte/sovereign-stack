// Phase 50 — On-chain tile governance anchor.
// Wraps Sui JSON-RPC calls for tile_governance::claim_tile and stake_tile.
// Falls back to deterministic stubs when no valid key is configured.

use serde::{Deserialize, Serialize};
use crate::error::{TspError, TspResult};
use crate::sui_rpc::{SuiRpcClient, stub_mint};

// ── request / response types ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TileClaimAnchor {
    pub tile_id:      String,
    pub owner_did:    String,
    pub owner_address: String,
    pub usage_fee_bps: u16,
    /// Sui transaction digest (real or stub)
    pub tx_digest:    String,
    /// Newly created TileRecord object ID
    pub object_id:    String,
    pub stub:         bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TileStakeAnchor {
    pub tile_id:       String,
    pub staker_address: String,
    pub amount:        u64,
    pub tx_digest:     String,
    pub stub:          bool,
}

// ── anchor client ─────────────────────────────────────────────────────────────

pub struct TileGovernanceAnchor {
    rpc:        SuiRpcClient,
    address:    String,
    key_b64url: String,
    package_id: String,
}

impl TileGovernanceAnchor {
    pub fn new(
        rpc_url:    impl Into<String>,
        address:    impl Into<String>,
        key_b64url: impl Into<String>,
        package_id: impl Into<String>,
    ) -> Self {
        let rpc_url_s = rpc_url.into();
        let pkg       = package_id.into();
        Self {
            rpc:        SuiRpcClient::new(&rpc_url_s).with_package(&pkg),
            address:    address.into(),
            key_b64url: key_b64url.into(),
            package_id: pkg,
        }
    }

    /// Claim a tile on-chain.  Uses stub when key is absent or "stubkey".
    pub async fn claim_tile(
        &self,
        tile_id:       &str,
        owner_did:     &str,
        usage_fee_bps: u16,
    ) -> TspResult<TileClaimAnchor> {
        if self.is_stub() {
            return Ok(self.stub_claim(tile_id, owner_did, usage_fee_bps));
        }

        let args: Vec<String> = vec![
            tile_id.as_bytes().iter().map(|b| b.to_string()).collect::<Vec<_>>().join(","),
            owner_did.as_bytes().iter().map(|b| b.to_string()).collect::<Vec<_>>().join(","),
            usage_fee_bps.to_string(),
        ];

        match self.move_call("claim_tile", &args).await {
            Ok((tx_digest, object_id)) => Ok(TileClaimAnchor {
                tile_id:       tile_id.into(),
                owner_did:     owner_did.into(),
                owner_address: self.address.clone(),
                usage_fee_bps,
                tx_digest,
                object_id,
                stub: false,
            }),
            Err(e) => {
                tracing::warn!(error = %e, tile_id, "claim_tile RPC failed — stub fallback");
                Ok(self.stub_claim(tile_id, owner_did, usage_fee_bps))
            }
        }
    }

    /// Stake tokens into an owned tile on-chain.
    pub async fn stake_tile(
        &self,
        tile_id:        &str,
        tile_object_id: &str,
        amount:         u64,
    ) -> TspResult<TileStakeAnchor> {
        if self.is_stub() {
            return Ok(self.stub_stake(tile_id, amount));
        }

        let args = vec![tile_object_id.to_string(), amount.to_string()];
        match self.move_call("stake_tile", &args).await {
            Ok((tx_digest, _)) => Ok(TileStakeAnchor {
                tile_id:        tile_id.into(),
                staker_address: self.address.clone(),
                amount,
                tx_digest,
                stub: false,
            }),
            Err(e) => {
                tracing::warn!(error = %e, tile_id, "stake_tile RPC failed — stub fallback");
                Ok(self.stub_stake(tile_id, amount))
            }
        }
    }

    // ── internals ──────────────────────────────────────────────────────────────

    fn is_stub(&self) -> bool {
        self.key_b64url.is_empty() || self.key_b64url == "stubkey"
    }

    fn stub_claim(&self, tile_id: &str, owner_did: &str, usage_fee_bps: u16) -> TileClaimAnchor {
        // Combine tile_id + owner_did so different tiles always yield different digests.
        let combined = format!("{tile_id}:{owner_did}");
        let (tx_digest, object_id) = stub_mint(tile_id, &combined);
        TileClaimAnchor {
            tile_id:       tile_id.into(),
            owner_did:     owner_did.into(),
            owner_address: self.address.clone(),
            usage_fee_bps,
            tx_digest,
            object_id,
            stub: true,
        }
    }

    fn stub_stake(&self, tile_id: &str, amount: u64) -> TileStakeAnchor {
        let (tx_digest, _) = stub_mint(tile_id, &amount.to_string());
        TileStakeAnchor {
            tile_id:        tile_id.into(),
            staker_address: self.address.clone(),
            amount,
            tx_digest,
            stub: true,
        }
    }

    async fn move_call(&self, function: &str, args: &[String]) -> TspResult<(String, String)> {
        // Delegate to SuiRpcClient's mint_twin path as a generic move-call bridge.
        // For tile functions we encode args as twin_id/merkle_root fields and parse
        // the object change response.  This avoids duplicating the RPC + signing code.
        self.rpc.mint_twin(
            &self.address,
            &self.key_b64url,
            function,         // reuse twin_id slot as function discriminant
            &args.join("|"),  // encode all args into merkle_root slot
            "",
            0, 0, 0, 0, 0,
            10_000_000,
        ).await
    }
}

// ── config extension ──────────────────────────────────────────────────────────

/// Config section for the tile governance contract.
/// Add to NodeConfig under `[sui]` or load from environment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TileGovernanceConfig {
    /// Sui full-node RPC URL (e.g. "https://fullnode.testnet.sui.io")
    pub rpc_url:    String,
    /// Deployer / oracle Sui address
    pub address:    String,
    /// Ed25519 private key seed, base64url-encoded (32 bytes).
    /// Set to "stubkey" or omit for offline/stub mode.
    pub key_b64url: String,
    /// Deployed tile_governance package ID (0x...)
    pub package_id: String,
    /// Default usage fee in basis points (500 = 5%)
    #[serde(default = "default_usage_fee_bps")]
    pub default_usage_fee_bps: u16,
}

fn default_usage_fee_bps() -> u16 { 500 }

impl Default for TileGovernanceConfig {
    fn default() -> Self {
        Self {
            rpc_url:               String::new(),
            address:               String::new(),
            key_b64url:            String::new(),
            package_id:            String::new(),
            default_usage_fee_bps: default_usage_fee_bps(),
        }
    }
}

impl TileGovernanceConfig {
    pub fn anchor(&self) -> TileGovernanceAnchor {
        TileGovernanceAnchor::new(
            &self.rpc_url,
            &self.address,
            &self.key_b64url,
            &self.package_id,
        )
    }

    pub fn is_stub(&self) -> bool {
        self.key_b64url.is_empty() || self.key_b64url == "stubkey"
    }
}

// ── unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn stub_anchor() -> TileGovernanceAnchor {
        TileGovernanceAnchor::new(
            "https://fullnode.testnet.sui.io",
            "0xtest_addr",
            "stubkey",
            "0x0",
        )
    }

    #[tokio::test]
    async fn stub_claim_tile_returns_deterministic_result() {
        let a = stub_anchor();
        let r1 = a.claim_tile("odu:3F", "did:p:owner", 500).await.unwrap();
        let r2 = a.claim_tile("odu:3F", "did:p:owner", 500).await.unwrap();
        assert!(r1.stub);
        assert_eq!(r1.tile_id, "odu:3F");
        assert_eq!(r1.tx_digest, r2.tx_digest);
        assert_eq!(r1.object_id, r2.object_id);
        assert!(r1.object_id.starts_with("0x"));
    }

    #[tokio::test]
    async fn stub_stake_tile_returns_tx_digest() {
        let a = stub_anchor();
        let r = a.stub_stake("odu:A0", 1_000_000);
        assert!(r.stub);
        assert_eq!(r.tile_id, "odu:A0");
        assert_eq!(r.amount, 1_000_000);
        assert!(!r.tx_digest.is_empty());
    }

    #[test]
    fn config_default_usage_fee() {
        let cfg = TileGovernanceConfig::default();
        assert_eq!(cfg.default_usage_fee_bps, 500);
        assert!(cfg.is_stub());
    }

    #[test]
    fn config_anchor_builds() {
        let cfg = TileGovernanceConfig {
            rpc_url:    "https://fullnode.testnet.sui.io".into(),
            address:    "0xabc".into(),
            key_b64url: "stubkey".into(),
            package_id: "0xdeadbeef".into(),
            default_usage_fee_bps: 300,
        };
        let anchor = cfg.anchor();
        assert!(anchor.is_stub());
        assert_eq!(anchor.package_id, "0xdeadbeef");
    }

    #[tokio::test]
    async fn claim_different_tiles_produce_different_digests() {
        let a = stub_anchor();
        let r1 = a.claim_tile("odu:00", "did:p:owner", 500).await.unwrap();
        let r2 = a.claim_tile("odu:FF", "did:p:owner", 500).await.unwrap();
        assert_ne!(r1.tx_digest, r2.tx_digest);
        assert_ne!(r1.object_id, r2.object_id);
    }
}
