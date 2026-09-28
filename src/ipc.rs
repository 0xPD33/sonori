//! IPC module for external control via Unix socket.
//!
//! Enables CLI subcommands (e.g., `sonori toggle`) to control the running instance.
//! Used for compositor keybindings on Wayland (niri, sway, etc.) where XDG
//! GlobalShortcuts portal isn't available.

use anyhow::{anyhow, Context, Result};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;

use crate::hotkey::{HotkeyState, SharedHotkeyState};
use speechcore::{BackendStatus, BackendStatusState, ManualSessionCommand, TranscriptionMode};

/// App-level actions that IPC, global shortcuts and the UI share.
#[derive(Debug, Clone)]
pub enum AppCommand {
    CopyLast,
    PasteLast,
    ToggleMagicMode,
    SetLanguage(String),
}

/// IPC command sent from CLI client to running instance
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum IpcCommand {
    /// Toggle recording (start if stopped, stop if recording)
    Toggle,
    /// Start recording session
    Start,
    /// Stop recording session
    Stop,
    /// Cancel current session without processing
    Cancel,
    /// Get current status
    Status,
    /// Switch transcription mode
    SwitchMode { mode: String },
    /// Shut the instance down (sent by a newer launch that replaces it)
    Quit,
    /// Copy the last transcript to the clipboard
    CopyLast,
    /// Paste the last transcript again
    PasteLast,
    /// Turn Magic Mode on or off
    ToggleMagic,
    /// Set the transcription language (e.g. "en", "de")
    Language { code: String },
}

/// Response from running instance to CLI client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<IpcStatus>,
}

/// Current status of the running instance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcStatus {
    pub mode: String,
    pub recording: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<IpcBackendStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hotkey: Option<HotkeyState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcBackendStatus {
    pub name: String,
    pub model: String,
    /// "ready", "loading: <step>", "downloading: <percent>" or "no model"
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

impl IpcResponse {
    pub fn success(message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: Some(message.into()),
            status: None,
        }
    }

    pub fn success_with_status(status: IpcStatus) -> Self {
        Self {
            success: true,
            message: None,
            status: Some(status),
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            success: false,
            message: Some(message.into()),
            status: None,
        }
    }
}

/// Get the default socket path
pub fn get_socket_path() -> PathBuf {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| {
        // Fallback: try to determine UID from /proc/self
        let uid = std::fs::read_to_string("/proc/self/loginuid")
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .unwrap_or(1000);
        format!("/run/user/{}", uid)
    });
    PathBuf::from(runtime_dir)
        .join("sonori")
        .join("control.sock")
}

/// Holds the PID of the GUI instance that owns the socket. A new launch uses it
/// to end the old instance even when that instance no longer answers IPC.
fn pid_path() -> PathBuf {
    get_socket_path().with_file_name("sonori.pid")
}

/// IPC server that listens for commands from CLI clients
pub struct IpcServer {
    socket_path: PathBuf,
    manual_session_tx: mpsc::Sender<ManualSessionCommand>,
    app_tx: mpsc::UnboundedSender<AppCommand>,
    transcription_mode: Arc<AtomicU8>,
    recording: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    backend_status: Arc<RwLock<BackendStatus>>,
    hotkey_state: SharedHotkeyState,
}

impl IpcServer {
    pub fn new(
        manual_session_tx: mpsc::Sender<ManualSessionCommand>,
        app_tx: mpsc::UnboundedSender<AppCommand>,
        transcription_mode: Arc<AtomicU8>,
        recording: Arc<AtomicBool>,
        running: Arc<AtomicBool>,
        backend_status: Arc<RwLock<BackendStatus>>,
        hotkey_state: SharedHotkeyState,
    ) -> Self {
        Self {
            socket_path: get_socket_path(),
            manual_session_tx,
            app_tx,
            transcription_mode,
            recording,
            running,
            backend_status,
            hotkey_state,
        }
    }

    /// Run the IPC server, listening for commands until shutdown
    pub async fn run(&self) -> Result<()> {
        // Create socket directory
        if let Some(socket_dir) = self.socket_path.parent() {
            std::fs::create_dir_all(socket_dir).context("Failed to create socket directory")?;
        }

        // Remove stale socket if exists
        let _ = std::fs::remove_file(&self.socket_path);

        // Bind to socket
        let listener =
            UnixListener::bind(&self.socket_path).context("Failed to bind IPC socket")?;

        // Set permissions (user-only: 0600)
        std::fs::set_permissions(&self.socket_path, std::fs::Permissions::from_mode(0o600))
            .context("Failed to set socket permissions")?;

        if let Err(e) = std::fs::write(pid_path(), std::process::id().to_string()) {
            eprintln!("Failed to write PID file: {}", e);
        }

        println!("IPC server listening on {:?}", self.socket_path);

        // Accept connections until shutdown.
        // Handle each connection inline so the response is sent before accepting
        // the next one. This avoids a race where a spawned task's response never
        // reaches the client (causing the CLI process to hang and preventing
        // subsequent hotkey invocations).
        loop {
            tokio::select! {
                accept_result = listener.accept() => {
                    match accept_result {
                        Ok((stream, _)) => {
                            if let Err(e) = self.handle_connection(stream).await {
                                eprintln!("IPC connection error: {}", e);
                            }
                        }
                        Err(e) => {
                            eprintln!("IPC accept error: {}", e);
                        }
                    }
                }
                _ = tokio::time::sleep(tokio::time::Duration::from_millis(100)) => {
                    if !self.running.load(Ordering::Relaxed) {
                        break;
                    }
                }
            }
        }

        self.remove_files();
        println!("IPC server shut down");
        Ok(())
    }

