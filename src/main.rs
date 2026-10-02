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
use tracing::warn;
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
    Run {
        /// Start minimized to system tray (hide console window)
        #[arg(short, long)]
        tray: bool,
    },
    /// Manage automatic startup on Windows login
    Autostart {
        #[command(subcommand)]
        action: Option<AutostartAction>,
    },
    /// Run the interactive configuration setup wizard
    Setup,
    /// Run system diagnostics (audio input, network latency, Win32 hooks)
    Doctor,
}

#[derive(Subcommand, Debug)]
pub enum AutostartAction {
    /// Enable starting Voisu automatically on Windows login
    Enable,
    /// Disable starting Voisu automatically on Windows login
    Disable,
    /// Check current autostart status
    Status,
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

    match cli.command.unwrap_or(Commands::Run { tray: false }) {
        Commands::Autostart { action } => {
            match action.unwrap_or(AutostartAction::Status) {
                AutostartAction::Enable => {
                    match voisu_win::platform::autostart::enable_autostart() {
                        Ok(path) => {
                            println!("[AUTOSTART] Successfully enabled! Voisu will start on Windows login.");
                            println!("Executable  : {}", path.display());
                            println!("Mode        : Silent system tray attachment (--tray)");
                        }
                        Err(e) => eprintln!("[AUTOSTART ERROR] Failed to enable: {}", e),
                    }
                }
                AutostartAction::Disable => {
                    match voisu_win::platform::autostart::disable_autostart() {
                        Ok(()) => println!("[AUTOSTART] Successfully disabled from Windows login."),
                        Err(e) => eprintln!("[AUTOSTART ERROR] Failed to disable: {}", e),
                    }
                }
                AutostartAction::Status => {
                    let enabled = voisu_win::platform::autostart::is_autostart_enabled();
                    if enabled {
                        println!("[AUTOSTART] Status: ENABLED (Starts automatically with Windows in tray mode)");
                    } else {
                        println!("[AUTOSTART] Status: DISABLED");
                    }
                }
            }
            return Ok(());
        }
        Commands::Setup => {
            AppConfig::run_interactive_setup()?;
        }
        Commands::Doctor => {
            SystemDoctor::run_diagnostics(&config).await;
        }
        Commands::Run { tray: start_in_tray } => {
            if start_in_tray {
                unsafe {
                    let console_hwnd = windows_sys::Win32::System::Console::GetConsoleWindow();
                    if console_hwnd != 0 as _ {
                        windows_sys::Win32::UI::WindowsAndMessaging::ShowWindow(
                            console_hwnd,
                            windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE,
                        );
                    }
                }
            }
            println!("============================================================");
            println!("       Voisu for Windows — Speech Dictation Daemon          ");
            println!("============================================================");
            println!("Version       : {}", env!("CARGO_PKG_VERSION"));
            println!("Trigger Key   : {:?}", config.trigger_key);
            println!("Interaction   : {:?}", config.interaction_mode);
            println!("Delivery Mode : {:?}", config.delivery_mode);
            println!("Language      : {} (Pinned)", config.language);

            let audio_engine = match AudioCaptureEngine::with_config(&config) {
                Ok(engine) => {
                    let ext_badge = if engine.is_external() {
                        " [EXTERNAL EARPHONES/MIC]"
                    } else {
                        " [Smart Auto-Detect Active]"
                    };
                    println!(
                        "Audio Input   : OK ('{}'{}, Native: {} Hz, {} ch)",
                        engine.device_name(),
                        ext_badge,
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
                                    TrayEvent::ToggleAutostart => {
                                        match voisu_win::platform::autostart::toggle_autostart() {
                                            Ok(true) => {
                                                println!("\n[TRAY] Autostart ENABLED via system tray.");
                                                tray.set_tooltip(&format!(
                                                    "Voisu Dictation (Active: {:?}) [Autostart: ON]",
                                                    config.trigger_key
                                                ));
                                            }
                                            Ok(false) => {
                                                println!("\n[TRAY] Autostart DISABLED via system tray.");
                                                tray.set_tooltip(&format!(
                                                    "Voisu Dictation (Active: {:?})",
                                                    config.trigger_key
                                                ));
                                            }
                                            Err(e) => {
                                                eprintln!("\n[TRAY] Failed to toggle autostart: {}", e);
                                            }
                                        }
                                    }
                                    TrayEvent::ToggleConsole => {
                                        unsafe {
                                            let console_hwnd = windows_sys::Win32::System::Console::GetConsoleWindow();
                                            if console_hwnd != 0 as _ {
                                                let is_visible = windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible(console_hwnd) != 0;
                                                windows_sys::Win32::UI::WindowsAndMessaging::ShowWindow(
                                                    console_hwnd,
                                                    if is_visible {
                                                        windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE
                                                    } else {
                                                        windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOW
                                                    },
                                                );
                                            }
                                        }
                                    }
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
                                    TrayEvent::About => {
                                        println!("\n[TRAY] About Voisu for Windows");
                                        println!("Version       : {}", env!("CARGO_PKG_VERSION"));
                                        println!("Author        : TheCodeLord-INX");
                                        println!("Repository    : https://github.com/TheCodeLord-INX/voisu-win");
                                        println!("Architecture  : Dual-Engine Parallel LPU Race (Deepgram Nova-2 + Groq Whisper)");
                                        unsafe {
                                            use std::ffi::OsStr;
                                            use std::os::windows::ffi::OsStrExt;
                                            let title: Vec<u16> = OsStr::new("About Voisu for Windows")
                                                .encode_wide()
                                                .chain(std::iter::once(0))
                                                .collect();
                                            let msg: Vec<u16> = OsStr::new(
                                                "Voisu for Windows v0.1.0\n\nFast, dual-engine speech dictation client.\nParallel LPU Race: Deepgram Nova-2 & Groq Whisper Large v3.\n\nAuthor: TheCodeLord-INX\nGitHub: https://github.com/TheCodeLord-INX/voisu-win"
                                            )
                                            .encode_wide()
                                            .chain(std::iter::once(0))
                                            .collect();
                                            windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
                                                0 as _,
                                                msg.as_ptr(),
                                                title.as_ptr(),
                                                windows_sys::Win32::UI::WindowsAndMessaging::MB_OK | windows_sys::Win32::UI::WindowsAndMessaging::MB_ICONINFORMATION,
                                            );
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
                                    overlay.set_recording(0.0);
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

                                                            // Check for Groq LPU Semantic Reconciliation on provider disagreements
                                                            let reconciled_text = match (&result.deepgram, &result.groq) {
                                                                (Some(dg), Some(gq)) => {
                                                                    let dg_text = dg.raw_text.trim();
                                                                    let gq_text = gq.raw_text.trim();

                                                                    if dg_text.is_empty() || gq_text.is_empty() {
                                                                        None
                                                                    } else if dg_text.eq_ignore_ascii_case(gq_text) {
                                                                        // Identical agreement: deliver immediately (0ms overhead)
                                                                        None
                                                                    } else if let Some(reconciler) = coord.reconciler() {
                                                                        // Semantic reconciliation for Hinglish, Indian names, and acronyms
                                                                        match reconciler.reconcile(dg_text, gq_text).await {
                                                                            Ok(res) => {
                                                                                println!("  [Groq LPU Reconciled] ({}ms): {}", res.latency_ms, res.text);
                                                                                Some(res.text)
                                                                            }
                                                                            Err(e) => {
                                                                                warn!("Reconciliation skipped: {}. Falling back to Slice B4 arbitration.", e);
                                                                                None
                                                                            }
                                                                        }
                                                                    } else {
                                                                        None
                                                                    }
                                                                }
                                                                _ => None,
                                                            };

                                                            let candidate_text = if let Some(rec) = reconciled_text {
                                                                rec
                                                            } else if let Some(arb) = ArbitrationEngine::arbitrate(result.deepgram, result.groq) {
                                                                if !arb.flipped_regions.is_empty() {
                                                                    println!("  [Arbitration] Applied {} Slice B4 substitutions (Mode: {})", arb.flipped_regions.len(), arb.arbitration_mode);
                                                                    for flip in &arb.flipped_regions {
                                                                        let orig_words: Vec<&str> = flip.original_tokens.iter().map(|t| t.word.as_str()).collect();
                                                                        let repl_words: Vec<&str> = flip.replacement_tokens.iter().map(|t| t.word.as_str()).collect();
                                                                        println!("    - Replaced '{}' -> '{}' ({})", orig_words.join(" "), repl_words.join(" "), flip.arbitration_reason);
                                                                    }
                                                                }
                                                                arb.selected_text
                                                            } else {
                                                                String::new()
                                                            };

                                                            // Check for voice editing command ("scratch that" / "undo that")
                                                            if FormattingEngine::is_scratch_command(&candidate_text) {
                                                                println!("  [COMMAND] Spoken 'scratch that' detected. Reverting last dictation...");
                                                                let _ = inj.synthesize_ctrl_z();
                                                                println!("------------------------------------------------------------");
                                                                overlay_done.set_done();
                                                                tokio::time::sleep(Duration::from_millis(800)).await;
                                                                overlay_done.hide();
                                                                return;
                                                            }

                                                            // Deterministic Spoken Punctuation & Formatting
                                                            let formatted = FormattingEngine::format(&candidate_text);
                                                            if formatted.trim().is_empty() {
                                                                println!("  [Final Text] (No speech detected)");
                                                                println!("------------------------------------------------------------");
                                                                overlay_done.hide();
                                                                return;
                                                            }
                                                            println!("  [Final Text] >>> \"{}\"", formatted);
                                                            println!("------------------------------------------------------------");

                                                            // Smart Delivery: Injects if focused on text box, or copies to clipboard if not
                                                            match inj.deliver(&formatted, delivery_mode) {
                                                                Ok(voisu_win::delivery::DeliveryOutcome::Injected) => {
                                                                    println!("[DELIVERED] Injected into focused text box via {:?}.", delivery_mode);
                                                                }
                                                                Ok(voisu_win::delivery::DeliveryOutcome::CopiedToClipboard) => {
                                                                    println!("[COPIED] Cursor not focused on text box. Transcribed text copied to clipboard.");
                                                                }
                                                                Err(e) => {
                                                                    eprintln!("[ERROR] Text delivery failed: {}", e);
                                                                }
                                                            }

                                                            // Visual Pill Feedback: Show Done for 800ms then hide
                                                            overlay_done.set_done();
                                                            tokio::time::sleep(Duration::from_millis(800)).await;
                                                            overlay_done.hide();
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
