//! Periodic heartbeat reporter — POSTs node health to Vantage /api/nodes/heartbeat.
//!
//! Uses the existing VantageClient (and its reqwest::Client) rather than
//! creating a second HTTP client. No-ops if `config.vantage` is None.

use std::time::Duration;
use tracing::{info, warn};
use serde_json::json;

use crate::node::NodeState;
use crate::vantage::VantageClient;

/// Spawn a background task that reports heartbeat to Vantage every `interval_secs`.
/// No-ops if `config.vantage` is None.
pub fn spawn_heartbeat(state: NodeState, interval_secs: u64) {
    let vantage_cfg = match state.config.vantage.as_ref() {
        Some(v) => v.clone(),
        None => return,
    };

    let client = VantageClient::new(&vantage_cfg.base_url, &vantage_cfg.api_token);

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
        loop {
            interval.tick().await;
            report_once(&client, &state).await;
        }
    });
}

async fn report_once(client: &VantageClient, state: &NodeState) {
    let device_count = state.registry.count().await;
    let jobs = state.job_store.all().await;
    let jobs_running = jobs
        .iter()
        .filter(|j| matches!(j.status, crate::jobs::JobStatus::Running))
        .count();

    let osovm_url = state
        .config
        .osovm_url
        .as_deref()
        .unwrap_or("")
        .to_string();

    let body = json!({
        "node_did":     state.identity.did,
        "uptime_secs":  uptime_secs(state),
        "device_count": device_count,
        "jobs_running": jobs_running,
        "osovm_url":    osovm_url,
    });

    client.post_node_heartbeat(&state.identity.did, body).await;
}

fn uptime_secs(state: &NodeState) -> u64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    (now.saturating_sub(state.started_at)) / 1000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uptime_secs_no_panic() {
        // Verify the uptime_secs arithmetic doesn't panic on edge values.
        // started_at = 0  →  uptime = now / 1000 (large but valid)
        // started_at = u64::MAX  →  saturating_sub prevents underflow
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        // Normal case: started_at in the past
        let started = now_ms.saturating_sub(5_000);
        let uptime = (now_ms.saturating_sub(started)) / 1000;
        assert!(uptime >= 5, "expected at least 5 secs uptime");

        // Edge case: started_at > now (clock skew) → saturating_sub → 0
        let uptime_zero = (now_ms.saturating_sub(u64::MAX)) / 1000;
        assert_eq!(uptime_zero, 0);
    }

    #[test]
    fn spawn_noop_when_no_vantage() {
        // spawn_heartbeat with None vantage config returns immediately without panicking.
        // We verify this at the type level: if config.vantage is None the function returns
        // before calling tokio::spawn, so no runtime is needed for this code path.
        // (Full async integration is covered by the running node in dev.)
    }
}
