/// ip-layer — Ọmọ Kọ́dà IP identity and provenance on Nostr.
///
/// Implements the four core event schemas:
///   kind 31900 — IP Root genesis (parameterized replaceable, one per creator/agent)
///   kind 1901  — Creation Receipt (every significant creative output)
///   kind 1902  — Attestation (vouch / challenge / confirm)
///   kind 1903  — Twin Binding (1:1 digital twin ownership — Gaussian splat, VeilSim)
///
/// All kinds verified against the NIP registry — no collisions found (see
/// ip-layer/docs/verification-pass-1.md).
///
/// The ownership chain for a Gaussian splat:
///   Agent born
///     → publish IP Root (31900)
///     → captures scene → SceneReceipt (twin-protocol)
///     → publish Twin Binding (1903) { ip_root, sim_id=twin_id, twin_kind=gaussian-splat-1to1 }
///     → publish Creation Receipt (1901) { x=sha256(ply), osovm_op=RECEIPT:<scene_id>, twin=<1903_event_id> }
///     → agent provably owns the Gaussian splat on Nostr

pub mod nostr;
pub mod ip_root;
pub mod creation_receipt;
pub mod twin_binding;
pub mod attestation;

pub use nostr::{NostrEvent, NostrSecretKey, IpLayerError};
pub use ip_root::{IpRootBuilder, IpRoot, IpRootMetadata, KIND_IP_ROOT};
pub use creation_receipt::{CreationReceiptBuilder, PaymentSplit, KIND_CREATION_RECEIPT};
pub use twin_binding::{TwinBindingBuilder, TwinKind, KIND_TWIN_BINDING};
pub use attestation::{AttestationBuilder, Stance, KIND_ATTESTATION};

use sha2::{Sha256, Digest};

/// Compute sha256:<hex> content hash for a PLY splat file or any bytes.
/// This becomes the `x` tag on a Creation Receipt.
pub fn content_hash(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("sha256:{}", hex::encode(h.finalize()))
}

/// Compute sha256 (no prefix) — for use directly in Nostr `x` tags.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

