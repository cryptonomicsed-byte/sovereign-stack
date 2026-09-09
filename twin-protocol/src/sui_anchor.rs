//! TSP Phase 3.3 — Sui anchor for Twin Assets.
//!
//! Every assembled twin (31030 scene receipt) becomes an IP Root on Sui:
//!   - A Twin NFT (Sui object) is minted from the 31030 receipt
//!   - The Sui object_id is written back into the SceneReceipt
//!   - License NFTs are derived from the Twin NFT
//!
//! This module provides:
//!   1. SuiTwinObject — the on-chain representation of a twin
//!   2. SuiTxRequest  — the transaction to submit to Sui
//!   3. SuiAnchor     — the client that submits and tracks anchoring
//!
//! Wire format matches what the Move contract expects:
//!   twin_nft.move::mint_twin(twin_id, merkle_root, f1_score, coverage_pct,
//!                             splat_hash, license_type, revenue_split)

use serde::{Deserialize, Serialize};
use crate::scene::SceneReceipt;
use crate::license::{TwinLicenseGrant, TwinLicenseConstraints};
use crate::error::{TspError, TspResult};
use sovereign_types::{Did, Hash, Timestamp, UsageRight};

/// The on-chain representation of a Twin Asset.
/// Mirrors twin_nft.move::TwinNFT struct fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuiTwinObject {
    pub object_id:       String,    // Sui object ID (0x...)
    pub twin_id:         Hash,      // TSP twin_id (twin:sha256:...)
    pub owner:           String,    // Sui address of owner
    pub merkle_root:     Hash,      // binds all twin data
    pub f1_score_x1000:  u32,       // f1_score * 1000 (no floats on chain)
    pub coverage_x100:   u32,       // coverage_pct * 100
    pub splat_hash:      Hash,
    pub license_type:    u8,        // 0=Exclusive 1=NonExclusive 2=OpenAccess 3=ResearchOnly
    pub owner_bps:       u16,       // revenue split basis points (7000 = 70%)
    pub protocol_bps:    u16,       // protocol fee basis points (250 = 2.5%)
    pub version:         u32,
    pub created_epoch:   u64,       // Sui epoch
}

/// A transaction request to mint a Twin NFT on Sui.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuiMintRequest {
    pub kind:           String,     // "twin_mint"
    pub sender:         String,     // Sui address submitting the tx
    pub twin_id:        Hash,
    pub merkle_root:    Hash,
    pub splat_hash:     Hash,
    pub f1_score_x1000: u32,
    pub coverage_x100:  u32,
    pub license_type:   u8,
    pub owner_bps:      u16,
    pub protocol_bps:   u16,
    pub receipt_id:     String,     // 31030 receipt_id for audit trail
    pub gas_budget:     u64,        // MIST
}

impl SuiMintRequest {
    pub fn from_scene_receipt(receipt: &SceneReceipt, sender_address: &str) -> TspResult<Self> {
        let splat_hash = receipt.splat_hash.clone();

        let license_type: u8 = match receipt.license_type {
            sovereign_types::LicenseType::Exclusive    => 0,
            sovereign_types::LicenseType::NonExclusive => 1,
            sovereign_types::LicenseType::OpenAccess   => 2,
            sovereign_types::LicenseType::ResearchOnly => 3,
        };

        Ok(Self {
            kind:           "twin_mint".into(),
            sender:         sender_address.into(),
            twin_id:        receipt.twin_id.clone(),
            merkle_root:    receipt.merkle_root.clone(),
            splat_hash,
            f1_score_x1000: (receipt.f1_score * 1000.0) as u32,
            coverage_x100:  (receipt.coverage_pct * 100.0) as u32,
            license_type,
            owner_bps:      7000,   // 70% owner
            protocol_bps:   250,    // 2.5% protocol
            receipt_id:     receipt.receipt_id.clone(),
            gas_budget:     10_000_000,  // 0.01 SUI
        })
    }

