//! sovereign — CLI for sovereign-node.
//!
//! Usage:
//!   sovereign status                              — node status
//!   sovereign devices [--format table]            — nearby VCP devices
//!   sovereign capture <device_id>                 — trigger capture, print job_id
//!   sovereign jobs [--format table]               — list all jobs
//!   sovereign job <job_id>                        — get job status
//!   sovereign receipts                            — list completed receipts
//!   sovereign receipt <twin_id>                   — get receipt for a twin
//!   sovereign mcp <tool> [args_json]              — call an MCP tool directly
//!   sovereign watch <twin_id>                     — stream WebSocket events for a twin
//!   sovereign delegate <device_id> [opts]         — delegate a capture task
//!   sovereign a2a agent|task|poll                 — A2A protocol operations
//!   sovereign finetune extract|stats|script       — mycelium fine-tune helpers

use clap::{Parser, Subcommand};
use serde_json::{json, Value};
use urlencoding;

// ── CLI definition ────────────────────────────────────────────────────────────

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
    Devices {
        /// Output format: json (default) or table
        #[arg(long, default_value = "json")]
        format: String,
    },

    /// Trigger a capture pipeline for a device
    Capture {
        /// VCP device ID
        device_id: String,
    },

    /// List all capture jobs
    Jobs {
        /// Output format: json (default) or table
        #[arg(long, default_value = "json")]
        format: String,
    },

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

    /// Stream WebSocket events for a twin until Ctrl-C
    Watch {
        /// Twin ID to watch
        twin_id: String,
    },

    /// Delegate a capture task to a peer
    Delegate {
        /// VCP device ID
        device_id: String,
        /// Peer name to delegate to
        #[arg(long)]
        peer: Option<String>,
        /// Hint text for the delegated task
        #[arg(long)]
        hint: Option<String>,
    },

    /// A2A protocol operations
    A2a {
        #[command(subcommand)]
        sub: A2aCmd,
    },

    /// mycelium fine-tune helpers
    Finetune {
        #[command(subcommand)]
        sub: FinetuneCmd,
    },

    /// List all 256 Odù tiles with receipt counts
    Tiles,

    /// Get receipts for a specific Odù tile
    Tile {
        tile_id: String,
    },

    /// List all 4D twin provenance timelines
    Timelines,

    /// Get the 4D provenance timeline for a specific twin
    Timeline {
        twin_id: String,
    },

    /// Compute the 4D change diff between first and last snapshot of a twin's timeline
    TimelineDiff {
        twin_id: String,
    },

    /// Trigger a swarm capture across multiple devices
    Swarm {
        /// Space-separated list of device IDs
        #[arg(required = true)]
        device_ids: Vec<String>,
    },

    /// Get status of a swarm capture job
    SwarmStatus {
        swarm_id: String,
    },

    /// List all swarm capture jobs
    SwarmList,

    /// Discover sovereign nodes on the local network via mDNS
    Peers,

    /// Download PLY splat file for a twin over WebSocket
    Splat {
        /// Twin ID
        twin_id: String,
        /// Output file path (default: <twin_id>.ply)
        #[arg(short, long)]
        out: Option<String>,
    },

    /// Get the Merkle root of all receipts
    MerkleRoot,

    /// Get Àṣẹ tile economy for a specific tile
    TileEconomy {
        tile_id: String,
    },

    /// Push recent receipts to all configured peer nodes via DIP gossip
    Gossip {
        /// Number of receipts to push (default: 10)
        #[arg(short, long, default_value_t = 10)]
        count: usize,
    },

    /// Get this node's IP Root Nostr event (kind:31900)
    IpRoot,

    /// Get the Nostr IP provenance events for a twin (Twin Binding 1903 + Creation Receipt 1901)
    IpReceipt {
        twin_id: String,
    },

    /// Verify that a receipt ID is included in the current Merkle tree
    Verify {
        receipt_id: String,
    },

    /// Retry a failed capture job
    JobRetry {
        job_id: String,
    },

    /// Stream real-time job lifecycle events via SSE
    StreamJobs,

    /// Standalone IP provenance commands (no node required)
    Ip {
        #[command(subcommand)]
        sub: IpCmd,
    },
    // ── Governance (Phase 44) ─────────────────────────────────────────────
    /// List all governance proposals
    Proposals,
    /// Create a new governance grant proposal
    Propose {
        #[arg(long)] proposer: String,
        #[arg(long)] recipient: String,
        #[arg(long)] amount: u64,
        #[arg(long)] purpose: String,
        #[arg(long, default_value_t = 1)] veil_id: u64,
    },
    /// Vote for a governance proposal
    VoteFor {
        #[arg(long)] id: u64,
    },
    /// Vote against a governance proposal
    VoteAgainst {
        #[arg(long)] id: u64,
    },
    /// Execute a passed governance proposal
    Execute {
        #[arg(long)] id: u64,
    },
    // ── Cowrie Oracle + Emission (Phase 45) ──────────────────────────────
    /// Query the Cowrie Oracle for today's active Odù tile
    Oracle,
    /// Query the Cowrie Oracle for a specific day
    OracleDay {
        #[arg(long)] day: u32,
    },
    /// Show current emission allocator status
    EmissionStatus,
    /// Submit a proof claim to the emission allocator
    EmissionClaim {
        #[arg(long)] proof_id: String,
        #[arg(long)] worker_did: String,
        #[arg(long, default_value_t = 1)] veil_id: u16,
        #[arg(long, default_value = "0000000000000000000000000000000000000000000000000000000000000000")]
        trajectory_hash: String,
        #[arg(long, default_value_t = 0.85)] f1_score: f64,
        #[arg(long, default_value_t = 0.8)]  proof_value: f64,
        #[arg(long, default_value = "0000000000000000000000000000000000000000000000000000000000000000")]
        env_hash: String,
    },
    // ── Sovereign Wallets (Phase 46) ─────────────────────────────────────
    /// List all sovereign wallets
    Wallets,
    /// Get a specific wallet balance
    Wallet {
        #[arg(long)] did: String,
    },
    /// Credit µÀṣẹ to a wallet
    WalletCredit {
        #[arg(long)] did: String,
        #[arg(long)] amount: u64,
        #[arg(long, default_value = "manual")] reason: String,
    },
    // ── Proof submission (Phase 47) ──────────────────────────────────────
    /// Submit or query proofs (simulation, Gaussian, physical, observation)
    Proof {
        #[command(subcommand)]
        sub: ProofCmd,
    },
    // ── License marketplace (Phase 48) ───────────────────────────────────
    /// List all twin license grants
    Licenses,
    /// List license grants for a specific twin
    LicensesForTwin {
        twin_id: String,
    },
    /// Issue a license grant for a twin
    LicenseIssue {
        #[arg(long)] twin_id: String,
        #[arg(long)] grantee: String,
        #[arg(long, default_value_t = 5.0)] usage_fee_pct: f64,
        #[arg(long)] expires_at: Option<u64>,
        #[arg(long, default_value = "exclusive=false")] terms: String,
    },
    /// Get a specific license grant
    License {
        grant_id: String,
    },
    /// Accept (counter-sign) a license grant as the grantee
    LicenseAccept {
        grant_id: String,
        #[arg(long)] grantee_sig: String,
    },
    // ── Body / VCP sessions (Phase 49) ────────────────────────────────────
    /// List all VCP body sessions
    BodySessions,
    /// Open a new VCP body session
    BodyOpen {
        #[arg(long)] body_id: String,
        #[arg(long)] device_id: String,
        #[arg(long)] operator_did: String,
        #[arg(long, default_value = "capture")] session_type: String,
    },
    /// Get a specific body session
    BodySession {
        session_id: String,
    },
    /// Push a telemetry frame to an open body session
    BodyTelemetry {
        #[arg(long)] session_id: String,
        #[arg(long)] frame_index: u32,
        #[arg(long, default_value_t = 0.0)] lat: f64,
        #[arg(long, default_value_t = 0.0)] lon: f64,
        #[arg(long, default_value_t = 0.0)] altitude_m: f64,
        #[arg(long, default_value_t = 100)] battery_pct: u8,
    },
    /// Close (finalize) a body session and emit a FlightReceipt
    BodyClose {
        #[arg(long)] session_id: String,
        #[arg(long, default_value = "did:node:system")] operator_did: String,
    },
    /// Get flight receipts for a body
    BodyReceipts {
        body_id: String,
    },
    /// List VCP body capabilities
    BodyCapabilities,
}

