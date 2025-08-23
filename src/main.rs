use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::io::Write;

mod audio_capture;
mod audio_processor;
mod config;
mod download;
mod real_time_transcriber;
mod silero_audio_processor;
mod stats_reporter;
mod transcribe;
mod transcription_processor;
mod transcription_stats;
mod ui;
mod copy;
mod portal_input;

use clap::Parser;
use std::sync::mpsc as std_mpsc;
use std::thread;
use std::time::Duration;
use config::read_app_config;
use download::ModelType;
use real_time_transcriber::RealTimeTranscriber;

#[derive(Parser)]
#[command(name = "sonori")]
#[command(about = "Real-time speech transcription with Whisper")]
#[command(version)]
struct Args {
    /// Run in CLI mode (no GUI)
    #[arg(long, help = "Run in CLI mode without GUI, displaying transcription in the terminal")]
    cli: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    
    println!("Loading configuration...");
    let app_config = read_app_config();
    let log_stats_enabled = app_config.log_stats_enabled;

    println!("Initializing models...");
    let (whisper_model_path, _silero_model_path) =
        download::init_all_models(Some(&app_config.model)).await?;

    println!("Whisper model ready at: {:?}", whisper_model_path);

    let mut transcriber = RealTimeTranscriber::new(whisper_model_path, app_config.clone())?;

    transcriber.start()?;

    println!("Starting transcription automatically...");
    transcriber.toggle_recording();

    if args.cli {
        // CLI mode - no GUI
        run_cli_mode(transcriber).await?;
    } else {
        // GUI mode - existing behavior
        run_gui_mode(transcriber, app_config).await?;
    }

    Ok(())
}

async fn run_cli_mode(mut transcriber: RealTimeTranscriber) -> anyhow::Result<()> {
    println!("Running in CLI mode. Press Ctrl+C to exit.");
    println!("Transcription will appear below:");
    println!("=====================================");
    
    let mut transcript_rx = transcriber.get_transcript_rx();
    let running = transcriber.get_running();
    
    // Set up Ctrl+C handler
    let running_clone = running.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
        println!("\nShutting down...");
        running_clone.store(false, Ordering::Relaxed);
    });
    
    // Listen for transcriptions and print them
    let mut current_line = String::new();
    
    loop {
        tokio::select! {
            Ok(transcription) = transcript_rx.recv() => {
                // Clear the current line and print the new transcription
                print!("\r{:100}\r", ""); // Clear line with spaces
                current_line.push(' ');
                current_line.push_str(&transcription);
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

async fn run_gui_mode(transcriber: RealTimeTranscriber, app_config: config::AppConfig) -> anyhow::Result<()> {
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

    tokio::spawn(async move {
        while let Ok(transcription) = transcript_rx.recv().await {
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

            // Forward chunk to clipboard and portal workers
            let _ = clipboard_tx_clone.send(transcription.clone());
            let _ = portal_tx_clone.send(transcription);
        }
    });

    // Portal paste worker: establish a portal session and paste on demand (only if enabled)
    if app_config.portal_config.enable_xdg_portal {
        tokio::spawn(async move {
            // Attempt to start screencast + remote desktop session
            let portal = crate::portal_input::PortalInput::new(true).await;
            let portal = match portal {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("Portal integration disabled: {}", e);
                    return;
                }
            };
            // Drain the channel and paste using portal Ctrl+V after wl-copy
            loop {
                match portal_rx.recv_timeout(Duration::from_millis(500)) {
                    Ok(text) => {
                        // Copy to clipboard (using our simplified wayland connection)
                        let _ = crate::copy::WlCopy::copy_to_clipboard(&text);

                        // Trigger paste via portal
                        if let Err(e) = portal.paste_via_ctrl_v().await {
                            eprintln!("Portal paste failed: {}", e);
                        }
                    }
                    Err(std_mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std_mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        });
    } else {
        thread::spawn(move || {
            loop {
                match clipboard_rx.recv_timeout(Duration::from_millis(500)) {
                    Ok(text) => {
                        let _ = crate::copy::WlCopy::copy_to_clipboard(&text);
                    }
                    Err(std_mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std_mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        });
    }

    let running = transcriber.get_running();
    let recording = transcriber.get_recording();

    // Run the UI with AtomicBool values directly and pass the configuration
    ui::run_with_audio_data(audio_visualization_data, running, recording, app_config);

    Ok(())
}
