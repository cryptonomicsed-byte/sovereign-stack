use ip_layer::*;

fn test_seckey() -> NostrSecretKey {
    // deterministic test key — never use in production
    NostrSecretKey([0x01; 32])
}

fn now() -> u64 { 1_755_800_000 }

// ── NostrSecretKey / signing ──────────────────────────────────────────────

#[test]
fn seckey_derives_pubkey_hex() {
    let k = test_seckey();
    let p = k.pubkey_hex();
    assert_eq!(p.len(), 64, "pubkey must be 64-char hex");
}

#[test]
fn sign_event_produces_valid_id() {
    use ip_layer::nostr::{event_id, sign_event};
    let k = test_seckey();
    let ev = sign_event(1, vec![], "hello".into(), &k, now()).unwrap();
    // re-compute the id
    let expected = event_id(&ev.pubkey, ev.created_at, ev.kind, &ev.tags, &ev.content);
    assert_eq!(ev.id, expected);
}

#[test]
fn sign_event_sig_is_64_bytes_hex() {
    use ip_layer::nostr::sign_event;
    let k = test_seckey();
    let ev = sign_event(1, vec![], "test".into(), &k, now()).unwrap();
    assert_eq!(ev.sig.len(), 128, "Schnorr sig is 64 bytes = 128 hex chars");
}

#[test]
fn different_content_produces_different_id() {
    use ip_layer::nostr::sign_event;
    let k = test_seckey();
    let a = sign_event(1, vec![], "hello".into(), &k, now()).unwrap();
    let b = sign_event(1, vec![], "world".into(), &k, now()).unwrap();
    assert_ne!(a.id, b.id);
}

// ── IP Root (kind:31900) ──────────────────────────────────────────────────

#[test]
fn ip_root_produces_correct_kind() {
    let k   = test_seckey();
    let pub_hex = k.pubkey_hex();
    let ev  = IpRootBuilder::for_agent(&pub_hex).sign(&k, now()).unwrap();
    assert_eq!(ev.kind, KIND_IP_ROOT);
}

#[test]
fn ip_root_has_d_tag() {
    let k   = test_seckey();
    let pub_hex = k.pubkey_hex();
    let ev  = IpRootBuilder::for_agent(&pub_hex).sign(&k, now()).unwrap();
    let d   = ev.tags.iter().find(|t| t.first().map(|s| s == "d").unwrap_or(false));
    assert!(d.is_some());
    assert_eq!(d.unwrap()[1], pub_hex);
}

#[test]
fn ip_root_has_license_tag() {
    let k   = test_seckey();
    let pub_hex = k.pubkey_hex();
    let ev  = IpRootBuilder::for_agent(&pub_hex)
        .with_license("mit")
        .sign(&k, now())
        .unwrap();
    let lic = ev.tags.iter().find(|t| t.first().map(|s| s == "license").unwrap_or(false));
    assert_eq!(lic.unwrap()[1], "mit");
}

#[test]
fn ip_root_with_owner_tag() {
    let k   = test_seckey();
    let pub_hex = k.pubkey_hex();
    let ev  = IpRootBuilder::for_agent(&pub_hex)
        .with_owner("deadbeef".repeat(8))
        .sign(&k, now())
        .unwrap();
    let owner = ev.tags.iter().find(|t| t.first().map(|s| s == "owner").unwrap_or(false));
    assert!(owner.is_some());
}

#[test]
fn ip_root_roundtrips_via_parse() {
    let k   = test_seckey();
    let pub_hex = k.pubkey_hex();
    let ev  = IpRootBuilder::for_agent(&pub_hex)
        .with_display_name("Test Agent")
        .sign(&k, now())
        .unwrap();
    let root = IpRoot::from_event(&ev).unwrap();
    assert_eq!(root.ip_root_id, pub_hex);
    assert_eq!(root.metadata.display_name, "Test Agent");
}

// ── Creation Receipt (kind:1901) ──────────────────────────────────────────