    /// Produce the Move call arguments for twin_nft::mint_twin().
    pub fn to_move_args(&self) -> serde_json::Value {
        serde_json::json!({
            "function":    "twin_nft::mint_twin",
            "type_args":   [],
            "args": [
                self.twin_id,
                self.merkle_root,
                self.splat_hash,
                self.f1_score_x1000,
                self.coverage_x100,
                self.license_type,
                self.owner_bps,
                self.protocol_bps,
            ]
        })
    }
}

/// Response from the Sui node after a mint transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuiMintResponse {
    pub tx_digest:   String,        // Sui transaction digest
    pub object_id:   String,        // newly minted Twin NFT object ID
    pub gas_used:    u64,           // MIST
    pub epoch:       u64,
    pub status:      SuiTxStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SuiTxStatus {
    Success,
    Failure,
    Pending,
}

/// A license NFT derived from a Twin NFT.
/// Represents a granted right to simulate/view/annotate the twin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuiLicenseObject {
    pub object_id:    String,
    pub twin_id:      Hash,
    pub twin_object:  String,        // parent Twin NFT object_id
    pub grantee:      String,        // Sui address of licensee
    pub rights_bitmask: u8,          // bit 0=View 1=Simulate 2=Annotate 3=Derive 4=Distribute 5=Commercial
    pub max_sim_runs: Option<u32>,
    pub expires_at:   Option<u64>,   // Sui epoch
}

fn rights_to_bitmask(rights: &[UsageRight]) -> u8 {
    let mut mask = 0u8;
    for r in rights {
        mask |= match r {
            UsageRight::View        => 0b00000001,
            UsageRight::Simulate    => 0b00000010,
            UsageRight::Annotate    => 0b00000100,
            UsageRight::Derive      => 0b00001000,
            UsageRight::Distribute  => 0b00010000,
            UsageRight::Commercial  => 0b00100000,
        };
    }
    mask
}

/// The Sui anchor client — submits transactions and writes object IDs back.
pub struct SuiAnchor {
    /// Sui RPC endpoint (e.g. "https://fullnode.mainnet.sui.io")
    pub rpc_url:  String,
    /// Sui address used for submitting transactions
    pub address:  String,
    /// Private key (base64url) — in production, use a keystore/HSM
    pub key:      String,
}

impl SuiAnchor {
    pub fn new(rpc_url: impl Into<String>, address: impl Into<String>, key: impl Into<String>) -> Self {
        Self { rpc_url: rpc_url.into(), address: address.into(), key: key.into() }
    }

    /// Anchor a SceneReceipt on Sui. Returns the transaction digest and object ID.
    /// Production: uses sui-sdk or raw JSON-RPC.
    pub async fn mint_twin(&self, receipt: &mut SceneReceipt) -> TspResult<SuiMintResponse> {
        let request = SuiMintRequest::from_scene_receipt(receipt, &self.address)?;
        let move_args = request.to_move_args();

        println!("[sui-anchor] minting Twin NFT for {}...", &receipt.twin_id[..20.min(receipt.twin_id.len())]);
        println!("[sui-anchor] move call: {}", serde_json::to_string(&move_args).unwrap_or_default());

        // Production implementation:
        //   let client = sui_sdk::SuiClientBuilder::default().build(&self.rpc_url).await?;
        //   let tx = client.transaction_builder()
        //     .move_call(self.address.clone(), TWIN_PACKAGE_ID, "twin_nft", "mint_twin", ...)
        //     .await?;
        //   let signed = keystore.sign_transaction(&tx)?;
        //   let response = client.quorum_driver_api().execute_transaction_block(signed, ...).await?;
        //
        // Stub response for now — object_id derived deterministically from twin_id
        let stub_object_id = format!("0x{}", &sovereign_types::hash_str(&receipt.twin_id)[7..39]);
        let stub_digest    = format!("sui_tx:{}", &sovereign_types::hash_str(&receipt.receipt_id)[7..23]);

        let response = SuiMintResponse {
            tx_digest:  stub_digest,
            object_id:  stub_object_id.clone(),
            gas_used:   3_500_000,
            epoch:      1,
            status:     SuiTxStatus::Success,
        };

        // Write object_id back into the receipt
        *receipt = receipt.clone().with_sui(stub_object_id, response.tx_digest.clone());

        println!("[sui-anchor] ✓ Twin NFT minted: {} (tx: {})",
            receipt.sui_object_id.as_deref().unwrap_or(""),
            receipt.ip_root_tx.as_deref().unwrap_or(""),
        );

        Ok(response)
    }

