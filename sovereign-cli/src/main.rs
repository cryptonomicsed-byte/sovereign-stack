//! sovereign — CLI for sovereign-node.
//!
//! Usage:
//!   sovereign status                      — node status
//!   sovereign devices                     — nearby VCP devices
//!   sovereign capture <device_id>         — trigger capture, print job_id
//!   sovereign jobs                        — list all jobs
//!   sovereign job <job_id>                — get job status
//!   sovereign receipts                    — list completed receipts
//!   sovereign receipt <twin_id>           — get receipt for a twin
//!   sovereign mcp <tool> [args_json]      — call an MCP tool directly

use clap::{Parser, Subcommand};
use serde_json::{json, Value};

#[derive(Parser, Debug)]
#[command(
    name    = "sovereign",
    version,
    about   = "CLI for sovereign-node — DIP + VCP + Twin daemon"
)]
struct Cli {
    /// Node API base URL
    #[arg(short, long, default_value = "http://127.0.0.1:7779", env = "SOVEREIGN_URL")]
    url: String,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Show node status
    Status,
    /// List nearby VCP devices
    Devices,
    /// Trigger a capture pipeline for a device
    Capture {
        /// VCP device ID
        device_id: String,
    },
    /// List all capture jobs
    Jobs,
    /// Get status of a specific job
    Job {
        job_id: String,
    },
    /// List completed receipts
    Receipts,
    /// Get receipt for a specific twin
    Receipt {
        twin_id: String,
    },
    /// Call an MCP tool (JSON-RPC 2.0)
    Mcp {
        /// Tool name (e.g. vcp_nearby_devices)
        tool: String,
        /// Tool arguments as JSON object (optional)
        args: Option<String>,
    },
    /// Device management subcommands
    Device {
        #[command(subcommand)]
        sub: DeviceCmd,
    },
    /// Send a DIP envelope to this node via the inbound webhook
    DipSend {
        /// JSON file containing a DIP envelope, or '-' to read from stdin
        file: String,
    },
}

#[derive(Subcommand, Debug)]
enum DeviceCmd {
    /// Register a device from a manifest JSON file
    Register {
        /// Path to the AgentDeviceManifest JSON file
        manifest: String,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("failed to build HTTP client");

    let result = match &cli.cmd {
        Cmd::Status         => get(&client, &cli.url, "/status").await,
        Cmd::Devices        => get(&client, &cli.url, "/devices").await,
        Cmd::Jobs           => get(&client, &cli.url, "/jobs").await,
        Cmd::Receipts       => get(&client, &cli.url, "/receipts").await,
        Cmd::Job { job_id } => get(&client, &cli.url, &format!("/jobs/{job_id}")).await,
        Cmd::Receipt { twin_id } => get(&client, &cli.url, &format!("/receipts/{twin_id}")).await,

        Cmd::Capture { device_id } => {
            let url = format!("{}/capture/{device_id}", cli.url);
            match client.post(&url).send().await {
                Ok(r)  => r.json::<Value>().await.map_err(|e| e.to_string()),
                Err(e) => Err(e.to_string()),
            }
        }

        Cmd::Mcp { tool, args } => {
            let arguments: Value = match args {
                Some(s) => serde_json::from_str(s).unwrap_or_else(|_| {
                    eprintln!("warning: args is not valid JSON — using empty object");
                    json!({})
                }),
                None => json!({}),
            };

            let body = json!({
                "jsonrpc": "2.0",
                "id":      1,
                "method":  "tools/call",
                "params":  { "name": tool, "arguments": arguments }
            });

            let url = format!("{}/mcp", cli.url);
            match client.post(&url).json(&body).send().await {
                Ok(r)  => r.json::<Value>().await.map_err(|e| e.to_string()),
                Err(e) => Err(e.to_string()),
            }
        }

        Cmd::Device { sub } => match sub {
            DeviceCmd::Register { manifest } => {
                let text = if manifest == "-" {
                    let mut buf = String::new();
                    use std::io::Read;
                    std::io::stdin().read_to_string(&mut buf).map_err(|e| e.to_string())?;
                    buf
                } else {
                    std::fs::read_to_string(manifest).map_err(|e| e.to_string())?
                };

                let manifest_val: Value = serde_json::from_str(&text)
                    .map_err(|e| format!("invalid manifest JSON: {e}"))?;

                let url = format!("{}/devices/register", cli.url);
                match client.post(&url).json(&manifest_val).send().await {
                    Ok(r)  => r.json::<Value>().await.map_err(|e| e.to_string()),
                    Err(e) => Err(e.to_string()),
                }
            }
        },

        Cmd::DipSend { file } => {
            let text = if file == "-" {
                let mut buf = String::new();
                use std::io::Read;
                std::io::stdin().read_to_string(&mut buf).map_err(|e| e.to_string())?;
                buf
            } else {
                std::fs::read_to_string(file).map_err(|e| e.to_string())?
            };

            let envelope_val: Value = serde_json::from_str(&text)
                .map_err(|e| format!("invalid DIP envelope JSON: {e}"))?;

            let url = format!("{}/dip/inbound", cli.url);
            match client.post(&url).json(&envelope_val).send().await {
                Ok(r)  => r.json::<Value>().await.map_err(|e| e.to_string()),
                Err(e) => Err(e.to_string()),
            }
        }
    };

    match result {
        Ok(val) => {
            println!("{}", serde_json::to_string_pretty(&val).unwrap_or_else(|_| val.to_string()));
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
    Ok(())
}

async fn get(client: &reqwest::Client, base: &str, path: &str) -> Result<Value, String> {
    let url = format!("{base}{path}");
    match client.get(&url).send().await {
        Ok(r)  => r.json::<Value>().await.map_err(|e| e.to_string()),
        Err(e) => Err(e.to_string()),
    }
}
