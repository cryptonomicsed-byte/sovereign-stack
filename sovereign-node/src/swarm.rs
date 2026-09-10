//! Swarm capture coordination — trigger simultaneous captures across multiple VCP devices.
//!
//! POST /capture/swarm   { "device_ids": ["unitree:go2:192.168.1.10", "unitree:go2:192.168.1.11"] }
//! GET  /swarm           list all swarms
//! GET  /swarm/:id       poll swarm status (aggregated over all child jobs)
//!
//! A SwarmJob creates N child jobs (one per device), monitors them concurrently,
//! and produces an aggregated status. When all children complete, a CaptureComplete
//! event is broadcast for each child twin.

use std::collections::HashMap;
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use tokio::sync::{RwLock, broadcast};
use tracing::{info, warn};

use crate::events::TwinEvent;
use crate::jobs::{Job, JobStatus, JobStore};

/// Aggregated status for a swarm of capture jobs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SwarmStatus {
    Pending,
    Running { completed: usize, total: usize },
    Completed {
        twin_ids:    Vec<String>,
        receipt_ids: Vec<String>,
    },
    PartiallyFailed {
        twin_ids: Vec<String>,
        failed:   Vec<String>,  // device_ids that failed
    },
    Failed { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmJob {
    pub swarm_id:   String,
    pub device_ids: Vec<String>,
    pub child_jobs: Vec<String>,   // job_ids
    pub status:     SwarmStatus,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SwarmRequest {
    pub device_ids: Vec<String>,
}

#[derive(Debug, Default, Clone)]
pub struct SwarmStore {
    inner: Arc<RwLock<HashMap<String, SwarmJob>>>,
}

impl SwarmStore {
    pub fn new() -> Self { Self::default() }

    pub async fn insert(&self, job: SwarmJob) {
        self.inner.write().await.insert(job.swarm_id.clone(), job);
    }

    pub async fn get(&self, swarm_id: &str) -> Option<SwarmJob> {
        self.inner.read().await.get(swarm_id).cloned()
    }

    pub async fn all(&self) -> Vec<SwarmJob> {
        let mut v: Vec<_> = self.inner.read().await.values().cloned().collect();
        v.sort_by_key(|s| s.created_at);
        v
    }

    pub async fn update_status(&self, swarm_id: &str, status: SwarmStatus) {
        let mut inner = self.inner.write().await;
        if let Some(job) = inner.get_mut(swarm_id) {
            job.status     = status;
            job.updated_at = now_ms();
        }
    }
}

/// Background swarm monitor — polls all child jobs and updates swarm status.
pub async fn monitor_swarm(
    swarm_id:   String,
    device_ids: Vec<String>,
    child_jobs: Vec<String>,
    job_store:  JobStore,
    swarm_store: SwarmStore,
    events_tx:  broadcast::Sender<TwinEvent>,
) {
    let total = child_jobs.len();
    swarm_store.update_status(&swarm_id, SwarmStatus::Running {
        completed: 0,
        total,
    }).await;

    // Poll every 2s, up to 15 minutes
    for _ in 0..450u32 {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let jobs: Vec<Option<Job>> = {
            let mut out = Vec::with_capacity(child_jobs.len());
            for jid in &child_jobs {
                out.push(job_store.get(jid).await);
            }
            out
        };

        let completed_count = jobs.iter().filter(|j| {
            matches!(j.as_ref().map(|j| &j.status),
                Some(JobStatus::Completed { .. }))
        }).count();

        let failed_count = jobs.iter().filter(|j| {
            matches!(j.as_ref().map(|j| &j.status),
                Some(JobStatus::Failed { .. }))
        }).count();

        let running_count = jobs.iter().filter(|j| {
            matches!(j.as_ref().map(|j| &j.status),
                Some(JobStatus::Running) | Some(JobStatus::Queued))
        }).count();

        // Update running status
        swarm_store.update_status(&swarm_id, SwarmStatus::Running {
            completed: completed_count + failed_count,
            total,
        }).await;

        // Done when nothing is still running/queued
        if running_count == 0 && (completed_count + failed_count) == total {
            let mut twin_ids    = vec![];
            let mut receipt_ids = vec![];
            let mut failed_devs = vec![];

            for (i, job) in jobs.iter().enumerate() {
                let dev_id = device_ids.get(i).cloned().unwrap_or_default();
                match job.as_ref().map(|j| &j.status) {
                    Some(JobStatus::Completed { twin_id, scene_receipt_id, .. }) => {
                        twin_ids.push(twin_id.clone());
                        receipt_ids.push(scene_receipt_id.clone());
                        let _ = events_tx.send(TwinEvent::CaptureComplete {
                            twin_id:    twin_id.clone(),
                            device_id:  dev_id,
                            receipt_id: scene_receipt_id.clone(),
                            job_id:     child_jobs[i].clone(),
                        });
                    }
                    _ => { failed_devs.push(dev_id); }
                }
            }

            let final_status = if failed_devs.is_empty() {
                info!(swarm_id = %swarm_id, twins = twin_ids.len(), "swarm complete");
                SwarmStatus::Completed { twin_ids, receipt_ids }
            } else if twin_ids.is_empty() {
                warn!(swarm_id = %swarm_id, "swarm fully failed");
                SwarmStatus::Failed { reason: format!("all {} devices failed", total) }
            } else {
                warn!(swarm_id = %swarm_id, failed = failed_devs.len(), "swarm partially failed");
                SwarmStatus::PartiallyFailed { twin_ids, failed: failed_devs }
            };

            swarm_store.update_status(&swarm_id, final_status).await;
            return;
        }
    }

    // Timeout
    warn!(swarm_id = %swarm_id, "swarm monitor timed out after 15 minutes");
    swarm_store.update_status(&swarm_id, SwarmStatus::Failed {
        reason: "timed out after 15 minutes".into(),
    }).await;
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn swarm_store_insert_and_get() {
        let store = SwarmStore::new();
        let job = SwarmJob {
            swarm_id:   "swarm:test".into(),
            device_ids: vec!["go2:1".into(), "go2:2".into()],
            child_jobs: vec!["job:a".into(), "job:b".into()],
            status:     SwarmStatus::Pending,
            created_at: 0,
            updated_at: 0,
        };
        store.insert(job).await;
        let got = store.get("swarm:test").await.unwrap();
        assert_eq!(got.device_ids.len(), 2);
    }

    #[tokio::test]
    async fn swarm_status_update() {
        let store = SwarmStore::new();
        let job = SwarmJob {
            swarm_id:   "swarm:x".into(),
            device_ids: vec!["go2:1".into()],
            child_jobs: vec!["job:1".into()],
            status:     SwarmStatus::Pending,
            created_at: 0,
            updated_at: 0,
        };
        store.insert(job).await;
        store.update_status("swarm:x", SwarmStatus::Running { completed: 0, total: 1 }).await;
        let got = store.get("swarm:x").await.unwrap();
        assert!(matches!(got.status, SwarmStatus::Running { total: 1, .. }));
    }

    #[test]
    fn swarm_request_deserialises() {
        let json = r#"{"device_ids":["go2:1","go2:2"]}"#;
        let req: SwarmRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.device_ids.len(), 2);
    }
}
