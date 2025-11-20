use anyhow::{Context, Result};
use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
use ashpd::ActivationToken;
use futures_util::StreamExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};
use zbus::zvariant::OwnedValue;

use sonori::real_time_transcriber::{ManualSessionCommand, TranscriptionMode};

/// Manages global shortcuts through the XDG Desktop Portal.
///
/// This struct keeps the portal session alive and handles:
/// - Session lifecycle management
/// - Shortcut binding (one attempt only - respects user declining permission)
/// - Shortcut activation with token extraction
/// - Portal signal monitoring (Activated, Deactivated, ShortcutsChanged)
/// - Clean shutdown
pub struct GlobalShortcutsManager {
    accelerator: String,
    manual_session_tx: mpsc::Sender<ManualSessionCommand>,
    transcription_mode: Arc<parking_lot::Mutex<TranscriptionMode>>,
    recording: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
}

impl GlobalShortcutsManager {
    /// Create a new global shortcuts manager
    pub fn new(
        accelerator: String,
        manual_session_tx: mpsc::Sender<ManualSessionCommand>,
        transcription_mode: Arc<parking_lot::Mutex<TranscriptionMode>>,
        recording: Arc<AtomicBool>,
        shutdown: Arc<AtomicBool>,
    ) -> Self {
        Self {
            accelerator,
            manual_session_tx,
            transcription_mode,
            recording,
            shutdown,
        }
    }

    /// Run the global shortcuts listener
    pub async fn run(self) -> Result<()> {
        // Try once to bind shortcuts - no retries
        // If the user declines the portal dialog, we shouldn't keep asking them
        self.run_session().await
    }

    /// Run a single session with the portal
    async fn run_session(&self) -> Result<()> {
        let normalized_accelerator = normalize_accelerator_for_portal(&self.accelerator);

        let gs = GlobalShortcuts::new()
            .await
            .context("Failed to connect to GlobalShortcuts portal")?;

        // Create a session - this must be kept alive for the shortcuts to work
        let session = gs
            .create_session()
            .await
            .context("Failed to create global shortcuts session")?;

        // Bind our shortcut
        let shortcut = NewShortcut::new("toggle_manual", "Toggle Manual Transcription Session")
            .preferred_trigger(Some(normalized_accelerator.as_str()));

        let request = gs
            .bind_shortcuts(&session, &[shortcut], None)
            .await
            .context("Failed to bind shortcuts")?;

        let response = request
            .response()
            .context("Failed to get bind shortcuts response")?;

        // Check what was actually bound
        let shortcuts = response.shortcuts();

        if shortcuts
            .iter()
            .find(|s| s.id() == "toggle_manual")
            .is_none()
        {
            // User likely declined the portal dialog or binding was rejected
            eprintln!(
                "Shortcut '{}' was not bound by portal - user may have declined permission",
                normalized_accelerator
            );
            return Err(anyhow::anyhow!(
                "Shortcut binding was not approved by user or portal"
            ));
        }

        // Listen to all portal signals
        let mut activated_stream = gs
            .receive_activated()
            .await
            .context("Failed to subscribe to Activated signal")?;

        let mut deactivated_stream = gs
            .receive_deactivated()
            .await
            .context("Failed to subscribe to Deactivated signal")?;

        let mut shortcuts_changed_stream = gs
            .receive_shortcuts_changed()
            .await
            .context("Failed to subscribe to ShortcutsChanged signal")?;

        // Process signals concurrently - keep session alive until shutdown
        loop {
            if self.shutdown.load(Ordering::Relaxed) {
                break;
            }

            tokio::select! {
                Some(activated) = activated_stream.next() => {
                    self.handle_activated(activated).await;
                }
                Some(deactivated) = deactivated_stream.next() => {
                    self.handle_deactivated(deactivated).await;
                }
                Some(changed) = shortcuts_changed_stream.next() => {
                    self.handle_shortcuts_changed(changed).await;
                }
                _ = sleep(Duration::from_millis(100)) => {
                    // Periodic wake-up to check shutdown flag
                }
            }
        }

        // Keep the session alive until we exit
        drop(session);

        Ok(())
    }

