use anyhow::Result;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use zbus::object_server::SignalEmitter;
use zbus::{connection, interface, Connection};

use speechcore::TranscriptionMode;

/// Commands that the system tray can send to the main application
#[derive(Debug, Clone)]
pub enum TrayCommand {
    ToggleRecording,
    ToggleManualSession,
    SwitchMode,
    OpenSettings,
    Quit,
}

/// StatusNotifierItem implementation
struct StatusNotifierItem {
    _command_tx: mpsc::UnboundedSender<TrayCommand>,
    is_recording: Arc<AtomicBool>,
    transcription_mode: Arc<AtomicU8>,
}

/// DBusMenu implementation for context menu
struct DbusMenu {
    command_tx: mpsc::UnboundedSender<TrayCommand>,
    is_recording: Arc<AtomicBool>,
    transcription_mode: Arc<AtomicU8>,
    /// Bumped on each state change, so hosts know the cached layout is stale.
    revision: Arc<AtomicU32>,
}

#[interface(name = "org.kde.StatusNotifierItem")]
impl StatusNotifierItem {
    /// Activate method - called on left click
    async fn activate(&self, _x: i32, _y: i32) {
        // No action - window visibility is no longer controllable
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
        // Always return "Active" to keep icon visible in tray
        // "Passive" would hide it in overflow/hidden icons area
        "Active"
    }

    /// IconName property: a record icon while recording
    #[zbus(property)]
    async fn icon_name(&self) -> &str {
        if self.is_recording.load(Ordering::Relaxed) {
            "media-record"
        } else {
            "audio-input-microphone"
        }
    }

    #[zbus(signal)]
    async fn new_icon(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn new_tool_tip(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    /// ToolTip property - returns (icon_name, icon_pixmap, title, description)
    #[zbus(property)]
    async fn tool_tip(&self) -> (String, Vec<(i32, i32, Vec<u8>)>, String, String) {
        let status = if self.is_recording.load(Ordering::Relaxed) {
            "Recording"
        } else {
            "Idle"
        };

        let mode = match TranscriptionMode::from_u8(self.transcription_mode.load(Ordering::Relaxed))
        {
            TranscriptionMode::RealTime => "Real-time",
            TranscriptionMode::Manual => "Manual",
        };

        let description = format!("{} | {}", mode, status);

        (
            self.icon_name().await.to_string(),
            vec![], // No icon pixmap
            "Sonori".to_string(),
            description,
        )
    }

    /// Menu property - we'll implement a simple menu path
    #[zbus(property)]
    async fn menu(&self) -> zbus::zvariant::ObjectPath<'static> {
        "/StatusNotifierItem/menu".try_into().unwrap()
    }
}

/// Menu item IDs for DBusMenu
const MENU_TOGGLE_RECORDING: i32 = 1;
const MENU_TOGGLE_MODE: i32 = 2;
const MENU_SEPARATOR: i32 = 3;
const MENU_QUIT: i32 = 4;
const MENU_SETTINGS: i32 = 5;

#[interface(name = "com.canonical.dbusmenu")]
impl DbusMenu {
    /// Get the menu layout
    async fn get_layout(
        &self,
        _parent_id: i32,
        _recursion_depth: i32,
        _property_names: Vec<String>,
    ) -> (
        u32,
        (
            i32,
            std::collections::HashMap<String, zbus::zvariant::Value<'_>>,
            Vec<zbus::zvariant::Value<'_>>,
        ),
    ) {
        use std::collections::HashMap;
        use zbus::zvariant::Value;

        let is_recording = self.is_recording.load(Ordering::Relaxed);
        let mode = TranscriptionMode::from_u8(self.transcription_mode.load(Ordering::Relaxed));
        let is_manual = matches!(mode, TranscriptionMode::Manual);

        // Build menu items
        let mut items = Vec::new();

        // Item 1: Start/Stop Recording
        let mut item1_props = HashMap::new();
        item1_props.insert(
            "label".to_string(),
            Value::new(if is_recording {
                "Stop Recording"
            } else {
                "Start Recording"
            }),
        );
        item1_props.insert("enabled".to_string(), Value::new(is_manual));
        let item1 = Value::new((MENU_TOGGLE_RECORDING, item1_props, Vec::<Value>::new()));
        items.push(item1);

        // Item 2: Mode Toggle
        let mut item2_props = HashMap::new();
        item2_props.insert(
            "label".to_string(),
            Value::new(if is_manual {
                "Mode: Manual"
            } else {
                "Mode: Real-time"
            }),
        );
        item2_props.insert("toggle-type".to_string(), Value::new("checkmark"));
        item2_props.insert("toggle-state".to_string(), Value::new(1i32)); // Always checked for current mode
        item2_props.insert("enabled".to_string(), Value::new(true));
        let item2 = Value::new((MENU_TOGGLE_MODE, item2_props, Vec::<Value>::new()));
        items.push(item2);

        let mut settings_props = HashMap::new();
        settings_props.insert("label".to_string(), Value::new("Settings"));
        settings_props.insert("enabled".to_string(), Value::new(true));
        items.push(Value::new((
            MENU_SETTINGS,
            settings_props,
            Vec::<Value>::new(),
        )));

        // Item 3: Separator
        let mut item3_props = HashMap::new();
        item3_props.insert("type".to_string(), Value::new("separator"));
        let item3 = Value::new((MENU_SEPARATOR, item3_props, Vec::<Value>::new()));
        items.push(item3);

        // Item 4: Quit
        let mut item4_props = HashMap::new();
        item4_props.insert("label".to_string(), Value::new("Quit"));
        item4_props.insert("enabled".to_string(), Value::new(true));
        let item4 = Value::new((MENU_QUIT, item4_props, Vec::<Value>::new()));
        items.push(item4);

        // Root menu item
        let root_props = HashMap::new();
        let layout = (0, root_props, items);

        (self.revision.load(Ordering::Relaxed), layout)
    }

