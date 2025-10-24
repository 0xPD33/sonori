use anyhow::Result;
use parking_lot::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use zbus::{connection, interface, Connection};

use crate::real_time_transcriber::TranscriptionMode;

/// Commands that the system tray can send to the main application
#[derive(Debug, Clone)]
pub enum TrayCommand {
    ToggleWindow,
    ShowWindow,
    HideWindow,
    ToggleRecording,
    ToggleManualSession,
    Quit,
}

/// State updates from the application to the tray icon
#[derive(Debug, Clone)]
pub enum TrayUpdate {
    Recording(bool),
    Mode(TranscriptionMode),
    Transcript(String),
    WindowVisible(bool),
}

/// StatusNotifierItem implementation
struct StatusNotifierItem {
    command_tx: mpsc::UnboundedSender<TrayCommand>,
    is_recording: Arc<AtomicBool>,
    transcription_mode: Arc<parking_lot::Mutex<TranscriptionMode>>,
    transcript_preview: Arc<RwLock<String>>,
}

#[interface(name = "org.kde.StatusNotifierItem")]
impl StatusNotifierItem {
    /// Activate method - called on left click
    async fn activate(&self, _x: i32, _y: i32) {
        let _ = self.command_tx.send(TrayCommand::ToggleWindow);
    }

    /// SecondaryActivate - called on middle click
    async fn secondary_activate(&self, _x: i32, _y: i32) {
        let _ = self.command_tx.send(TrayCommand::ToggleRecording);
    }

    /// Scroll method
    async fn scroll(&self, _delta: i32, _orientation: &str) {
        // Ignore scroll events
    }

    /// Category property
    #[zbus(property)]
    async fn category(&self) -> &str {
        "ApplicationStatus"
    }

    /// ID property
    #[zbus(property)]
    async fn id(&self) -> &str {
        "dev.paddy.sonori"
    }

    /// Title property
    #[zbus(property)]
    async fn title(&self) -> &str {
        "Sonori"
    }

    /// Status property
    #[zbus(property)]
    async fn status(&self) -> &str {
        if self.is_recording.load(Ordering::Relaxed) {
            "Active"
        } else {
            "Passive"
        }
    }

    /// IconName property
    #[zbus(property)]
    async fn icon_name(&self) -> &str {
        if self.is_recording.load(Ordering::Relaxed) {
            "media-record"
        } else {
            "audio-input-microphone"
        }
    }

    /// ToolTip property - returns (icon_name, icon_pixmap, title, description)
    #[zbus(property)]
    async fn tool_tip(&self) -> (String, Vec<(i32, i32, Vec<u8>)>, String, String) {
        let status = if self.is_recording.load(Ordering::Relaxed) {
            "Recording"
        } else {
            "Ready"
        };

        let mode = match *self.transcription_mode.lock() {
            TranscriptionMode::RealTime => "Real-time",
            TranscriptionMode::Manual => "Manual",
        };

        let preview = self.transcript_preview.read().clone();
        let description = if preview.is_empty() {
            format!("Mode: {} | Status: {}", mode, status)
        } else {
            format!("Mode: {} | Status: {} | {}", mode, status, preview)
        };

        (
            self.icon_name().await.to_string(),
            vec![], // No icon pixmap
            "Sonori - Speech Transcription".to_string(),
            description,
        )
    }

    /// Menu property - we'll implement a simple menu path
    #[zbus(property)]
    async fn menu(&self) -> zbus::zvariant::ObjectPath<'static> {
        "/StatusNotifierItem/menu".try_into().unwrap()
    }
}

/// Start the system tray service
pub async fn run_system_tray(
    is_recording: Arc<AtomicBool>,
    is_window_visible: Arc<AtomicBool>,
    transcription_mode: Arc<parking_lot::Mutex<TranscriptionMode>>,
    running: Arc<AtomicBool>,
) -> Result<(
    mpsc::UnboundedSender<TrayUpdate>,
    mpsc::UnboundedReceiver<TrayCommand>,
)> {
    let (command_tx, command_rx) = mpsc::unbounded_channel();
    let (update_tx, mut update_rx) = mpsc::unbounded_channel();

    let transcript_preview = Arc::new(RwLock::new(String::new()));

    // Create our StatusNotifierItem
    let sni = StatusNotifierItem {
        command_tx: command_tx.clone(),
        is_recording: is_recording.clone(),
        transcription_mode: transcription_mode.clone(),
        transcript_preview: transcript_preview.clone(),
    };

    // Build DBus connection and register our service
    let conn = connection::Builder::session()?
        .name("org.kde.StatusNotifierItem-sonori")?
        .serve_at("/StatusNotifierItem", sni)?
        .build()
        .await?;

    // Register with StatusNotifierWatcher
    register_with_watcher(&conn).await?;

    // Spawn update handler and keep connection alive
    let is_recording_clone = is_recording.clone();
    let transcription_mode_clone = transcription_mode.clone();
    let is_window_visible_clone = is_window_visible.clone();

    tokio::spawn(async move {
        // Keep the connection alive for the lifetime of the app
        let _conn = conn;

        while running.load(Ordering::Relaxed) {
            if let Some(update) = update_rx.recv().await {
                match update {
                    TrayUpdate::Recording(recording) => {
                        is_recording_clone.store(recording, Ordering::Relaxed);
                        // Properties are updated automatically through getters
                    }
                    TrayUpdate::Mode(mode) => {
                        *transcription_mode_clone.lock() = mode;
                    }
                    TrayUpdate::Transcript(text) => {
                        let preview = if text.len() > 50 {
                            format!("{}...", &text[..47])
                        } else {
                            text
                        };
                        *transcript_preview.write() = preview;
                    }
                    TrayUpdate::WindowVisible(visible) => {
                        is_window_visible_clone.store(visible, Ordering::Relaxed);
                    }
                }
            }
        }
    });

    Ok((update_tx, command_rx))
}

/// Register our tray icon with the StatusNotifierWatcher
async fn register_with_watcher(conn: &Connection) -> Result<()> {
    let proxy = zbus::Proxy::new(
        conn,
        "org.kde.StatusNotifierWatcher",
        "/StatusNotifierWatcher",
        "org.kde.StatusNotifierWatcher",
    )
    .await?;

    // Register our service
    proxy
        .call_method(
            "RegisterStatusNotifierItem",
            &("org.kde.StatusNotifierItem-sonori"),
        )
        .await?;

    Ok(())
}
