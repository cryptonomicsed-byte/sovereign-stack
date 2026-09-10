/// NIP-01 Nostr event signing — BIP-340 Schnorr on secp256k1.
///
/// Nostr event ID = sha256(serialised_event_json) per NIP-01.
/// Signature = BIP-340 Schnorr over the event id.

use sha2::{Sha256, Digest};
use serde_json::{json, Value};

/// A fully-formed, signed Nostr event.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NostrEvent {
    pub id:         String,        // 32-byte hex event id
    pub pubkey:     String,        // 32-byte hex pubkey
    pub created_at: u64,           // unix seconds
    pub kind:       u32,
    pub tags:       Vec<Vec<String>>,
    pub content:    String,
    pub sig:        String,        // 64-byte hex Schnorr sig
}

/// A secp256k1 secret key — 32 raw bytes.
pub struct NostrSecretKey(pub [u8; 32]);

impl NostrSecretKey {
    pub fn from_hex(h: &str) -> Result<Self, IpLayerError> {
        let bytes = hex::decode(h)
            .map_err(|e| IpLayerError::KeyError(e.to_string()))?;
        bytes.try_into()
            .map(Self)
            .map_err(|_| IpLayerError::KeyError("secret key must be 32 bytes".into()))
    }

    /// Derive the x-only pubkey hex (32 bytes, even-y convention).
    pub fn pubkey_hex(&self) -> String {
        use k256::schnorr::SigningKey;
        let key = SigningKey::from_bytes(&self.0)
            .expect("valid secret key");
        hex::encode(key.verifying_key().to_bytes())
    }
}

/// NIP-01 canonical serialisation for ID computation.
/// `[0, pubkey, created_at, kind, tags, content]`
pub fn event_id(pubkey: &str, created_at: u64, kind: u32,
                tags: &[Vec<String>], content: &str) -> String {
    let serialised = json!([
        0,
        pubkey,
        created_at,
        kind,
        tags,
        content,
    ]).to_string();

    let mut h = Sha256::new();
    h.update(serialised.as_bytes());
    hex::encode(h.finalize())
}

/// Build and sign a Nostr event.
pub fn sign_event(
    kind:       u32,
    tags:       Vec<Vec<String>>,
    content:    String,
    seckey:     &NostrSecretKey,
    created_at: u64,
) -> Result<NostrEvent, IpLayerError> {
    use k256::schnorr::SigningKey;
    use k256::schnorr::signature::Signer;

    let pubkey_hex = seckey.pubkey_hex();
    let id         = event_id(&pubkey_hex, created_at, kind, &tags, &content);

    let signing_key = SigningKey::from_bytes(&seckey.0)
        .map_err(|e| IpLayerError::SignError(e.to_string()))?;

    let id_bytes = hex::decode(&id)
        .map_err(|e| IpLayerError::SignError(e.to_string()))?;

    let sig: k256::schnorr::Signature = signing_key.sign(&id_bytes);
    let sig_hex = hex::encode(sig.to_bytes());

    Ok(NostrEvent {
        id,
        pubkey: pubkey_hex,
        created_at,
        kind,
        tags,
        content,
        sig: sig_hex,
    })
}

#[derive(Debug, thiserror::Error)]
pub enum IpLayerError {
    #[error("key error: {0}")]
    KeyError(String),
    #[error("sign error: {0}")]
    SignError(String),
    #[error("publish error: {0}")]
    PublishError(String),
    #[error("json error: {0}")]
    JsonError(#[from] serde_json::Error),
    #[error("validation error: {0}")]
    Validation(String),
}