    /// Handle menu item activation
    async fn event(
        &self,
        id: i32,
        _event_id: &str,
        _data: zbus::zvariant::Value<'_>,
        _timestamp: u32,
    ) {
        let command = match id {
            MENU_TOGGLE_RECORDING => {
                // Send the appropriate command based on current mode
                let mode =
                    TranscriptionMode::from_u8(self.transcription_mode.load(Ordering::Relaxed));
                match mode {
                    TranscriptionMode::Manual => Some(TrayCommand::ToggleManualSession),
                    TranscriptionMode::RealTime => Some(TrayCommand::ToggleRecording),
                }
            }
            MENU_TOGGLE_MODE => Some(TrayCommand::SwitchMode),
            MENU_SETTINGS => Some(TrayCommand::OpenSettings),
            MENU_QUIT => Some(TrayCommand::Quit),
            _ => None,
        };

        if let Some(cmd) = command {
            let _ = self.command_tx.send(cmd);
        }
    }

    #[zbus(signal)]
    async fn layout_updated(
        emitter: &SignalEmitter<'_>,
        revision: u32,
        parent: i32,
    ) -> zbus::Result<()>;

    /// DBusMenu version
    #[zbus(property)]
    async fn version(&self) -> u32 {
        4
    }

    /// Text direction
    #[zbus(property)]
    async fn text_direction(&self) -> &str {
        "ltr"
    }

    /// Menu status
    #[zbus(property)]
    async fn status(&self) -> &str {
        "normal"
    }
}

/// Start the system tray service
pub async fn run_system_tray(
    is_recording: Arc<AtomicBool>,
    transcription_mode: Arc<AtomicU8>,
    running: Arc<AtomicBool>,
) -> Result<mpsc::UnboundedReceiver<TrayCommand>> {
    let (command_tx, command_rx) = mpsc::unbounded_channel();
    let revision = Arc::new(AtomicU32::new(1));

    // Create our StatusNotifierItem
    let sni = StatusNotifierItem {
        _command_tx: command_tx.clone(),
        is_recording: is_recording.clone(),
        transcription_mode: transcription_mode.clone(),
    };

    // Create our DBusMenu
    let menu = DbusMenu {
        command_tx: command_tx.clone(),
        is_recording: is_recording.clone(),
        transcription_mode: transcription_mode.clone(),
        revision: revision.clone(),
    };

    // Build DBus connection and register our services
    let conn = connection::Builder::session()?
        .name("org.kde.StatusNotifierItem-sonori")?
        .serve_at("/StatusNotifierItem", sni)?
        .serve_at("/StatusNotifierItem/menu", menu)?
        .build()
        .await?;

    // Register with StatusNotifierWatcher
    register_with_watcher(&conn).await?;

    // Watch the shared state and tell tray hosts when icon, tooltip or menu change.
    // Hotkeys and IPC change it too, so the UI cannot be the one to report it.
    tokio::spawn(async move {
        let state = || {
            (
                is_recording.load(Ordering::Relaxed),
                transcription_mode.load(Ordering::Relaxed),
            )
        };
        let mut last = state();

        while running.load(Ordering::Relaxed) {
            tokio::time::sleep(tokio::time::Duration::from_millis(250)).await;
            let current = state();
            if current == last {
                continue;
            }
            last = current;
            let revision = revision.fetch_add(1, Ordering::Relaxed) + 1;

            let server = conn.object_server();
            if let Ok(item) = server
                .interface::<_, StatusNotifierItem>("/StatusNotifierItem")
                .await
            {
                let _ = StatusNotifierItem::new_icon(item.signal_emitter()).await;
                let _ = StatusNotifierItem::new_tool_tip(item.signal_emitter()).await;
            }
            if let Ok(menu) = server
                .interface::<_, DbusMenu>("/StatusNotifierItem/menu")
                .await
            {
                let _ = DbusMenu::layout_updated(menu.signal_emitter(), revision, 0).await;
            }
        }
    });

    Ok(command_rx)
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
