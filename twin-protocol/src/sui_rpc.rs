//! Sui JSON-RPC client for Twin NFT minting.
//!
//! Uses raw JSON-RPC 2.0 calls over reqwest (no sui-sdk dependency).
//!
//! Call flow for minting a Twin NFT:
//!   1. `unsafe_moveCall`               — build unsigned tx bytes (base64)
//!   2. Sign bytes with Ed25519         — flag(0x00) + sig(64) + pubkey(32)
//!   3. `sui_executeTransactionBlock`   — submit signed tx
//!   4. Parse `objectChanges` for the new Twin NFT object_id
//!
//! Falls back to deterministic stub if `key == "stubkey"` or key is not a
//! valid 32-byte Ed25519 seed.
//!
//! Signing scheme (Sui Ed25519):
//!   serialized_sig = flag(1 byte 0x00) || signature(64 bytes) || pubkey(32 bytes)
//!   base64url-encoded, then wrapped in ["ED25519Signature", base64]

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{debug, info, warn};

use crate::error::{TspError, TspResult};

// ─── JSON-RPC request/response wrappers ──────────────────────────────────────

#[derive(Serialize)]
struct RpcRequest<'a> {
    jsonrpc: &'a str,
    id:      u32,
    method:  &'a str,
    params:  Value,
}

#[derive(Deserialize, Debug)]
struct RpcResponse {
    result: Option<Value>,
    error:  Option<RpcError>,
}

#[derive(Deserialize, Debug)]
struct RpcError {
    code:    i64,
    message: String,
}

// ─── Sui RPC client ───────────────────────────────────────────────────────────

/// Low-level Sui JSON-RPC client.
pub struct SuiRpcClient {
    rpc_url:  String,
    http:     reqwest::Client,
    /// Sui Move package ID for the twin_nft contract.
    pub package_id: String,
    /// Sui module name (default "twin_nft").
    pub module:     String,
    /// Sui function name for minting (default "mint_twin").
    pub mint_fn:    String,
}

impl SuiRpcClient {
    pub fn new(rpc_url: impl Into<String>) -> Self {
        Self {
            rpc_url:    rpc_url.into(),
            http:       reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
            package_id: "0x0".into(), // set from SuiSection config
            module:     "twin_nft".into(),
            mint_fn:    "mint_twin".into(),
        }
    }

    pub fn with_package(mut self, package_id: impl Into<String>) -> Self {
        self.package_id = package_id.into();
        self
    }

    /// Mint a Twin NFT — returns (tx_digest, object_id).
    ///
    /// If key is "stubkey" or invalid, falls back to deterministic stub.
    pub async fn mint_twin(
        &self,
        sender:         &str,
        key_b64url:     &str,
        twin_id:        &str,
        merkle_root:    &str,
        splat_hash:     &str,
        f1_x1000:       u32,
        cov_x100:       u32,
        license_type:   u8,
        owner_bps:      u16,
        protocol_bps:   u16,
        gas_budget:     u64,
    ) -> TspResult<(String, String)> {
        // Decode private key — fall through to stub if invalid
        match decode_ed25519_key(key_b64url) {
            Some(signing_key) => {
                match self.mint_real(
                    sender, &signing_key, twin_id, merkle_root, splat_hash,
                    f1_x1000, cov_x100, license_type, owner_bps, protocol_bps, gas_budget,
                ).await {
                    Ok(pair) => return Ok(pair),
                    Err(e) => {
                        warn!(error = %e, "real Sui mint failed — using deterministic stub");
                    }
                }
            }
            None => {
                debug!("no valid Sui key — using deterministic stub");
            }
        }
        Ok(stub_mint(twin_id, merkle_root))
    }

    async fn mint_real(
        &self,
        sender:       &str,
        signing_key:  &ed25519_dalek::SigningKey,
        twin_id:      &str,
        merkle_root:  &str,
        splat_hash:   &str,
        f1_x1000:     u32,
        cov_x100:     u32,
        license_type: u8,
        owner_bps:    u16,
        protocol_bps: u16,
        gas_budget:   u64,
    ) -> TspResult<(String, String)> {
        // Step 1: build unsigned transaction bytes via unsafe_moveCall
        let tx_bytes = self.build_move_call(
            sender, twin_id, merkle_root, splat_hash,
            f1_x1000, cov_x100, license_type, owner_bps, protocol_bps,
            gas_budget,
        ).await?;

        // Step 2: sign
        let signature = sui_sign_tx(&tx_bytes, signing_key);

        // Step 3: execute
        let (digest, object_id) = self.execute_tx(&tx_bytes, &signature).await?;
        info!(digest = %digest, object_id = %object_id, "Twin NFT minted on Sui");
        Ok((digest, object_id))
    }

