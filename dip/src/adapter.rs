use async_trait::async_trait;
use crate::envelope::DipEnvelope;
use crate::error::DipResult;

/// The two-function interface every DIP adapter must implement.
/// to_dip: native message → DIP envelope
/// from_dip: DIP envelope → native message bytes (for the target network)
#[async_trait]
pub trait DipAdapter: Send + Sync {
    /// Adapter name (matches DipNetwork variant)
    fn name(&self) -> &'static str;

    /// Convert a native message (raw bytes) into a DIP envelope.
    async fn to_dip(&self, native: &[u8]) -> DipResult<DipEnvelope>;

    /// Convert a DIP envelope into native message bytes for this network.
    async fn from_dip(&self, envelope: &DipEnvelope) -> DipResult<Vec<u8>>;

    /// Send a DIP envelope onto this network.
    async fn send(&self, envelope: &DipEnvelope) -> DipResult<()>;
}

/// Stub adapter for testing — captures sent envelopes in memory.
pub struct StubAdapter {
    pub name_str: &'static str,
    pub sent: std::sync::Arc<tokio::sync::Mutex<Vec<DipEnvelope>>>,
}

impl StubAdapter {
    pub fn new(name: &'static str) -> Self {
        Self { name_str: name, sent: Default::default() }
    }
}

#[async_trait]
impl DipAdapter for StubAdapter {
    fn name(&self) -> &'static str { self.name_str }

    async fn to_dip(&self, _native: &[u8]) -> DipResult<DipEnvelope> {
        unimplemented!("stub to_dip")
    }

    async fn from_dip(&self, _envelope: &DipEnvelope) -> DipResult<Vec<u8>> {
        Ok(serde_json::to_vec(_envelope)?)
    }

    async fn send(&self, envelope: &DipEnvelope) -> DipResult<()> {
        self.sent.lock().await.push(envelope.clone());
        Ok(())
    }
}
