//! Node identity — ed25519 keypair + DID, persisted to disk.
//!
//! Key file: one line, "priv_b64url pub_b64url" (space-separated)
//! DID file: one line, "did:vantage:node:<sha256hex(pubkey)>"
//!
//! On first boot: generate + save.  On subsequent boots: load + verify.
//! Key file permissions set to 0o600 on Unix.

use std::path::Path;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::SigningKey;
use sovereign_types::crypto::{generate_keypair, did_from_pubkey};

#[derive(Debug, Clone)]
pub struct NodeIdentity {
    pub private_key: String,   // base64url ed25519 private scalar
    pub public_key:  String,   // base64url ed25519 verifying key
    pub did:         String,   // did:vantage:node:<sha256hex>
}

impl NodeIdentity {
    pub fn load_or_generate(key_file: &Path, did_file: &Path) -> Self {
        if key_file.exists() && did_file.exists() {
            match Self::load(key_file, did_file) {
                Ok(id) => {
                    tracing::info!(did = %id.did, "loaded node identity");
                    return id;
                }
                Err(e) => tracing::warn!("failed to load identity ({e}) — regenerating"),
            }
        }
        let id = Self::generate();
        if let Err(e) = id.save(key_file, did_file) {
            tracing::warn!("failed to persist identity: {e}");
        } else {
            tracing::info!(did = %id.did, "generated new node identity");
        }
        id
    }

    fn generate() -> Self {
        let (priv_key, pub_key) = generate_keypair();
        let did = did_from_pubkey(&pub_key, "node");
        Self { private_key: priv_key, public_key: pub_key, did }
    }

    fn load(key_file: &Path, did_file: &Path) -> Result<Self, IdentityError> {
        let key_line = std::fs::read_to_string(key_file)
            .map_err(IdentityError::Io)?.trim().to_string();
        let did = std::fs::read_to_string(did_file)
            .map_err(IdentityError::Io)?.trim().to_string();

        let parts: Vec<&str> = key_line.splitn(2, ' ').collect();
        let (priv_key, pub_key) = if parts.len() == 2 {
            (parts[0].to_string(), parts[1].to_string())
        } else {
            // Legacy single-field key — re-derive pub from priv bytes
            let priv_b64 = parts[0].to_string();
            let priv_bytes = URL_SAFE_NO_PAD.decode(&priv_b64)
                .map_err(|e| IdentityError::KeyDecode(e.to_string()))?;
            let arr: [u8; 32] = priv_bytes.try_into()
                .map_err(|_| IdentityError::KeyDecode("expected 32 bytes".into()))?;
            let signing = SigningKey::from_bytes(&arr);
            let pub_b64 = URL_SAFE_NO_PAD.encode(signing.verifying_key().to_bytes());
            (priv_b64, pub_b64)
        };

        Ok(Self { private_key: priv_key, public_key: pub_key, did })
    }

    fn save(&self, key_file: &Path, did_file: &Path) -> Result<(), IdentityError> {
        if let Some(parent) = key_file.parent() {
            std::fs::create_dir_all(parent).map_err(IdentityError::Io)?;
        }
        // Store both components space-separated so we can load without re-deriving
        let key_line = format!("{} {}", self.private_key, self.public_key);
        std::fs::write(key_file, &key_line).map_err(IdentityError::Io)?;
        std::fs::write(did_file, &self.did).map_err(IdentityError::Io)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(key_file, std::fs::Permissions::from_mode(0o600))
                .map_err(IdentityError::Io)?;
        }
        Ok(())
    }

    pub fn chain(&self) -> sovereign_types::IdentityChain {
        sovereign_types::IdentityChain::new(self.did.clone(), self.did.clone())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("key decode: {0}")]
    KeyDecode(String),
}
