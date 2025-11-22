use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

// Use library modules (the binary should not redeclare modules)
use sonori::config::{read_app_config_with_path, AppConfig};
use sonori::copy;
use sonori::download;
use sonori::portal_input;
use sonori::real_time_transcriber::{RealTimeTranscriber, TranscriptionMode};
use sonori::ui::common::ProcessingState;
use sonori::sound_player::SoundPlayer;
use sonori::system_tray;
use sonori::ui;

// Binary-specific modules (not in library)
mod global_shortcuts;

use ashpd::register_host_app;
use ashpd::AppID;
use clap::{Parser, ValueEnum};
use std::sync::mpsc as std_mpsc;
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone, ValueEnum)]
enum TranscriptionModeArg {
    Realtime,
    Manual,
}

#[derive(Parser)]
#[command(name = "sonori")]
#[command(about = "Real-time speech transcription with Whisper")]
#[command(version)]
struct Args {
    /// Run in CLI mode (no GUI)
    #[arg(
        long,
        help = "Run in CLI mode without GUI, displaying transcription in the terminal"
    )]
    cli: bool,

    /// Transcription mode: realtime or manual
    #[arg(long, value_enum, help = "Set transcription mode")]
    mode: Option<TranscriptionModeArg>,

    /// Start in manual mode (shorthand for --mode manual)
    #[arg(long, help = "Start in manual transcription mode")]
    manual: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    println!("Loading configuration...");
    let (mut app_config, config_path) = read_app_config_with_path();
    match &config_path {
        Some(path) => println!("Configuration loaded from {}", path.display()),
        None => println!("Configuration: using defaults (no config file found)"),
    }

    // Set stable portal App ID env var early for consistent identity across launches
    if std::env::var_os("XDG_DESKTOP_PORTAL_APPLICATION_ID").is_none() {
        std::env::set_var("XDG_DESKTOP_PORTAL_APPLICATION_ID", sonori::config::APPLICATION_ID);
    }

    // Register with the portal system for persistent permissions
    // This is critical for GlobalShortcuts and other portals to recognize the app across launches
    let app_id = AppID::try_from(sonori::config::APPLICATION_ID)
        .expect("Invalid application ID constant");
    if let Err(e) = register_host_app(app_id).await {
        eprintln!("Warning: Failed to register host app with portals: {}", e);
        eprintln!("Portal permissions may not persist across restarts.");
    }

    // Override transcription mode from CLI arguments
    let transcription_mode = if args.manual {
        TranscriptionMode::Manual
    } else if let Some(mode_arg) = args.mode {
        match mode_arg {
            TranscriptionModeArg::Manual => TranscriptionMode::Manual,
            TranscriptionModeArg::Realtime => TranscriptionMode::RealTime,
        }
    } else {
        TranscriptionMode::from(app_config.general_config.transcription_mode.as_str())
    };

    // Update config with CLI override
    app_config.general_config.transcription_mode = match transcription_mode {
        TranscriptionMode::Manual => "manual".to_string(),
        TranscriptionMode::RealTime => "realtime".to_string(),
    };

    println!("Transcription mode: {:?}", transcription_mode);

    println!("Initializing models...");
    let (whisper_model_path, _silero_model_path) = download::init_all_models(
        Some(&app_config.general_config.model),
        app_config.backend_config.backend,
        &app_config.backend_config.quantization_level,
    )
    .await?;

    println!("Whisper model ready at: {:?}", whisper_model_path);

    // Initialize sound player
    let sound_player = match SoundPlayer::new(&app_config.sound_config) {
        Ok(player) => {
            println!("Sound player initialized successfully");
            Some(player)
        }
        Err(e) => {
            eprintln!("Failed to initialize sound player: {}", e);
            None
        }
    };

    let mut transcriber =
        RealTimeTranscriber::new(whisper_model_path, app_config.clone(), sound_player)?;

    transcriber.start()?;

    // Only auto-start recording in real-time mode
    // In manual mode, user explicitly starts/stops sessions
    if matches!(transcription_mode, TranscriptionMode::RealTime) {
        println!("Starting real-time transcription automatically...");
        transcriber.toggle_recording();
    } else {
        println!("Manual mode - ready to start recording on demand");
    }

    if args.cli {
        // CLI mode - no GUI
        run_cli_mode(transcriber, transcription_mode).await?;
    } else {
        // GUI mode - existing behavior
        run_gui_mode(transcriber, app_config).await?;
    }

    Ok(())
}

