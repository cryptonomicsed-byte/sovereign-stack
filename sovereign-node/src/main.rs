mod config;
mod identity;
mod jobs;
mod node;
mod nostr_relay;
mod vantage;

use clap::Parser;
use std::path::PathBuf;
use tracing_subscriber::{EnvFilter, fmt};

use config::NodeConfig;
use identity::NodeIdentity;
use node::SovereignNode;

#[derive(Parser, Debug)]
#[command(name = "sovereign-node", version, about = "Sovereign Node — DIP + VCP + TSP daemon")]
struct Cli {
    /// Path to config TOML (overrides search path)
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Override log level
    #[arg(short, long)]
    log_level: Option<String>,

    /// Print resolved config and exit
    #[arg(long)]
    print_config: bool,

    /// Generate node identity, print DID, exit
    #[arg(long)]
    init_identity: bool,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let config = match &cli.config {
        Some(path) => NodeConfig::load(path).unwrap_or_else(|e| {
            eprintln!("fatal: {e}");
            std::process::exit(1);
        }),
        None => NodeConfig::load_default(),
    };

    let log_level = cli.log_level
        .as_deref()
        .unwrap_or(&config.node.log_level)
        .to_string();

    fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new(&log_level))
        )
        .with_target(false)
        .with_thread_ids(false)
        .compact()
        .init();

    if cli.print_config {
        println!("{}", toml::to_string_pretty(&config).unwrap_or_default());
        return;
    }

    if let Err(e) = std::fs::create_dir_all(&config.node.data_dir) {
        eprintln!("fatal: cannot create data_dir {}: {e}", config.node.data_dir.display());
        std::process::exit(1);
    }

    let identity = NodeIdentity::load_or_generate(
        &config.identity.key_file,
        &config.identity.did_file,
    );

    if cli.init_identity {
        println!("{}", identity.did);
        return;
    }

    SovereignNode::new(identity, config).start().await;
}