#[test]
fn creation_receipt_correct_kind() {
    let k   = test_seckey();
    let ev  = CreationReceiptBuilder::new("ip-root-001", "abc123")
        .sign(&k, now())
        .unwrap();
    assert_eq!(ev.kind, KIND_CREATION_RECEIPT);
}

#[test]
fn creation_receipt_has_ip_root_and_x_tags() {
    let k   = test_seckey();
    let ev  = CreationReceiptBuilder::new("ip-root-001", "abc123")
        .sign(&k, now())
        .unwrap();
    let has_ip_root = ev.tags.iter().any(|t| t.get(0).map(|s| s == "ip_root").unwrap_or(false));
    let has_x       = ev.tags.iter().any(|t| t.get(0).map(|s| s == "x").unwrap_or(false));
    assert!(has_ip_root && has_x);
}

#[test]
fn creation_receipt_has_nip32_license_labels() {
    let k   = test_seckey();
    let ev  = CreationReceiptBuilder::new("ip-root-001", "abc123")
        .sign(&k, now())
        .unwrap();
    let has_L = ev.tags.iter().any(|t| t.get(0).map(|s| s == "L").unwrap_or(false));
    let has_l = ev.tags.iter().any(|t| t.get(0).map(|s| s == "l").unwrap_or(false));
    assert!(has_L && has_l);
}

#[test]
fn creation_receipt_for_splat_has_mime_and_osovm() {
    let k   = test_seckey();
    let ev  = CreationReceiptBuilder::for_splat(
        "ip-root-001", "plysha256abc", "scene-rcpt-001", None, "My Splat"
    ).sign(&k, now()).unwrap();
    let has_mime   = ev.tags.iter().any(|t| t.get(1).map(|s| s == "model/splat+ply").unwrap_or(false));
    let has_osovm  = ev.tags.iter().any(|t| t.get(0).map(|s| s == "osovm_op").unwrap_or(false));
    assert!(has_mime && has_osovm);
}

#[test]
fn creation_receipt_with_twin_tag() {
    let k   = test_seckey();
    let ev  = CreationReceiptBuilder::for_splat(
        "ip-root-001", "sha256hash", "rcpt-001", Some("twin-binding-001".into()), "Splat"
    ).sign(&k, now()).unwrap();
    let has_twin = ev.tags.iter().any(|t| t.get(0).map(|s| s == "twin").unwrap_or(false));
    assert!(has_twin);
}

#[test]
fn creation_receipt_split_tag() {
    let k   = test_seckey();
    let ev  = CreationReceiptBuilder::new("root-001", "hash-001")
        .with_split("aa".repeat(32), 5000)
        .with_split("bb".repeat(32), 5000)
        .sign(&k, now())
        .unwrap();
    let splits: Vec<_> = ev.tags.iter().filter(|t| t.get(0).map(|s| s == "split").unwrap_or(false)).collect();
    assert_eq!(splits.len(), 2);
}

// ── Twin Binding (kind:1903) ──────────────────────────────────────────────

#[test]
fn twin_binding_correct_kind() {
    let k   = test_seckey();
    let ev  = TwinBindingBuilder::new("root-001", "twin-001", TwinKind::GaussianSplat1to1)
        .sign(&k, now())
        .unwrap();
    assert_eq!(ev.kind, KIND_TWIN_BINDING);
}

#[test]
fn twin_binding_has_required_tags() {
    let k   = test_seckey();
    let ev  = TwinBindingBuilder::for_splat("root-001", "twin-001", "scene-001", Some(0.85))
        .sign(&k, now())
        .unwrap();
    let has_ip_root   = ev.tags.iter().any(|t| t.get(0).map(|s| s == "ip_root").unwrap_or(false));
    let has_sim_id    = ev.tags.iter().any(|t| t.get(0).map(|s| s == "sim_id").unwrap_or(false));
    let has_twin_kind = ev.tags.iter().any(|t| t.get(0).map(|s| s == "twin_kind").unwrap_or(false));
    assert!(has_ip_root && has_sim_id && has_twin_kind);
}