async fn run_cli_mode(
    transcriber: RealTimeTranscriber,
    mode: TranscriptionMode,
) -> anyhow::Result<()> {
    match mode {
        TranscriptionMode::RealTime => run_realtime_cli(transcriber).await,
        TranscriptionMode::Manual => run_manual_cli(transcriber).await,
    }
}

async fn run_realtime_cli(mut transcriber: RealTimeTranscriber) -> anyhow::Result<()> {
    println!("Running in real-time CLI mode. Press Ctrl+C to exit.");
    println!("Transcription will appear below:");
    println!("=====================================");

    let mut transcript_rx = transcriber.get_transcript_rx();
    let running = transcriber.get_running();

    // Set up Ctrl+C handler
    let running_clone = running.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to listen for Ctrl+C");
        println!("\nShutting down...");
        running_clone.store(false, Ordering::Relaxed);
    });

    // Listen for transcriptions and print them
    let mut current_line = String::new();

    loop {
        tokio::select! {
            Ok(message) = transcript_rx.recv() => {
                // Clear the current line and print the new transcription
                print!("\r{:100}\r", ""); // Clear line with spaces
                current_line.push(' ');
                current_line.push_str(&message.text);
                print!("{}", current_line);
                std::io::Write::flush(&mut std::io::stdout()).unwrap();
            }
            _ = tokio::time::sleep(tokio::time::Duration::from_millis(100)) => {
                if !running.load(Ordering::Relaxed) {
                    break;
                }
            }
        }
    }

    transcriber.shutdown().await?;
    Ok(())
}

async fn run_manual_cli(mut transcriber: RealTimeTranscriber) -> anyhow::Result<()> {
    println!("Running in manual CLI mode. Controls:");
    println!("  SPACE - Start/Stop recording session");
    println!("  c     - Copy current transcript");
    println!("  r     - Reset transcript");
    println!("  q     - Quit");
    println!("====================================");

    let mut transcript_rx = transcriber.get_transcript_rx();
    let running = transcriber.get_running();
    // Get transcription mode to determine configuration behavior
    let transcription_mode = transcriber.get_transcription_mode();

    // Set up Ctrl+C handler
    let running_clone = running.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to listen for Ctrl+C");
        println!("\nShutting down...");
        running_clone.store(false, Ordering::Relaxed);
    });

    // Set up keyboard input handling with blocking thread
    let (input_tx, mut input_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let running_for_input = running.clone();

    // Spawn blocking task for stdin reading
    std::thread::spawn(move || {
        use std::io::{self, BufRead};
        let stdin = io::stdin();

        loop {
            if !running_for_input.load(Ordering::Relaxed) {
                break;
            }

            let mut line = String::new();
            match stdin.lock().read_line(&mut line) {
                Ok(_) => {
                    let _ = input_tx.send(line.trim().to_lowercase());
                }
                Err(_) => break,
            }
        }
    });

    // Status display
    let mut current_transcript = String::new();
    let mut session_status = "Ready";

    println!(
        "\nStatus: {} | Transcript: {}",
        session_status, current_transcript
    );

    // Main event loop
    loop {
        tokio::select! {
            Ok(message) = transcript_rx.recv() => {
                current_transcript.push(' ');
                current_transcript.push_str(&message.text);

                // Clear previous line and print updated status
                print!("\r{:100}\r", ""); // Clear line
                print!("Status: {} | Transcript: {}", session_status, current_transcript);
                std::io::Write::flush(&mut std::io::stdout()).unwrap();
            }
            Some(input) = input_rx.recv() => {
                match input.as_str() {
                    " " | "space" => {
                        println!("\nSpace pressed - toggling session...");
                        // Toggle manual session based on current state
                        let is_currently_recording = transcriber.get_recording().load(std::sync::atomic::Ordering::Relaxed);

                        if is_currently_recording {
                            // Currently recording, stop the session
                            match transcriber.stop_manual_session().await {
                                Ok(()) => {
                                    session_status = "Processing";
                                    println!("Manual session stopped and processing...");
                                }
                                Err(e) => {
                                    eprintln!("Failed to stop manual session: {}", e);
                                }
                            }
                        } else {
                            // Not recording, start a new session
                            match transcriber.start_manual_session().await {
                                Ok(session_id) => {
                                    session_status = "Recording";
                                    println!("Started new manual session: {}", session_id);
                                }
                                Err(e) => {
                                    eprintln!("Failed to start manual session: {}", e);
                                }
                            }
                        }
                    }
                    "c" => {
                        println!("\nCopy transcript requested");
                        let transcript = transcriber.get_transcript();
                        if !transcript.is_empty() {
                            match std::process::Command::new("wl-copy")
                                .arg(&transcript)
                                .spawn()
                                .and_then(|mut child| child.wait())
                            {
                                Ok(exit_status) if exit_status.success() => {
                                    println!("Transcript copied to clipboard successfully");
                                }
                                Ok(_) => {
                                    eprintln!("Failed to copy transcript: wl-copy exited with error");
                                }
                                Err(e) => {
                                    eprintln!("Failed to copy transcript: {}", e);
                                }
                            }
                        } else {
                            println!("No transcript to copy (transcript is empty)");
                        }
                    }
                    "r" => {
                        println!("\nReset transcript requested");
                        // Clear the transcript history
                        let transcript_history = transcriber.get_transcript_history();
                        let mut history = transcript_history.write();
                        history.clear();
                        drop(history);

                        // Clear the local current_transcript display
                        current_transcript.clear();

                        // Clear audio visualization data transcript
                        let audio_data = transcriber.get_audio_visualization_data();
                        let mut audio_data_lock = audio_data.write();
                        audio_data_lock.transcript.clear();
                        audio_data_lock.reset_requested = true;
                        drop(audio_data_lock);

                        println!("Transcript reset successfully");
                    }
                    "q" | "quit" => {
                        println!("\nQuit requested");
                        running.store(false, Ordering::Relaxed);
                        break;
                    }
                    _ => {
                        if !input.is_empty() {
                            println!("\nUnknown command: '{}'. Use SPACE, c, r, or q.", input);
                        }
                    }
                }
                // Reprint status after command
                print!("Status: {} | Transcript: {}", session_status, current_transcript);
                std::io::Write::flush(&mut std::io::stdout()).unwrap();
            }
            _ = tokio::time::sleep(tokio::time::Duration::from_millis(500)) => {
                if !running.load(Ordering::Relaxed) {
                    break;
                }

                // Update session status based on manual session state
                if let Some(manual_status) = transcriber.get_manual_session_status() {
                    session_status = if manual_status.is_recording {
                        "Recording"
                    } else if manual_status.is_processing {
                        "Processing"
                    } else {
                        "Session Active"
                    };
                } else {
                    session_status = "Ready";
                }
            }
        }
    }

    transcriber.shutdown().await?;
    Ok(())
}