    fn remove_files(&self) {
        let _ = std::fs::remove_file(&self.socket_path);
        // Leave a newer instance's PID file alone.
        if std::fs::read_to_string(pid_path())
            .is_ok_and(|pid| pid.trim() == std::process::id().to_string())
        {
            let _ = std::fs::remove_file(pid_path());
        }
    }

    async fn handle_connection(&self, stream: UnixStream) -> Result<()> {
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let mut line = String::new();

        // Read command (single line JSON) with timeout so one idle client
        // cannot block this connection task forever.
        match tokio::time::timeout(
            tokio::time::Duration::from_secs(5),
            reader.read_line(&mut line),
        )
        .await
        {
            Ok(Ok(0)) => return Ok(()), // Peer closed connection
            Ok(Ok(_)) => {}
            Ok(Err(e)) => return Err(e.into()),
            Err(_) => return Err(anyhow!("Timed out waiting for IPC command")),
        }
        let line = line.trim();

        if line.is_empty() {
            return Ok(());
        }

        // Parse and execute command
        let response = match serde_json::from_str::<IpcCommand>(line) {
            Ok(cmd) => self.execute_command(cmd),
            Err(e) => IpcResponse::error(format!("Invalid command: {}", e)),
        };

        // Send response
        let response_json = serde_json::to_string(&response)?;
        writer.write_all(response_json.as_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;

        Ok(())
    }

    fn execute_command(&self, cmd: IpcCommand) -> IpcResponse {
        match cmd {
            // The transcriber decides between start and stop, since the
            // recording flag stays up while Stop drains.
            IpcCommand::Toggle => {
                self.send_manual(ManualSessionCommand::Toggle, "Recording toggled")
            }
            IpcCommand::Start => self.send_manual(
                ManualSessionCommand::StartSession { responder: None },
                "Recording started",
            ),
            IpcCommand::Stop => self.send_manual(
                ManualSessionCommand::StopSession { responder: None },
                "Recording stopped",
            ),
            IpcCommand::Cancel => self.send_manual(
                ManualSessionCommand::CancelSession { responder: None },
                "Session cancelled",
            ),
            IpcCommand::Status => self.handle_status(),
            IpcCommand::SwitchMode { mode } => self.handle_switch_mode(&mode),
            IpcCommand::Quit => {
                self.running.store(false, Ordering::Relaxed);
                IpcResponse::success("Quitting")
            }
            IpcCommand::CopyLast => self.send_app(AppCommand::CopyLast, "Copying last transcript"),
            IpcCommand::PasteLast => {
                self.send_app(AppCommand::PasteLast, "Pasting last transcript")
            }
            IpcCommand::ToggleMagic => {
                self.send_app(AppCommand::ToggleMagicMode, "Magic Mode toggled")
            }
            IpcCommand::Language { code } => {
                let message = format!("Language set to {code}");
                self.send_app(AppCommand::SetLanguage(code), message)
            }
        }
    }

    /// Queues a session command. Never waits: a full queue would stall every
    /// later IPC client, so the caller gets an error instead.
    fn send_manual(&self, command: ManualSessionCommand, ok: &str) -> IpcResponse {
        let mode = TranscriptionMode::from_u8(self.transcription_mode.load(Ordering::Relaxed));
        if mode != TranscriptionMode::Manual {
            return IpcResponse::error(
                "This command only works in manual mode. Use 'sonori switch-mode manual' first.",
            );
        }
        self.try_send_manual(command, ok)
    }

    fn try_send_manual(&self, command: ManualSessionCommand, ok: impl Into<String>) -> IpcResponse {
        match self.manual_session_tx.try_send(command) {
            Ok(()) => IpcResponse::success(ok),
            Err(mpsc::error::TrySendError::Full(_)) => {
                IpcResponse::error("Sonori is busy; try again")
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                IpcResponse::error("Sonori is shutting down")
            }
        }
    }

    fn send_app(&self, command: AppCommand, ok: impl Into<String>) -> IpcResponse {
        match self.app_tx.send(command) {
            Ok(()) => IpcResponse::success(ok),
            Err(_) => IpcResponse::error("Sonori is shutting down"),
        }
    }

    fn handle_status(&self) -> IpcResponse {
        let mode = TranscriptionMode::from_u8(self.transcription_mode.load(Ordering::Relaxed));
        let recording = self.recording.load(Ordering::Relaxed);

        let backend = {
            let status = self.backend_status.read();
            let state = match (&status.state, status.download_progress) {
                (_, Some(progress)) => format!("downloading: {:.0}%", progress * 100.0),
                (BackendStatusState::Ready, None) => "ready".to_string(),
                (BackendStatusState::Loading(step), None) => format!("loading: {step}"),
                (BackendStatusState::NoModel, None) => "no model".to_string(),
            };
            IpcBackendStatus {
                name: status.backend_name.clone(),
                model: status.model_name.clone(),
                state,
                last_error: status
                    .last_error
                    .as_ref()
                    .map(|(message, _)| message.clone()),
            }
        };

        let status = IpcStatus {
            mode: match mode {
                TranscriptionMode::Manual => "manual".to_string(),
                TranscriptionMode::RealTime => "realtime".to_string(),
            },
            recording,
            session_id: None, // Could be extended to include session ID
            backend: Some(backend),
            hotkey: Some(self.hotkey_state.read().clone()),
        };

        IpcResponse::success_with_status(status)
    }

    fn handle_switch_mode(&self, mode_str: &str) -> IpcResponse {
        let new_mode = match mode_str.to_lowercase().as_str() {
            "manual" => TranscriptionMode::Manual,
            "realtime" => TranscriptionMode::RealTime,
            _ => {
                return IpcResponse::error(format!(
                    "Unknown mode: {}. Use 'manual' or 'realtime'.",
                    mode_str
                ))
            }
        };

        self.try_send_manual(
            ManualSessionCommand::SwitchMode(new_mode),
            format!("Switched to {} mode", mode_str),
        )
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        // Best-effort cleanup
        self.remove_files();
    }
}

/// Ends an already running GUI instance before this one loads a model, so two
/// models never sit in memory. Without this, a new launch would also unlink the
/// old socket and leave that instance running unreachable.
pub async fn replace_running_instance() {
    let pid = running_instance_pid();
    let socket_path = get_socket_path();
    if pid.is_none() && !socket_path.exists() {
        return;
    }

    // A wedged instance may not answer; the kill below still ends it.
    let answered = match send_command(IpcCommand::Quit).await {
        Ok(response) => response.success,
        Err(e) => {
            eprintln!("Previous instance did not answer Quit: {}", e);
            false
        }
    };

    let Some(pid) = pid else {
        // Instances from before the PID file only release their socket.
        if answered {
            println!("Asked the running Sonori instance to quit");
            for _ in 0..50 {
                if !socket_path.exists() {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
        return;
    };
    println!("Replacing the running Sonori instance (pid {pid})");
    if wait_for_exit(pid, Duration::from_secs(10)).await {
        return;
    }

    eprintln!("Previous Sonori instance (pid {pid}) did not exit; killing it");
    let _ = std::process::Command::new("kill")
        .args(["-KILL", &pid.to_string()])
        .status();
    if !wait_for_exit(pid, Duration::from_secs(2)).await {
        eprintln!("Previous Sonori instance (pid {pid}) is still running");
    }
}

/// The PID from the PID file, if that process still runs Sonori (not a reused PID).
fn running_instance_pid() -> Option<u32> {
    let pid: u32 = std::fs::read_to_string(pid_path())
        .ok()?
        .trim()
        .parse()
        .ok()?;
    let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    // Nix wraps the binary as `.sonori-wrapped`.
    (pid != std::process::id() && comm.contains("sonori") && process_alive(pid)).then_some(pid)
}

async fn wait_for_exit(pid: u32, timeout: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    while process_alive(pid) {
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    true
}

/// A zombie has exited and freed its memory, so it counts as gone.
fn process_alive(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
        stat.rsplit_once(')')
            .and_then(|(_, rest)| rest.trim_start().chars().next())
            .is_some_and(|state| state != 'Z' && state != 'X')
    })
}

/// Send a command to the running Sonori instance
pub async fn send_command(cmd: IpcCommand) -> Result<IpcResponse> {
    let socket_path = get_socket_path();

    if !socket_path.exists() {
        return Err(anyhow!(
            "Sonori is not running (socket not found at {:?})",
            socket_path
        ));
    }

    // A wedged instance must not hang the hotkey that ran this command.
    tokio::time::timeout(Duration::from_secs(5), async {
        let stream = UnixStream::connect(&socket_path)
            .await
            .context("Failed to connect to Sonori (is it running?)")?;

        let (reader, mut writer) = stream.into_split();

        // Send command
        let cmd_json = serde_json::to_string(&cmd)?;
        writer.write_all(cmd_json.as_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;

        // Read response
        let mut reader = BufReader::new(reader);
        let mut response_line = String::new();
        reader.read_line(&mut response_line).await?;

        serde_json::from_str::<IpcResponse>(response_line.trim())
            .context("Invalid response from Sonori")
    })
    .await
    .map_err(|_| anyhow!("Sonori did not answer within 5 seconds"))?
}
