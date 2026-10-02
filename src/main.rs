//! Voisu for Windows (`voisu-win`) CLI Entry Point.
//!
//! Subcommands:
//! - `run`: Start the speech-to-text dictation daemon.
//! - `setup`: Run the interactive configuration setup wizard.
//! - `doctor`: Run hardware, network, and subsystem diagnostics.

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use voisu_win::config::AppConfig;
use voisu_win::doctor::SystemDoctor;

#[derive(Parser, Debug)]
#[command(
    name = "voisu-win",
    version = env!("CARGO_PKG_VERSION"),
    author = "Voisu Team",
    about = "Ultra-low-latency dual-engine speech-to-text dictation client for Windows"
)]
struct Cli {
    /// Custom configuration file path override
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Start the Voisu dictation daemon
    Run,
    /// Run the interactive configuration setup wizard
    Setup,
    /// Run system diagnostics (audio input, network latency, Win32 hooks)
    Doctor,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing/logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    // Determine config path
    let config = if let Some(custom_path) = cli.config {
        match AppConfig::load_from_path(&custom_path) {
            Ok(cfg) => cfg,
            Err(e) => {
                eprintln!(
                    "Failed to load custom config from {}: {}. Falling back to default.",
                    custom_path.display(),
                    e
                );
                AppConfig::default()
            }
        }
    } else {
        AppConfig::load().unwrap_or_default()
    };

    match cli.command.unwrap_or(Commands::Run) {
        Commands::Setup => {
            AppConfig::run_interactive_setup()?;
        }
        Commands::Doctor => {
            SystemDoctor::run_diagnostics(&config).await;
        }
        Commands::Run => {
            println!("============================================================");
            println!("       Voisu for Windows — Speech Dictation Daemon          ");
            println!("============================================================");
            println!("Version       : {}", env!("CARGO_PKG_VERSION"));
            println!("Trigger Key   : {:?}", config.trigger_key);
            println!("Interaction   : {:?}", config.interaction_mode);
            println!("Delivery Mode : {:?}", config.delivery_mode);

            if !config.has_active_provider() {
                println!("\n[WARNING] No STT provider API keys configured!");
                println!("Run 'voisu-win setup' to configure Deepgram or Groq credentials.");
                println!("Or run 'voisu-win doctor' to diagnose system status.\n");
            } else {
                println!("\nService initialized and waiting for hotkey trigger...");
                println!("Press Ctrl+C to terminate the daemon.");
            }

            // Keep daemon alive until Ctrl+C
            tokio::signal::ctrl_c().await?;
            println!("\nShutting down Voisu daemon cleanly.");
        }
    }

    Ok(())
}