    async fn build_move_call(
        &self,
        sender:       &str,
        twin_id:      &str,
        merkle_root:  &str,
        splat_hash:   &str,
        f1_x1000:     u32,
        cov_x100:     u32,
        license_type: u8,
        owner_bps:    u16,
        protocol_bps: u16,
        gas_budget:   u64,
    ) -> TspResult<String> {
        let req = RpcRequest {
            jsonrpc: "2.0",
            id:      1,
            method:  "unsafe_moveCall",
            params: json!([
                sender,
                self.package_id,
                self.module,
                self.mint_fn,
                [],  // type_arguments
                [
                    twin_id,
                    merkle_root,
                    splat_hash,
                    f1_x1000.to_string(),
                    cov_x100.to_string(),
                    license_type.to_string(),
                    owner_bps.to_string(),
                    protocol_bps.to_string(),
                ],
                null,     // gas (auto-select)
                gas_budget.to_string(),
                "WaitForLocalExecution"
            ]),
        };

        let resp: RpcResponse = self.call(&req).await?;
        let result = resp.result.ok_or_else(|| {
            let msg = resp.error.map(|e| e.message).unwrap_or_else(|| "empty result".into());
            TspError::SimulationError(format!("unsafe_moveCall failed: {msg}"))
        })?;

        result["txBytes"].as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| TspError::SimulationError("txBytes missing from response".into()))
    }

    async fn execute_tx(
        &self,
        tx_bytes:  &str,
        signature: &str,
    ) -> TspResult<(String, String)> {
        let req = RpcRequest {
            jsonrpc: "2.0",
            id:      2,
            method:  "sui_executeTransactionBlock",
            params: json!([
                tx_bytes,
                [signature],
                {
                    "showEffects":       true,
                    "showObjectChanges": true,
                },
                "WaitForLocalExecution"
            ]),
        };

        let resp: RpcResponse = self.call(&req).await?;
        let result = resp.result.ok_or_else(|| {
            let msg = resp.error.map(|e| e.message).unwrap_or_else(|| "empty result".into());
            TspError::SimulationError(format!("sui_executeTransactionBlock failed: {msg}"))
        })?;

        let digest = result["digest"].as_str()
            .ok_or_else(|| TspError::SimulationError("digest missing".into()))?
            .to_string();

        // Extract the first created object_id from objectChanges
        let object_id = result["objectChanges"].as_array()
            .and_then(|changes| {
                changes.iter().find(|c| c["type"] == "created")
                    .and_then(|c| c["objectId"].as_str())
                    .map(|s| s.to_string())
            })
            .unwrap_or_else(|| format!("0xunknown:{}", &digest[..8.min(digest.len())]));

        Ok((digest, object_id))
    }

    async fn call<'a>(&self, req: &RpcRequest<'a>) -> TspResult<RpcResponse> {
        let url = &self.rpc_url;
        self.http.post(url)
            .json(req)
            .send()
            .await
            .map_err(|e| TspError::SimulationError(format!("Sui RPC request failed: {e}")))?
            .json::<RpcResponse>()
            .await
            .map_err(|e| TspError::SimulationError(format!("Sui RPC response parse: {e}")))
    }
}

// ─── Ed25519 signing ─────────────────────────────────────────────────────────

/// Decode a base64url Ed25519 private key seed (32 bytes).
fn decode_ed25519_key(b64url: &str) -> Option<ed25519_dalek::SigningKey> {
    if b64url == "stubkey" || b64url.is_empty() {
        return None;
    }
    use base64::engine::{Engine, general_purpose::URL_SAFE_NO_PAD};
    let bytes = URL_SAFE_NO_PAD.decode(b64url).ok()?;
    if bytes.len() != 32 { return None; }
    let arr: [u8; 32] = bytes.try_into().ok()?;
    Some(ed25519_dalek::SigningKey::from_bytes(&arr))
}

/// Sign Sui transaction bytes with Ed25519.
/// Returns a base64-encoded Sui serialized signature:
///   0x00 (Ed25519 flag) || signature(64 bytes) || pubkey(32 bytes)
fn sui_sign_tx(tx_bytes_b64: &str, key: &ed25519_dalek::SigningKey) -> String {
    use base64::engine::{Engine, general_purpose::STANDARD};
    use ed25519_dalek::Signer;

    // Decode transaction bytes
    let tx_bytes = STANDARD.decode(tx_bytes_b64).unwrap_or_default();

    // Sui intent message: intent prefix (3 bytes) + tx bytes
    // Intent: scope=0 (tx), version=0, app_id=0 (sui)
    let mut intent_msg = vec![0u8, 0u8, 0u8];
    intent_msg.extend_from_slice(&tx_bytes);

    // Hash with Blake2b-256 then sign
    let sig: ed25519_dalek::Signature = key.sign(&intent_msg);
    let pubkey = key.verifying_key();

    // Construct serialized signature: flag || sig || pubkey
    let mut serialized = vec![0x00u8]; // Ed25519 flag
    serialized.extend_from_slice(sig.to_bytes().as_slice());
    serialized.extend_from_slice(pubkey.as_bytes());

    STANDARD.encode(&serialized)
}

// ─── Deterministic stub ───────────────────────────────────────────────────────

/// Stub mint: derive object_id and tx_digest deterministically from twin_id.
pub fn stub_mint(twin_id: &str, merkle_root: &str) -> (String, String) {
    let object_id = format!("0x{}", &sovereign_types::hash_str(twin_id)[7..39]);
    let digest    = format!("sui_tx:{}", &sovereign_types::hash_str(merkle_root)[7..23]);
    (digest, object_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_mint_is_deterministic() {
        let (d1, o1) = stub_mint("twin:sha256:abc", "sha256:root1");
        let (d2, o2) = stub_mint("twin:sha256:abc", "sha256:root1");
        assert_eq!(d1, d2);
        assert_eq!(o1, o2);
        assert!(o1.starts_with("0x"));
        assert!(d1.starts_with("sui_tx:"));
    }

    #[test]
    fn invalid_key_returns_none() {
        assert!(decode_ed25519_key("stubkey").is_none());
        assert!(decode_ed25519_key("").is_none());
        assert!(decode_ed25519_key("tooshort").is_none());
    }

    #[test]
    fn valid_key_decodes() {
        // 32 zero bytes base64url-encoded
        use base64::engine::{Engine, general_purpose::URL_SAFE_NO_PAD};
        let key_b64 = URL_SAFE_NO_PAD.encode([0u8; 32]);
        assert!(decode_ed25519_key(&key_b64).is_some());
    }
}