#[derive(Subcommand, Debug)]
enum ProofCmd {
    /// Submit a simulation proof for evaluation
    Simulate {
        #[arg(long)] env_hash: String,
        #[arg(long, default_value_t = 0.85)] f1_score: f64,
        #[arg(long, default_value_t = 0.80)] proof_value: f64,
        #[arg(long, default_value_t = 0.70)] difficulty: f64,
        #[arg(long, default_value_t = 1)] veil_id: u64,
        #[arg(long)] worker_did: Option<String>,
    },
    /// Get a previously submitted simulation proof by ID
    Get {
        id: String,
    },
    /// Submit a Gaussian splat quality proof
    Gaussian {
        #[arg(long)] twin_id: String,
        #[arg(long, default_value_t = 0.85)] quality_score: f64,
        #[arg(long, default_value_t = 1000)] point_count: u64,
        #[arg(long, default_value_t = 0.9)]  coverage: f64,
        #[arg(long, default_value_t = 0.88)] sharpness: f64,
        #[arg(long, default_value_t = 0.0)]  lat: f64,
        #[arg(long, default_value_t = 0.0)]  lon: f64,
        #[arg(long)] worker_did: Option<String>,
    },
    /// Submit a physical Reality Transfer Score proof
    Physical {
        #[arg(long)] twin_id: String,
        #[arg(long, default_value_t = 0.80)] rts_score: f64,
        #[arg(long, default_value_t = 0.90)] semantic_fidelity: f64,
        #[arg(long, default_value_t = 0.85)] geometric_accuracy: f64,
        #[arg(long, default_value_t = 0.0)]  lat: f64,
        #[arg(long, default_value_t = 0.0)]  lon: f64,
        #[arg(long)] worker_did: Option<String>,
    },
    /// Submit an observation proof (raw JSON body)
    Observation {
        /// JSON file path or inline JSON string
        payload: String,
    },
    /// Get a previously submitted observation proof by ID
    ObservationGet {
        id: String,
    },
}

