//! VCP Device Discovery Daemon.
//!
//! Scans for nearby VCP-compatible devices via BLE advertisements and mDNS.
//! Maintains a live registry with TTL-based expiry.
//!
//! Integration with Ọmọ Kọ́dà 2 heartbeat:
//!   Ọmọ Kọ́dà's perceive→think→act heartbeat loop calls observe_mesh_context(),
//!   which queries our MCP tool `vcp_nearby_devices`. The device list becomes
//!   part of the agent's perception — it knows what physical machines are
//!   reachable before deciding what to do this cycle.
//!
//!   On the ACT phase, the heartbeat_pulse carries:
//!     details.nearby_vcp_devices: [{ device_id, manufacturer, capabilities[] }]
//!   Vantage's swarm dashboard shows this alongside WorkState, making physical
//!   machine availability visible to the whole guild.
//!
//! Integration with Vantage /me/heartbeat:
//!   The daemon also posts to Vantage's POST /api/me/heartbeat on each scan
//!   cycle, appending nearby device context so last_seen_at stays fresh and
//!   the guild knows the agent has physical capability.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use crate::manifest::AgentDeviceManifest;

/// A discovered VCP device — what the scanner sees before the full handshake.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredDevice {
    pub device_id:            String,
    pub manufacturer:         String,
    pub model:                String,
    pub protocol_version:     String,
    pub capability_summary:   Vec<String>,    // top-level capability IDs only
    pub transport:            Vec<String>,
    pub rssi:                 Option<i16>,    // BLE signal strength (dBm)
    pub last_seen_ms:         u64,
    pub discovery_method:     DiscoveryMethod,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryMethod {
    Ble,
    Mdns,
    Meshtastic,
    Manual,    // explicitly registered (e.g. over USB)
}

impl DiscoveredDevice {
    pub fn from_manifest(manifest: &AgentDeviceManifest, rssi: Option<i16>, method: DiscoveryMethod) -> Self {
        Self {
            device_id:          manifest.device_id.clone(),
            manufacturer:       manifest.manufacturer.clone(),
            model:              manifest.model.clone(),
            protocol_version:   manifest.protocol_version.clone(),
            capability_summary: manifest.capabilities.iter()
                .filter(|c| !c.ungrantable)
                .map(|c| c.id.clone())
                .collect(),
            transport:          manifest.transport.iter()
                .map(|t| format!("{:?}", t).to_lowercase())
                .collect(),
            rssi,
            last_seen_ms:       now_ms(),
            discovery_method:   method,
        }
    }

    pub fn age_secs(&self) -> u64 {
        (now_ms() - self.last_seen_ms) / 1000
    }
}

/// Thread-safe device registry with TTL expiry.
#[derive(Debug, Default, Clone)]
pub struct DeviceRegistry {
    inner: Arc<RwLock<RegistryInner>>,
}

#[derive(Debug, Default)]
struct RegistryInner {
    devices: HashMap<String, DiscoveredDevice>,
    ttl_secs: u64,
}

impl DeviceRegistry {
    pub fn new(ttl_secs: u64) -> Self {
        Self { inner: Arc::new(RwLock::new(RegistryInner { devices: HashMap::new(), ttl_secs })) }
    }

    pub async fn upsert(&self, device: DiscoveredDevice) {
        let mut inner = self.inner.write().await;
        inner.devices.insert(device.device_id.clone(), device);
    }

    pub async fn remove_expired(&self) {
        let mut inner = self.inner.write().await;
        let ttl = inner.ttl_secs;
        inner.devices.retain(|_, d| d.age_secs() < ttl);
    }

    pub async fn all(&self) -> Vec<DiscoveredDevice> {
        let inner = self.inner.read().await;
        inner.devices.values().cloned().collect()
    }

    pub async fn get(&self, device_id: &str) -> Option<DiscoveredDevice> {
        let inner = self.inner.read().await;
        inner.devices.get(device_id).cloned()
    }

    pub async fn count(&self) -> usize {
        let inner = self.inner.read().await;
        inner.devices.len()
    }

