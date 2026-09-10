//! Agent memory store — local key-value memory with namespaced entries.
//! Persisted to disk as JSON. Used by Omo-Koda for cross-invocation recall.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use sovereign_types::identity::Timestamp;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum MemoryNamespace {
    /// Core agent facts — who I am, my did, key relationships.
    Identity,
    /// Working memory — current task context, recent observations.
    Working,
    /// Long-term episodic memory — past events and outcomes.
    Episodic,
    /// Semantic memory — learned facts about the world.
    Semantic,
    /// Custom namespace
    Custom(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub key:       String,
    pub value:     serde_json::Value,
    pub namespace: MemoryNamespace,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub ttl_secs:   Option<u64>,
}

impl MemoryEntry {
    pub fn new(ns: MemoryNamespace, key: String, value: serde_json::Value, now: Timestamp) -> Self {
        Self { key, value, namespace: ns, created_at: now, updated_at: now, ttl_secs: None }
    }

    pub fn is_expired(&self, now: Timestamp) -> bool {
        self.ttl_secs.map_or(false, |ttl| {
            now > self.updated_at + ttl * 1000
        })
    }
}

#[derive(Clone, Default)]
pub struct MemoryStore {
    entries: Arc<RwLock<HashMap<(MemoryNamespace, String), MemoryEntry>>>,
}

impl MemoryStore {
    pub fn new() -> Self { Self::default() }

    pub async fn set(&self, entry: MemoryEntry) {
        self.entries.write().await
            .insert((entry.namespace.clone(), entry.key.clone()), entry);
    }

    pub async fn get(&self, ns: &MemoryNamespace, key: &str) -> Option<MemoryEntry> {
        self.entries.read().await.get(&(ns.clone(), key.to_string())).cloned()
    }

    pub async fn get_namespace(&self, ns: &MemoryNamespace) -> Vec<MemoryEntry> {
        self.entries.read().await
            .values()
            .filter(|e| &e.namespace == ns)
            .cloned()
            .collect()
    }

    pub async fn delete(&self, ns: &MemoryNamespace, key: &str) -> bool {
        self.entries.write().await
            .remove(&(ns.clone(), key.to_string()))
            .is_some()
    }

    pub async fn len(&self) -> usize {
        self.entries.read().await.len()
    }

    pub async fn prune_expired(&self, now: Timestamp) -> usize {
        let mut store = self.entries.write().await;
        let before = store.len();
        store.retain(|_, e| !e.is_expired(now));
        before - store.len()
    }
}