#[derive(Subcommand, Debug)]
enum IpCmd {
    /// Publish an IP Root (kind:31900) to Nostr — run once at agent birth.
    PublishRoot {
        /// 32-byte secret key as hex (64 chars). Env: SOVEREIGN_NOSTR_NSEC
        #[arg(long, env = "SOVEREIGN_NOSTR_NSEC")]
        nsec: String,
        /// Nostr relay WebSocket URL
        #[arg(long, default_value = "wss://relay.damus.io")]
        relay: String,
        /// Display name for the IP Root
        #[arg(long)]
        name: Option<String>,
        /// License (SPDX id). Default: cc-by-4.0
        #[arg(long, default_value = "cc-by-4.0")]
        license: String,
    },
    /// Seal a Gaussian splat's IP provenance (Twin Binding 1903 + Creation Receipt 1901).
    /// Optionally publish to a relay; always prints the signed events as JSON.
    Seal {
        /// 32-byte secret key as hex. Env: SOVEREIGN_NOSTR_NSEC
        #[arg(long, env = "SOVEREIGN_NOSTR_NSEC")]
        nsec: String,
        /// Digital twin ID (from sovereign-node)
        twin_id: String,
        /// Scene receipt ID (from sovereign-node capture job)
        scene_id: String,
        /// sha256 hex of the PLY splat file
        splat_sha256: String,
        /// F1 quality score (0.0–1.0)
        #[arg(long)]
        f1: Option<f32>,
        /// Title for the Creation Receipt
        #[arg(long, default_value = "Sovereign Gaussian splat")]
        title: String,
        /// Publish to this relay (if set)
        #[arg(long)]
        relay: Option<String>,
    },
    /// Show your IP Root pubkey derived from an nsec (no network needed).
    Pubkey {
        /// 32-byte secret key as hex. Env: SOVEREIGN_NOSTR_NSEC
        #[arg(long, env = "SOVEREIGN_NOSTR_NSEC")]
        nsec: String,
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

#[derive(Subcommand, Debug)]
enum A2aCmd {
    /// Fetch the A2A AgentCard
    Agent,
    /// Submit a task and poll until complete
    Task {
        /// User message text
        text: String,
    },
    /// Poll an existing task by ID
    Poll {
        task_id: String,
    },
}

#[derive(Subcommand, Debug)]
enum FinetuneCmd {
    /// Extract fine-tune traces from conversation logs
    Extract {
        /// Input directory of logs
        #[arg(long, default_value = ".")]
        input_dir: String,
        /// Output JSONL file
        #[arg(long, default_value = "finetune_data.jsonl")]
        output: String,
    },
    /// Print stats about extracted data
    Stats {
        /// Input directory
        #[arg(long, default_value = ".")]
        input_dir: String,
    },
    /// Generate a QLoRA training script
    Script {
        /// Base model name
        #[arg(long, default_value = "Qwen/Qwen2.5-3B-Instruct")]
        model: String,
        /// Number of training steps
        #[arg(long, default_value_t = 500)]
        steps: u32,
        /// Output shell script path
        #[arg(long, default_value = "train_qlora.sh")]
        output: String,
    },
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("failed to build HTTP client: {e}"))?;

    match &cli.cmd {
        // ── Simple GET endpoints ───────────────────────────────────────────
        Cmd::Status => {
            let val = get(&client, &cli.url, "/status").await?;
            print_json(&val);
        }

        Cmd::Devices { format } => {
            let val = get(&client, &cli.url, "/devices").await?;
            if format == "table" {
                print_devices_table(&val);
            } else {
                print_json(&val);
            }
        }

        Cmd::Jobs { format } => {
            let val = get(&client, &cli.url, "/jobs").await?;
            if format == "table" {
                print_jobs_table(&val);
            } else {
                print_json(&val);
            }
        }

        Cmd::Receipts => {
            let val = get(&client, &cli.url, "/receipts").await?;
            print_json(&val);
        }

        Cmd::Job { job_id } => {
            let val = get(&client, &cli.url, &format!("/jobs/{job_id}")).await?;
            print_json(&val);
        }

        Cmd::Receipt { twin_id } => {
            let val = get(&client, &cli.url, &format!("/receipts/{twin_id}")).await?;
            print_json(&val);
        }

        // ── Capture ────────────────────────────────────────────────────────
        Cmd::Capture { device_id } => {
            let url = format!("{}/capture/{device_id}", cli.url);
            let val = client
                .post(&url)
                .send()
                .await
                .map_err(|e| e.to_string())?
                .json::<Value>()
                .await
                .map_err(|e| e.to_string())?;
            print_json(&val);
        }

        // ── MCP ────────────────────────────────────────────────────────────
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
            let val = client
                .post(&url)
                .json(&body)
                .send()
                .await
                .map_err(|e| e.to_string())?
                .json::<Value>()
                .await
                .map_err(|e| e.to_string())?;
            print_json(&val);
        }

        // ── Device subcommand ──────────────────────────────────────────────
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
                let val = client
                    .post(&url)
                    .json(&manifest_val)
                    .send()
                    .await
                    .map_err(|e| e.to_string())?
                    .json::<Value>()
                    .await
                    .map_err(|e| e.to_string())?;
                print_json(&val);
            }
        },

        // ── DIP Send ───────────────────────────────────────────────────────
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
            let val = client
                .post(&url)
                .json(&envelope_val)
                .send()
                .await
                .map_err(|e| e.to_string())?
                .json::<Value>()
                .await
                .map_err(|e| e.to_string())?;
            print_json(&val);
        }

        // ── Watch (WebSocket) ──────────────────────────────────────────────
        Cmd::Watch { twin_id } => {
            cmd_watch(&cli.url, twin_id).await?;
        }

        // ── Delegate ───────────────────────────────────────────────────────
        Cmd::Delegate { device_id, peer, hint } => {
            cmd_delegate(&client, &cli.url, device_id, peer.as_deref(), hint.as_deref()).await?;
        }

        // ── A2A ───────────────────────────────────────────────────────────
        Cmd::A2a { sub } => match sub {
            A2aCmd::Agent => {
                let val = get(&client, &cli.url, "/a2a/agent").await?;
                print_json(&val);
            }
            A2aCmd::Task { text } => {
                cmd_a2a_task(&client, &cli.url, text).await?;
            }
            A2aCmd::Poll { task_id } => {
                let val = get(&client, &cli.url, &format!("/a2a/tasks/{task_id}")).await?;
                print_json(&val);
            }
        },

        // ── Finetune ──────────────────────────────────────────────────────
        Cmd::Finetune { sub } => match sub {
            FinetuneCmd::Extract { input_dir, output } => {
                run_mycelium(&["extract", "--input-dir", input_dir, "--output", output])?;
            }
            FinetuneCmd::Stats { input_dir } => {
                run_mycelium(&["stats", "--input-dir", input_dir])?;
            }
            FinetuneCmd::Script { model, steps, output } => {
                run_mycelium(&[
                    "script",
                    "--model",
                    model,
                    "--steps",
                    &steps.to_string(),
                    "--output",
                    output,
                ])?;
            }
        },

        // ── Tiles ─────────────────────────────────────────────────────────
        Cmd::Tiles => {
            let val = get(&client, &cli.url, "/tiles").await?;
            print_json(&val);
        }
        Cmd::Tile { tile_id } => {
            let val = get(&client, &cli.url, &format!("/tiles/{tile_id}/receipts")).await?;
            print_json(&val);
        }

        // ── Timelines ─────────────────────────────────────────────────────
        Cmd::Timelines => {
            let val = get(&client, &cli.url, "/timelines").await?;
            print_json(&val);
        }
        Cmd::Timeline { twin_id } => {
            let val = get(&client, &cli.url, &format!("/twins/{twin_id}/timeline")).await?;
            print_json(&val);
        }
        Cmd::TimelineDiff { twin_id } => {
            let val = get(&client, &cli.url, &format!("/twins/{twin_id}/timeline/diff")).await?;
            print_json(&val);
        }

        // ── Swarm ─────────────────────────────────────────────────────────
        Cmd::Swarm { device_ids } => {
            cmd_swarm(&client, &cli.url, device_ids.clone()).await?;
        }
        Cmd::SwarmStatus { swarm_id } => {
            let val = get(&client, &cli.url, &format!("/swarm/{swarm_id}")).await?;
            print_json(&val);
        }
        Cmd::SwarmList => {
            let val = get(&client, &cli.url, "/swarm").await?;
            print_json(&val);
        }

        // ── Peers ─────────────────────────────────────────────────────────
        Cmd::Peers => {
            let val = get(&client, &cli.url, "/federation/peers").await?;
            print_json(&val);
        }

        // ── Splat ─────────────────────────────────────────────────────────
        Cmd::Splat { twin_id, out } => {
            cmd_splat(&cli.url, twin_id, out.as_deref()).await?;
        }

        // ── MerkleRoot ────────────────────────────────────────────────────
        Cmd::MerkleRoot => {
            let val = get(&client, &cli.url, "/receipts/root").await?;
            print_json(&val);
        }

        // ── TileEconomy ───────────────────────────────────────────────────
        Cmd::TileEconomy { tile_id } => {
            let val = get(&client, &cli.url, &format!("/tiles/{tile_id}/economy")).await?;
            print_json(&val);
        }

        // ── Gossip ────────────────────────────────────────────────────────
        Cmd::Gossip { count } => {
            let val = post(&client, &cli.url, "/dip/gossip", Some(json!({ "count": count }))).await?;
            print_json(&val);
        }

        // ── IP provenance ─────────────────────────────────────────────
        Cmd::IpRoot => {
            let val = get(&client, &cli.url, "/ip/root").await?;
            print_json(&val);
        }
        Cmd::IpReceipt { twin_id } => {
            let val = get(&client, &cli.url, &format!("/ip/receipt/{twin_id}")).await?;
            print_json(&val);
        }

        // ── Verify ────────────────────────────────────────────────────
        Cmd::Verify { receipt_id } => {
            let val = get(&client, &cli.url, &format!("/receipts/verify/{receipt_id}")).await?;
            print_json(&val);
        }

        // ── Job retry ─────────────────────────────────────────────────
        Cmd::JobRetry { job_id } => {
            let val = post(&client, &cli.url, &format!("/jobs/{job_id}/retry"), None).await?;
            print_json(&val);
        }

        // ── Stream Jobs (SSE) ──────────────────────────────────────────
        Cmd::StreamJobs => {
            cmd_stream_jobs(&cli.url).await?;
        }

        // ── Standalone IP provenance (no node required) ────────────────
        Cmd::Ip { sub } => {
            cmd_ip(sub).await?;
        }

        // ── Governance (Phase 44) ──────────────────────────────────────────────
        Cmd::Proposals => {
            let val = get(&client, &cli.url, "/governance/proposals").await?;
            print_json(&val);
        }
        Cmd::Propose { proposer, recipient, amount, purpose, veil_id } => {
            let body = serde_json::json!({
                "proposer":          proposer,
                "recipient":         recipient,
                "amount_micro_ase":  amount,
                "purpose":           purpose,
                "veil_id":           veil_id,
            });
            let val = post(&client, &cli.url, "/governance/proposals", Some(body)).await?;
            print_json(&val);
        }
        Cmd::VoteFor { id } => {
            let val = post(&client, &cli.url, &format!("/governance/proposals/{id}/vote_for"), None).await?;
            print_json(&val);
        }
        Cmd::VoteAgainst { id } => {
            let val = post(&client, &cli.url, &format!("/governance/proposals/{id}/vote_against"), None).await?;
            print_json(&val);
        }
        Cmd::Execute { id } => {
            let val = post(&client, &cli.url, &format!("/governance/proposals/{id}/execute"), None).await?;
            print_json(&val);
        }

        // ── Cowrie Oracle + Emission (Phase 45) ───────────────────────────────
        Cmd::Oracle => {
            let val = get(&client, &cli.url, "/oracle/today").await?;
            print_json(&val);
        }
        Cmd::OracleDay { day } => {
            let val = get(&client, &cli.url, &format!("/oracle/day/{day}")).await?;
            print_json(&val);
        }
        Cmd::EmissionStatus => {
            let val = get(&client, &cli.url, "/emission/status").await?;
            print_json(&val);
        }
        Cmd::EmissionClaim { proof_id, worker_did, veil_id, trajectory_hash, f1_score, proof_value, env_hash } => {
            let body = serde_json::json!({
                "proof_id":         proof_id,
                "worker_did":       worker_did,
                "veil_id":          veil_id,
                "trajectory_hash":  trajectory_hash,
                "f1_score":         f1_score,
                "proof_value":      proof_value,
                "env_hash":         env_hash,
            });
            let val = post(&client, &cli.url, "/emission/claim", Some(body)).await?;
            print_json(&val);
        }

        // ── Sovereign Wallets (Phase 46) ──────────────────────────────────────
        Cmd::Wallets => {
            let val = get(&client, &cli.url, "/wallets").await?;
            print_json(&val);
        }
        Cmd::Wallet { did } => {
            let encoded = urlencoding::encode(&did);
            let val = get(&client, &cli.url, &format!("/wallets/{encoded}")).await?;
            print_json(&val);
        }
        Cmd::WalletCredit { did, amount, reason } => {
            let encoded = urlencoding::encode(&did);
            let body = serde_json::json!({ "amount_micro_ase": amount, "reason": reason });
            let val = post(&client, &cli.url, &format!("/wallets/{encoded}/credit"), Some(body)).await?;
            print_json(&val);
        }
        // ── Proof commands (Phase 47) ─────────────────────────────────────
        Cmd::Proof { sub } => match sub {
            ProofCmd::Simulate { env_hash, f1_score, proof_value, difficulty, veil_id, worker_did } => {
                let body = serde_json::json!({
                    "env_hash": env_hash,
                    "f1_score": f1_score,
                    "proof_value": proof_value,
                    "difficulty": difficulty,
                    "veil_id": veil_id,
                    "worker_did": worker_did.clone().unwrap_or_else(|| "did:node:cli".into()),
                });
                let val = post(&client, &cli.url, "/proofs/simulation", Some(body)).await?;
                print_json(&val);
            }
            ProofCmd::Get { id } => {
                let encoded = urlencoding::encode(&id);
                let val = get(&client, &cli.url, &format!("/proofs/simulation/{encoded}")).await?;
                print_json(&val);
            }
            ProofCmd::Gaussian { twin_id, quality_score, point_count, coverage, sharpness, lat, lon, worker_did } => {
                let body = serde_json::json!({
                    "twin_id": twin_id,
                    "quality_score": quality_score,
                    "point_count": point_count,
                    "coverage": coverage,
                    "sharpness": sharpness,
                    "lat": lat,
                    "lon": lon,
                    "worker_did": worker_did.clone().unwrap_or_else(|| "did:node:cli".into()),
                });
                let val = post(&client, &cli.url, "/proofs/gaussian", Some(body)).await?;
                print_json(&val);
            }
            ProofCmd::Physical { twin_id, rts_score, semantic_fidelity, geometric_accuracy, lat, lon, worker_did } => {
                let body = serde_json::json!({
                    "twin_id": twin_id,
                    "rts_score": rts_score,
                    "semantic_fidelity": semantic_fidelity,
                    "geometric_accuracy": geometric_accuracy,
                    "lat": lat,
                    "lon": lon,
                    "worker_did": worker_did.clone().unwrap_or_else(|| "did:node:cli".into()),
                });
                let val = post(&client, &cli.url, "/proofs/physical", Some(body)).await?;
                print_json(&val);
            }
            ProofCmd::Observation { payload } => {
                let body: Value = if payload.trim_start().starts_with('{') {
                    serde_json::from_str(&payload)?
                } else {
                    let raw = std::fs::read_to_string(&payload)?;
                    serde_json::from_str(&raw)?
                };
                let val = post(&client, &cli.url, "/proofs/observation", Some(body)).await?;
                print_json(&val);
            }
            ProofCmd::ObservationGet { id } => {
                let encoded = urlencoding::encode(&id);
                let val = get(&client, &cli.url, &format!("/proofs/observation/{encoded}")).await?;
                print_json(&val);
            }
        },
        // ── License marketplace commands (Phase 48) ───────────────────────
        Cmd::Licenses => {
            let val = get(&client, &cli.url, "/licenses").await?;
            print_json(&val);
        }
        Cmd::LicensesForTwin { twin_id } => {
            let encoded = urlencoding::encode(&twin_id);
            let val = get(&client, &cli.url, &format!("/twins/{encoded}/licenses")).await?;
            print_json(&val);
        }
        Cmd::LicenseIssue { twin_id, grantee, usage_fee_pct, expires_at, terms } => {
            let encoded_twin = urlencoding::encode(&twin_id);
            let body = serde_json::json!({
                "grantee_did": grantee,
                "usage_fee_pct": usage_fee_pct,
                "expires_at": expires_at,
                "terms": terms,
            });
            let val = post(&client, &cli.url, &format!("/twins/{encoded_twin}/licenses"), Some(body)).await?;
            print_json(&val);
        }
        Cmd::License { grant_id } => {
            let encoded = urlencoding::encode(&grant_id);
            let val = get(&client, &cli.url, &format!("/licenses/{encoded}")).await?;
            print_json(&val);
        }
        Cmd::LicenseAccept { grant_id, grantee_sig } => {
            let encoded = urlencoding::encode(&grant_id);
            let body = serde_json::json!({ "grantee_sig": grantee_sig });
            let val = post(&client, &cli.url, &format!("/licenses/{encoded}/accept"), Some(body)).await?;
            print_json(&val);
        }
        // ── Body / VCP session commands (Phase 49) ────────────────────────
        Cmd::BodySessions => {
            let val = get(&client, &cli.url, "/body/sessions").await?;
            print_json(&val);
        }
        Cmd::BodyOpen { body_id, device_id, operator_did, session_type } => {
            let body = serde_json::json!({
                "body_id": body_id,
                "device_id": device_id,
                "operator_did": operator_did,
                "session_type": session_type,
            });
            let val = post(&client, &cli.url, "/body/sessions", Some(body)).await?;
            print_json(&val);
        }
        Cmd::BodySession { session_id } => {
            let encoded = urlencoding::encode(&session_id);
            let val = get(&client, &cli.url, &format!("/body/sessions/{encoded}")).await?;
            print_json(&val);
        }
        Cmd::BodyTelemetry { session_id, frame_index, lat, lon, altitude_m, battery_pct } => {
            let encoded = urlencoding::encode(&session_id);
            let body = serde_json::json!({
                "frame_index": frame_index,
                "lat": lat,
                "lon": lon,
                "altitude_m": altitude_m,
                "battery_pct": battery_pct,
                "timestamp_ms": std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
            });
            let val = post(&client, &cli.url, &format!("/body/sessions/{encoded}/telemetry"), Some(body)).await?;
            print_json(&val);
        }
        Cmd::BodyClose { session_id, operator_did } => {
            let encoded = urlencoding::encode(&session_id);
            let body = serde_json::json!({ "operator_did": operator_did });
            let val = post(&client, &cli.url, &format!("/body/sessions/{encoded}/close"), Some(body)).await?;
            print_json(&val);
        }
        Cmd::BodyReceipts { body_id } => {
            let encoded = urlencoding::encode(&body_id);
            let val = get(&client, &cli.url, &format!("/body/{encoded}/receipts")).await?;
            print_json(&val);
        }
        Cmd::BodyCapabilities => {
            let val = get(&client, &cli.url, "/body/capabilities").await?;
            print_json(&val);
        }
    }

    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn print_json(val: &Value) {
    println!(
        "{}",
        serde_json::to_string_pretty(val).unwrap_or_else(|_| val.to_string())
    );
}

