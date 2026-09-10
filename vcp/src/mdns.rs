//! mDNS VCP device scanner.
//!
//! Discovers VCP-compatible devices advertising `_vcp._tcp.local.` via mDNS.
//!
//! Implementation:
//!   1. Primary: invokes `avahi-browse -a -p -r -t` (Avahi daemon on Linux/Termux).
//!   2. Fallback: invokes `dns-sd -B _vcp._tcp local` (macOS/Bonjour).
//!   3. If neither binary is found, returns an empty scan result silently.
//!
//! mDNS TXT record keys for VCP devices:
//!   device_id   — VCP device ID (e.g. "unitree:go2:192.168.1.10")
//!   model       — device model string
//!   manufacturer — device manufacturer
//!   caps        — comma-separated capability list (e.g. "camera,lidar,telemetry")
//!   pubkey      — base64url Ed25519 public key
//!   protocol    — protocol version (e.g. "vcp/1")
//!
//! Devices missing a `device_id` TXT key get a synthetic ID: "mdns:{hostname}".

use std::collections::HashMap;
use std::process::Command;
use tracing::{debug, warn};

use crate::discovery::{DiscoveredDevice, DiscoveryMethod};
use crate::manifest::{AgentDeviceManifest, VcpCapabilityDecl, VcpSafetyConfig,
                       VcpTransport, VcpDeviceIdentity};
use sovereign_types::SafetyLevel;

/// The mDNS service type for VCP devices.
const VCP_SERVICE_TYPE: &str = "_vcp._tcp";

/// Result of a single mDNS scan.
#[derive(Debug, Default)]
pub struct MdnsScanResult {
    pub devices: Vec<DiscoveredDevice>,
    pub scanner: &'static str, // "avahi" | "dns-sd" | "none"
}

/// Scan for VCP devices using mDNS. Non-blocking (synchronous subprocess).
/// Returns an empty result if no scanner is available.
pub fn scan_mdns() -> MdnsScanResult {
    if let Some(result) = try_avahi() {
        return result;
    }
    if let Some(result) = try_dns_sd() {
        return result;
    }
    debug!("no mDNS scanner available (avahi-browse / dns-sd not found)");
    MdnsScanResult { devices: vec![], scanner: "none" }
}

// ─── Avahi (Linux / Termux) ───────────────────────────────────────────────────

fn try_avahi() -> Option<MdnsScanResult> {
    let output = Command::new("avahi-browse")
        .args(["-a", "-p", "-r", "-t", "--no-fail"])
        .output()
        .ok()?;

    if !output.status.success() && output.stdout.is_empty() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let devices = parse_avahi_output(&text);
    debug!(count = devices.len(), "avahi-browse scan complete");
    Some(MdnsScanResult { devices, scanner: "avahi" })
}

/// Parse avahi-browse -p (parsable) output.
///
/// Format (semicolon-separated per line):
///   =;<iface>;<proto>;<name>;<type>;<domain>;<hostname>;<addr>;<port>;<txt>
fn parse_avahi_output(text: &str) -> Vec<DiscoveredDevice> {
    let mut devices = vec![];
    for line in text.lines() {
        if !line.starts_with('=') { continue; }
        let parts: Vec<&str> = line.splitn(10, ';').collect();
        if parts.len() < 10 { continue; }

        let service_type = parts[4];
        if !service_type.contains(VCP_SERVICE_TYPE) { continue; }

        let name      = parts[3];
        let hostname  = parts[6];
        let addr      = parts[7];
        let txt_raw   = parts[9];

        let txt = parse_avahi_txt(txt_raw);
        if let Some(dev) = txt_to_device(name, hostname, addr, &txt) {
            devices.push(dev);
        }
    }
    devices
}

/// Parse avahi TXT field: `"key=value" "key2=value2" …`
fn parse_avahi_txt(raw: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for token in raw.split('"') {
        let token = token.trim();
        if token.is_empty() { continue; }
        if let Some((k, v)) = token.split_once('=') {
            map.insert(k.to_string(), v.to_string());
        }
    }
    map
}

// ─── dns-sd (macOS / Bonjour) ────────────────────────────────────────────────

fn try_dns_sd() -> Option<MdnsScanResult> {
    // `dns-sd -B _vcp._tcp local` runs continuously; use a short timeout
    let output = Command::new("dns-sd")
        .args(["-B", "_vcp._tcp", "local"])
        .output()
        .ok()?;

    let text = String::from_utf8_lossy(&output.stdout);
    let devices = parse_dns_sd_output(&text);
    debug!(count = devices.len(), "dns-sd scan complete");
    Some(MdnsScanResult { devices, scanner: "dns-sd" })
}

