use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

// Use library modules (the binary should not redeclare modules)
use sonori::config::{read_app_config_with_path, AppConfig, OutputMode};
use sonori::copy;
use sonori::hotkey::{HotkeyState, SharedHotkeyState};
use sonori::ipc::{self, AppCommand, IpcCommand};
use sonori::portal_input;
use sonori::sound_player::SoundPlayer;
use sonori::system_tray;
use sonori::ui;
use speechcore::{FeedbackSink, RealTimeTranscriber, SpeechConfig, TranscriptionMode};

// Binary-specific modules (not in library)
mod global_shortcuts;

use ashpd::register_host_app;
use ashpd::AppID;
use clap::{Parser, Subcommand, ValueEnum};
use std::time::Duration;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Clone, ValueEnum)]
enum TranscriptionModeArg {
    Realtime,
    Manual,
}

/// IPC subcommands for controlling a running Sonori instance
#[derive(Subcommand, Debug)]
enum Command {
    /// Toggle recording (start if stopped, stop if recording)
    Toggle,
    /// Start a recording session
    Start,
    /// Stop the current recording session
    Stop,
    /// Cancel the current session without processing
    Cancel,
    /// Get current status as JSON
    Status,
    /// Switch transcription mode
    SwitchMode {
        /// Mode to switch to: "manual" or "realtime"
        mode: String,
    },
    /// Copy the last transcript to the clipboard
    CopyLast,
    /// Paste the last transcript again
    PasteLast,
    /// Turn Magic Mode on or off
    Magic,
    /// Set the transcription language, e.g. "en" or "de"
    Language {
        /// Language code
        code: String,
    },
}

#[derive(Parser)]
#[command(name = "sonori")]
#[command(about = "Real-time speech transcription")]
#[command(version)]
struct Args {
    /// Subcommand to control a running Sonori instance
    #[command(subcommand)]
    command: Option<Command>,

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
    init_tracing();

    let args = Args::parse();

    // Handle IPC subcommands (control running instance)
    if let Some(cmd) = args.command {
        return handle_ipc_command(cmd).await;
    }

    if !args.cli {
        ipc::replace_running_instance().await;
    }

    println!("Loading configuration...");
    let (mut app_config, config_path) = read_app_config_with_path();
    match &config_path {
        Some(path) => println!("Configuration loaded from {}", path.display()),
        None => println!("Configuration: using defaults (no config file found)"),
    }

    // Set stable portal App ID env var early for consistent identity across launches
    if std::env::var_os("XDG_DESKTOP_PORTAL_APPLICATION_ID").is_none() {
        std::env::set_var(
            "XDG_DESKTOP_PORTAL_APPLICATION_ID",
            sonori::config::APPLICATION_ID,
        );
    }

    // Register with the portal system for persistent permissions
    // This is critical for GlobalShortcuts and other portals to recognize the app across launches
    let app_id =
        AppID::try_from(sonori::config::APPLICATION_ID).expect("Invalid application ID constant");
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
    speechcore::download::init_silero_model().await?;
    // The GUI resolves the model in the background, so the overlay shows the
    // download progress and any error. The CLI has no overlay and waits here.
    let transcription_model_path = if args.cli {
        let path = speechcore::resolve_model_path(
            &app_config.general_config.model,
            app_config.backend_config.backend,
            &app_config.backend_config.quantization_level,
        )
        .await?;
        println!("Transcription model ready at: {:?}", path);
        Some(path)
    } else {
        None
    };

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

    let feedback_sink = sound_player.map(|player| player as std::sync::Arc<dyn FeedbackSink>);
    let magic_mode_enabled = Arc::new(AtomicBool::new(
        app_config.enhancement_config.enabled && app_config.enhancement_config.active,
    ));
    let magic_mode_enhancer = if app_config.enhancement_config.enabled {
        Some(Arc::new(sonori::enhancement::MagicModeEnhancer::new(
            app_config.enhancement_config.clone(),
            magic_mode_enabled.clone(),
        )))
    } else {
        None
    };