async fn get(client: &reqwest::Client, base: &str, path: &str) -> Result<Value, Box<dyn std::error::Error>> {
    let url = format!("{base}{path}");
    let val = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("GET {url}: {e}"))?
        .json::<Value>()
        .await
        .map_err(|e| format!("parse response: {e}"))?;
    Ok(val)
}

async fn post(client: &reqwest::Client, base: &str, path: &str, body: Option<Value>)
    -> Result<Value, Box<dyn std::error::Error>>
{
    let url = format!("{base}{path}");
    let req = match body {
        Some(b) => client.post(&url).json(&b),
        None    => client.post(&url),
    };
    let val = req
        .send()
        .await
        .map_err(|e| format!("POST {url}: {e}"))?
        .json::<Value>()
        .await
        .map_err(|e| format!("parse response: {e}"))?;
    Ok(val)
}

// ── Table printers ────────────────────────────────────────────────────────────

fn print_devices_table(val: &Value) {
    let rows: Vec<[String; 4]> = match val.as_array() {
        Some(arr) => arr
            .iter()
            .map(|d| {
                [
                    d["device_id"].as_str().unwrap_or("-").to_string(),
                    d["model"].as_str().unwrap_or("-").to_string(),
                    d["rssi"]
                        .as_i64()
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "-".to_string()),
                    d["last_seen"].as_str().unwrap_or("-").to_string(),
                ]
            })
            .collect(),
        None => {
            eprintln!("warning: expected array for devices, falling back to JSON");
            print_json(val);
            return;
        }
    };

    let headers = ["DEVICE_ID", "MODEL", "RSSI", "LAST_SEEN"];
    let mut widths = [
        headers[0].len(),
        headers[1].len(),
        headers[2].len(),
        headers[3].len(),
    ];
    for row in &rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.len());
        }
    }

    let fmt_row = |cells: [&str; 4]| {
        format!(
            "{:<w0$}  {:<w1$}  {:<w2$}  {:<w3$}",
            cells[0],
            cells[1],
            cells[2],
            cells[3],
            w0 = widths[0],
            w1 = widths[1],
            w2 = widths[2],
            w3 = widths[3],
        )
    };

    println!("{}", fmt_row(["DEVICE_ID", "MODEL", "RSSI", "LAST_SEEN"]));
    println!("{}", "-".repeat(widths[0] + widths[1] + widths[2] + widths[3] + 6));
    for row in &rows {
        println!("{}", fmt_row([&row[0], &row[1], &row[2], &row[3]]));
    }
}