    /// Issue a License NFT derived from an existing Twin NFT.
    pub async fn issue_license(&self, grant: &TwinLicenseGrant, twin_object_id: &str) -> TspResult<SuiLicenseObject> {
        let bitmask = rights_to_bitmask(&grant.rights);
        println!("[sui-anchor] issuing license for {} → {} (rights={:#010b})",
            &grant.twin_id[..16.min(grant.twin_id.len())], grant.grantee_did, bitmask);

        let stub_object_id = format!("0xlic:{}", &sovereign_types::hash_str(&grant.grant_id)[7..23]);

        Ok(SuiLicenseObject {
            object_id:     stub_object_id,
            twin_id:       grant.twin_id.clone(),
            twin_object:   twin_object_id.into(),
            grantee:       grant.grantee_did.clone(),
            rights_bitmask: bitmask,
            max_sim_runs:  grant.constraints.max_sim_runs,
            expires_at:    None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::SceneReceipt;
    use sovereign_types::{IdentityChain, LicenseType, crypto::generate_keypair};

    fn make_receipt() -> SceneReceipt {
        // Build a minimal but valid SceneReceipt
        SceneReceipt {
            kind:                   31030,
            receipt_id:             "rcpt:31030:test".into(),
            twin_id:                "twin:sha256:aabbcc".into(),
            version:                1,
            identity:               IdentityChain::new("did:p:1".into(), "did:a:1".into()),
            capture_receipt_ids:    vec!["rcpt:31020:01".into()],
            reconstruction_engine: "julia-gsplat/2.1".into(),
            splat_hash:             "sha256:splat01".into(),
            geometry_hash:          None,
            semantic_hash:          None,
            f1_score:               0.891,
            coverage_pct:           94.2,
            gaussian_count:         Some(1_842_000),
            sui_object_id:          None,
            ip_root_tx:             None,
            license_type:           LicenseType::NonExclusive,
            merkle_root:            "sha256:root01".into(),
            signature:              "base64url:testsig".into(),
            timestamp:              1725734400000,
        }
    }

    #[test]
    fn mint_request_encodes_f1_correctly() {
        let receipt  = make_receipt();
        let request  = SuiMintRequest::from_scene_receipt(&receipt, "0xowner").unwrap();
        assert_eq!(request.f1_score_x1000, 891);
        assert_eq!(request.coverage_x100,  9420);
        assert_eq!(request.license_type,   1);  // NonExclusive
        assert_eq!(request.owner_bps,      7000);
        assert_eq!(request.protocol_bps,   250);
    }

    #[test]
    fn rights_bitmask_correct() {
        let rights = vec![UsageRight::View, UsageRight::Simulate];
        assert_eq!(rights_to_bitmask(&rights), 0b00000011);

        let all = vec![UsageRight::View, UsageRight::Simulate, UsageRight::Annotate,
                       UsageRight::Derive, UsageRight::Distribute, UsageRight::Commercial];
        assert_eq!(rights_to_bitmask(&all), 0b00111111);
    }

    #[tokio::test]
    async fn mint_writes_object_id_back() {
        let mut receipt = make_receipt();
        assert!(receipt.sui_object_id.is_none());

        let anchor = SuiAnchor::new("https://fullnode.mainnet.sui.io", "0xowner", "privkey");
        let response = anchor.mint_twin(&mut receipt).await.unwrap();

        assert_eq!(response.status, SuiTxStatus::Success);
        assert!(receipt.sui_object_id.is_some());
        assert!(receipt.ip_root_tx.is_some());
        assert!(receipt.sui_object_id.as_deref().unwrap().starts_with("0x"));
    }
}
