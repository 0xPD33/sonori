//! IPC module for external control via Unix socket.
//!
//! Enables CLI subcommands (e.g., `sonori toggle`) to control the running instance.
//! Used for compositor keybindings on Wayland (niri, sway, etc.) where XDG
//! GlobalShortcuts portal isn't available.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;

use crate::real_time_transcriber::{ManualSessionCommand, TranscriptionMode};

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

/// IPC server that listens for commands from CLI clients
#[derive(Clone)]
pub struct IpcServer {
    socket_path: PathBuf,
    manual_session_tx: mpsc::Sender<ManualSessionCommand>,
    transcription_mode: Arc<AtomicU8>,
    recording: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
}

impl IpcServer {
    pub fn new(
        manual_session_tx: mpsc::Sender<ManualSessionCommand>,
        transcription_mode: Arc<AtomicU8>,
        recording: Arc<AtomicBool>,
        running: Arc<AtomicBool>,
    ) -> Self {
        Self {
            socket_path: get_socket_path(),
            manual_session_tx,
            transcription_mode,
            recording,
            running,
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

        println!("IPC server listening on {:?}", self.socket_path);

        // Accept connections until shutdown
        loop {
            tokio::select! {
                accept_result = listener.accept() => {
                    match accept_result {
                        Ok((stream, _)) => {
                            let server = self.clone();
                            tokio::spawn(async move {
                                let response = server.handle_connection(stream).await;
                                if let Err(e) = response {
                                    eprintln!("IPC connection error: {}", e);
                                }
                            });
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

        // Cleanup socket on shutdown
        let _ = std::fs::remove_file(&self.socket_path);
        println!("IPC server shut down");
        Ok(())
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
            Ok(cmd) => self.execute_command(cmd).await,
            Err(e) => IpcResponse::error(format!("Invalid command: {}", e)),
        };

        // Send response
        let response_json = serde_json::to_string(&response)?;
        writer.write_all(response_json.as_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;

        Ok(())
    }

    async fn execute_command(&self, cmd: IpcCommand) -> IpcResponse {
        match cmd {
            IpcCommand::Toggle => self.handle_toggle().await,
            IpcCommand::Start => self.handle_start().await,
            IpcCommand::Stop => self.handle_stop().await,
            IpcCommand::Cancel => self.handle_cancel().await,
            IpcCommand::Status => self.handle_status(),
            IpcCommand::SwitchMode { mode } => self.handle_switch_mode(&mode).await,
        }
    }

    async fn handle_toggle(&self) -> IpcResponse {
        let is_recording = self.recording.load(Ordering::Relaxed);
        let mode = TranscriptionMode::from_u8(self.transcription_mode.load(Ordering::Relaxed));

        // In manual mode, toggle the session
        if mode == TranscriptionMode::Manual {
            let command = if is_recording {
                ManualSessionCommand::StopSession { responder: None }
            } else {
                ManualSessionCommand::StartSession { responder: None }
            };

            if let Err(e) = self.manual_session_tx.send(command).await {
                return IpcResponse::error(format!("Failed to send command: {}", e));
            }

            if is_recording {
                IpcResponse::success("Recording stopped")
            } else {
                IpcResponse::success("Recording started")
            }
        } else {
            // In realtime mode, just report status
            IpcResponse::error(
                "Toggle only works in manual mode. Use 'sonori switch-mode manual' first.",
            )
        }
    }

    async fn handle_start(&self) -> IpcResponse {
        let mode = TranscriptionMode::from_u8(self.transcription_mode.load(Ordering::Relaxed));

        if mode != TranscriptionMode::Manual {
            return IpcResponse::error("Start only works in manual mode");
        }

        let command = ManualSessionCommand::StartSession { responder: None };
        if let Err(e) = self.manual_session_tx.send(command).await {
            return IpcResponse::error(format!("Failed to send command: {}", e));
        }

        IpcResponse::success("Recording started")
    }

    async fn handle_stop(&self) -> IpcResponse {
        let mode = TranscriptionMode::from_u8(self.transcription_mode.load(Ordering::Relaxed));

        if mode != TranscriptionMode::Manual {
            return IpcResponse::error("Stop only works in manual mode");
        }

        let command = ManualSessionCommand::StopSession { responder: None };
        if let Err(e) = self.manual_session_tx.send(command).await {
            return IpcResponse::error(format!("Failed to send command: {}", e));
        }

        IpcResponse::success("Recording stopped")
    }

    async fn handle_cancel(&self) -> IpcResponse {
        let mode = TranscriptionMode::from_u8(self.transcription_mode.load(Ordering::Relaxed));

        if mode != TranscriptionMode::Manual {
            return IpcResponse::error("Cancel only works in manual mode");
        }

        let command = ManualSessionCommand::CancelSession { responder: None };
        if let Err(e) = self.manual_session_tx.send(command).await {
            return IpcResponse::error(format!("Failed to send command: {}", e));
        }

        IpcResponse::success("Session cancelled")
    }

    fn handle_status(&self) -> IpcResponse {
        let mode = TranscriptionMode::from_u8(self.transcription_mode.load(Ordering::Relaxed));
        let recording = self.recording.load(Ordering::Relaxed);

        let status = IpcStatus {
            mode: match mode {
                TranscriptionMode::Manual => "manual".to_string(),
                TranscriptionMode::RealTime => "realtime".to_string(),
            },
            recording,
            session_id: None, // Could be extended to include session ID
        };

        IpcResponse::success_with_status(status)
    }

    async fn handle_switch_mode(&self, mode_str: &str) -> IpcResponse {
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

        let command = ManualSessionCommand::SwitchMode(new_mode);
        if let Err(e) = self.manual_session_tx.send(command).await {
            return IpcResponse::error(format!("Failed to send command: {}", e));
        }

        IpcResponse::success(format!("Switched to {} mode", mode_str))
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        // Best-effort cleanup
        let _ = std::fs::remove_file(&self.socket_path);
    }
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

    let response: IpcResponse =
        serde_json::from_str(response_line.trim()).context("Invalid response from Sonori")?;

    Ok(response)
}