    /// Produce the JSON summary Ọmọ Kọ́dà includes in its heartbeat_pulse details.
    /// Koda's PERCEIVE phase calls observe_mesh_context() → vcp_nearby_devices tool
    /// returns this structure, making physical device awareness part of every heartbeat.
    pub async fn heartbeat_summary(&self) -> serde_json::Value {
        let devices = self.all().await;
        serde_json::json!({
            "nearby_vcp_devices": devices.iter().map(|d| serde_json::json!({
                "device_id":          d.device_id,
                "manufacturer":       d.manufacturer,
                "model":              d.model,
                "capabilities":       d.capability_summary,
                "transport":          d.transport,
                "rssi":               d.rssi,
                "age_secs":           d.age_secs(),
                "discovery_method":   d.discovery_method,
            })).collect::<Vec<_>>(),
            "device_count": devices.len(),
        })
    }
}

/// The discovery daemon — runs scan loops and maintains the registry.
pub struct DiscoveryDaemon {
    pub registry:    DeviceRegistry,
    scan_interval:   Duration,
    device_ttl_secs: u64,

    /// Vantage base URL for heartbeat integration (e.g. "http://localhost:8001")
    vantage_url:     Option<String>,
    /// Ọmọ Kọ́dà base URL for perception injection (e.g. "http://localhost:7777")
    koda_url:        Option<String>,
}

impl DiscoveryDaemon {
    pub fn new(scan_interval_secs: u64, device_ttl_secs: u64) -> Self {
        Self {
            registry:        DeviceRegistry::new(device_ttl_secs),
            scan_interval:   Duration::from_secs(scan_interval_secs),
            device_ttl_secs,
            vantage_url:     std::env::var("VANTAGE_URL").ok(),
            koda_url:        std::env::var("KODA_URL").ok(),
        }
    }

    pub fn with_vantage(mut self, url: impl Into<String>) -> Self {
        self.vantage_url = Some(url.into());
        self
    }

    pub fn with_koda(mut self, url: impl Into<String>) -> Self {
        self.koda_url = Some(url.into());
        self
    }

    /// Simulate a BLE beacon arrival (real impl uses btleplug / bluer crate).
    pub async fn on_ble_beacon(&self, manifest: AgentDeviceManifest, rssi: i16) {
        let device = DiscoveredDevice::from_manifest(&manifest, Some(rssi), DiscoveryMethod::Ble);
        println!("[vcp-discovery] BLE beacon: {} {} (rssi={}dBm, {} caps)",
            device.manufacturer, device.model, rssi, device.capability_summary.len());
        self.registry.upsert(device).await;
    }

    /// Simulate an mDNS announcement (real impl uses mdns-sd crate).
    pub async fn on_mdns_announcement(&self, manifest: AgentDeviceManifest) {
        let device = DiscoveredDevice::from_manifest(&manifest, None, DiscoveryMethod::Mdns);
        println!("[vcp-discovery] mDNS: {} {} ({} caps)",
            device.manufacturer, device.model, device.capability_summary.len());
        self.registry.upsert(device).await;
    }

    /// Manually register a device (e.g. USB-connected, or entered by user).
    pub async fn register_manual(&self, manifest: AgentDeviceManifest) {
        let device = DiscoveredDevice::from_manifest(&manifest, None, DiscoveryMethod::Manual);
        println!("[vcp-discovery] manual: {} {}", device.manufacturer, device.model);
        self.registry.upsert(device).await;
    }