fn print_jobs_table(val: &Value) {
    let rows: Vec<[String; 4]> = match val.as_array() {
        Some(arr) => arr
            .iter()
            .map(|j| {
                [
                    j["job_id"].as_str().unwrap_or("-").to_string(),
                    j["device_id"].as_str().unwrap_or("-").to_string(),
                    j["state"].as_str().unwrap_or("-").to_string(),
                    j["created_at"].as_str().unwrap_or("-").to_string(),
                ]
            })
            .collect(),
        None => {
            eprintln!("warning: expected array for jobs, falling back to JSON");
            print_json(val);
            return;
        }
    };

    let headers = ["JOB_ID", "DEVICE_ID", "STATE", "CREATED_AT"];
    let mut widths = [
        headers[0].len(),
        headers[1].len(),
        headers[2].len(),
        headers[3].len(),
    ];
    for row in &rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.len());
        }
    }

    let fmt_row = |cells: [&str; 4]| {
        format!(
            "{:<w0$}  {:<w1$}  {:<w2$}  {:<w3$}",
            cells[0],
            cells[1],
            cells[2],
            cells[3],
            w0 = widths[0],
            w1 = widths[1],
            w2 = widths[2],
            w3 = widths[3],
        )
    };

    println!("{}", fmt_row(["JOB_ID", "DEVICE_ID", "STATE", "CREATED_AT"]));
    println!("{}", "-".repeat(widths[0] + widths[1] + widths[2] + widths[3] + 6));
    for row in &rows {
        println!("{}", fmt_row([&row[0], &row[1], &row[2], &row[3]]));
    }
}

