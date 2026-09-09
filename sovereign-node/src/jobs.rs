//! Async job store for capture pipeline runs.
//!
//! POST /capture/:device_id → job queued → tokio task runs pipeline → status updated
//! GET  /jobs/:id           → poll status
//! GET  /jobs               → list all jobs

use std::collections::HashMap;
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Completed {
        twin_id:            String,
        scene_receipt_id:   String,
        capture_receipt_id: String,
        sui_object_id:      Option<String>,
        dip_message_count:  usize,
    },
    Failed {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub job_id:     String,
    pub device_id:  String,
    pub status:     JobStatus,
    pub created_at: u64,
    pub updated_at: u64,
}

impl Job {
    pub fn new(job_id: impl Into<String>, device_id: impl Into<String>) -> Self {
        let now = now_ms();
        Self {
            job_id:     job_id.into(),
            device_id:  device_id.into(),
            status:     JobStatus::Queued,
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct JobStore {
    inner: Arc<RwLock<HashMap<String, Job>>>,
}

impl JobStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn insert(&self, job: Job) {
        self.inner.write().await.insert(job.job_id.clone(), job);
    }

    pub async fn get(&self, job_id: &str) -> Option<Job> {
        self.inner.read().await.get(job_id).cloned()
    }

    pub async fn all(&self) -> Vec<Job> {
        self.inner.read().await.values().cloned().collect()
    }

    pub async fn update_status(&self, job_id: &str, status: JobStatus) {
        let mut inner = self.inner.write().await;
        if let Some(job) = inner.get_mut(job_id) {
            job.status     = status;
            job.updated_at = now_ms();
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
