//! Nostr relay WebSocket client for DIP event publishing and subscription.
//!
//! Protocol: NIP-01 (basic event publication and subscription).
//!
//! Publishes: DIP envelopes wrapped as NostrEvents via the DIP NostrAdapter.
//!   ["EVENT", <event>]
//!
//! Subscribes: events tagged with this node's npub so we receive inbound DIP.
//!   ["REQ", <sub_id>, {"#p": [<our_npub>], "kinds": [20001,20002,20003,20004,30001,30002]}]
//!
//! Reconnect: exponential backoff (1s, 2s, 4s … capped at 60s) on any WS error.

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, info, warn};

use dip::adapters::{NostrAdapter, NostrEvent};
use dip::envelope::DipEnvelope;

/// Commands the relay task accepts from the node.
pub enum RelayCommand {
    /// Publish a DIP envelope to the relay.
    Publish(DipEnvelope),
    /// Shut down the relay loop.
    Shutdown,
}

/// Handle to the Nostr relay background task.
#[derive(Clone)]
pub struct NostrRelayHandle {
    tx: mpsc::Sender<RelayCommand>,
}

impl NostrRelayHandle {
    pub async fn publish(&self, envelope: DipEnvelope) {
        let _ = self.tx.send(RelayCommand::Publish(envelope)).await;
    }

    pub async fn shutdown(&self) {
        let _ = self.tx.send(RelayCommand::Shutdown).await;
    }
}

/// Spawn the Nostr relay background task.
///
/// Returns a handle for sending commands.  The task reconnects automatically
/// on WS errors with exponential backoff capped at 60 s.
pub fn spawn_nostr_relay(
    relay_url:    String,
    our_npub:     String,
    our_nprivkey: String,
) -> NostrRelayHandle {
    let (tx, mut rx) = mpsc::channel::<RelayCommand>(64);

    tokio::spawn(async move {
        let adapter = Arc::new(NostrAdapter::new(
            relay_url.clone(),
            our_npub.clone(),
            our_nprivkey,
        ));
        let sub_id  = format!("dip-{}", &our_npub[..8.min(our_npub.len())]);

        let mut backoff_secs: u64 = 1;

        loop {
            info!(url = %relay_url, "connecting to Nostr relay");

            match connect_async(&relay_url).await {
                Err(e) => {
                    warn!(url = %relay_url, error = %e, backoff = backoff_secs, "Nostr relay connect failed");
                }
                Ok((mut ws, _)) => {
                    info!(url = %relay_url, "Nostr relay connected");
                    backoff_secs = 1;

                    // Subscribe to DIP events addressed to our npub
                    let req = json!([
                        "REQ",
                        sub_id,
                        {
                            "#p": [our_npub],
                            "kinds": [20001, 20002, 20003, 20004, 30001, 30002]
                        }
                    ]);
                    if let Err(e) = ws.send(Message::Text(req.to_string())).await {
                        warn!(error = %e, "failed to send REQ — will reconnect");
                        sleep_backoff(&mut backoff_secs).await;
                        continue;
                    }
                    debug!(sub_id = %sub_id, "subscribed to DIP events");

                    // Drive the WS connection until error or shutdown
                    let done = relay_loop(&mut ws, &mut rx, &adapter).await;
                    if done {
                        info!("Nostr relay task shutting down");
                        let _ = ws.close(None).await;
                        return;
                    }
                    warn!("Nostr relay WS error — reconnecting");
                }
            }

            sleep_backoff(&mut backoff_secs).await;
        }
    });

    NostrRelayHandle { tx }
}

/// Run the main send/receive loop for one WS connection.
/// Returns `true` if a Shutdown command was received (caller should exit),
/// `false` if the WS died and we should reconnect.
async fn relay_loop(
    ws:      &mut (impl SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin),
    rx:      &mut mpsc::Receiver<RelayCommand>,
    adapter: &NostrAdapter,
) -> bool {
    loop {
        tokio::select! {
            // Outbound: publish events from handle
            cmd = rx.recv() => {
                match cmd {
                    None | Some(RelayCommand::Shutdown) => return true,
                    Some(RelayCommand::Publish(envelope)) => {
                        match adapter.wrap(&envelope) {
                            Err(e) => warn!(error = %e, "failed to wrap DIP envelope for Nostr"),
                            Ok(event) => {
                                let msg = json!(["EVENT", event]);
                                if let Err(e) = ws.send(Message::Text(msg.to_string())).await {
                                    warn!(error = %e, "Nostr send failed");
                                    return false;
                                }
                                debug!(msg_id = %envelope.message_id, "published DIP envelope to Nostr");
                            }
                        }
                    }
                }
            }

            // Inbound: receive events from relay
            msg = ws.next() => {
                match msg {
                    None => {
                        warn!("Nostr relay WS stream ended");
                        return false;
                    }
                    Some(Err(e)) => {
                        warn!(error = %e, "Nostr WS receive error");
                        return false;
                    }
                    Some(Ok(Message::Text(txt))) => {
                        handle_relay_message(&txt, adapter);
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        let _ = ws.send(Message::Pong(payload)).await;
                    }
                    Some(Ok(Message::Close(_))) => {
                        info!("Nostr relay sent Close");
                        return false;
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Parse and log a relay message.  Inbound DIP envelopes are logged;
/// production would route them through the DipRouter.
fn handle_relay_message(text: &str, adapter: &NostrAdapter) {
    let Ok(val): Result<Value, _> = serde_json::from_str(text) else { return };
    let Some(arr) = val.as_array() else { return };
    if arr.is_empty() { return }

    match arr[0].as_str() {
        Some("EVENT") if arr.len() >= 3 => {
            let Some(event_val) = arr.get(2) else { return };
            match serde_json::from_value::<NostrEvent>(event_val.clone()) {
                Err(e) => debug!(error = %e, "could not parse Nostr event"),
                Ok(event) => match adapter.unwrap(&event) {
                    Err(e) => debug!(error = %e, "received non-DIP Nostr event"),
                    Ok(envelope) => {
                        info!(
                            msg_id = %envelope.message_id,
                            kind   = ?envelope.kind,
                            "received inbound DIP envelope via Nostr"
                        );
                        // Production: route through DipRouter + local handler
                    }
                }
            }
        }
        Some("OK") if arr.len() >= 3 => {
            let id     = arr[1].as_str().unwrap_or("?");
            let ok     = arr[2].as_bool().unwrap_or(false);
            let reason = arr.get(3).and_then(|v| v.as_str()).unwrap_or("");
            if ok {
                debug!(event_id = id, "Nostr relay accepted event");
            } else {
                warn!(event_id = id, reason = reason, "Nostr relay rejected event");
            }
        }
        Some("NOTICE") => {
            let msg = arr.get(1).and_then(|v| v.as_str()).unwrap_or("(empty)");
            info!(notice = msg, "Nostr relay notice");
        }
        Some("EOSE") => {
            let sub = arr.get(1).and_then(|v| v.as_str()).unwrap_or("?");
            debug!(sub_id = sub, "Nostr relay EOSE (end of stored events)");
        }
        _ => {}
    }
}

async fn sleep_backoff(backoff_secs: &mut u64) {
    tokio::time::sleep(Duration::from_secs(*backoff_secs)).await;
    *backoff_secs = (*backoff_secs * 2).min(60);
}