// ── Watch (WebSocket) ─────────────────────────────────────────────────────────

async fn cmd_watch(base_url: &str, twin_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    use futures_util::StreamExt;
    use tokio_tungstenite::connect_async;
    use tokio_tungstenite::tungstenite::Message;

    // Build WS URL by replacing scheme
    let ws_base = base_url
        .replacen("https://", "wss://", 1)
        .replacen("http://", "ws://", 1);
    let ws_url = format!("{ws_base}/ws/twin/{twin_id}");

    eprintln!("connecting to {ws_url} …");

    let (ws_stream, _) = connect_async(&ws_url)
        .await
        .map_err(|e| format!("WebSocket connect failed: {e}"))?;

    let (_, mut read) = ws_stream.split();

    loop {
        match read.next().await {
            Some(Ok(Message::Text(text))) => {
                let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
                // Try to parse as JSON for pretty-print and event extraction
                match serde_json::from_str::<Value>(&text) {
                    Ok(v) => {
                        let event = v["event"].as_str().unwrap_or("unknown");
                        let pretty = serde_json::to_string_pretty(&v)
                            .unwrap_or_else(|_| text.clone());
                        println!("[{ts}] {event} → {pretty}");
                    }
                    Err(_) => {
                        println!("[{ts}] (raw) → {text}");
                    }
                }
            }
            Some(Ok(Message::Binary(b))) => {
                let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
                println!("[{ts}] (binary {} bytes)", b.len());
            }
            Some(Ok(Message::Close(_))) | None => {
                eprintln!("connection closed — reconnect with same command");
                break;
            }
            Some(Ok(_)) => {} // ping/pong frames — ignore
            Some(Err(e)) => {
                eprintln!("WebSocket error: {e}");
                eprintln!("connection closed — reconnect with same command");
                break;
            }
        }
    }

    Ok(())
}