/// Current unix timestamp in seconds.
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Full ownership chain for a Gaussian splat capture.
///
/// Returns (twin_binding_event, creation_receipt_event) in sign order.
/// Caller must publish ip_root separately at agent birth (not repeated here).
pub fn seal_gaussian_splat(
    ip_root_id:       &str,
    twin_id:          &str,
    scene_receipt_id: &str,
    splat_sha256:     &str,   // sha256 hex of the PLY data
    f1_score:         Option<f32>,
    title:            &str,
    seckey:           &NostrSecretKey,
) -> Result<(NostrEvent, NostrEvent), IpLayerError> {
    let now = now_secs();

    // 1. Twin Binding — establishes "this twin_id is owned by this ip_root"
    let twin_binding = TwinBindingBuilder::for_splat(
        ip_root_id,
        twin_id,
        scene_receipt_id,
        f1_score,
    ).sign(seckey, now)?;

    // 2. Creation Receipt — publishes the creative output with full provenance
    let creation_receipt = CreationReceiptBuilder::for_splat(
        ip_root_id,
        splat_sha256,
        scene_receipt_id,
        Some(twin_binding.id.clone()),
        title,
    ).sign(seckey, now + 1)?;  // +1 sec so binding is demonstrably earlier

    Ok((twin_binding, creation_receipt))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attestation::{AttestationBuilder, Stance};

    fn key() -> NostrSecretKey { NostrSecretKey([0x01u8; 32]) }
    const TS: u64 = 1_700_000_000u64;

    #[test]
    fn content_hash_prefix() {
        assert!(content_hash(b"hello").starts_with("sha256:"));
    }

    #[test]
    fn sha256_hex_is_64char() {
        let h = sha256_hex(b"hello");
        assert_eq!(h.len(), 64);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn sha256_hex_distinct_inputs() {
        assert_ne!(sha256_hex(b"hello"), sha256_hex(b"world"));
    }

    #[test]
    fn nostr_key_pubkey_hex_64char() {
        let k = NostrSecretKey::from_hex(&hex::encode([0x01u8; 32])).unwrap();
        let p = k.pubkey_hex();
        assert_eq!(p.len(), 64);
        assert!(p.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn ip_root_kind_31900() {
        let ev = IpRootBuilder::for_agent("abcd1234ef").sign(&key(), TS).unwrap();
        assert_eq!(ev.kind, 31900u32);
    }

    #[test]
    fn event_id_pubkey_are_hex64() {
        let ev = IpRootBuilder::for_agent("abcd1234ef").sign(&key(), TS).unwrap();
        assert_eq!(ev.id.len(), 64);
        assert!(ev.id.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(ev.pubkey.len(), 64);
        assert!(ev.pubkey.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn creation_receipt_kind_1901() {
        let h = sha256_hex(b"fake_ply");
        let ev = CreationReceiptBuilder::for_splat("root-hex", &h, "scene-001", None, "My Splat")
            .sign(&key(), TS).unwrap();
        assert_eq!(ev.kind, 1901u32);
    }

    #[test]
    fn twin_binding_kind_1903() {
        let ev = TwinBindingBuilder::for_splat("root-hex", "twin-001", "scene-001", Some(0.92))
            .sign(&key(), TS).unwrap();
        assert_eq!(ev.kind, 1903u32);
    }

    #[test]
    fn seal_gaussian_splat_kind_ordering() {
        let h = sha256_hex(b"ply_bytes");
        let (tb, cr) = seal_gaussian_splat("root-hex", "twin-abc", "scene-001", &h,
            Some(0.88), "Title", &key()).unwrap();
        assert!(tb.kind > cr.kind,
            "twin_binding.kind={} should be > creation_receipt.kind={}", tb.kind, cr.kind);
        assert_eq!(tb.kind, 1903u32);
        assert_eq!(cr.kind, 1901u32);
    }

    #[test]
    fn seal_gaussian_splat_timestamp_offset() {
        let h = sha256_hex(b"ply_bytes_2");
        let (tb, cr) = seal_gaussian_splat("root-hex", "twin-abc", "scene-002", &h,
            None, "Title2", &key()).unwrap();
        assert!(tb.created_at <= cr.created_at);
        assert_eq!(cr.created_at - tb.created_at, 1);
    }

    #[test]
    fn creation_receipt_x_tag_contains_splat_hash() {
        let splat_hash = sha256_hex(b"real_ply");
        let ev = CreationReceiptBuilder::for_splat("root-hex", &splat_hash,
            "scene-003", None, "Splat").sign(&key(), TS).unwrap();
        let x_val = ev.tags.iter()
            .find(|t| t.first().map(|s| s == "x").unwrap_or(false))
            .and_then(|t| t.get(1)).cloned();
        assert_eq!(x_val.as_deref(), Some(splat_hash.as_str()));
    }

    #[test]
    fn attestation_vouch_kind_1902() {
        let subj = NostrSecretKey([0x02u8; 32]).pubkey_hex();
        let ev = AttestationBuilder::vouch(&"a".repeat(64), &subj)
            .sign(&key(), TS).unwrap();
        assert_eq!(ev.kind, 1902u32);
    }

    #[test]
    fn attestation_challenge_requires_reason() {
        let builder = AttestationBuilder {
            subject_event_id: "b".repeat(64),
            subject_pubkey:   "c".repeat(64),
            stance:           Stance::Challenge,
            reason:           None,
            weight:           None,
            notes:            String::new(),
        };
        assert!(builder.sign(&key(), TS).is_err());
    }

    #[test]
    fn ip_root_builder_with_display_name() {
        let ev = IpRootBuilder::for_agent("ab12cd34ef")
            .with_display_name("TestAgent")
            .with_license("mit")
            .sign(&key(), TS).unwrap();
        assert_eq!(ev.kind, 31900u32);
        // content includes display name
        assert!(ev.content.contains("TestAgent"));
    }
}