    /// Start the background scan + expiry + heartbeat-sync loop.
    /// This is what runs permanently inside the Vantage-Voice or Koda process.
    pub fn spawn(self: Arc<Self>) {
        let daemon = self.clone();
        let http   = reqwest::Client::builder()
            .timeout(Duration::from_secs(8))
            .build()
            .unwrap_or_default();

        tokio::spawn(async move {
            tracing::info!(
                scan_secs = daemon.scan_interval.as_secs(),
                ttl_secs  = daemon.device_ttl_secs,
                "VCP discovery daemon started"
            );

            let mut interval = tokio::time::interval(daemon.scan_interval);
            loop {
                interval.tick().await;

                // 1. mDNS scan — discover devices advertising _vcp._tcp.local.
                //    Runs as a blocking subprocess; offloaded to the blocking pool.
                let registry_clone = daemon.registry.clone();
                tokio::task::spawn_blocking(move || {
                    let result = crate::mdns::scan_mdns();
                    if !result.devices.is_empty() {
                        tracing::info!(
                            count   = result.devices.len(),
                            scanner = result.scanner,
                            "mDNS VCP devices discovered"
                        );
                    }
                    // Note: registry.upsert is async; we collect device IDs for logging
                    // and return them so the async side can upsert.
                    result
                })
                .await
                .map(|mdns_result| {
                    let reg = registry_clone.clone();
                    tokio::spawn(async move {
                        for dev in mdns_result.devices {
                            reg.upsert(dev).await;
                        }
                    });
                })
                .ok();

                // 2. Expire stale devices
                daemon.registry.remove_expired().await;

                let count = daemon.registry.count().await;
                tracing::debug!(device_count = count, "VCP scan tick");

                // 3. Push context to Ọmọ Kọ́dà's perception endpoint.
                if let Some(koda_url) = &daemon.koda_url {
                    let summary = daemon.registry.heartbeat_summary().await;
                    let url = format!("{koda_url}/v1/context/vcp");
                    if let Err(e) = http.post(&url).json(&summary).send().await {
                        tracing::debug!(url = %url, error = %e, "Koda VCP context push failed");
                    }
                }

                // 4. Vantage heartbeat with live device context.
                if let Some(vantage_url) = &daemon.vantage_url {
                    let summary = daemon.registry.heartbeat_summary().await;
                    let url = format!("{vantage_url}/api/me/heartbeat");
                    let body = serde_json::json!({
                        "work_state":        "ALIVE",
                        "intent":            "vcp_scan",
                        "details": { "nearby_vcp_devices": summary },
                    });
                    if let Err(e) = http.post(&url).json(&body).send().await {
                        tracing::debug!(url = %url, error = %e, "Vantage heartbeat failed");
                    }
                }
            }
        });
    }

    /// MCP tool handler: `vcp_nearby_devices`
    /// Koda calls this during its PERCEIVE phase on every heartbeat cycle.
    /// Returns JSON that Koda's heartbeat includes in observe_mesh_context().
    pub async fn tool_nearby_devices(&self) -> serde_json::Value {
        let mut summary = self.registry.heartbeat_summary().await;

        // Add perception hint so Koda's THINK step knows what to do with this
        summary["perception_hint"] = serde_json::json!(
            "These VCP devices are physically nearby. You can connect to any of them \
             via the vcp_connect tool if a task requires physical interaction."
        );
        summary
    }

    /// MCP tool handler: `vcp_connect`
    /// Called by Koda's ACT phase when it decides to connect to a device.
    pub async fn tool_connect(&self, device_id: &str) -> serde_json::Value {
        match self.registry.get(device_id).await {
            Some(device) => serde_json::json!({
                "status":    "ready_for_handshake",
                "device_id": device.device_id,
                "manufacturer": device.manufacturer,
                "model":     device.model,
                "next_step": "call vcp_handshake to begin capability negotiation",
            }),
            None => serde_json::json!({
                "status":  "not_found",
                "device_id": device_id,
                "hint":    "device may have left range — call vcp_nearby_devices to refresh",
            }),
        }
    }
}