    let speech_config: SpeechConfig = app_config.clone().into();
    let mut transcriber =
        RealTimeTranscriber::new(transcription_model_path, speech_config, feedback_sink)?;
    if let Some(problem) = sonori::config::config_file_problem() {
        transcriber
            .get_backend_status()
            .write()
            .report_error(problem);
    }

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
        run_gui_mode(
            transcriber,
            app_config,
            magic_mode_enabled,
            magic_mode_enhancer,
        )
        .await?;
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
                if !message.is_final {
                    continue; // CLI prints committed text only
                }
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
    let _transcription_mode = transcriber.get_transcription_mode();

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
                if !message.is_final {
                    continue; // CLI prints committed text only
                }
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
                            match copy::WlCopy::copy_to_clipboard(&transcript) {
                                Ok(()) => {
                                    println!("Transcript copied to clipboard successfully");
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

/// Longest transcript the overlay keeps. A long realtime session would otherwise
/// grow it, and the copy of it on every final, without bound.
const MAX_OVERLAY_TRANSCRIPT_BYTES: usize = 20_000;

/// A finished transcript and what to do with it.
struct Output {
    text: String,
    mode: OutputMode,
}

async fn run_gui_mode(
    transcriber: RealTimeTranscriber,
    app_config: AppConfig,
    magic_mode_enabled: Arc<AtomicBool>,
    magic_mode_enhancer: Option<Arc<sonori::enhancement::MagicModeEnhancer>>,
) -> anyhow::Result<()> {
    let transcript_history = transcriber.get_transcript_history();
    let transcript_rx = transcriber.get_transcript_rx();
    let audio_visualization_data = transcriber.get_audio_visualization_data();
    let backend_status = transcriber.get_backend_status();
    let last_transcript = Arc::new(parking_lot::RwLock::new(String::new()));

    // Settings changes reach the pipeline through this channel, so they apply
    // without a restart.
    let (config_tx, config_rx) = tokio::sync::watch::channel(app_config.clone());

    let (final_tx, final_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    // Single bounded queue for clipboard/paste work.
    let (output_tx, output_rx) = tokio::sync::mpsc::channel::<Output>(128);

    tokio::spawn(consume_transcripts(
        transcript_rx,
        final_tx,
        transcript_history.clone(),
        audio_visualization_data.clone(),
    ));
    tokio::spawn(finalize_transcripts(
        final_rx,
        magic_mode_enhancer.clone(),
        transcript_history,
        audio_visualization_data.clone(),
        last_transcript.clone(),
        config_rx.clone(),
        output_tx.clone(),
    ));
    tokio::spawn(output_worker(
        output_rx,
        app_config.portal_config.enable_xdg_portal,
        config_rx.clone(),
        backend_status.clone(),
    ));

    let (app_tx, app_rx) = tokio::sync::mpsc::unbounded_channel::<AppCommand>();
    tokio::spawn(handle_app_commands(
        app_rx,
        last_transcript,
        output_tx,
        magic_mode_enabled.clone(),
        magic_mode_enhancer.is_some(),
        config_tx.clone(),
        backend_status.clone(),
    ));

    // Settings and `sonori language` change the language without a restart.
    {
        let language = transcriber.get_language();
        let mut config_rx = config_rx;
        tokio::spawn(async move {
            while config_rx.changed().await.is_ok() {
                let new_language = config_rx.borrow().general_config.language.clone();
                if *language.read() != new_language {
                    println!("Transcription language set to {}", new_language);
                    *language.write() = new_language;
                }
            }
        });
    }

    let running = transcriber.get_running();
    let recording = transcriber.get_recording();
    let manual_session_sender = transcriber.get_manual_session_sender();

    // SIGTERM and Ctrl+C shut down cleanly: the event loop sees the running flag
    // within one idle poll. A second signal exits at once, in case shutdown hangs.
    {
        let running = running.clone();
        tokio::spawn(async move {
            use tokio::signal::unix::{signal, SignalKind};
            let (Ok(mut term), Ok(mut int)) = (
                signal(SignalKind::terminate()),
                signal(SignalKind::interrupt()),
            ) else {
                eprintln!("Failed to install signal handlers");
                return;
            };
            tokio::select! {
                _ = term.recv() => {}
                _ = int.recv() => {}
            }
            println!("Shutdown signal received");
            running.store(false, Ordering::Relaxed);
            tokio::select! {
                _ = term.recv() => {}
                _ = int.recv() => {}
            }
            std::process::exit(130);
        });
    }
    let transcription_mode_ref = transcriber.get_transcription_mode_ref();
    let backend_command_tx = transcriber.backend_command_sender();

    // System tray: start if enabled in configuration
    let tray_command_rx = if app_config.window_behavior_config.show_in_system_tray {
        match system_tray::run_system_tray(
            recording.clone(),
            transcription_mode_ref.clone(),
            running.clone(),
        )
        .await
        {
            Ok(command_rx) => {
                println!("System tray initialized successfully");
                Some(command_rx)
            }
            Err(e) => {
                eprintln!("Failed to initialize system tray: {}", e);
                None
            }
        }
    } else {
        None
    };

    // Global shortcuts: register Super+\ (or configured) to toggle manual session
    let hotkey_state: SharedHotkeyState = Arc::new(parking_lot::RwLock::new(HotkeyState::Disabled));
    if app_config.portal_config.enable_global_shortcuts {
        let accelerator = app_config.portal_config.manual_toggle_accelerator.clone();
        let shortcut_mode = app_config.portal_config.shortcut_mode;
        let manual_tx = manual_session_sender.clone();
        let app_tx = app_tx.clone();
        let mode_ref = transcription_mode_ref.clone();
        let running_ref = running.clone();
        let hotkey_state = hotkey_state.clone();
        tokio::spawn(async move {
            if let Err(e) = crate::global_shortcuts::run_listener(
                &accelerator,
                shortcut_mode,
                manual_tx,
                app_tx,
                mode_ref,
                running_ref,
                hotkey_state,
            )
            .await
            {
                eprintln!("Global shortcuts failed: {:#}", e);
            }
        });
    }

    // IPC server: enable external control via CLI (for niri/sway keybindings)
    {
        let ipc_server = ipc::IpcServer::new(
            manual_session_sender.clone(),
            app_tx.clone(),
            transcription_mode_ref.clone(),
            recording.clone(),
            running.clone(),
            backend_status.clone(),
            hotkey_state.clone(),
        );
        tokio::spawn(async move {
            if let Err(e) = ipc_server.run().await {
                eprintln!("IPC server error: {}", e);
            }
        });
    }

    // Run the UI with AtomicBool values directly and pass the configuration
    ui::run_with_audio_data(ui::UiHandles {
        audio_data: audio_visualization_data,
        running,
        recording,
        magic_mode_enabled,
        config: app_config,
        manual_session_sender: Some(manual_session_sender),
        transcription_mode_ref,
        tray_command_rx,
        backend_status: Some(backend_status),
        backend_command_tx,
        config_tx,
        app_tx,
        hotkey_state,
    });

    // UI has exited, perform cleanup
    let mut transcriber = transcriber;
    transcriber.shutdown().await?;

    Ok(())
}

/// Shows interim hypotheses and hands finals on. It never waits on slow work,
/// so the broadcast channel cannot lag and drop final transcripts.
async fn consume_transcripts(
    mut transcript_rx: tokio::sync::broadcast::Receiver<speechcore::TranscriptionMessage>,
    final_tx: tokio::sync::mpsc::UnboundedSender<String>,
    transcript_history: Arc<parking_lot::RwLock<String>>,
    audio_visualization_data: Arc<parking_lot::RwLock<speechcore::AudioVisualizationData>>,
) {
    loop {
        let message = match transcript_rx.recv().await {
            Ok(message) => message,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                eprintln!("Transcript consumer lagged; skipped {} message(s)", skipped);
                continue;
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        };

        // Speechcore already drops transcripts of cancelled sessions. A finished
        // session's text still arrives after the next session started, and is kept.

        // Interim streaming hypotheses: show as a live preview only — no
        // history append, enhancement, file save, or clipboard paste. The
        // final message for this utterance commits and supersedes it.
        if !message.is_final {
            let preview = {
                let history = transcript_history.read();
                if history.is_empty() {
                    message.text
                } else {
                    format!("{} {}", history, message.text)
                }
            };
            // Trailing ellipsis marks the live, provisional tail; the final
            // message replaces it with committed text (no marker).
            audio_visualization_data.write().transcript = format!("{preview} …");
            continue;
        }

        if final_tx.send(message.text).is_err() {
            break;
        }
    }
}

/// Runs Magic Mode, then commits each final transcript in arrival order.
async fn finalize_transcripts(
    mut final_rx: tokio::sync::mpsc::UnboundedReceiver<String>,
    magic_mode_enhancer: Option<Arc<sonori::enhancement::MagicModeEnhancer>>,
    transcript_history: Arc<parking_lot::RwLock<String>>,
    audio_visualization_data: Arc<parking_lot::RwLock<speechcore::AudioVisualizationData>>,
    last_transcript: Arc<parking_lot::RwLock<String>>,
    config_rx: tokio::sync::watch::Receiver<AppConfig>,
    output_tx: tokio::sync::mpsc::Sender<Output>,
) {
    // clear_on_new_session empties the history, so it alone cannot tell
    // whether this dictation follows an earlier one in the same text field.
    let mut pasted_before = false;

    while let Some(mut transcription) = final_rx.recv().await {
        if let Some(enhancer) = &magic_mode_enhancer {
            let raw_transcription = transcription.clone();
            let enhancer = Arc::clone(enhancer);
            match tokio::task::spawn_blocking(move || enhancer.enhance(&raw_transcription)).await {
                Ok(Ok(enhanced)) => {
                    if !enhanced.trim().is_empty() {
                        transcription = enhanced;
                    }
                }
                Ok(Err(e)) => eprintln!("Magic Mode enhancement failed: {e}"),
                Err(e) => eprintln!("Magic Mode enhancement worker failed: {e}"),
            }
        }

        // Check if this is the first segment before updating history
        let history_len_before = transcript_history.read().len();

        let updated_transcript = {
            let mut history = transcript_history.write();
            if !history.is_empty() {
                history.push(' ');
            }
            history.push_str(&transcription);
            trim_front(&mut history, MAX_OVERLAY_TRANSCRIPT_BYTES);
            history.clone()
        };
        audio_visualization_data.write().transcript = updated_transcript;
        *last_transcript.write() = transcription.clone();

        let (history_enabled, history_path, output_mode) = {
            let config = config_rx.borrow();
            (
                config.history_config.enabled,
                config.history_config.path.clone(),
                config.portal_config.output_mode,
            )
        };
        if let Err(e) = sonori::transcript_writer::append_to_transcript_history(
            &transcription,
            &history_path,
            history_enabled,
        ) {
            eprintln!("Failed to save transcript history: {}", e);
        }

        // Forward chunk to clipboard and portal workers with leading space (except for first segment)
        let segment_with_space = if history_len_before > 0 || pasted_before {
            format!(" {}", transcription)
        } else {
            transcription
        };
        pasted_before = true;
        let output = Output {
            text: segment_with_space,
            mode: output_mode,
        };
        if let Err(e) = output_tx.try_send(output) {
            match e {
                tokio::sync::mpsc::error::TrySendError::Full(_) => {
                    eprintln!("Paste queue full; dropping transcript paste update");
                }
                tokio::sync::mpsc::error::TrySendError::Closed(_) => break,
            }
        }
    }
}

/// Drops whole words from the front until `text` fits in `max_bytes`.
fn trim_front(text: &mut String, max_bytes: usize) {
    if text.len() <= max_bytes {
        return;
    }
    let mut cut = text.len() - max_bytes;
    while !text.is_char_boundary(cut) {
        cut += 1;
    }
    let cut = text[cut..]
        .find(char::is_whitespace)
        .map_or(cut, |space| cut + space + 1);
    text.drain(..cut);
}

/// Delivers transcripts: clipboard and paste shortcut (through the portal when it
/// is enabled, else or on failure through wtype/dotool), typing, or clipboard only.
async fn output_worker(
    mut output_rx: tokio::sync::mpsc::Receiver<Output>,
    enable_portal: bool,
    config_rx: tokio::sync::watch::Receiver<AppConfig>,
    status: Arc<parking_lot::RwLock<speechcore::BackendStatus>>,
) {
    let portal = if enable_portal {
        match portal_input::PortalInput::new().await {
            Ok(p) => Some(p),
            Err(e) => {
                eprintln!(
                    "Portal integration disabled: {}. Falling back to wtype/dotool.",
                    e
                );
                None
            }
        }
    } else {
        None
    };

    while let Some(Output { text, mode }) = output_rx.recv().await {
        let paste_shortcut = config_rx.borrow().portal_config.paste_shortcut.clone();
        let with_shift = paste_shortcut != "ctrl_v";
        let shortcut_label = if with_shift { "Ctrl+Shift+V" } else { "Ctrl+V" };

        if mode == OutputMode::Type {
            let typed_text = text.clone();
            let typed = tokio::task::spawn_blocking(move || copy::type_text(&typed_text))
                .await
                .unwrap_or_else(|e| Err(e.to_string()));
            let Err(e) = typed else {
                continue;
            };
            eprintln!("Typing failed: {}", e);
            // Fall through to the clipboard, so the text is not lost.
            let copied = copy_to_clipboard(text).await;
            status.write().report_error(match copied {
                Ok(()) => format!("Typing failed; copied — press {shortcut_label}"),
                Err(e) => format!("Typing and clipboard copy failed: {e}"),
            });
            continue;
        }

        if let Err(e) = copy_to_clipboard(text).await {
            eprintln!("Clipboard copy failed: {}", e);
            status
                .write()
                .report_error(format!("Clipboard copy failed: {e}"));
            continue;
        }
        if mode == OutputMode::Clipboard {
            continue;
        }

        // Give clipboard managers a short moment before paste injection.
        tokio::time::sleep(Duration::from_millis(50)).await;

        if let Some(portal) = portal.as_ref() {
            match portal.paste(with_shift).await {
                Ok(()) => continue,
                Err(e) => eprintln!("Portal paste failed: {}; trying wtype/dotool", e),
            }
        }

        let pasted =
            tokio::task::spawn_blocking(move || copy::paste_via_keystroke(&paste_shortcut))
                .await
                .unwrap_or_else(|e| Err(e.to_string()));
        if let Err(e) = pasted {
            eprintln!("Paste failed: {}", e);
            status
                .write()
                .report_error(format!("Copied, paste failed — press {shortcut_label}"));
        }
    }
}

async fn copy_to_clipboard(text: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || copy::WlCopy::copy_to_clipboard(&text))
        .await
        .unwrap_or_else(|e| Err(e.to_string()))
}

async fn handle_app_commands(
    mut app_rx: tokio::sync::mpsc::UnboundedReceiver<AppCommand>,
    last_transcript: Arc<parking_lot::RwLock<String>>,
    output_tx: tokio::sync::mpsc::Sender<Output>,
    magic_mode_enabled: Arc<AtomicBool>,
    magic_mode_available: bool,
    config_tx: tokio::sync::watch::Sender<AppConfig>,
    status: Arc<parking_lot::RwLock<speechcore::BackendStatus>>,
) {
    while let Some(command) = app_rx.recv().await {
        match command {
            AppCommand::CopyLast | AppCommand::PasteLast => {
                let text = last_transcript.read().clone();
                if text.is_empty() {
                    status.write().report_error("No transcript yet");
                    continue;
                }
                let mode = if matches!(command, AppCommand::CopyLast) {
                    OutputMode::Clipboard
                } else {
                    OutputMode::Paste
                };
                if output_tx.try_send(Output { text, mode }).is_err() {
                    status.write().report_error("Paste queue full; try again");
                }
            }
            AppCommand::ToggleMagicMode => {
                if !magic_mode_available {
                    status
                        .write()
                        .report_error("Magic Mode is off: set enhancement_config.enabled");
                    continue;
                }
                let active = !magic_mode_enabled.load(Ordering::Relaxed);
                magic_mode_enabled.store(active, Ordering::Relaxed);
                println!("Magic Mode {}", if active { "on" } else { "off" });
                persist_config(&config_tx, &status, |config| {
                    config.enhancement_config.active = active
                });
            }
            AppCommand::SetLanguage(code) => {
                persist_config(&config_tx, &status, |config| {
                    config.general_config.language = code.clone()
                });
            }
        }
    }
}

/// Applies `change` to the running config and the config file.
fn persist_config(
    config_tx: &tokio::sync::watch::Sender<AppConfig>,
    status: &parking_lot::RwLock<speechcore::BackendStatus>,
    change: impl Fn(&mut AppConfig),
) {
    config_tx.send_modify(&change);
    let (mut config, _) = read_app_config_with_path();
    change(&mut config);
    if let Err(e) = sonori::config::write_app_config(&config) {
        eprintln!("Failed to save config: {}", e);
        status
            .write()
            .report_error(format!("Setting not saved: {e}"));
    }
}

fn init_tracing() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("speechcore=info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

/// Handle IPC subcommands by sending them to the running Sonori instance
async fn handle_ipc_command(cmd: Command) -> anyhow::Result<()> {
    let ipc_cmd = match cmd {
        Command::Toggle => IpcCommand::Toggle,
        Command::Start => IpcCommand::Start,
        Command::Stop => IpcCommand::Stop,
        Command::Cancel => IpcCommand::Cancel,
        Command::Status => IpcCommand::Status,
        Command::SwitchMode { mode } => IpcCommand::SwitchMode { mode },
        Command::CopyLast => IpcCommand::CopyLast,
        Command::PasteLast => IpcCommand::PasteLast,
        Command::Magic => IpcCommand::ToggleMagic,
        Command::Language { code } => IpcCommand::Language { code },
    };

    match ipc::send_command(ipc_cmd).await {
        Ok(response) => {
            if let Some(status) = response.status {
                // Status command: print JSON
                println!("{}", serde_json::to_string_pretty(&status)?);
            } else if let Some(message) = response.message {
                // Other commands: print message
                if response.success {
                    println!("{}", message);
                } else {
                    eprintln!("Error: {}", message);
                    std::process::exit(1);
                }
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}
