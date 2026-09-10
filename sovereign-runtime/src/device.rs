use serde::{Deserialize, Serialize};

/// Hardware tier — determines which substrate Omo-Koda is running on.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HardwareTier {
    Micro,      // MCU: Zephyr/FreeRTOS, ~100KB, MIDI/GPIO/BLE
    Portable,   // ARM64 Linux: phone, SBC, Termux
    Spatial,    // ARM64 + GPU/NPU: Gaussian splatting device
    Robotics,   // ARM64 + RTOS + PX4: quadruped, drone, arm
}

/// The provider that backs hardware key operations.
/// Abstracted so the same `Principal` works across all tiers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KeyProviderKind {
    Atecc608b,
    Tpm2,
    Se050,
    Software,   // dev/testing only — no hardware binding guarantee
}

/// Abstract interface for hardware key operations.
/// Implemented once per KeyProviderKind; Sovereign Runtime calls through this.
pub trait HardwareKeyProvider: Send + Sync {
    fn provider_kind(&self) -> KeyProviderKind;

    /// Sign arbitrary bytes with the device's root key.
    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, DeviceError>;

    /// Verify a signature produced by this device.
    fn verify(&self, message: &[u8], signature: &[u8]) -> Result<bool, DeviceError>;

    /// Derive a child key from the root using the given path (BIP-32 style).
    fn derive(&self, path: &str) -> Result<Vec<u8>, DeviceError>;

    /// Return a hardware attestation over the given principal_id + nonce.
    /// The attestation is signed by the root key stored in the secure element.
    fn attest(&self, principal_id: &str, nonce: &str) -> Result<String, DeviceError>;

    /// Return the raw public key bytes (32-byte Ed25519 or 33-byte secp256k1).
    fn public_key_bytes(&self) -> Result<Vec<u8>, DeviceError>;
}

/// Lightweight software-only key provider for dev/testing.
/// Never use in production — provides no hardware binding guarantee.
pub struct SoftwareKeyProvider {
    keypair_seed: [u8; 32],
}

impl SoftwareKeyProvider {
    pub fn new(seed: [u8; 32]) -> Self { Self { keypair_seed: seed } }

    pub fn ephemeral() -> Self {
        use sha2::{Sha256, Digest};
        let mut h = Sha256::new();
        h.update(b"sovereign-runtime:ephemeral-key");
        h.update(&std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .to_le_bytes());
        let seed: [u8; 32] = h.finalize().into();
        Self { keypair_seed: seed }
    }
}

impl HardwareKeyProvider for SoftwareKeyProvider {
    fn provider_kind(&self) -> KeyProviderKind { KeyProviderKind::Software }

    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, DeviceError> {
        use sha2::{Sha256, Digest};
        let mut h = Sha256::new();
        h.update(&self.keypair_seed);
        h.update(message);
        Ok(h.finalize().to_vec())
    }

    fn verify(&self, message: &[u8], signature: &[u8]) -> Result<bool, DeviceError> {
        let expected = self.sign(message)?;
        Ok(expected == signature)
    }

    fn derive(&self, path: &str) -> Result<Vec<u8>, DeviceError> {
        use sha2::{Sha256, Digest};
        let mut h = Sha256::new();
        h.update(&self.keypair_seed);
        h.update(path.as_bytes());
        Ok(h.finalize().to_vec())
    }

    fn attest(&self, principal_id: &str, nonce: &str) -> Result<String, DeviceError> {
        let msg = format!("{}:{}", principal_id, nonce);
        let sig = self.sign(msg.as_bytes())?;
        Ok(hex::encode(sig))
    }

    fn public_key_bytes(&self) -> Result<Vec<u8>, DeviceError> {
        use sha2::{Sha256, Digest};
        let mut h = Sha256::new();
        h.update(b"pubkey:");
        h.update(&self.keypair_seed);
        Ok(h.finalize().to_vec())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DeviceError {
    #[error("hardware not available: {0}")]
    NotAvailable(String),
    #[error("signature failed: {0}")]
    SignatureFailed(String),
    #[error("attestation failed: {0}")]
    AttestationFailed(String),
    #[error("key derivation failed: {0}")]
    DerivationFailed(String),
}
