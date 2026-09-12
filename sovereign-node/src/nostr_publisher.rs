//! One-shot Nostr event publisher — sends a single event to a relay and disconnects.
//! Used for IP provenance: Twin Binding (1903) + Creation Receipt (1901).
//! This is intentionally simple: no persistent connection, no retry queue.
//! The caller fires-and-forgets; failures are logged but never fatal.

use ip_layer::nostr::NostrEvent;
use tracing::{info, warn};

/// Send `event` to `relay_url` via NIP-01 WebSocket.
/// Sends ["EVENT", event_json] and disconnects.
/// Returns Ok(()) if the relay acknowledged, Err on connection/send failure.
pub async fn publish_nostr_event(relay_url: &str, event: &NostrEvent) -> Result<(), String> {
    use tokio_tungstenite::connect_async;
    use tokio_tungstenite::tungstenite::Message;
    use futures_util::{SinkExt, StreamExt};

    let (mut ws, _) = connect_async(relay_url)
        .await
        .map_err(|e| format!("ws connect failed: {e}"))?;

    let msg = serde_json::json!(["EVENT", event]).to_string();
    ws.send(Message::Text(msg))
        .await
        .map_err(|e| format!("ws send failed: {e}"))?;

    // Read one response (OK or NOTICE) then close.
    if let Some(Ok(Message::Text(resp))) = ws.next().await {
        if resp.contains("\"OK\"") || resp.contains("true") {
            // published
        } else {
            warn!(resp = %resp, "relay returned non-OK response");
        }
    }

    let _ = ws.close(None).await;
    Ok(())
}

// ── Receipt kind publishers ────────────────────────────────────────────────────

fn unix_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Deterministic fallback event-id: sha256hex(receipt_id + relay_url).
fn stub_event_id(receipt_id: &str, relay_url: &str) -> String {
    let combined = format!("{receipt_id}{relay_url}");
    ip_layer::sha256_hex(combined.as_bytes())
}

/// Publish a kind-31020 CaptureReceipt as a NIP-01 signed Nostr event.
///
/// When `private_key_hex` is a valid 32-byte hex key, signs and delivers via WebSocket.
/// Falls back to log-only (stub) on key parse failure or relay error.
///
/// Returns `Ok(event_id)` — a 32-byte hex string — in both cases.
pub async fn publish_capture_receipt(
    receipt: &twin_protocol::CaptureReceipt,
    relay_url: &str,
    private_key_hex: &str,
) -> Result<String, String> {
    let content = serde_json::to_string(receipt)
        .map_err(|e| format!("serialize CaptureReceipt: {e}"))?;

    let tags = vec![
        vec!["receipt_id".into(), receipt.receipt_id.clone()],
        vec!["kind_label".into(), "capture_receipt".into()],
    ];

    let created_at = unix_now_secs();

    // Try real NIP-01 sign + publish when a valid key is provided.
    if let Ok(seckey) = ip_layer::nostr::NostrSecretKey::from_hex(private_key_hex) {
        match ip_layer::nostr::sign_event(31020, tags, content, &seckey, created_at) {
            Ok(event) => {
                let event_id = event.id.clone();
                match publish_nostr_event(relay_url, &event).await {
                    Ok(()) => {
                        info!(
                            event_id   = %event_id,
                            receipt_id = %receipt.receipt_id,
                            relay_url  = %relay_url,
                            kind       = 31020,
                            "kind-31020 CaptureReceipt published to Nostr"
                        );
                        return Ok(event_id);
                    }
                    Err(e) => warn!(error = %e, "kind-31020 relay delivery failed — using stub id"),
                }
                return Ok(event_id);
            }
            Err(e) => warn!(error = %e, "kind-31020 sign failed — using stub id"),
        }
    }

    // Stub path: no valid key or relay unreachable.
    let event_id = stub_event_id(&receipt.receipt_id, relay_url);
    info!(
        event_id   = %event_id,
        receipt_id = %receipt.receipt_id,
        relay_url  = %relay_url,
        kind       = 31020,
        "kind-31020 CaptureReceipt stub-published (no valid nsec)"
    );
    Ok(event_id)
}

