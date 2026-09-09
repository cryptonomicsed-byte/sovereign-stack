//! Receipt persistence — writes completed job receipts to disk in data_dir.
//!
//! On job completion, the SceneReceipt (kind 31030) JSON is written to:
//!   {data_dir}/receipts/{twin_id}.json
//!
//! On node restart, all existing receipts are loaded into a read-only cache
//! so the REST API can serve them without rerunning the pipeline.
//!
//! File format: newline-delimited JSON lines with a header.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{info, warn};

/// A persisted receipt record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiptRecord {
    /// Receipt kind (31020 = CaptureReceipt, 31030 = SceneReceipt).
    pub kind:             u32,
    pub receipt_id:       String,
    pub twin_id:          String,
    pub device_id:        String,
    pub scene_receipt_id: String,
    pub capture_receipt_id: String,
    pub sui_object_id:    Option<String>,
    pub dip_message_count: usize,
    pub completed_at:     u64,
}

/// Thread-safe receipt cache backed by disk.
#[derive(Clone)]
pub struct ReceiptStore {
    dir:   PathBuf,
    cache: Arc<RwLock<Vec<ReceiptRecord>>>,
}

impl ReceiptStore {
    /// Open (or create) the receipt store at `{data_dir}/receipts/`.
    pub async fn open(data_dir: &Path) -> Self {
        let dir = data_dir.join("receipts");
        if let Err(e) = tokio::fs::create_dir_all(&dir).await {
            warn!(dir = %dir.display(), error = %e, "could not create receipts dir");
        }

        let cache = load_all(&dir).await;
        info!(dir = %dir.display(), count = cache.len(), "receipt store loaded");

        Self { dir, cache: Arc::new(RwLock::new(cache)) }
    }

    /// Persist a new receipt record and add it to the in-memory cache.
    pub async fn save(&self, record: ReceiptRecord) {
        let path = self.dir.join(format!("{}.json", sanitize(&record.twin_id)));
        let json = match serde_json::to_string_pretty(&record) {
            Ok(j) => j,
            Err(e) => { warn!(error = %e, "failed to serialize receipt"); return; }
        };
        if let Err(e) = tokio::fs::write(&path, &json).await {
            warn!(path = %path.display(), error = %e, "failed to write receipt");
        } else {
            info!(twin_id = %record.twin_id, path = %path.display(), "receipt persisted");
        }
        self.cache.write().await.push(record);
    }

    /// List all persisted receipts (most recent first).
    pub async fn list(&self) -> Vec<ReceiptRecord> {
        let mut records = self.cache.read().await.clone();
        records.sort_by(|a, b| b.completed_at.cmp(&a.completed_at));
        records
    }

    /// Look up a receipt by twin_id.
    pub async fn get_by_twin(&self, twin_id: &str) -> Option<ReceiptRecord> {
        self.cache.read().await
            .iter()
            .find(|r| r.twin_id == twin_id)
            .cloned()
    }

    /// Count of persisted receipts.
    pub async fn count(&self) -> usize {
        self.cache.read().await.len()
    }
}

async fn load_all(dir: &Path) -> Vec<ReceiptRecord> {
    let mut records = vec![];
    let mut rd = match tokio::fs::read_dir(dir).await {
        Ok(r) => r,
        Err(_) => return records,
    };
    while let Ok(Some(entry)) = rd.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") { continue; }
        match tokio::fs::read_to_string(&path).await {
            Err(e) => warn!(path = %path.display(), error = %e, "could not read receipt"),
            Ok(text) => match serde_json::from_str::<ReceiptRecord>(&text) {
                Err(e) => warn!(path = %path.display(), error = %e, "could not parse receipt"),
                Ok(r)  => records.push(r),
            }
        }
    }
    records
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}
