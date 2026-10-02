//! Voisu for Windows (`voisu-win`) CLI Entry Point.
//!
//! Subcommands:
//! - `run`: Start the speech-to-text dictation daemon.
//! - `setup`: Run the interactive configuration setup wizard.
//! - `doctor`: Run hardware, network, and subsystem diagnostics.

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::task::JoinHandle;
use voisu_win::config::AppConfig;
use voisu_win::core::audio::{AudioCaptureEngine, RecordingSession};
use voisu_win::core::hotkey::{HotkeyEvent, HotkeyManager};
use voisu_win::core::types::SourceTranscript;
use voisu_win::doctor::SystemDoctor;
use voisu_win::providers::DualProviderCoordinator;
use voisu_win::providers::deepgram::DeepgramError;

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

            let audio_engine = match AudioCaptureEngine::new() {
                Ok(engine) => {
                    println!(
                        "Audio Input   : OK (Native: {} Hz, {} ch)",
                        engine.sample_rate(),
                        engine.channels()
                    );
                    Some(engine)
                }
                Err(e) => {
                    eprintln!("[ERROR] Failed to initialize audio input device: {}", e);
                    None
                }
            };

            let coordinator = Arc::new(DualProviderCoordinator::new(&config));

            let (hotkey_mgr, hotkey_rx) =
                match HotkeyManager::start(config.trigger_key, config.interaction_mode) {
                    Ok((mgr, rx)) => {
                        println!("Hotkey Hook   : OK (Installed on dedicated Win32 thread)");
                        (Some(mgr), Some(rx))
                    }
                    Err(e) => {
                        eprintln!("[ERROR] Failed to install keyboard hook: {}", e);
                        (None, None)
                    }
                };

            if !config.has_active_provider() {
                println!("\n[WARNING] No STT provider API keys configured!");
                println!("Run 'voisu-win setup' to configure Deepgram or Groq credentials.");
                println!("Or run 'voisu-win doctor' to diagnose system status.\n");
            } else {
                println!("\nService initialized and waiting for hotkey trigger...");
                println!("Hold or tap your trigger key to dictate. Press Ctrl+C to exit.\n");
            }

            type ActiveSession = (
                Option<JoinHandle<Result<SourceTranscript, DeepgramError>>>,
                RecordingSession,
            );

            if let (Some(engine), Some(rx)) = (audio_engine, hotkey_rx) {
                let mut current_session: Option<ActiveSession> = None;

                let (event_async_tx, mut event_async_rx) =
                    tokio::sync::mpsc::unbounded_channel::<HotkeyEvent>();

                // Bridge std_mpsc from hook thread to tokio async channel
                std::thread::Builder::new()
                    .name("voisu-event-bridge".to_string())
                    .spawn(move || {
                        while let Ok(event) = rx.recv() {
                            if event_async_tx.send(event).is_err() {
                                break;
                            }
                        }
                    })?;

                loop {
                    tokio::select! {
                        _ = tokio::signal::ctrl_c() => {
                            println!("\n[SHUTDOWN] Ctrl+C received. Cleaning up...");
                            break;
                        }
                        event = event_async_rx.recv() => {
                            match event {
                                Some(HotkeyEvent::StartRecording) => {
                                    println!("[● RECORDING] Listening... Speak now.");
                                    match engine.start_session() {
                                        Ok((frame_rx, session)) => {
                                            let dg_task = coordinator.start_deepgram_stream(frame_rx);
                                            current_session = Some((dg_task, session));
                                        }
                                        Err(e) => {
                                            eprintln!("[ERROR] Failed to start audio session: {}", e);
                                        }
                                    }
                                }
                                Some(HotkeyEvent::StopRecording) => {
                                    println!("[◼ PROCESSING] Utterance complete. Racing Deepgram & Groq LPUs...");
                                    if let Some((dg_task, session)) = current_session.take() {
                                        match session.stop() {
                                            Ok((_, wav_bytes)) => {
                                                let coord = Arc::clone(&coordinator);
                                                tokio::spawn(async move {
                                                    match coord.resolve_race(dg_task, wav_bytes).await {
                                                        Ok(result) => {
                                                            println!("------------------------------------------------------------");
                                                            if let Some(dg) = result.deepgram {
                                                                println!("  [Deepgram Nova-2] ({}ms): {}", dg.latency_ms, dg.raw_text);
                                                            }
                                                            if let Some(gq) = result.groq {
                                                                println!("  [Groq Whisper]    ({}ms): {}", gq.latency_ms, gq.raw_text);
                                                            }
                                                            println!("------------------------------------------------------------");
                                                        }
                                                        Err(e) => {
                                                            eprintln!("[ERROR] Transcription race failed: {}", e);
                                                        }
                                                    }
                                                });
                                            }
                                            Err(e) => {
                                                eprintln!("[ERROR] Failed to stop recording session: {}", e);
                                            }
                                        }
                                    }
                                }
                                None => break,
                            }
                        }
                    }
                }
            } else {
                tokio::signal::ctrl_c().await?;
            }

            if let Some(mgr) = hotkey_mgr {
                mgr.stop();
            }
            println!("Shut down cleanly.");
        }
    }

    Ok(())
}