// ── Delegate ──────────────────────────────────────────────────────────────────

async fn cmd_delegate(
    client: &reqwest::Client,
    base_url: &str,
    device_id: &str,
    peer: Option<&str>,
    hint: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let body = json!({
        "device_id": device_id,
        "peer":      peer,
        "hint":      hint,
    });

    let url = format!("{base_url}/capture/delegate");
    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("POST {url}: {e}"))?;

    let status = resp.status();
    let val: Value = resp
        .json()
        .await
        .map_err(|e| format!("parse delegate response: {e}"))?;

    if !status.is_success() {
        let reason = val["reason"]
            .as_str()
            .or_else(|| val["error"].as_str())
            .or_else(|| val["message"].as_str())
            .unwrap_or("unknown error");
        eprintln!("Delegation failed: {reason}");
        std::process::exit(1);
    }

    let peer_name = val["peer"]
        .as_str()
        .unwrap_or(peer.unwrap_or("(none)"));
    let task_id = val["task_id"].as_str().unwrap_or("-");
    let poll_url = val["poll_url"].as_str().unwrap_or("-");

    println!("Delegated to {peer_name} — task_id: {task_id} — poll: {poll_url}");
    Ok(())
}

// ── A2A task submit + poll ────────────────────────────────────────────────────

async fn cmd_a2a_task(
    client: &reqwest::Client,
    base_url: &str,
    text: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let body = json!({
        "message": {
            "role": "user",
            "parts": [{"type": "text", "text": text}]
        }
    });

    let url = format!("{base_url}/a2a/tasks");
    let val: Value = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("POST {url}: {e}"))?
        .json()
        .await
        .map_err(|e| format!("parse a2a/tasks response: {e}"))?;

    let task_id = val["id"]
        .as_str()
        .or_else(|| val["task_id"].as_str())
        .ok_or("a2a/tasks response missing 'id' field")?
        .to_string();

    println!("task_id: {task_id}");

    // Poll loop
    let terminal_states = ["completed", "failed", "canceled", "input_required"];
    let mut elapsed_secs = 0u64;

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        elapsed_secs += 2;

        let poll_url = format!("{base_url}/a2a/tasks/{task_id}");
        let poll_val: Value = client
            .get(&poll_url)
            .send()
            .await
            .map_err(|e| format!("GET {poll_url}: {e}"))?
            .json()
            .await
            .map_err(|e| format!("parse poll response: {e}"))?;

        let state = poll_val["status"]["state"]
            .as_str()
            .unwrap_or("unknown");

        if terminal_states.contains(&state) {
            println!("\n  [{state}] done.");
            print_json(&poll_val);
            break;
        }

        print!("  [{state}] {elapsed_secs}s elapsed...\r");
        use std::io::Write;
        let _ = std::io::stdout().flush();
    }

    Ok(())
}

// ── Finetune subprocess runner ────────────────────────────────────────────────

fn run_mycelium(args: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    // Try $PATH first, then local debug build
    let binary = which_mycelium();

    let mut cmd = std::process::Command::new(&binary);
    cmd.args(args);

    let status = cmd
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                format!(
                    "mycelium-finetune not found.\n\
                     Install with: cargo install --path mycelium-finetune"
                )
            } else {
                format!("failed to spawn mycelium-finetune: {e}")
            }
        })?
        .wait()
        .map_err(|e| format!("mycelium-finetune subprocess error: {e}"))?;

    if !status.success() {
        eprintln!(
            "mycelium-finetune exited with code {}",
            status.code().unwrap_or(-1)
        );
        std::process::exit(status.code().unwrap_or(1));
    }

    Ok(())
}

fn which_mycelium() -> String {
    // Check $PATH via std::process::Command dry-run (or just try the name first,
    // then fall back to local build path)
    let candidates = [
        "mycelium-finetune",
        "./target/debug/mycelium-finetune",
    ];

    for candidate in &candidates {
        // A quick existence check for absolute/relative paths; for bare names we trust
        // the OS to resolve via PATH when Command::new is called.
        if candidate.starts_with('.') || candidate.starts_with('/') {
            if std::path::Path::new(candidate).exists() {
                return candidate.to_string();
            }
        } else {
            // Bare binary name — let the OS try; return it and handle NotFound above
            return candidate.to_string();
        }
    }

    // Last resort — the fallback path even if it doesn't exist (error will be raised)
    "./target/debug/mycelium-finetune".to_string()
}

// ── Swarm capture ─────────────────────────────────────────────────────────────

async fn cmd_swarm(
    client: &reqwest::Client,
    base_url: &str,
    device_ids: Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let url = format!("{base_url}/capture/swarm");
    let payload = json!({ "device_ids": device_ids });
    let resp = client
        .post(&url)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("POST {url}: {e}"))?;
    let val: Value = resp.json().await.map_err(|e| format!("parse: {e}"))?;
    eprintln!("swarm launched: {}", val["swarm_id"].as_str().unwrap_or("?"));
    eprintln!("poll with: sovereign swarm-status {}", val["swarm_id"].as_str().unwrap_or("?"));
    print_json(&val);
    Ok(())
}

// ── Stream Jobs (SSE) ─────────────────────────────────────────────────────────

