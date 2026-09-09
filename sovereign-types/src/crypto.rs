use ed25519_dalek::{SigningKey, VerifyingKey, Signer, Verifier, Signature};
use rand::rngs::OsRng;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use crate::SovereignError;

/// Generate a new ed25519 keypair. Returns (private_b64, public_b64).
pub fn generate_keypair() -> (String, String) {
    let signing_key = SigningKey::generate(&mut OsRng);
    let verifying_key = signing_key.verifying_key();
    (
        URL_SAFE_NO_PAD.encode(signing_key.to_bytes()),
        URL_SAFE_NO_PAD.encode(verifying_key.to_bytes()),
    )
}

/// Sign a merkle root (or any bytes) with a private key (base64url).
pub fn sign(data: &str, private_key_b64: &str) -> Result<String, SovereignError> {
    let key_bytes = URL_SAFE_NO_PAD.decode(private_key_b64)
        .map_err(|e| SovereignError::Crypto(e.to_string()))?;
    let key_array: [u8; 32] = key_bytes.try_into()
        .map_err(|_| SovereignError::Crypto("invalid key length".into()))?;
    let signing_key = SigningKey::from_bytes(&key_array);
    let sig = signing_key.sign(data.as_bytes());
    Ok(format!("base64url:{}", URL_SAFE_NO_PAD.encode(sig.to_bytes())))
}

/// Verify a signature (base64url:...) against data and public key (base64url).
pub fn verify(data: &str, signature: &str, public_key_b64: &str) -> Result<(), SovereignError> {
    let sig_bytes_b64 = signature.strip_prefix("base64url:")
        .ok_or_else(|| SovereignError::Crypto("signature must start with base64url:".into()))?;
    let sig_bytes = URL_SAFE_NO_PAD.decode(sig_bytes_b64)
        .map_err(|e| SovereignError::Crypto(e.to_string()))?;
    let sig_array: [u8; 64] = sig_bytes.try_into()
        .map_err(|_| SovereignError::Crypto("invalid signature length".into()))?;

    let key_bytes = URL_SAFE_NO_PAD.decode(public_key_b64)
        .map_err(|e| SovereignError::Crypto(e.to_string()))?;
    let key_array: [u8; 32] = key_bytes.try_into()
        .map_err(|_| SovereignError::Crypto("invalid key length".into()))?;

    let verifying_key = VerifyingKey::from_bytes(&key_array)
        .map_err(|e| SovereignError::Crypto(e.to_string()))?;
    let signature = Signature::from_bytes(&sig_array);

    verifying_key.verify(data.as_bytes(), &signature)
        .map_err(|_| SovereignError::InvalidSignature)
}

/// Derive a DID from a public key.
pub fn did_from_pubkey(pubkey_b64: &str, kind: &str) -> String {
    let hash = crate::hash_str(pubkey_b64);
    let hex = hash.strip_prefix("sha256:").unwrap_or(&hash);
    format!("did:vantage:{}:{}", kind, &hex[..16])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_verify_roundtrip() {
        let (priv_key, pub_key) = generate_keypair();
        let data = "sha256:aabbcc112233";
        let sig = sign(data, &priv_key).unwrap();
        assert!(sig.starts_with("base64url:"));
        verify(data, &sig, &pub_key).unwrap();
    }

    #[test]
    fn wrong_key_fails() {
        let (priv_key, _) = generate_keypair();
        let (_, wrong_pub) = generate_keypair();
        let sig = sign("test", &priv_key).unwrap();
        assert!(verify("test", &sig, &wrong_pub).is_err());
    }

    #[test]
    fn did_format() {
        let (_, pub_key) = generate_keypair();
        let did = did_from_pubkey(&pub_key, "principal");
        assert!(did.starts_with("did:vantage:principal:"));
    }
}
