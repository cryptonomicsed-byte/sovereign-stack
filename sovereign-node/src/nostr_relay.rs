//! Nostr relay stub — the full Nostr relay implementation has been moved to the
//! ip-layer crate.  This module provides the minimal NostrRelayHandle type so
//! that dip_gateway.rs compiles while ip-layer handles actual connectivity.

use tokio::sync::mpsc;
use dip::DipEnvelope;

/// Handle to a Nostr relay connection.
/// In production ip-layer manages the relay; this stub satisfies the type system.
#[derive(Clone)]
pub struct NostrRelayHandle {
    /// Channel for publishing outbound DIP envelopes to the Nostr relay.
    #[allow(dead_code)]
    pub(crate) tx: mpsc::Sender<DipEnvelope>,
}

impl NostrRelayHandle {
    /// Create a disconnected stub handle (no actual relay connection).
    pub fn stub() -> Self {
        let (tx, _rx) = mpsc::channel(1);
        Self { tx }
    }

    /// Publish a DIP envelope to the Nostr relay (no-op stub).
    pub async fn publish(&self, _envelope: &DipEnvelope) {
        // No-op: ip-layer handles actual Nostr publishing.
    }
}

/// Spawn a Nostr relay connection and return a handle.
/// Stub implementation — real relay logic lives in ip-layer.
pub fn spawn_nostr_relay(
    _relay_url:  String,
    _npub:       String,
    _private_key: String,
    _inbound_tx: Option<mpsc::Sender<DipEnvelope>>,
) -> NostrRelayHandle {
    tracing::info!("nostr relay stub — real relay is managed by ip-layer");
    NostrRelayHandle::stub()
}
