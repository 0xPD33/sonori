use anyhow::{Context, Result};
use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
use ashpd::ActivationToken;
use futures_util::StreamExt;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};
use zbus::zvariant::OwnedValue;

use sonori::config::ShortcutMode;
use sonori::hotkey::{HotkeyState, SharedHotkeyState};
use sonori::ipc::AppCommand;
use speechcore::{ManualSessionCommand, TranscriptionMode};

const TOGGLE_ID: &str = "toggle_manual";
const CANCEL_ID: &str = "cancel_session";
const PASTE_LAST_ID: &str = "paste_last";
const MAGIC_MODE_ID: &str = "toggle_magic_mode";

/// Manages global shortcuts through the XDG Desktop Portal.
///
/// The session binds once. It binds again only when the portal drops a session
/// that worked (e.g. the portal restarted), so a declined dialog is never repeated.
pub struct GlobalShortcutsManager {
    accelerator: String,
    shortcut_mode: ShortcutMode,
    manual_session_tx: mpsc::Sender<ManualSessionCommand>,
    app_tx: mpsc::UnboundedSender<AppCommand>,
    transcription_mode: Arc<AtomicU8>,
    running: Arc<AtomicBool>,
    state: SharedHotkeyState,
}

impl GlobalShortcutsManager {
    /// Run the global shortcuts listener
    pub async fn run(self) -> Result<()> {
        loop {
            *self.state.write() = HotkeyState::Pending;
            match self.run_session().await {
                Ok(true) => {
                    eprintln!("Global shortcuts portal session ended; binding again");
                    sleep(Duration::from_secs(2)).await;
                }
                Ok(false) => return Ok(()),
                Err(e) => {
                    *self.state.write() = HotkeyState::Unavailable {
                        reason: format!("{e:#}"),
                    };
                    return Err(e);
                }
            }
        }
    }

    /// Runs one portal session. Returns whether the portal ended it (bind again)
    /// rather than the app shutting down.
    async fn run_session(&self) -> Result<bool> {
        let normalized_accelerator = normalize_accelerator_for_portal(&self.accelerator);

        let gs = GlobalShortcuts::new()
            .await
            .context("no GlobalShortcuts portal")?;

        // Create a session - this must be kept alive for the shortcuts to work
        let session = gs
            .create_session()
            .await
            .context("could not create a portal session")?;

        let shortcuts = [
            NewShortcut::new(TOGGLE_ID, "Toggle Manual Transcription Session")
                .preferred_trigger(Some(normalized_accelerator.as_str())),
            NewShortcut::new(CANCEL_ID, "Cancel Transcription Session"),
            NewShortcut::new(PASTE_LAST_ID, "Paste Last Transcript"),
            NewShortcut::new(MAGIC_MODE_ID, "Toggle Magic Mode"),
        ];

        let response = gs
            .bind_shortcuts(&session, &shortcuts, None)
            .await
            .context("the portal refused the bind request")?
            .response()
            .context("the bind request was declined")?;

        let Some(bound) = response.shortcuts().iter().find(|s| s.id() == TOGGLE_ID) else {
            eprintln!(
                "Shortcut '{}' was not bound by portal - user may have declined permission",
                normalized_accelerator
            );
            return Err(anyhow::anyhow!("the shortcut was not approved"));
        };
        let trigger = if bound.trigger_description().is_empty() {
            normalized_accelerator.clone()
        } else {
            bound.trigger_description().to_string()
        };
        println!("Global shortcut bound: {}", trigger);
        *self.state.write() = HotkeyState::Bound { trigger };

        // Listen to all portal signals
        let mut activated_stream = gs
            .receive_activated()
            .await
            .context("Failed to subscribe to Activated signal")?;

        let mut deactivated_stream = gs
            .receive_deactivated()
            .await
            .context("Failed to subscribe to Deactivated signal")?;

        // Process signals concurrently - keep session alive until app stops running.
        // A stream that ends means the portal went away.
        let portal_ended = loop {
            if !self.running.load(Ordering::Relaxed) {
                break false;
            }

            tokio::select! {
                activated = activated_stream.next() => match activated {
                    Some(activated) => self.handle_activated(activated).await,
                    None => break true,
                },
                deactivated = deactivated_stream.next() => match deactivated {
                    Some(deactivated) => self.handle_deactivated(deactivated).await,
                    None => break true,
                },
                _ = sleep(Duration::from_millis(100)) => {
                    // Periodic wake-up to check shutdown flag
                }
            }
        };

        // Keep the session alive until we exit
        drop(session);

        Ok(portal_ended)
    }

    /// Handle shortcut activation (key pressed)
    async fn handle_activated(&self, activated: ashpd::desktop::global_shortcuts::Activated) {
        let app_command = match activated.shortcut_id() {
            TOGGLE_ID | CANCEL_ID => None,
            PASTE_LAST_ID => Some(AppCommand::PasteLast),
            MAGIC_MODE_ID => Some(AppCommand::ToggleMagicMode),
            _ => return,
        };
        if let Some(command) = app_command {
            let _ = self.app_tx.send(command);
            return;
        }

        // Extract activation token if present
        if let Some(_token) = extract_activation_token(activated.options()) {
            // TODO: Use token to request window focus via portal or Wayland protocol
        }

        // Only act in Manual mode
        let mode = TranscriptionMode::from_u8(self.transcription_mode.load(Ordering::Relaxed));
        if mode != TranscriptionMode::Manual {
            return;
        }

        let command = match (activated.shortcut_id(), self.shortcut_mode) {
            (CANCEL_ID, _) => ManualSessionCommand::CancelSession { responder: None },
            (_, ShortcutMode::Toggle) => ManualSessionCommand::Toggle,
            // Push-to-talk: always start on press
            (_, ShortcutMode::PushToTalk) => ManualSessionCommand::StartSession { responder: None },
        };

        if let Err(e) = self.manual_session_tx.send(command).await {
            eprintln!("Failed to send manual session command: {}", e);
        }
    }

    /// Handle shortcut deactivation (key released)
    async fn handle_deactivated(&self, deactivated: ashpd::desktop::global_shortcuts::Deactivated) {
        if deactivated.shortcut_id() != TOGGLE_ID {
            return;
        }

        // Only act in Manual mode with push-to-talk
        if self.shortcut_mode != ShortcutMode::PushToTalk {
            return;
        }

        let mode = TranscriptionMode::from_u8(self.transcription_mode.load(Ordering::Relaxed));
        if mode != TranscriptionMode::Manual {
            return;
        }

        // Stop recording on key release
        let command = ManualSessionCommand::StopSession { responder: None };
        if let Err(e) = self.manual_session_tx.send(command).await {
            eprintln!("Failed to send stop command: {}", e);
        }
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
    normalized = normalized.replace(['<', '>'], "");

    normalized
}

pub async fn run_listener(
    accelerator: &str,
    shortcut_mode: ShortcutMode,
    manual_session_tx: mpsc::Sender<ManualSessionCommand>,
    app_tx: mpsc::UnboundedSender<AppCommand>,
    transcription_mode: Arc<AtomicU8>,
    running: Arc<AtomicBool>,
    state: SharedHotkeyState,
) -> Result<()> {
    GlobalShortcutsManager {
        accelerator: accelerator.to_string(),
        shortcut_mode,
        manual_session_tx,
        app_tx,
        transcription_mode,
        running,
        state,
    }
    .run()
    .await
}
