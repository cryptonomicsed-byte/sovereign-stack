//! Sovereign Node mDNS federation.
//!
//! Advertises this node on the local network as `_sovereign._tcp` and
//! discovers other sovereign nodes using the same service type.
//!
//! Uses `avahi-publish-service` and `avahi-browse` subprocesses — no extra
//! Rust dependency required.
//!
//! Service TXT records:
//!   did=<node DID>          — node identity
//!   a2a=/a2a                — A2A endpoint path
//!   ver=0.1                 — protocol version

use std::collections::HashMap;
use tokio::process::Command;
use tracing::{info, warn};

/// A peer sovereign node discovered via mDNS.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DiscoveredPeer {
    pub name:    String,
    pub host:    String,
    pub port:    u16,
    pub did:     Option<String>,
    pub a2a_url: String,
}

/// Advertise this sovereign node on the local network via mDNS.
///
/// Spawns `avahi-publish-service` in the background. The process runs
/// until dropped (via the returned handle) or the node shuts down.
/// Returns the child process handle so the caller can keep it alive.
pub async fn advertise_node(
    node_name: &str,
    port: u16,
    did: &str,
) -> std::io::Result<tokio::process::Child> {
    let txt_did    = format!("did={did}");
    let txt_a2a    = "a2a=/a2a";
    let txt_ver    = "ver=0.1";

    info!(name = %node_name, port, "mDNS: advertising _sovereign._tcp");

    Command::new("avahi-publish-service")
        .args([
            node_name,
            "_sovereign._tcp",
            &port.to_string(),
            &txt_did,
            txt_a2a,
            txt_ver,
        ])
        .spawn()
}

/// Discover sovereign nodes on the local network via mDNS.
///
/// Runs `avahi-browse -p -r -t _sovereign._tcp` (parseable, resolve, terminate).
/// Returns whatever peers are found within the subprocess timeout (~5 s typical).
pub async fn discover_sovereign_nodes() -> Vec<DiscoveredPeer> {
    let output = match Command::new("avahi-browse")
        .args(["-p", "-r", "-t", "_sovereign._tcp"])
        .output()
        .await
    {
        Ok(o)  => o,
        Err(e) => {
            warn!("avahi-browse not available: {e}");
            return vec![];
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_avahi_browse(&stdout)
}

/// Parse avahi-browse -p (parseable) output into DiscoveredPeer entries.
///
/// Line format (= resolved records):
///   =;iface;proto;name;type;domain;host;atype;addr;port;txt
fn parse_avahi_browse(output: &str) -> Vec<DiscoveredPeer> {
    let mut peers: HashMap<String, DiscoveredPeer> = HashMap::new();

    for line in output.lines() {
        if !line.starts_with('=') {
            continue;
        }
        let parts: Vec<&str> = line.splitn(11, ';').collect();
        if parts.len() < 11 {
            continue;
        }
        let name = parts[3].to_string();
        let host = parts[6].to_string();
        let port: u16 = parts[9].parse().unwrap_or(8080);
        let txt_raw = parts[10];

        // Parse TXT records: "\"key=val\" \"key2=val2\" ..."
        let mut did: Option<String> = None;
        let mut a2a_path = "/a2a".to_string();
        for segment in txt_raw.split('"') {
            let s = segment.trim();
            if let Some(v) = s.strip_prefix("did=") {
                did = Some(v.to_string());
            } else if let Some(v) = s.strip_prefix("a2a=") {
                a2a_path = v.to_string();
            }
        }

        let a2a_url = format!("http://{host}:{port}{a2a_path}");
        peers.insert(name.clone(), DiscoveredPeer { name, host, port, did, a2a_url });
    }

    peers.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
+;eth0;IPv4;sovereign-alpha;_sovereign._tcp;local
=;eth0;IPv4;sovereign-alpha;_sovereign._tcp;local;sovereign-alpha.local;IPv4;192.168.1.50;8080;"did=did:vantage:abc123" "a2a=/a2a" "ver=0.1"
=;eth0;IPv4;sovereign-beta;_sovereign._tcp;local;sovereign-beta.local;IPv4;192.168.1.51;9090;"did=did:vantage:xyz789" "a2a=/a2a" "ver=0.1"
"#;

    #[test]
    fn parse_avahi_extracts_peers() {
        let peers = parse_avahi_browse(SAMPLE);
        assert_eq!(peers.len(), 2);
        let alpha = peers.iter().find(|p| p.name == "sovereign-alpha").unwrap();
        assert_eq!(alpha.port, 8080);
        assert_eq!(alpha.did.as_deref(), Some("did:vantage:abc123"));
        assert_eq!(alpha.a2a_url, "http://sovereign-alpha.local:8080/a2a");
    }

    #[test]
    fn parse_avahi_ignores_browse_lines() {
        // + lines (not yet resolved) should be ignored
        let out = "+;eth0;IPv4;foo;_sovereign._tcp;local\n";
        assert!(parse_avahi_browse(out).is_empty());
    }

    #[test]
    fn parse_avahi_handles_missing_txt() {
        let out = "=;eth0;IPv4;node;_sovereign._tcp;local;node.local;IPv4;10.0.0.1;8080;\"\"\n";
        let peers = parse_avahi_browse(out);
        assert_eq!(peers.len(), 1);
        assert!(peers[0].did.is_none());
        assert_eq!(peers[0].a2a_url, "http://node.local:8080/a2a");
    }
}
