/// In-memory store for active BodySessions and completed FlightReceipts.
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use vcp::{BodySession, FlightReceipt};

#[derive(Clone, Default)]
pub struct BodyStore {
    sessions:  Arc<RwLock<HashMap<String, BodySession>>>,
    receipts:  Arc<RwLock<Vec<FlightReceipt>>>,
}

impl BodyStore {
    pub fn new() -> Self { Self::default() }

    pub async fn insert_session(&self, s: BodySession) {
        self.sessions.write().await.insert(s.session_id.clone(), s);
    }

    pub async fn get_session(&self, id: &str) -> Option<BodySession> {
        self.sessions.read().await.get(id).cloned()
    }

    pub async fn all_sessions(&self) -> Vec<BodySession> {
        self.sessions.read().await.values().cloned().collect()
    }

    pub async fn add_receipt(&self, r: FlightReceipt) {
        self.receipts.write().await.push(r);
    }

    pub async fn receipts_for_body(&self, body_id: &str) -> Vec<FlightReceipt> {
        self.receipts.read().await
            .iter()
            .filter(|r| r.body_id == body_id)
            .cloned()
            .collect()
    }

    pub async fn all_receipts(&self) -> Vec<FlightReceipt> {
        self.receipts.read().await.clone()
    }
}
