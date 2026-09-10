//! WebSocket endpoints.
//!
//! GET /ws/twin/:twin_id  — real-time TwinEvent JSON stream
//! GET /ws/splat/:twin_id — one-shot PLY binary stream (send bytes, then close)
//!
//! twin filter:
//!   /ws/twin/<id>  — only events where event.twin_id() == id, plus all
//!                    CaptureFailed / StatusUpdate events (no twin_id yet)
//!   /ws/twin/*     — all events

use axum::{
    extract::{Path, State, WebSocketUpgrade},
    http::StatusCode,
    response::IntoResponse,
};
use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

use crate::events::TwinEvent;
use crate::node::NodeState;

/// GET /ws/twin/:twin_id
pub async fn handle_ws_twin(
    ws:              WebSocketUpgrade,
    Path(twin_id):   Path<String>,
    State(state):    State<NodeState>,
) -> impl IntoResponse {
    let rx = state.twin_events.subscribe();
    ws.on_upgrade(move |socket| twin_ws_stream(socket, twin_id, rx))
}

async fn twin_ws_stream(
    socket:    WebSocket,
    filter_id: String,
    mut rx:    broadcast::Receiver<TwinEvent>,
) {
    let (mut tx, mut reader) = socket.split();
    info!(twin_id = %filter_id, "WS twin stream opened");

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(event) => {
                        let pass = filter_id == "*"
                            || event.twin_id().map(|id| id == filter_id).unwrap_or(true);
                        if pass {
                            let json = match serde_json::to_string(&event) {
                                Ok(s)  => s,
                                Err(e) => { warn!("WS serialize: {e}"); continue; }
                            };
                            if tx.send(Message::Text(json.into())).await.is_err() {
                                debug!(twin_id = %filter_id, "WS client disconnected");
                                break;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(twin_id = %filter_id, skipped = n, "WS broadcast lagged");
                    }
                }
            }
            msg = reader.next() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => {
                        debug!(twin_id = %filter_id, "WS client closed");
                        break;
                    }
                    _ => {}
                }
            }
        }
    }
    info!(twin_id = %filter_id, "WS twin stream closed");
}

// ---------------------------------------------------------------------------
// GET /ws/splat/:twin_id
// Streams the pre-built PLY file for a twin over binary WebSocket frames,
// then closes. Returns 404 via WS close-reason if no PLY exists.
// ---------------------------------------------------------------------------

/// GET /ws/splat/:twin_id
pub async fn handle_ws_splat(
    ws:            WebSocketUpgrade,
    Path(twin_id): Path<String>,
    State(state):  State<NodeState>,
) -> impl IntoResponse {
    let splat_dir = state.config.pipeline.splat_output_dir.clone();
    ws.on_upgrade(move |socket| splat_ws_stream(socket, twin_id, splat_dir))
}

async fn splat_ws_stream(
    socket:    WebSocket,
    twin_id:   String,
    splat_dir: Option<String>,
) {
    let (mut tx, _rx) = socket.split();

    let ply_path = match splat_dir {
        Some(dir) => {
            let safe = twin_id.replace([':', '/'], "_");
            std::path::PathBuf::from(dir)
                .join(&safe)
                .join("export")
                .join("splat.ply")
        }
        None => {
            warn!(twin_id = %twin_id, "splat_output_dir not configured");
            let _ = tx.send(Message::Close(None)).await;
            return;
        }
    };

    info!(twin_id = %twin_id, path = %ply_path.display(), "WS splat stream starting");

    match tokio::fs::read(&ply_path).await {
        Ok(bytes) => {
            // Send in 64 KiB chunks so large PLYs don't hit frame size limits.
            for chunk in bytes.chunks(65536) {
                if tx.send(Message::Binary(chunk.to_vec().into())).await.is_err() {
                    debug!(twin_id = %twin_id, "WS splat client disconnected mid-stream");
                    return;
                }
            }
            info!(twin_id = %twin_id, bytes = bytes.len(), "WS splat stream complete");
        }
        Err(e) => {
            warn!(twin_id = %twin_id, error = %e, "PLY file not found for twin");
        }
    }

    let _ = tx.send(Message::Close(None)).await;
}
