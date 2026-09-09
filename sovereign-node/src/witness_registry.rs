//! Peer witness registry — maintains a list of trusted simulation witnesses.
//!
//! Witnesses co-sign ỌSỌVM SimulationReceipts during proof chain runs.
//! Each witness has a DID and a signing key pair.
//!
//! Witnesses are configured in config.toml as static entries or discovered
//! dynamically via DIP capability exchange (production).
//!
//! The proof chain requires >= 2 witnesses. If fewer are configured, the
//! node generates ephemeral stub witnesses for development use only.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{RwLock, oneshot};
use tracing::{info, warn};

use sovereign_types::{WitnessAttestation, crypto::generate_keypair};

/// A registered witness peer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitnessPeer {
    pub did:        String,
    pub public_key: String,
    /// Signing key — only held locally when this node IS the witness.
    /// None for remote witnesses (they sign via DIP exchange in production).
    #[serde(skip)]
    pub private_key: Option<String>,
}

/// Pending remote witness signature request — job_id → oneshot sender.
type PendingSignatures = Arc<RwLock<HashMap<String, oneshot::Sender<WitnessAttestation>>>>;

/// Thread-safe witness registry.
#[derive(Clone)]
pub struct WitnessRegistry {
    inner:   Arc<RwLock<Vec<WitnessPeer>>>,
    pending: PendingSignatures,
}

impl Default for WitnessRegistry {
    fn default() -> Self {
        Self {
            inner:   Arc::new(RwLock::new(vec![])),
            pending: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl WitnessRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a witness from config (static).
    pub async fn register(&self, peer: WitnessPeer) {
        let mut inner = self.inner.write().await;
        if !inner.iter().any(|p| p.did == peer.did) {
            info!(did = %peer.did, "witness registered");
            inner.push(peer);
        }
    }

    /// Number of registered witnesses.
    pub async fn count(&self) -> usize {
        self.inner.read().await.len()
    }

    /// Return all registered witness peers.
    pub async fn inner_peers(&self) -> Vec<WitnessPeer> {
        self.inner.read().await.clone()
    }

    /// Return all witnesses that have a private_key (i.e. locally held).
    /// These can actually co-sign. Remote witnesses require DIP exchange.
    pub async fn local_signers(&self) -> Vec<WitnessPeer> {
        self.inner.read().await
            .iter()
            .filter(|w| w.private_key.is_some())
            .cloned()
            .collect()
    }

    /// Return at least `min_count` (did, key) pairs for a proof chain.
    ///
    /// Preference order:
    ///   1. Local witnesses (have private_key)
    ///   2. Stub ephemeral witnesses (dev only — logged as warning)
    ///
    /// In production, remote witnesses would sign via DIP request/grant.
    pub async fn get_witnesses_for_proof(
        &self,
        min_count: usize,
    ) -> Vec<(String, String)> {
        let mut result = vec![];

        // Collect local signers
        for w in self.local_signers().await {
            if let Some(key) = w.private_key {
                result.push((w.did, key));
            }
        }

        // Pad with ephemeral stubs if needed
        let needed = min_count.saturating_sub(result.len());
        if needed > 0 {
            warn!(
                have   = result.len(),
                needed = min_count,
                stubs  = needed,
                "using ephemeral stub witnesses — configure real witnesses in config.toml"
            );
            for i in 0..needed {
                let (priv_key, _) = generate_keypair();
                result.push((
                    format!("did:witness:stub:{:02}", i + 1),
                    priv_key,
                ));
            }
        }

        result
    }

    /// Send a witness sign request over DIP and wait up to `timeout` for the response.
    ///
    /// Returns `Some(WitnessAttestation)` on success, `None` on timeout or error.
    pub async fn request_remote_signature(
        &self,
        job_id:     &str,
        commitment: &str,
        witness:    &WitnessPeer,
        gateway:    &crate::dip_gateway::DipGateway,
        identity:   &crate::identity::NodeIdentity,
        timeout:    Duration,
    ) -> Option<WitnessAttestation> {
        use dip::{DipEnvelope, DipKind, DipAddress, address::DipNetwork};
        use sovereign_types::IdentityChain;

        let (tx, rx) = oneshot::channel::<WitnessAttestation>();
        self.pending.write().await.insert(job_id.to_string(), tx);

        let payload = serde_json::json!({
            "type":          "witness_sign_request",
            "job_id":        job_id,
            "commitment":    commitment,
            "requester_did": identity.did,
        });

        let chain  = IdentityChain::new(identity.did.clone(), identity.did.clone());
        let origin = DipAddress::vantage(&identity.did);
        let dest   = DipAddress {
            network: DipNetwork::Vantage,
            address: witness.did.clone(),
            did:     Some(witness.did.clone()),
        };

        match DipEnvelope::build(origin, dest, chain, DipKind::Capability, payload, 120, &identity.private_key) {
            Ok(env) => gateway.send(env).await,
            Err(e)  => {
                warn!(error = %e, job_id = %job_id, "failed to build witness sign request");
                self.pending.write().await.remove(job_id);
                return None;
            }
        }

        info!(
            job_id  = %job_id,
            witness = %witness.did,
            "witness sign request sent — waiting up to {}s",
            timeout.as_secs()
        );

        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(attestation)) => {
                info!(job_id = %job_id, "witness signature received");
                Some(attestation)
            }
            _ => {
                warn!(job_id = %job_id, witness = %witness.did, "witness sign request timed out");
                self.pending.write().await.remove(job_id);
                None
            }
        }
    }

    /// Complete a pending witness sign request from an inbound DIP response payload.
    pub async fn complete_pending_signature(&self, payload: &serde_json::Value) {
        use sovereign_types::WitnessAttestation;

        let job_id     = payload.get("job_id").and_then(|v| v.as_str()).unwrap_or("");
        let signer_did = payload.get("signer_did").and_then(|v| v.as_str()).unwrap_or("");
        let signature  = payload.get("signature").and_then(|v| v.as_str()).unwrap_or("");
        let commitment = payload.get("commitment").and_then(|v| v.as_str()).unwrap_or("");

        if job_id.is_empty() || signer_did.is_empty() || signature.is_empty() {
            warn!("incomplete witness_sign_response payload");
            return;
        }

        let attestation = WitnessAttestation {
            witness_id:        signer_did.to_string(),
            merkle_commitment: commitment.to_string(),
            timestamp:         now_ms(),
            signature:         signature.to_string(),
        };

        let mut pending = self.pending.write().await;
        if let Some(tx) = pending.remove(job_id) {
            let _ = tx.send(attestation);
            info!(job_id = %job_id, signer = %signer_did, "witness signature completed");
        } else {
            warn!(job_id = %job_id, "no pending request for witness sign response");
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Build a registry pre-loaded with stub witnesses for development.
pub fn dev_registry() -> WitnessRegistry {
    let registry = WitnessRegistry::new();
    // Witnesses registered at startup via config — in production these come
    // from the `[witnesses]` section in config.toml.
    registry
}