/// The Ọmọ Kọ́dà heartbeat integration glue.
///
/// When Koda emits a heartbeat_pulse, it carries details that Vantage
/// broadcasts to the guild. We extend those details with VCP context.
///
/// Koda server.rs heartbeat loop (excerpt):
///   let details = json!({
///     "state": "alive",
///     "intent": intent,
///     "perceived_mesh": perception.is_some(),
///     // ↓ we inject this:
///     "nearby_vcp_devices": vcp_summary,
///   });
///
/// This function is called from the heartbeat ACT phase to extend the pulse.
pub async fn extend_heartbeat_pulse(
    base_details: &serde_json::Value,
    registry: &DeviceRegistry,
) -> serde_json::Value {
    let mut details = base_details.clone();
    let vcp = registry.heartbeat_summary().await;

    if let (Some(obj), Some(vcp_obj)) = (details.as_object_mut(), vcp.as_object()) {
        for (k, v) in vcp_obj {
            obj.insert(k.clone(), v.clone());
        }
    }
    details
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
    use crate::manifest::{VcpCapabilityDecl, VcpSafetyConfig, VcpTransport, VcpDeviceIdentity};
    use sovereign_types::SafetyLevel;

    fn test_manifest(device_id: &str, model: &str) -> AgentDeviceManifest {
        AgentDeviceManifest {
            device_id:        device_id.into(),
            manufacturer:     "Unitree".into(),
            model:            model.into(),
            protocol_version: "vcp/1".into(),
            firmware_version: "1.0".into(),
            dip_identity:     format!("did:device:{device_id}"),
            capabilities:     vec![
                VcpCapabilityDecl {
                    id: "locomotion".into(), description: "walk".into(),
                    params: None, requires_grant: true,
                    safety_level: SafetyLevel::Standard, ungrantable: false,
                },
                VcpCapabilityDecl {
                    id: "firmware.update".into(), description: "fw".into(),
                    params: None, requires_grant: true,
                    safety_level: SafetyLevel::Critical, ungrantable: true,
                },
            ],
            safety: VcpSafetyConfig {
                emergency_stop: true, geofence: true,
                collision_avoidance: None, max_speed_ms: Some(1.5),
                ungrantable: vec!["firmware.update".into()],
            },
            transport:   vec![VcpTransport::Ble, VcpTransport::Wifi],
            identity:    VcpDeviceIdentity { public_key: "pk".into(), cert_chain: None },
            timestamp:   0, merkle_root: String::new(), signature: String::new(),
        }
    }

    #[tokio::test]
    async fn registry_upsert_and_list() {
        let daemon = Arc::new(DiscoveryDaemon::new(30, 120));
        daemon.on_ble_beacon(test_manifest("go2:01", "Go2"), -72).await;
        daemon.on_mdns_announcement(test_manifest("g1:01", "G1")).await;

        let devices = daemon.registry.all().await;
        assert_eq!(devices.len(), 2);
    }

    #[tokio::test]
    async fn ungrantable_excluded_from_summary() {
        let daemon = Arc::new(DiscoveryDaemon::new(30, 120));
        daemon.on_ble_beacon(test_manifest("go2:01", "Go2"), -65).await;

        let devices = daemon.registry.all().await;
        let caps = &devices[0].capability_summary;
        assert!(caps.contains(&"locomotion".to_string()));
        assert!(!caps.contains(&"firmware.update".to_string()),
            "ungrantable capabilities must be hidden from discovery summary");
    }

    #[tokio::test]
    async fn heartbeat_summary_shape() {
        let daemon = Arc::new(DiscoveryDaemon::new(30, 120));
        daemon.on_ble_beacon(test_manifest("go2:01", "Go2"), -70).await;

        let summary = daemon.registry.heartbeat_summary().await;
        assert_eq!(summary["device_count"], 1);
        assert!(summary["nearby_vcp_devices"].is_array());
    }

    #[tokio::test]
    async fn extend_heartbeat_pulse_merges() {
        let registry = DeviceRegistry::new(120);
        let manifest = test_manifest("go2:01", "Go2");
        let device = DiscoveredDevice::from_manifest(&manifest, Some(-68), DiscoveryMethod::Ble);
        registry.upsert(device).await;

        let base = serde_json::json!({ "state": "alive", "intent": "watching", "perceived_mesh": true });
        let extended = extend_heartbeat_pulse(&base, &registry).await;

        assert_eq!(extended["state"], "alive");
        assert_eq!(extended["device_count"], 1);
        assert!(extended["nearby_vcp_devices"].is_array());
    }

    #[tokio::test]
    async fn tool_nearby_devices_has_perception_hint() {
        let daemon = Arc::new(DiscoveryDaemon::new(30, 120));
        daemon.on_ble_beacon(test_manifest("go2:01", "Go2"), -55).await;
        let result = daemon.tool_nearby_devices().await;
        assert!(result["perception_hint"].is_string());
    }

    #[tokio::test]
    async fn connect_tool_found_and_not_found() {
        let daemon = Arc::new(DiscoveryDaemon::new(30, 120));
        daemon.on_ble_beacon(test_manifest("go2:01", "Go2"), -60).await;

        let found = daemon.tool_connect("go2:01").await;
        assert_eq!(found["status"], "ready_for_handshake");

        let missing = daemon.tool_connect("phantom:99").await;
        assert_eq!(missing["status"], "not_found");
    }
}
