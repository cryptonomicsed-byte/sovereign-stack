//! Node configuration — loaded from TOML.
//!
//! Search order:
//!   1. --config <path> CLI flag
//!   2. $SOVEREIGN_CONFIG env var
//!   3. /etc/sovereign-node/config.toml   (system install)
//!   4. ~/.config/sovereign-node/config.toml  (user install)
//!   5. ./sovereign-node.toml  (dev/local)

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    pub node:       NodeSection,
    pub identity:   IdentitySection,
    pub vcp:        VcpSection,
    pub dip:        DipSection,
    pub pipeline:   PipelineSection,
    pub api:        ApiSection,
    pub vantage:    Option<VantageSection>,
    pub sui:        Option<SuiSection>,
    pub meshtastic: Option<MeshtasticSection>,
    #[serde(default)]
    pub witnesses:  Vec<WitnessConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitnessConfig {
    pub did:        String,
    pub public_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeSection {
    /// Human-readable node name — shown in logs and Vantage dashboard.
    pub name:        String,
    /// Data directory: keys, receipts, twin cache.
    /// Default: /var/lib/sovereign-node  (system) or ~/.local/share/sovereign-node (user)
    pub data_dir:    PathBuf,
    /// Log level: trace | debug | info | warn | error
    pub log_level:   String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentitySection {
    /// Path to the ed25519 private key (base64url, one line).
    /// Generated automatically if missing.
    pub key_file:    PathBuf,
    /// Node DID — derived from key on first boot, then persisted.
    pub did_file:    PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VcpSection {
    /// BLE scan interval in seconds.
    pub scan_interval_secs: u64,
    /// Device TTL in seconds (remove if not seen).
    pub device_ttl_secs:    u64,
    /// Known device manifests to register on startup (USB/manual).
    pub manual_devices:     Vec<String>,  // paths to manifest JSON files
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DipSection {
    /// Enable Nostr adapter.
    pub nostr_enabled:  bool,
    /// Nostr relay WebSocket URL (e.g. wss://relay.damus.io).
    pub nostr_relay:    Option<String>,
    /// Nostr npub (base58) for this node's identity on Nostr.
    pub nostr_npub:     Option<String>,
    /// Vantage DID for receipt routing.
    pub vantage_did:    String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineSection {
    /// Reconstruction engine label (for SceneReceipt).
    pub reconstruction_engine: String,
    /// ỌSỌVM version string.
    pub osovm_version:         String,
    /// Simulation trajectory count per run.
    pub trajectory_count:      u32,
    /// Selection objective: balanced | min_energy | min_risk | max_throughput
    pub selection_objective:   String,
    /// Minimum number of witnesses for simulation proofs.
    pub min_witnesses:         usize,
    /// Optional ỌSỌVM engine endpoint.
    ///   "http://localhost:9000" → POST to /run (JSON body: twin + scenario)
    ///   "/usr/local/bin/osovm" → exec as binary (JSON on stdin, result on stdout)
    /// Omit to use the built-in stub engine.
    #[serde(default)]
    pub osovm_endpoint:        Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiSection {
    /// Bind address for the local HTTP API.
    pub bind:        String,   // e.g. "127.0.0.1:7779"
    /// Enable the API server.
    pub enabled:     bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VantageSection {
    pub base_url:  String,   // e.g. "https://vantage.example.com"
    pub api_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuiSection {
    pub rpc_url:    String,
    pub address:    String,
    pub key_file:   PathBuf,
}

/// Meshtastic device HTTP bridge.
/// When set, DIP envelopes destined for the mesh are POSTed to
/// `{device_url}/api/v1/toRadio` as base64url-encoded protobuf.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshtasticSection {
    /// HTTP URL of the Meshtastic device (e.g. "http://192.168.4.1")
    pub device_url: String,
    /// Channel index for encrypted DIP traffic (default 1)
    #[serde(default = "default_mesh_channel")]
    pub dip_channel: u32,
}

fn default_mesh_channel() -> u32 { 1 }

impl Default for NodeConfig {
    fn default() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
        Self {
            node: NodeSection {
                name:     "sovereign-node".into(),
                data_dir: PathBuf::from(format!("{home}/.local/share/sovereign-node")),
                log_level: "info".into(),
            },
            identity: IdentitySection {
                key_file: PathBuf::from(format!("{home}/.local/share/sovereign-node/node.key")),
                did_file: PathBuf::from(format!("{home}/.local/share/sovereign-node/node.did")),
            },
            vcp: VcpSection {
                scan_interval_secs: 30,
                device_ttl_secs:    120,
                manual_devices:     vec![],
            },
            dip: DipSection {
                nostr_enabled: false,
                nostr_relay:   None,
                nostr_npub:    None,
                vantage_did:   "did:vantage:api:receipts".into(),
            },
            pipeline: PipelineSection {
                reconstruction_engine: "nerfstudio/gaussian-splatting".into(),
                osovm_version:         "osovm/2.0".into(),
                trajectory_count:      6,
                selection_objective:   "balanced".into(),
                min_witnesses:         2,
                osovm_endpoint:        None,
            },
            api: ApiSection {
                bind:    "127.0.0.1:7779".into(),
                enabled: true,
            },
            vantage:    None,
            sui:        None,
            meshtastic: None,
            witnesses:  vec![],
        }
    }
}

impl NodeConfig {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| ConfigError::Io(path.to_path_buf(), e))?;
        toml::from_str(&text).map_err(ConfigError::Parse)
    }

    /// Find and load config from the default search path.
    pub fn load_default() -> Self {
        let candidates: Vec<PathBuf> = [
            std::env::var("SOVEREIGN_CONFIG").ok().map(PathBuf::from),
            Some(PathBuf::from("/etc/sovereign-node/config.toml")),
            std::env::var("HOME").ok()
                .map(|h| PathBuf::from(format!("{h}/.config/sovereign-node/config.toml"))),
            Some(PathBuf::from("sovereign-node.toml")),
        ]
        .into_iter()
        .flatten()
        .collect();

        for path in candidates {
            if path.exists() {
                match Self::load(&path) {
                    Ok(cfg) => {
                        eprintln!("[config] loaded from {}", path.display());
                        return cfg;
                    }
                    Err(e) => eprintln!("[config] error loading {}: {e}", path.display()),
                }
            }
        }
        eprintln!("[config] no config file found — using defaults");
        Self::default()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot read {0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("TOML parse error: {0}")]
    Parse(#[from] toml::de::Error),
}
