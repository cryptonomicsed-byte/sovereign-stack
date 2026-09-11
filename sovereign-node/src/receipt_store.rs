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
use twin_protocol::ObservationReceipt;

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
    /// Odù spatial tile where this capture occurred (e.g. "odu:5b").
    /// Derived from device GPS or manually set; None = location unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub odu_tile:         Option<String>,
}

/// Thread-safe receipt cache backed by disk.
#[derive(Clone)]
pub struct ReceiptStore {
    dir:              PathBuf,
    cache:            Arc<RwLock<Vec<ReceiptRecord>>>,
    obs_cache:        Arc<RwLock<Vec<ObservationReceipt>>>,
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

        // Load persisted ObservationReceipts from the obs sub-directory.
        let obs_dir = dir.join("observations");
        let _ = tokio::fs::create_dir_all(&obs_dir).await;
        let obs_cache = load_observations(&obs_dir).await;

        Self {
            dir,
            cache:     Arc::new(RwLock::new(cache)),
            obs_cache: Arc::new(RwLock::new(obs_cache)),
        }
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

    /// Look up all receipts tagged to a given Odù tile_id.
    pub async fn get_by_tile(&self, tile_id: &str) -> Vec<ReceiptRecord> {
        self.cache.read().await.iter()
            .filter(|r| r.odu_tile.as_deref() == Some(tile_id))
            .cloned()
            .collect()
    }

    /// Count of persisted receipts.
    pub async fn count(&self) -> usize {
        self.cache.read().await.len()
    }

    /// Persist an ObservationReceipt and add it to the in-memory cache.
    pub async fn add_observation(&self, obs: ObservationReceipt) {
        let obs_dir = self.dir.join("observations");
        let path = obs_dir.join(format!("{}.json", sanitize(&obs.receipt_id)));
        match serde_json::to_string_pretty(&obs) {
            Err(e) => { warn!(error = %e, "failed to serialize ObservationReceipt"); }
            Ok(json) => {
                if let Err(e) = tokio::fs::write(&path, &json).await {
                    warn!(path = %path.display(), error = %e, "failed to write ObservationReceipt");
                } else {
                    info!(receipt_id = %obs.receipt_id, path = %path.display(), "ObservationReceipt persisted");
                }
            }
        }
        self.obs_cache.write().await.push(obs);
    }

    /// Fetch a single ObservationReceipt by receipt_id.
    pub async fn get_observation(&self, receipt_id: &str) -> Option<ObservationReceipt> {
        self.obs_cache.read().await
            .iter()
            .find(|r| r.receipt_id == receipt_id)
            .cloned()
    }

    /// List all ObservationReceipts (most recent first by timestamp).
    pub async fn list_observations(&self) -> Vec<ObservationReceipt> {
        let mut obs = self.obs_cache.read().await.clone();
        obs.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        obs
    }

    /// Create an in-memory-only ReceiptStore (no disk I/O — for tests).
    pub fn in_memory() -> Self {
        Self {
            dir:       PathBuf::from("/dev/null"),
            cache:     Arc::new(RwLock::new(vec![])),
            obs_cache: Arc::new(RwLock::new(vec![])),
        }
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

async fn load_observations(dir: &Path) -> Vec<ObservationReceipt> {
    let mut records = vec![];
    let mut rd = match tokio::fs::read_dir(dir).await {
        Ok(r) => r,
        Err(_) => return records,
    };
    while let Ok(Some(entry)) = rd.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") { continue; }
        match tokio::fs::read_to_string(&path).await {
            Err(e) => warn!(path = %path.display(), error = %e, "could not read ObservationReceipt"),
            Ok(text) => match serde_json::from_str::<ObservationReceipt>(&text) {
                Err(e) => warn!(path = %path.display(), error = %e, "could not parse ObservationReceipt"),
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
