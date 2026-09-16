//! OsoRouter — sovereign transport cascade for OSO lifecycle signals.
//!
//! Implements the fallback chain: IP/Vantage → Nostr → Freenet → Meshtastic → BT.
//! Each leg is tried in order; first successful send wins.
//! Failure logging and per-leg metrics are recorded in the OsoRouteTrace.

use serde::{Deserialize, Serialize};
use crate::envelope::DipEnvelope;
use crate::address::DipNetwork;
use crate::error::{DipError, DipResult};
use crate::adapter::DipAdapter;

/// Ordered transport legs in the OSO fallback cascade.
/// Order = try first → try last.
pub const OSO_CASCADE: &[DipNetwork] = &[
    DipNetwork::Vantage,    // L0: direct IP / local Vantage node (fastest)
    DipNetwork::Nostr,      // L1: Nostr relay network (global, always-on)
    DipNetwork::Freenet,    // L2: Freenet mutable state (higher latency, offline-tolerant)
    DipNetwork::Meshtastic, // L3: LoRa mesh (offline, very low bandwidth)
    // Bluetooth is hardware-gated — included in the spec but no adapter yet.
];

/// One attempt in the cascade.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CascadeLeg {
    pub network: DipNetwork,
    pub outcome: LegOutcome,
    /// Milliseconds taken for this attempt.
    pub latency_ms: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegOutcome {
    Success,
    /// Adapter not registered on this node (skip, not an error).
    AdapterMissing,
    /// Adapter present but send failed.
    Failed(String),
}

/// Full trace of one `OsoRouter::send` call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsoRouteTrace {
    pub message_id: String,
    pub legs: Vec<CascadeLeg>,
    /// Which leg (if any) delivered successfully.
    pub delivered_via: Option<DipNetwork>,
}

impl OsoRouteTrace {
    pub fn succeeded(&self) -> bool {
        self.delivered_via.is_some()
    }
}

/// Registered transport adapter slot — owns one `DipAdapter` impl per network.
struct Slot {
    network: DipNetwork,
    adapter: Box<dyn DipAdapter>,
}

/// The sovereign transport cascade router.
///
/// Register adapters for each available network; `send()` walks the cascade
/// and returns a trace regardless of success or failure.
pub struct OsoRouter {
    slots: Vec<Slot>,
}

impl OsoRouter {
    pub fn new() -> Self {
        Self { slots: vec![] }
    }

    /// Register a transport adapter. Multiple adapters per network are NOT
    /// supported — the most recently registered one wins.
    pub fn register(&mut self, network: DipNetwork, adapter: Box<dyn DipAdapter>) {
        self.slots.retain(|s| s.network != network);
        self.slots.push(Slot { network, adapter });
    }

    fn find_adapter(&self, network: &DipNetwork) -> Option<&dyn DipAdapter> {
        self.slots.iter().find(|s| &s.network == network).map(|s| s.adapter.as_ref())
    }

    /// Send `envelope` via the cascade, returning a full route trace.
    ///
    /// The cascade is always walked in `OSO_CASCADE` order regardless of the
    /// envelope's destination network, so control signals reach the agent via
    /// whatever transport is available.
    pub async fn send(&self, envelope: &DipEnvelope) -> OsoRouteTrace {
        let mut legs = vec![];

        for network in OSO_CASCADE {
            let start = std::time::Instant::now();
            match self.find_adapter(network) {
                None => {
                    legs.push(CascadeLeg {
                        network: network.clone(),
                        outcome: LegOutcome::AdapterMissing,
                        latency_ms: 0,
                    });
                }
                Some(adapter) => {
                    let result = adapter.send(envelope).await;
                    let latency_ms = start.elapsed().as_millis() as u32;
                    match result {
                        Ok(()) => {
                            legs.push(CascadeLeg {
                                network: network.clone(),
                                outcome: LegOutcome::Success,
                                latency_ms,
                            });
                            return OsoRouteTrace {
                                message_id: envelope.message_id.clone(),
                                legs,
                                delivered_via: Some(network.clone()),
                            };
                        }
                        Err(e) => {
                            legs.push(CascadeLeg {
                                network: network.clone(),
                                outcome: LegOutcome::Failed(e.to_string()),
                                latency_ms,
                            });
                        }
                    }
                }
            }
        }

        OsoRouteTrace {
            message_id: envelope.message_id.clone(),
            legs,
            delivered_via: None,
        }
    }

    /// Route-or-error convenience wrapper: returns `DipError::NoAdapter` if
    /// all legs in the cascade failed or were missing.
    pub async fn send_required(&self, envelope: &DipEnvelope) -> DipResult<OsoRouteTrace> {
        let trace = self.send(envelope).await;
        if trace.succeeded() {
            Ok(trace)
        } else {
            Err(DipError::NoAdapter(DipNetwork::Vantage)) // "no viable transport"
        }
    }
}

impl Default for OsoRouter {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::StubAdapter;
    use crate::envelope::{DipEnvelope, DipKind};
    use crate::address::DipAddress;
    use sovereign_types::{IdentityChain, crypto::generate_keypair};

    fn make_envelope() -> DipEnvelope {
        let (priv_key, _) = generate_keypair();
        DipEnvelope::build(
            DipAddress::vantage("did:v:sender"),
            DipAddress::nostr("npub1test"),
            IdentityChain::new("did:p:1".into(), "did:a:1".into()),
            DipKind::Message,
            serde_json::json!({"op": "heartbeat"}),
            300,
            &priv_key,
        ).unwrap()
    }

    #[tokio::test]
    async fn delivers_via_first_available() {
        let mut router = OsoRouter::new();
        let stub = StubAdapter::new("nostr");
        let sent = stub.sent.clone();
        router.register(DipNetwork::Nostr, Box::new(stub));

        let trace = router.send(&make_envelope()).await;
        assert!(trace.succeeded());
        assert_eq!(trace.delivered_via, Some(DipNetwork::Nostr));
        assert_eq!(sent.lock().await.len(), 1);
    }

    #[tokio::test]
    async fn all_missing_returns_undelivered() {
        let router = OsoRouter::new(); // no adapters
        let trace = router.send(&make_envelope()).await;
        assert!(!trace.succeeded());
        assert!(trace.delivered_via.is_none());
        // All legs should be AdapterMissing
        for leg in &trace.legs {
            assert!(matches!(leg.outcome, LegOutcome::AdapterMissing));
        }
    }

    #[tokio::test]
    async fn send_required_errors_on_no_transport() {
        let router = OsoRouter::new();
        let result = router.send_required(&make_envelope()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn cascade_order_matches_constant() {
        let router = OsoRouter::new();
        let trace = router.send(&make_envelope()).await;
        let networks: Vec<&DipNetwork> = trace.legs.iter().map(|l| &l.network).collect();
        assert_eq!(networks, OSO_CASCADE.iter().collect::<Vec<_>>());
    }
}