#[test]
fn twin_binding_for_splat_has_fidelity() {
    let k   = test_seckey();
    let ev  = TwinBindingBuilder::for_splat("root-001", "twin-001", "scene-001", Some(0.777))
        .sign(&k, now())
        .unwrap();
    let fidelity = ev.tags.iter().find(|t| t.get(0).map(|s| s == "fidelity").unwrap_or(false));
    assert!(fidelity.is_some());
    assert!(fidelity.unwrap()[1].contains("0.777"));
}

#[test]
fn twin_kind_string_for_splat() {
    assert_eq!(TwinKind::GaussianSplat1to1.as_str(), "gaussian-splat-1to1");
    assert_eq!(TwinKind::VeilSim1to1.as_str(), "veilsim-1to1");
}

// ── Attestation (kind:1902) ───────────────────────────────────────────────

#[test]
fn attestation_vouch_correct_kind() {
    let k   = test_seckey();
    let ev  = AttestationBuilder::vouch("event-001", "pubkey-001")
        .sign(&k, now())
        .unwrap();
    assert_eq!(ev.kind, KIND_ATTESTATION);
}

#[test]
fn attestation_has_stance_and_nip32_label() {
    let k   = test_seckey();
    let ev  = AttestationBuilder::confirm("event-001", "pubkey-001")
        .sign(&k, now())
        .unwrap();
    let stance = ev.tags.iter().find(|t| t.get(0).map(|s| s == "stance").unwrap_or(false));
    let label  = ev.tags.iter().find(|t| t.get(0).map(|s| s == "l").unwrap_or(false));
    assert_eq!(stance.unwrap()[1], "confirm");
    assert!(label.is_some());
}

#[test]
fn attestation_challenge_requires_reason() {
    let k   = test_seckey();
    // challenge without reason should be caught at sign() time
    let builder = AttestationBuilder {
        subject_event_id: "ev-001".into(),
        subject_pubkey:   "pk-001".into(),
        stance:           Stance::Challenge,
        reason:           None,
        weight:           None,
        notes:            String::new(),
    };
    assert!(builder.sign(&k, now()).is_err());
}

#[test]
fn attestation_challenge_with_reason_signs_ok() {
    let k   = test_seckey();
    let ev  = AttestationBuilder::challenge("event-001", "pubkey-001", "hash mismatch")
        .sign(&k, now())
        .unwrap();
    let reason = ev.tags.iter().find(|t| t.get(0).map(|s| s == "reason").unwrap_or(false));
    assert_eq!(reason.unwrap()[1], "hash mismatch");
}

// ── seal_gaussian_splat helper ────────────────────────────────────────────

#[test]
fn seal_gaussian_splat_produces_two_events_in_order() {
    let k = test_seckey();
    let ip_root_id = k.pubkey_hex();

    let (twin_binding, creation_receipt) = seal_gaussian_splat(
        &ip_root_id,
        "twin:abc-123",
        "rcpt:scene:xyz",
        &sha256_hex(b"fake-ply-data"),
        Some(0.822),
        "Test Scene",
        &k,
    ).unwrap();

    assert_eq!(twin_binding.kind, KIND_TWIN_BINDING);
    assert_eq!(creation_receipt.kind, KIND_CREATION_RECEIPT);

    // creation receipt references the twin binding
    let twin_tag = creation_receipt.tags.iter()
        .find(|t| t.get(0).map(|s| s == "twin").unwrap_or(false));
    assert_eq!(twin_tag.unwrap()[1], twin_binding.id);

    // binding is demonstrably earlier
    assert!(twin_binding.created_at < creation_receipt.created_at);
}

#[test]
fn content_hash_includes_prefix() {
    let h = content_hash(b"test");
    assert!(h.starts_with("sha256:"));
    assert_eq!(h.len(), 7 + 64);
}

#[test]
fn sha256_hex_is_raw_hex() {
    let h = sha256_hex(b"test");
    assert_eq!(h.len(), 64);
    assert!(!h.starts_with("sha256:"));
}
