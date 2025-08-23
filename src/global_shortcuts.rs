use anyhow::Result;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tokio::sync::mpsc;
use futures_util::StreamExt;

use crate::real_time_transcriber::{ManualSessionCommand, TranscriptionMode};

use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};

/// Register a single global accelerator with the GlobalShortcuts portal and
/// listen for its activation. On activation, toggle a manual session.
pub async fn run_listener(
    accelerator: &str,
    manual_session_tx: mpsc::Sender<ManualSessionCommand>,
    transcription_mode_ref: Arc<parking_lot::Mutex<TranscriptionMode>>,
    recording: Arc<AtomicBool>,
) -> Result<()> {
    let gs = GlobalShortcuts::new().await?;

    // Create a session and bind our shortcut
    let session = gs.create_session().await?;

    let shortcut = NewShortcut::new("toggle_manual", "Toggle Manual Session")
        .preferred_trigger(Some(accelerator));

    // Request binding. We ignore window identifier and wait for response to complete binding.
    let request = gs.bind_shortcuts(&session, &[shortcut], None).await?;
    let _ = request.response()?; // ensure binding completed

    // Listen for Activated signals and process only ours
    let mut activated_stream = gs.receive_activated().await?;
    while let Some(activated) = activated_stream.next().await {
        if activated.shortcut_id() == "toggle_manual" {
            // Only act in Manual mode
            let mode = *transcription_mode_ref.lock();
            if mode == TranscriptionMode::Manual {
                let is_recording = recording.load(std::sync::atomic::Ordering::Relaxed);
                if is_recording {
                    let _ = manual_session_tx.send(ManualSessionCommand::StopSession).await;
                } else {
                    let _ = manual_session_tx.send(ManualSessionCommand::StartSession).await;
                }
            }
        }
    }

    Ok(())
}