async fn cmd_stream_jobs(base_url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let url = format!("{base_url}/events/jobs");
    eprintln!("streaming job events from {url} (Ctrl-C to stop)");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(0))  // no timeout for streaming
        .build()?;

    let mut response = client.get(&url).send().await
        .map_err(|e| format!("connect: {e}"))?;

    while let Some(chunk) = response.chunk().await.map_err(|e| format!("read: {e}"))? {
        let text = String::from_utf8_lossy(&chunk);
        for line in text.lines() {
            if line.starts_with("data:") {
                let data = line.trim_start_matches("data:").trim();
                // Pretty-print if valid JSON
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(data) {
                    println!("{}", serde_json::to_string_pretty(&val).unwrap_or_else(|_| data.to_string()));
                } else {
                    println!("{data}");
                }
            } else if line.starts_with("event:") {
                let event_type = line.trim_start_matches("event:").trim();
                eprintln!("[{event_type}]");
            }
        }
    }

    Ok(())
}

// ── Splat download ────────────────────────────────────────────────────────────

async fn cmd_splat(
    base_url: &str,
    twin_id:  &str,
    out_path: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;
    use tokio_tungstenite::connect_async;
    use tokio_tungstenite::tungstenite::Message;

    let ws_base = base_url
        .replacen("https://", "wss://", 1)
        .replacen("http://", "ws://", 1);
    let ws_url = format!("{ws_base}/ws/splat/{twin_id}");

    let default_path = format!("{}.ply", twin_id.replace([':', '/'], "_"));
    let out_path     = out_path.unwrap_or(&default_path);

    eprintln!("connecting to {ws_url}");
    let (ws_stream, _) = connect_async(&ws_url)
        .await
        .map_err(|e| format!("WebSocket connect: {e}"))?;

    let (_, mut read) = ws_stream.split();

    let mut file = tokio::fs::File::create(out_path)
        .await
        .map_err(|e| format!("create {out_path}: {e}"))?;

    let mut total_bytes: usize = 0;
    loop {
        match read.next().await {
            Some(Ok(Message::Binary(data))) => {
                file.write_all(&data).await.map_err(|e| format!("write: {e}"))?;
                total_bytes += data.len();
                eprint!("\r  received {} bytes…", total_bytes);
                use std::io::Write;
                let _ = std::io::stderr().flush();
            }
            Some(Ok(Message::Close(_))) | None => {
                eprintln!("\nwrote {total_bytes} bytes → {out_path}");
                break;
            }
            Some(Ok(_)) => {}
            Some(Err(e)) => return Err(format!("WebSocket error: {e}").into()),
        }
    }

    file.flush().await.map_err(|e| format!("flush: {e}"))?;
    Ok(())
}

// ── Standalone IP provenance ───────────────────────────────────────────────────

async fn cmd_ip(sub: &IpCmd) -> Result<(), Box<dyn std::error::Error>> {
    use ip_layer::{NostrSecretKey, IpRootBuilder, seal_gaussian_splat};

    match sub {
        IpCmd::Pubkey { nsec } => {
            let key = NostrSecretKey::from_hex(&nsec)
                .map_err(|e| format!("invalid nsec: {e}"))?;
            println!("{}", key.pubkey_hex());
        }

        IpCmd::PublishRoot { nsec, relay, name, license } => {
            let key = NostrSecretKey::from_hex(&nsec)
                .map_err(|e| format!("invalid nsec: {e}"))?;
            let pubkey = key.pubkey_hex();

            let mut builder = IpRootBuilder::for_agent(&pubkey).with_license(license.as_str());
            if let Some(n) = name {
                builder = builder.with_display_name(n.as_str());
            }

            let now = ip_layer::now_secs();
            let event = builder.sign(&key, now)
                .map_err(|e| format!("signing failed: {e}"))?;

            eprintln!("IP Root event id: {}", event.id);
            eprintln!("pubkey:           {pubkey}");
            eprintln!("Publishing to {relay} …");

            publish_nostr(&relay, &event).await?;
            println!("{}", serde_json::to_string_pretty(&event)?);
        }

        IpCmd::Seal { nsec, twin_id, scene_id, splat_sha256, f1, title, relay } => {
            let key = NostrSecretKey::from_hex(&nsec)
                .map_err(|e| format!("invalid nsec: {e}"))?;
            let ip_root_id = key.pubkey_hex();

            let (twin_binding, creation_receipt) = seal_gaussian_splat(
                &ip_root_id, twin_id, scene_id, splat_sha256, *f1, title, &key,
            ).map_err(|e| format!("sealing failed: {e}"))?;

            eprintln!("Twin Binding  (1903): {}", twin_binding.id);
            eprintln!("Creation Receipt (1901): {}", creation_receipt.id);

            if let Some(ref relay_url) = relay {
                eprintln!("Publishing twin binding …");
                publish_nostr(relay_url, &twin_binding).await?;
                eprintln!("Publishing creation receipt …");
                publish_nostr(relay_url, &creation_receipt).await?;
                eprintln!("Published to {relay_url}");
            } else {
                eprintln!("(--relay not set — events not published, printed below)");
            }

            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                "twin_binding":       twin_binding,
                "creation_receipt":   creation_receipt,
            }))?);
        }
    }

    Ok(())
}

async fn publish_nostr(
    relay_url: &str,
    event: &ip_layer::NostrEvent,
) -> Result<(), Box<dyn std::error::Error>> {
    use tokio_tungstenite::connect_async;
    use tokio_tungstenite::tungstenite::Message;
    use futures_util::{SinkExt, StreamExt};

    let (mut ws, _) = connect_async(relay_url).await
        .map_err(|e| format!("connect to {relay_url}: {e}"))?;

    let msg = serde_json::json!(["EVENT", event]).to_string();
    ws.send(Message::Text(msg)).await
        .map_err(|e| format!("send EVENT: {e}"))?;

    // Wait for OK or NOTICE then close
    if let Some(Ok(Message::Text(resp))) = ws.next().await {
        if !resp.contains("\"OK\"") && !resp.contains("true") {
            eprintln!("relay response: {resp}");
        }
    }
    let _ = ws.close(None).await;
    Ok(())
}
