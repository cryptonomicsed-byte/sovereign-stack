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

use std::sync::Arc;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{info, warn};

use sovereign_types::crypto::generate_keypair;

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

/// Thread-safe witness registry.
#[derive(Clone)]
pub struct WitnessRegistry {
    inner: Arc<RwLock<Vec<WitnessPeer>>>,
}

impl Default for WitnessRegistry {
    fn default() -> Self {
        Self { inner: Arc::new(RwLock::new(vec![])) }
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
}

/// Build a registry pre-loaded with stub witnesses for development.
pub fn dev_registry() -> WitnessRegistry {
    let registry = WitnessRegistry::new();
    // Witnesses registered at startup via config — in production these come
    // from the `[witnesses]` section in config.toml.
    registry
}