    /// Handle shortcut activation
    async fn handle_activated(&self, activated: ashpd::desktop::global_shortcuts::Activated) {
        if activated.shortcut_id() != "toggle_manual" {
            return;
        }

        // Extract activation token if present
        if let Some(_token) = extract_activation_token(activated.options()) {
            // TODO: Use token to request window focus via portal or Wayland protocol
        }

        // Only act in Manual mode
        let mode = *self.transcription_mode.lock();
        if mode != TranscriptionMode::Manual {
            return;
        }

        // Toggle recording state
        let is_recording = self.recording.load(Ordering::Relaxed);

        let command = if is_recording {
            ManualSessionCommand::StopSession { responder: None }
        } else {
            ManualSessionCommand::StartSession { responder: None }
        };

        if let Err(e) = self.manual_session_tx.send(command).await {
            eprintln!("Failed to send manual session command: {}", e);
        }
    }

    /// Handle shortcut deactivation
    async fn handle_deactivated(
        &self,
        _deactivated: ashpd::desktop::global_shortcuts::Deactivated,
    ) {
        // Nothing to do on deactivation
    }

    /// Handle shortcuts changed notification
    async fn handle_shortcuts_changed(
        &self,
        _changed: ashpd::desktop::global_shortcuts::ShortcutsChanged,
    ) {
        // Nothing to do when shortcuts change
    }
}

/// Extract activation token from the options HashMap
fn extract_activation_token(
    options: &std::collections::HashMap<String, OwnedValue>,
) -> Option<ActivationToken> {
    options.get("activation_token").and_then(|value| {
        // Try to extract string from OwnedValue
        if let Ok(s) = value.downcast_ref::<String>() {
            Some(ActivationToken::from(s.clone()))
        } else if let Ok(s) = value.downcast_ref::<&str>() {
            Some(ActivationToken::from(s.to_string()))
        } else {
            None
        }
    })
}

/// Normalize accelerator string for the portal following XDG shortcuts spec
/// Format: LOGO+key (uppercase, no angle brackets)
/// See: https://specifications.freedesktop.org/shortcuts-spec/latest/
fn normalize_accelerator_for_portal(accelerator: &str) -> String {
    // Remove angle brackets and convert to XDG format
    let mut normalized = accelerator
        .replace("<Super>", "LOGO+")
        .replace("<super>", "LOGO+")
        .replace("Super+", "LOGO+")
        .replace("super+", "LOGO+")
        .replace("<Meta>", "LOGO+")
        .replace("<meta>", "LOGO+")
        .replace("Meta+", "LOGO+")
        .replace("meta+", "LOGO+")
        .replace("<Control>", "CTRL+")
        .replace("<Ctrl>", "CTRL+")
        .replace("<ctrl>", "CTRL+")
        .replace("Control+", "CTRL+")
        .replace("Ctrl+", "CTRL+")
        .replace("ctrl+", "CTRL+")
        .replace("<Alt>", "ALT+")
        .replace("<alt>", "ALT+")
        .replace("Alt+", "ALT+")
        .replace("alt+", "ALT+")
        .replace("<Shift>", "SHIFT+")
        .replace("<shift>", "SHIFT+")
        .replace("Shift+", "SHIFT+")
        .replace("shift+", "SHIFT+");

    // Remove any remaining angle brackets
    normalized = normalized.replace('<', "").replace('>', "");

    normalized
}

/// Legacy function for backwards compatibility - spawns the manager in a task
pub async fn run_listener(
    accelerator: &str,
    manual_session_tx: mpsc::Sender<ManualSessionCommand>,
    transcription_mode_ref: Arc<parking_lot::Mutex<TranscriptionMode>>,
    recording: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
) -> Result<()> {
    let manager = GlobalShortcutsManager::new(
        accelerator.to_string(),
        manual_session_tx,
        transcription_mode_ref,
        recording,
        shutdown,
    );

    manager.run().await
}