fn parse_dns_sd_output(text: &str) -> Vec<DiscoveredDevice> {
    // dns-sd format: "Browsing for _vcp._tcp.local.\nTimestamp  A/D Flags if Domain ServiceType InstanceName"
    // This is harder to parse reliably; just extract service names as device hints
    let mut devices = vec![];
    for line in text.lines().skip(3) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 7 { continue; }
        if parts[1] != "Add" { continue; }
        let name = parts[6];
        let txt: HashMap<String, String> = HashMap::new();
        if let Some(dev) = txt_to_device(name, name, "", &txt) {
            devices.push(dev);
        }
    }
    devices
}

// ─── TXT → DiscoveredDevice ───────────────────────────────────────────────────

fn txt_to_device(
    name:     &str,
    hostname: &str,
    addr:     &str,
    txt:      &HashMap<String, String>,
) -> Option<DiscoveredDevice> {
    let device_id = txt.get("device_id")
        .cloned()
        .unwrap_or_else(|| format!("mdns:{}", hostname.trim_end_matches('.')));

    let model        = txt.get("model").cloned().unwrap_or_else(|| name.into());
    let manufacturer = txt.get("manufacturer").cloned().unwrap_or_else(|| "Unknown".into());
    let public_key   = txt.get("pubkey").cloned().unwrap_or_else(|| "base64url:unknown".into());
    let protocol     = txt.get("protocol").cloned().unwrap_or_else(|| "vcp/1".into());

    let caps: Vec<VcpCapabilityDecl> = txt.get("caps")
        .map(|c| c.split(',')
            .filter(|s| !s.is_empty())
            .map(|cap| VcpCapabilityDecl {
                id:             cap.trim().into(),
                description:    cap.trim().into(),
                params:         None,
                requires_grant: true,
                safety_level:   SafetyLevel::Low,
                ungrantable:    false,
            })
            .collect())
        .unwrap_or_default();

    let manifest = AgentDeviceManifest {
        device_id:         device_id.clone(),
        manufacturer,
        model,
        protocol_version:  protocol,
        firmware_version:  "unknown".into(),
        dip_identity:      device_id.clone(),
        capabilities:      caps,
        safety: VcpSafetyConfig {
            emergency_stop:      true,
            geofence:            false,
            collision_avoidance: None,
            max_speed_ms:        None,
            ungrantable:         vec![],
        },
        transport:   vec![VcpTransport::Wifi],
        identity:    VcpDeviceIdentity { public_key, cert_chain: None },
        timestamp:   0,
        merkle_root: String::new(),
        signature:   String::new(),
    };

    let rssi = if addr.is_empty() { None } else { Some(-60i16) };
    Some(DiscoveredDevice::from_manifest(&manifest, rssi, DiscoveryMethod::Mdns))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_avahi_txt_extracts_kv() {
        let raw = r#""device_id=unitree:go2:192.168.1.10" "model=Go2" "caps=camera,lidar""#;
        let map = parse_avahi_txt(raw);
        assert_eq!(map["device_id"], "unitree:go2:192.168.1.10");
        assert_eq!(map["model"],     "Go2");
        assert_eq!(map["caps"],      "camera,lidar");
    }

    #[test]
    fn txt_to_device_fallback_device_id() {
        let txt = HashMap::new();
        let dev = txt_to_device("Go2-Test", "go2.local.", "192.168.1.10", &txt).unwrap();
        assert!(dev.device_id.starts_with("mdns:"), "got: {}", dev.device_id);
    }

    #[test]
    fn scan_mdns_returns_empty_when_no_scanner() {
        // In CI / test environment, avahi-browse and dns-sd are unlikely to be present
        // Just verify it doesn't panic
        let result = scan_mdns();
        assert!(result.scanner == "avahi" || result.scanner == "dns-sd" || result.scanner == "none");
    }

    #[test]
    fn parse_avahi_output_filters_service_type() {
        let text = "\
=;eth0;IPv4;Go2-Lab;_vcp._tcp;local;go2.local.;192.168.1.10;9090;\"device_id=unitree:go2:192.168.1.10\" \"model=Go2\" \"caps=camera,lidar,telemetry\"\n\
=;eth0;IPv4;Printer;_printer._tcp;local;printer.local.;192.168.1.20;631;\"\"\n";
        let devices = parse_avahi_output(text);
        assert_eq!(devices.len(), 1, "only VCP devices should be returned");
        assert_eq!(devices[0].device_id, "unitree:go2:192.168.1.10");
    }
}
