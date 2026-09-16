//! StorageProvider trait — abstract interface over Walrus/Arweave/local/Freenet backends.
//!
//! Backends implement this trait; callers (OSOVM, Omo-Koda2, Vantage) program to it.
//! Storage operations are content-addressed: write returns a CID, read takes that CID.

use serde::{Deserialize, Serialize};

/// Content-addressed storage identifier (multihash-compatible hex string).
pub type StorageCid = String;

/// Metadata returned alongside stored content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageReceipt {
    pub cid: StorageCid,
    /// Backend that actually stored the content.
    pub backend: StorageBackend,
    /// Byte length of the stored payload.
    pub size_bytes: u64,
    /// Unix timestamp when the backend confirmed storage.
    pub confirmed_at: u64,
    /// Backend-native reference (e.g. Walrus blob ID, Arweave tx, Freenet key).
    pub native_ref: Option<String>,
}

/// Which physical storage backend handled the operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageBackend {
    /// Walrus decentralised blob store (primary for agent public state).
    Walrus,
    /// Arweave permanent web (immutable genesis records, DNAs).
    Arweave,
    /// Local filesystem / in-process (development / offline mode).
    Local,
    /// Freenet mutable contract state (mutable agent working memory).
    Freenet,
    /// Nostr replaceable event (kind 30000-range, small payloads only).
    NostrEvent,
}

/// Preference ordering when multiple backends are available.
///
/// Storage providers should attempt backends left-to-right and
/// fall back on network / capacity errors.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoragePolicy {
    pub preferred: StorageBackend,
    pub fallbacks: Vec<StorageBackend>,
    /// Minimum redundancy: how many backends must confirm before the
    /// write is considered durable. 1 = any single backend suffices.
    pub min_confirmations: u8,
}

impl Default for StoragePolicy {
    fn default() -> Self {
        Self {
            preferred: StorageBackend::Walrus,
            fallbacks: vec![StorageBackend::Local],
            min_confirmations: 1,
        }
    }
}

/// Capability an agent can grant to a third party for a specific CID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageCapability {
    /// Allow reading the content at a CID.
    Read,
    /// Allow writing new content (creates new CID, does not overwrite).
    Write,
    /// Allow deleting / unpinning content (where backend supports it).
    Delete,
}

/// Result type for storage operations (concrete error type per backend).
pub type StorageResult<T> = Result<T, StorageError>;

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("backend {0:?} unavailable: {1}")]
    BackendUnavailable(StorageBackend, String),
    #[error("CID not found: {0}")]
    NotFound(StorageCid),
    #[error("access denied for CID {0}")]
    AccessDenied(StorageCid),
    #[error("payload too large: {size} bytes (max {max})")]
    PayloadTooLarge { size: u64, max: u64 },
    #[error("encoding error: {0}")]
    Encoding(String),
}

/// Abstract storage interface all backends must implement.
///
/// This is NOT async_trait — callers bridge to their async runtime.
/// Implementations that require async wrap this in an executor or
/// expose a companion `AsyncStorageProvider` shim.
pub trait StorageProvider: Send + Sync {
    /// Store `payload` according to `policy`, return a `StorageReceipt`.
    fn store(&self, payload: &[u8], policy: &StoragePolicy) -> StorageResult<StorageReceipt>;

    /// Retrieve content by CID.
    fn fetch(&self, cid: &StorageCid) -> StorageResult<Vec<u8>>;

    /// Check whether a CID is reachable without downloading the full payload.
    fn exists(&self, cid: &StorageCid) -> bool;

    /// Compute what CID *would* be assigned to `payload` without storing it.
    /// Allows callers to deduplicate before committing to storage costs.
    fn preview_cid(&self, payload: &[u8]) -> StorageCid;

    /// Return the backend enum variant this provider represents.
    fn backend(&self) -> StorageBackend;
}

/// Registry that routes storage calls to the right backend.
pub struct StorageRouter {
    providers: Vec<Box<dyn StorageProvider>>,
}

impl StorageRouter {
    pub fn new() -> Self {
        Self { providers: vec![] }
    }

    pub fn register(&mut self, provider: Box<dyn StorageProvider>) {
        self.providers.push(provider);
    }

    fn find(&self, backend: &StorageBackend) -> Option<&dyn StorageProvider> {
        self.providers.iter().find(|p| &p.backend() == backend).map(|p| p.as_ref())
    }

    /// Store using the given policy, falling back to alternatives on failure.
    pub fn store(&self, payload: &[u8], policy: &StoragePolicy) -> StorageResult<StorageReceipt> {
        let order = std::iter::once(&policy.preferred).chain(policy.fallbacks.iter());
        let mut last_err = StorageError::BackendUnavailable(policy.preferred.clone(), "no providers registered".into());
        for backend in order {
            if let Some(provider) = self.find(backend) {
                match provider.store(payload, policy) {
                    Ok(receipt) => return Ok(receipt),
                    Err(e) => last_err = e,
                }
            }
        }
        Err(last_err)
    }

    /// Fetch from first backend that has the CID.
    pub fn fetch(&self, cid: &StorageCid) -> StorageResult<Vec<u8>> {
        for provider in &self.providers {
            if provider.exists(cid) {
                return provider.fetch(cid);
            }
        }
        Err(StorageError::NotFound(cid.clone()))
    }
}

impl Default for StorageRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Sha256, Digest};

    struct MockLocal {
        store: std::collections::HashMap<StorageCid, Vec<u8>>,
    }

    impl MockLocal {
        fn new() -> Self { Self { store: Default::default() } }
        fn sha_cid(payload: &[u8]) -> StorageCid {
            let mut h = Sha256::new();
            h.update(payload);
            hex::encode(h.finalize())
        }
    }

    impl StorageProvider for MockLocal {
        fn store(&self, payload: &[u8], _policy: &StoragePolicy) -> StorageResult<StorageReceipt> {
            let cid = Self::sha_cid(payload);
            Ok(StorageReceipt {
                cid: cid.clone(),
                backend: StorageBackend::Local,
                size_bytes: payload.len() as u64,
                confirmed_at: 0,
                native_ref: None,
            })
        }
        fn fetch(&self, cid: &StorageCid) -> StorageResult<Vec<u8>> {
            self.store.get(cid).cloned().ok_or_else(|| StorageError::NotFound(cid.clone()))
        }
        fn exists(&self, cid: &StorageCid) -> bool { self.store.contains_key(cid) }
        fn preview_cid(&self, payload: &[u8]) -> StorageCid { Self::sha_cid(payload) }
        fn backend(&self) -> StorageBackend { StorageBackend::Local }
    }

    #[test]
    fn preview_cid_is_deterministic() {
        let p = MockLocal::new();
        let a = p.preview_cid(b"hello");
        let b = p.preview_cid(b"hello");
        assert_eq!(a, b);
    }

    #[test]
    fn router_falls_back() {
        let mut router = StorageRouter::new();
        router.register(Box::new(MockLocal::new()));
        let policy = StoragePolicy {
            preferred: StorageBackend::Walrus,
            fallbacks: vec![StorageBackend::Local],
            min_confirmations: 1,
        };
        let receipt = router.store(b"payload", &policy).unwrap();
        assert_eq!(receipt.backend, StorageBackend::Local);
    }
}