async fn run_gui_mode(
    transcriber: RealTimeTranscriber,
    app_config: AppConfig,
) -> anyhow::Result<()> {
    // Set up shutdown channels and monitoring task
    let (_shutdown_tx, shutdown_rx) = tokio::sync::mpsc::channel::<()>(2);
    let transcript_history = transcriber.get_transcript_history();
    let mut transcript_rx = transcriber.get_transcript_rx();
    let audio_visualization_data = transcriber.get_audio_visualization_data();
    let audio_visualization_data_for_thread = audio_visualization_data.clone();
    let running_for_shutdown = transcriber.get_running().clone();

    // Single unified shutdown task that handles all shutdown paths
    tokio::spawn(async move {
        let mut shutdown_rx = shutdown_rx;

        let mut check_interval = tokio::time::interval(tokio::time::Duration::from_millis(100));

        loop {
            tokio::select! {
                Some(_) = shutdown_rx.recv() => {
                    println!("Shutdown signal received, starting graceful shutdown...");
                    break;
                }

                _ = check_interval.tick() => {
                    let is_running = running_for_shutdown.load(Ordering::Relaxed);

                    if !is_running {
                        println!("Running flag is now false, starting graceful shutdown...");
                        break;
                    }
                }
            }
        }

        // Just exit the process - the main thread will handle transcriber shutdown
        println!("Shutdown signal processed, exiting process");
        std::process::exit(0);
    });

    // Clipboard worker: forward transcript chunks to clipboard
    let (clipboard_tx, clipboard_rx) = std_mpsc::channel::<String>();
    // Portal worker: separate channel to avoid moving the same receiver twice
    let (portal_tx, portal_rx) = std_mpsc::channel::<String>();

    // In portal mode we will handle clipboard inside the portal worker to ensure paste uses
    // the correct, freshly-copied contents. Otherwise, run a dedicated clipboard worker.

    let clipboard_tx_clone = clipboard_tx.clone();
    let portal_tx_clone = portal_tx.clone();
    let audio_processor_for_session = transcriber.get_audio_processor();

    tokio::spawn(async move {
        while let Ok(message) = transcript_rx.recv().await {
            // Get current session ID to filter stale transcriptions
            let current_session_id = if let Some(ref ap) = audio_processor_for_session {
                ap.get_session_id()
            } else {
                None
            };

            // Discard transcriptions from old sessions
            if message.session_id != current_session_id {
                println!(
                    "Discarding stale transcription from session {:?} (current: {:?})",
                    message.session_id, current_session_id
                );
                continue;
            }

            let transcription = message.text;

            // Check if this is the first segment before updating history
            let history_len_before = transcript_history.read().len();

            let updated_transcript = {
                let mut history = transcript_history.write();
                if !history.is_empty() {
                    history.push(' ');
                }
                history.push_str(&transcription);
                history.clone()
            };
            let mut audio_data = audio_visualization_data_for_thread.write();
            audio_data.transcript = updated_transcript;

            // Forward chunk to clipboard and portal workers with leading space (except for first segment)
            let segment_with_space = if history_len_before > 0 {
                format!(" {}", transcription)
            } else {
                transcription.clone()
            };
            let _ = clipboard_tx_clone.send(segment_with_space.clone());
            let _ = portal_tx_clone.send(segment_with_space);
        }
    });

    // Portal paste worker: establish a portal session and paste on demand (only if enabled)
    if app_config.portal_config.enable_xdg_portal {
        let paste_shortcut = app_config.portal_config.paste_shortcut.clone();
        tokio::spawn(async move {
            // Attempt to start screencast + remote desktop session
            let portal = portal_input::PortalInput::new().await;
            let portal = match portal {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("Portal integration disabled: {}", e);
                    return;
                }
            };

            // Drain the channel and paste using configured shortcut
            loop {
                match portal_rx.recv_timeout(Duration::from_millis(500)) {
                    Ok(text) => {
                        // Copy to clipboard (using our simplified wayland connection)
                        let _ = copy::WlCopy::copy_to_clipboard(&text);

                        // Wait a bit for clipboard to be ready
                        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

                        // Use configured paste shortcut
                        let result = if paste_shortcut == "ctrl_v" {
                            portal.paste_via_ctrl_v().await
                        } else {
                            // Default to ctrl_shift_v (works in terminals, mostly harmless elsewhere)
                            portal.paste_via_ctrl_shift_v().await
                        };

                        if let Err(e) = result {
                            eprintln!("Portal paste failed: {}", e);
                        }
                    }
                    Err(std_mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std_mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        });
    } else {
        thread::spawn(move || loop {
            match clipboard_rx.recv_timeout(Duration::from_millis(500)) {
                Ok(text) => {
                    let _ = copy::WlCopy::copy_to_clipboard(&text);
                }
                Err(std_mpsc::RecvTimeoutError::Timeout) => {}
                Err(std_mpsc::RecvTimeoutError::Disconnected) => break,
            }
        });
    }

    let running = transcriber.get_running();
    let recording = transcriber.get_recording();
    let manual_session_sender = transcriber.get_manual_session_sender();
    let transcription_mode_ref = transcriber.get_transcription_mode_ref();

    // System tray: start if enabled in configuration
    let (tray_update_tx, tray_command_rx) = if app_config.window_behavior_config.show_in_system_tray
    {
        match system_tray::run_system_tray(
            recording.clone(),
            transcription_mode_ref.clone(),
            running.clone(),
        )
        .await
        {
            Ok((update_tx, command_rx)) => {
                println!("System tray initialized successfully");
                (Some(update_tx), Some(command_rx))
            }
            Err(e) => {
                eprintln!("Failed to initialize system tray: {}", e);
                (None, None)
            }
        }
    } else {
        (None, None)
    };

    // Global shortcuts: register Super+Tab (or configured) to toggle manual session
    if app_config.portal_config.enable_global_shortcuts {
        let accelerator = app_config.portal_config.manual_toggle_accelerator.clone();
        let manual_tx = manual_session_sender.clone();
        let mode_ref = transcription_mode_ref.clone();
        let recording_ref = recording.clone();
        // Pass "running" flag (TRUE=app running, FALSE=shutting down)
        // GlobalShortcutsManager will check !flag to detect shutdown
        let shutdown = running.clone();
        tokio::spawn(async move {
            if let Err(e) = crate::global_shortcuts::run_listener(
                &accelerator,
                manual_tx,
                mode_ref,
                recording_ref,
                shutdown,
            )
            .await
            {
                eprintln!("Global shortcuts failed: {}", e);
            }
        });
    }

    // Run the UI with AtomicBool values directly and pass the configuration
    ui::run_with_audio_data(
        audio_visualization_data,
        running,
        recording,
        app_config,
        Some(manual_session_sender),
        transcription_mode_ref,
        tray_update_tx,
        tray_command_rx,
    );

    Ok(())
}