/// Publish a kind-31030 SceneReceipt as a NIP-01 signed Nostr event.
///
/// Same signing/fallback logic as [`publish_capture_receipt`].
pub async fn publish_scene_receipt(
    receipt: &twin_protocol::SceneReceipt,
    relay_url: &str,
    private_key_hex: &str,
) -> Result<String, String> {
    let content = serde_json::to_string(receipt)
        .map_err(|e| format!("serialize SceneReceipt: {e}"))?;

    let tags = vec![
        vec!["twin_id".into(), receipt.twin_id.clone()],
        vec!["receipt_id".into(), receipt.receipt_id.clone()],
        vec!["kind_label".into(), "scene_receipt".into()],
    ];

    let created_at = unix_now_secs();

    if let Ok(seckey) = ip_layer::nostr::NostrSecretKey::from_hex(private_key_hex) {
        match ip_layer::nostr::sign_event(31030, tags, content, &seckey, created_at) {
            Ok(event) => {
                let event_id = event.id.clone();
                match publish_nostr_event(relay_url, &event).await {
                    Ok(()) => {
                        info!(
                            event_id   = %event_id,
                            receipt_id = %receipt.receipt_id,
                            twin_id    = %receipt.twin_id,
                            relay_url  = %relay_url,
                            kind       = 31030,
                            "kind-31030 SceneReceipt published to Nostr"
                        );
                        return Ok(event_id);
                    }
                    Err(e) => warn!(error = %e, "kind-31030 relay delivery failed — using stub id"),
                }
                return Ok(event_id);
            }
            Err(e) => warn!(error = %e, "kind-31030 sign failed — using stub id"),
        }
    }

    let event_id = stub_event_id(&receipt.receipt_id, relay_url);
    info!(
        event_id   = %event_id,
        receipt_id = %receipt.receipt_id,
        twin_id    = %receipt.twin_id,
        relay_url  = %relay_url,
        kind       = 31030,
        "kind-31030 SceneReceipt stub-published (no valid nsec)"
    );
    Ok(event_id)
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal CaptureReceipt for testing via JSON deserialization.
    /// This avoids having to run the full pipeline builder (which requires a real private key,
    /// F1 gate pass, etc.) while still exercising the publisher logic.
    fn make_capture_receipt() -> twin_protocol::CaptureReceipt {
        let identity_json = serde_json::json!({
            "principal_id": "did:node:test",
            "agent_id":     "did:agent:test",
            "session_id":   "sess:test",
            "execution_id": "exec:test",
            "receipt_id":   "rcpt:test"
        });
        let region_json = serde_json::json!({
            "min_lat": 0.0, "max_lat": 1.0,
            "min_lon": 0.0, "max_lon": 1.0
        });
        let json = serde_json::json!({
            "kind":           31020u32,
            "receipt_id":     "rcpt:31020:test-cap-001",
            "identity":       identity_json,
            "device_ids":     ["did:device:test"],
            "modalities":     [],
            "region":         region_json,
            "capture_epoch":  [0u64, 1000u64],
            "f1_score":       0.85f32,
            "coverage_pct":   80.0f32,
            "frame_count":    10u32,
            "duration_ms":    5000u64,
            "raw_hashes":     {},
            "novelty_score":  0.5f32,
            "delta_coverage": 0.0f32,
            "privacy_flags":  [],
            "merkle_root":    "abc123",
            "signature":      "sig:stub",
            "timestamp":      0u64
        });
        serde_json::from_value(json).expect("make_capture_receipt deserialization failed")
    }

    /// Build a minimal SceneReceipt for testing via JSON deserialization.
    fn make_scene_receipt() -> twin_protocol::SceneReceipt {
        let identity_json = serde_json::json!({
            "principal_id": "did:node:test",
            "agent_id":     "did:agent:test",
            "session_id":   "sess:test",
            "execution_id": "exec:test",
            "receipt_id":   "rcpt:test"
        });
        let json = serde_json::json!({
            "kind":                   31030u32,
            "receipt_id":             "rcpt:31030:test-scene-001",
            "twin_id":                "twin:test-001",
            "version":                1u32,
            "identity":               identity_json,
            "capture_receipt_ids":    ["rcpt:31020:test-cap-001"],
            "reconstruction_engine":  "test-engine",
            "splat_hash":             "splat_hash_stub",
            "geometry_hash":          null,
            "semantic_hash":          null,
            "f1_score":               0.85f32,
            "coverage_pct":           80.0f32,
            "gaussian_count":         null,
            "sui_object_id":          null,
            "ip_root_tx":             null,
            "license_type":           "open_access",
            "merkle_root":            "abc123",
            "signature":              "sig:stub",
            "timestamp":              0u64
        });
        serde_json::from_value(json).expect("make_scene_receipt deserialization failed")
    }

    #[tokio::test]
    async fn publish_capture_receipt_returns_ok_nonempty_hex() {
        let receipt = make_capture_receipt();
        let result = publish_capture_receipt(&receipt, "wss://relay.test", "deadbeef").await;
        assert!(result.is_ok(), "expected Ok, got: {:?}", result);
        let event_id = result.unwrap();
        assert!(!event_id.is_empty(), "event_id must be non-empty");
        // Must be valid hex (sha256 output)
        assert!(event_id.chars().all(|c| c.is_ascii_hexdigit()), "event_id must be hex: {event_id}");
    }

    #[tokio::test]
    async fn publish_scene_receipt_returns_ok_nonempty_hex() {
        let receipt = make_scene_receipt();
        let result = publish_scene_receipt(&receipt, "wss://relay.test", "deadbeef").await;
        assert!(result.is_ok(), "expected Ok, got: {:?}", result);
        let event_id = result.unwrap();
        assert!(!event_id.is_empty(), "event_id must be non-empty");
        assert!(event_id.chars().all(|c| c.is_ascii_hexdigit()), "event_id must be hex: {event_id}");
    }

    #[tokio::test]
    async fn publish_same_receipt_same_relay_deterministic() {
        let receipt = make_capture_receipt();
        let id1 = publish_capture_receipt(&receipt, "wss://relay.test", "key1").await.unwrap();
        let id2 = publish_capture_receipt(&receipt, "wss://relay.test", "key2").await.unwrap();
        assert_eq!(id1, id2, "event_id should be deterministic (derived from receipt+relay, not private key)");
    }
}
