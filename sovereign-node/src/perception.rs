//! Active Perception Loop — continuous VCP device health monitoring.
//!
//! Runs as a background task; at each interval:
//!   1. Queries the device registry for all known devices
//!   2. Marks devices as Stale if not seen within `stale_threshold_secs`
//!   3. Attempts a lightweight HTTP GET to the device's management URL
//!      (for Wi-Fi devices) and updates health
//!   4. Broadcasts StatusUpdate events for any state transitions
//!
//! For stub/BLE devices without a management URL, health is inferred
//! from the registry's last-seen timestamp.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

use vcp::DeviceRegistry;

use crate::events::TwinEvent;

/// Per-device health state tracked across perception ticks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceHealth {
    Online,
    Stale,
    Offline,
}

impl DeviceHealth {
    fn label(&self) -> &'static str {
        match self {
            DeviceHealth::Online  => "online",
            DeviceHealth::Stale   => "stale",
            DeviceHealth::Offline => "offline",
        }
    }
}

/// Spawn the active perception background task.
///
/// `interval_secs`       — how often to scan (seconds)
/// `stale_threshold_secs` — device not seen within this → Stale
pub fn spawn_perception_loop(
    registry:             DeviceRegistry,
    events_tx:            broadcast::Sender<TwinEvent>,
    interval_secs:        u64,
    stale_threshold_secs: u64,
    http_client:          Arc<reqwest::Client>,
) {
    tokio::spawn(async move {
        let mut health_map: HashMap<String, DeviceHealth> = HashMap::new();
        let mut tick = tokio::time::interval(Duration::from_secs(interval_secs));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        info!(
            interval_secs,
            stale_threshold_secs,
            "active perception loop started"
        );

        loop {
            tick.tick().await;

            let devices = registry.all().await;
            debug!(count = devices.len(), "perception tick");

            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;

            for device in &devices {
                let age_ms = now_ms.saturating_sub(device.last_seen_ms);
                let age_secs = age_ms / 1000;

                let new_health = if age_secs > stale_threshold_secs * 3 {
                    // Not seen in 3× threshold — attempt live ping for Wi-Fi devices
                    if device.device_id.starts_with("unitree:go2:") {
                        let ip = device.device_id
                            .strip_prefix("unitree:go2:")
                            .unwrap_or("unknown");
                        let url = format!("http://{ip}:8080/api/v1/health");
                        match http_client
                            .get(&url)
                            .timeout(Duration::from_secs(2))
                            .send()
                            .await
                        {
                            Ok(r) if r.status().is_success() => DeviceHealth::Online,
                            _ => DeviceHealth::Offline,
                        }
                    } else {
                        DeviceHealth::Offline
                    }
                } else if age_secs > stale_threshold_secs {
                    DeviceHealth::Stale
                } else {
                    DeviceHealth::Online
                };

                let prev = health_map.get(&device.device_id);
                if prev != Some(&new_health) {
                    let msg = format!(
                        "Device {} transitioned to {} (last seen {}s ago)",
                        device.device_id,
                        new_health.label(),
                        age_secs,
                    );
                    info!(
                        device_id = %device.device_id,
                        health    = %new_health.label(),
                        age_secs,
                        "device health changed"
                    );
                    let _ = events_tx.send(TwinEvent::StatusUpdate {
                        job_id:  format!("perception:{}", device.device_id),
                        message: msg,
                    });
                    health_map.insert(device.device_id.clone(), new_health);
                }
            }

            // Remove stale entries for devices no longer in registry
            health_map.retain(|id, _| devices.iter().any(|d| &d.device_id == id));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_labels() {
        assert_eq!(DeviceHealth::Online.label(),  "online");
        assert_eq!(DeviceHealth::Stale.label(),   "stale");
        assert_eq!(DeviceHealth::Offline.label(), "offline");
    }

    #[test]
    fn health_eq() {
        assert_eq!(DeviceHealth::Online, DeviceHealth::Online);
        assert_ne!(DeviceHealth::Online, DeviceHealth::Stale);
    }

    #[tokio::test]
    async fn spawn_does_not_panic_with_empty_registry() {
        let registry  = vcp::DeviceRegistry::new(120);
        let (tx, _rx) = broadcast::channel(16);
        let client    = Arc::new(reqwest::Client::new());
        // spawn with a very long interval so it doesn't actually tick during the test
        spawn_perception_loop(registry, tx, 3600, 120, client);
        // yield to let the task start
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}
