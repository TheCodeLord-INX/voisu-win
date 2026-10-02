//! Voisu for Windows (`voisu-win`) CLI Entry Point.
//!
//! Subcommands:
//! - `run`: Start the speech-to-text dictation daemon.
//! - `setup`: Run the interactive configuration setup wizard.
//! - `doctor`: Run hardware, network, and subsystem diagnostics.

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use voisu_win::config::AppConfig;
use voisu_win::core::arbitration::ArbitrationEngine;
use voisu_win::core::audio::{AudioCaptureEngine, RecordingSession};
use voisu_win::core::formatting::FormattingEngine;
use voisu_win::core::hotkey::{HotkeyEvent, HotkeyManager};
use voisu_win::core::types::SourceTranscript;
use voisu_win::delivery::ClipboardInjector;
use voisu_win::doctor::SystemDoctor;
use voisu_win::providers::DualProviderCoordinator;
use voisu_win::providers::deepgram::DeepgramError;
use voisu_win::ui::{OverlayController, TrayEvent, TrayManager};

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
            let injector = Arc::new(ClipboardInjector::new(config.clipboard_restore_timeout_ms));
            let delivery_mode = config.delivery_mode;

            // Initialize UI Overlay and System Tray
            let overlay = Arc::new(OverlayController::new());
            let tray = Arc::new(TrayManager::new());
            tray.set_tooltip(&format!(
                "Voisu Dictation (Active: {:?})",
                config.trigger_key
            ));

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

            if let (Some(engine), Some(rx)) = (audio_engine, hotkey_rx) {
                type ActiveSession = (
                    Option<JoinHandle<Result<SourceTranscript, DeepgramError>>>,
                    RecordingSession,
                );

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

                let mut tray_interval = tokio::time::interval(Duration::from_millis(100));

                loop {
                    tokio::select! {
                        _ = tokio::signal::ctrl_c() => {
                            println!("\n[SHUTDOWN] Ctrl+C received. Cleaning up...");
                            break;
                        }
                        _ = tray_interval.tick() => {
                            if let Some(tray_event) = tray.try_recv_event() {
                                match tray_event {
                                    TrayEvent::RunDoctor => {
                                        println!("\n[TRAY] Running diagnostics on request...");
                                        SystemDoctor::run_diagnostics(&config).await;
                                    }
                                    TrayEvent::OpenConfig => {
                                        println!("[TRAY] Opening configuration folder...");
                                        if let Some(config_dir) = dirs::config_dir() {
                                            let app_dir = config_dir.join("voisu");
                                            let _ = std::fs::create_dir_all(&app_dir);
                                            let _ = std::process::Command::new("explorer.exe")
                                                .arg(&app_dir)
                                                .spawn();
                                        }
                                    }
                                    TrayEvent::Exit => {
                                        println!("\n[TRAY] Exit requested from system tray menu.");
                                        break;
                                    }
                                }
                            }
                        }
                        event = event_async_rx.recv() => {
                            match event {
                                Some(HotkeyEvent::StartRecording) => {
                                    println!("[● RECORDING] Listening... Speak now.");
                                    overlay.set_recording(0.2);
                                    match engine.start_session() {
                                        Ok((mut frame_rx, session)) => {
                                            // Tap audio frames for live RMS UI feedback and feed to Deepgram
                                            let (dg_tx, dg_rx) = tokio::sync::mpsc::channel(64);
                                            let overlay_feed = Arc::clone(&overlay);
                                            tokio::spawn(async move {
                                                while let Some(frame) = frame_rx.recv().await {
                                                    overlay_feed.set_recording(frame.rms_level);
                                                    if dg_tx.send(frame).await.is_err() {
                                                        break;
                                                    }
                                                }
                                            });

                                            let dg_task = coordinator.start_deepgram_stream(dg_rx);
                                            current_session = Some((dg_task, session));
                                        }
                                        Err(e) => {
                                            eprintln!("[ERROR] Failed to start audio session: {}", e);
                                            overlay.hide();
                                        }
                                    }
                                }
                                Some(HotkeyEvent::StopRecording) => {
                                    println!("[◼ PROCESSING] Utterance complete. Racing Deepgram & Groq LPUs...");
                                    overlay.set_processing();
                                    if let Some((dg_task, session)) = current_session.take() {
                                        match session.stop() {
                                            Ok((_, wav_bytes)) => {
                                                let coord = Arc::clone(&coordinator);
                                                let inj = Arc::clone(&injector);
                                                let overlay_done = Arc::clone(&overlay);
                                                tokio::spawn(async move {
                                                    match coord.resolve_race(dg_task, wav_bytes).await {
                                                        Ok(result) => {
                                                            println!("------------------------------------------------------------");
                                                            if let Some(dg) = &result.deepgram {
                                                                println!("  [Deepgram Nova-2] ({}ms): {}", dg.latency_ms, dg.raw_text);
                                                            }
                                                            if let Some(gq) = &result.groq {
                                                                println!("  [Groq Whisper]    ({}ms): {}", gq.latency_ms, gq.raw_text);
                                                            }

                                                            // Asymmetric Slice B4 Arbitration
                                                            if let Some(arb) = ArbitrationEngine::arbitrate(result.deepgram, result.groq) {
                                                                if !arb.flipped_regions.is_empty() {
                                                                    println!("  [Arbitration] Applied {} Slice B4 substitutions (Mode: {})", arb.flipped_regions.len(), arb.arbitration_mode);
                                                                    for flip in &arb.flipped_regions {
                                                                        let orig_words: Vec<&str> = flip.original_tokens.iter().map(|t| t.word.as_str()).collect();
                                                                        let repl_words: Vec<&str> = flip.replacement_tokens.iter().map(|t| t.word.as_str()).collect();
                                                                        println!("    - Replaced '{}' -> '{}' ({})", orig_words.join(" "), repl_words.join(" "), flip.arbitration_reason);
                                                                    }
                                                                }

                                                                // Deterministic Spoken Punctuation & Formatting
                                                                let formatted = FormattingEngine::format(&arb.selected_text);
                                                                if formatted.trim().is_empty() {
                                                                    println!("  [Final Text] (No speech detected)");
                                                                    println!("------------------------------------------------------------");
                                                                    overlay_done.hide();
                                                                    return;
                                                                }
                                                                println!("  [Final Text] >>> \"{}\"", formatted);
                                                                println!("------------------------------------------------------------");

                                                                // Smart Clipboard Delivery into focused window
                                                                if let Err(e) = inj.deliver(&formatted, delivery_mode) {
                                                                    eprintln!("[ERROR] Text delivery failed: {}", e);
                                                                } else {
                                                                    println!("[DELIVERED] Injected into focused window via {:?}.", delivery_mode);
                                                                }

                                                                // Visual Pill Feedback: Show Done for 800ms then hide
                                                                overlay_done.set_done();
                                                                tokio::time::sleep(Duration::from_millis(800)).await;
                                                                overlay_done.hide();
                                                            } else {
                                                                overlay_done.hide();
                                                            }
                                                        }
                                                        Err(e) => {
                                                            eprintln!("[ERROR] Transcription race failed: {}", e);
                                                            overlay_done.hide();
                                                        }
                                                    }
                                                });
                                            }
                                            Err(e) => {
                                                eprintln!("[ERROR] Failed to stop recording session: {}", e);
                                                overlay.hide();
                                            }
                                        }
                                    } else {
                                        overlay.hide();
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

            // Graceful shutdown cleanup
            overlay.close();
            tray.close();
            if let Some(mgr) = hotkey_mgr {
                mgr.stop();
            }
            println!("Shut down cleanly.");
        }
    }

    Ok(())
}
