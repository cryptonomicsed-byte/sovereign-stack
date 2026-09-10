//! One-shot Nostr event publisher — sends a single event to a relay and disconnects.
//! Used for IP provenance: Twin Binding (1903) + Creation Receipt (1901).
//! This is intentionally simple: no persistent connection, no retry queue.
//! The caller fires-and-forgets; failures are logged but never fatal.

use ip_layer::nostr::NostrEvent;
use tracing::warn;

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
