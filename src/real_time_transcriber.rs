use anyhow::Context;
use chrono::Utc;
use parking_lot::{Mutex, RwLock};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc, oneshot};

// Use local modules
use crate::audio_capture::AudioCapture;
use crate::audio_processor::AudioProcessor;
use crate::backend::{create_backend, BackendType, TranscriptionBackend};
use crate::backend_manager::{BackendCommand, BackendManager};
use crate::config::{read_app_config, AppConfig};
use crate::silero_audio_processor::{AudioSegment, SileroVad};
use crate::stats_reporter::StatsReporter;
use crate::transcription_processor::TranscriptionProcessor;
use crate::transcription_stats::TranscriptionStats;
use crate::ui::common::{
    AudioVisualizationData, BackendStatus, BackendStatusState, ProcessingState,
};

/// Transcription mode enumeration
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TranscriptionMode {
    /// Continuous real-time transcription (existing behavior)
    RealTime,
    /// On-demand manual transcription sessions
    Manual,
}

impl TranscriptionMode {
    /// Convert to u8 for atomic storage
    pub fn as_u8(self) -> u8 {
        match self {
            TranscriptionMode::RealTime => 0,
            TranscriptionMode::Manual => 1,
        }
    }

    /// Convert from u8 for atomic retrieval
    pub fn from_u8(val: u8) -> Self {
        match val {
            1 => TranscriptionMode::Manual,
            _ => TranscriptionMode::RealTime, // Default to RealTime for safety
        }
    }
}

/// Transcription with session tracking
#[derive(Debug, Clone)]
pub struct TranscriptionMessage {
    pub text: String,
    pub session_id: Option<String>,
}

impl From<&str> for TranscriptionMode {
    fn from(mode: &str) -> Self {
        match mode.to_lowercase().as_str() {
            "manual" => TranscriptionMode::Manual,
            _ => TranscriptionMode::RealTime, // Default to real-time
        }
    }
}

/// Status of a manual transcription session
#[derive(Debug, Clone)]
pub struct ManualSessionStatus {
    pub session_id: String,
    pub is_recording: bool,
    pub is_processing: bool,
    pub duration_secs: u32,
    pub accumulated_samples: usize,
}

/// Manual transcription session data
#[derive(Debug, Clone)]
pub struct ManualSession {
    pub session_id: String,
    pub start_time: Instant,
    pub accumulated_audio: Vec<f32>,
    pub is_recording: bool,
    pub is_processing: bool,
    pub max_duration_secs: u32,
}

impl ManualSession {
    fn new(max_duration_secs: u32) -> Self {
        let session_id = format!("session_{}", Utc::now().timestamp_micros());
        Self {
            session_id,
            start_time: Instant::now(),
            accumulated_audio: Vec::new(),
            is_recording: true,
            is_processing: false,
            max_duration_secs,
        }
    }

    fn get_duration_secs(&self) -> u32 {
        self.start_time.elapsed().as_secs() as u32
    }

    fn get_status(&self) -> ManualSessionStatus {
        ManualSessionStatus {
            session_id: self.session_id.clone(),
            is_recording: self.is_recording,
            is_processing: self.is_processing,
            duration_secs: self.get_duration_secs(),
            accumulated_samples: self.accumulated_audio.len(),
        }
    }

    fn is_expired(&self) -> bool {
        self.get_duration_secs() >= self.max_duration_secs
    }
}

/// Manual session currently being processed (no longer recording)
#[derive(Debug, Clone)]
pub struct ManualProcessingSession {
    pub session: ManualSession,
    pub processing_start_time: Instant,
}

impl ManualProcessingSession {
    fn new(mut session: ManualSession) -> Self {
        session.is_recording = false;
        session.is_processing = true;
        Self {
            session,
            processing_start_time: Instant::now(),
        }
    }

    fn get_status(&self) -> ManualSessionStatus {
        let mut status = self.session.get_status();
        status.is_recording = false;
        status.is_processing = true;
        status.duration_secs = self.processing_start_time.elapsed().as_secs() as u32;
        status
    }

    fn session_id(&self) -> &str {
        &self.session.session_id
    }
}

/// Commands for manual session management
#[derive(Debug)]
pub enum ManualSessionCommand {
    StartSession {
        responder: Option<oneshot::Sender<anyhow::Result<String>>>,
    },
    StopSession {
        responder: Option<oneshot::Sender<anyhow::Result<()>>>,
    },
    CancelSession {
        responder: Option<oneshot::Sender<anyhow::Result<()>>>,
    },
    GetSessionStatus,
    SwitchMode(TranscriptionMode),
}

/// RAII guard to automatically reset finalizing flag when dropped
struct FinalizingGuard {
    flag: Arc<AtomicBool>,
}

impl FinalizingGuard {
    fn new(flag: Arc<AtomicBool>) -> Self {
        flag.store(true, std::sync::atomic::Ordering::Relaxed);
        Self { flag }
    }
}

impl Drop for FinalizingGuard {
    fn drop(&mut self) {
        self.flag.store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Main transcription coordinator that integrates all components
pub struct RealTimeTranscriber {
    // Audio capture (wrapped in Arc<Mutex> for sharing with command processor)
    audio_capture: Arc<Mutex<AudioCapture>>,

    // Audio processing
    tx: mpsc::Sender<Vec<f32>>,
    rx: Option<mpsc::Receiver<Vec<f32>>>,

    // Transcription
    pub transcript_tx: broadcast::Sender<TranscriptionMessage>,
    pub transcript_rx: broadcast::Receiver<TranscriptionMessage>,

    // State control
    running: Arc<AtomicBool>,
    recording: Arc<AtomicBool>,

    // Model and parameters
    backend: Arc<Mutex<Option<Arc<TranscriptionBackend>>>>,
    backend_ready: Arc<AtomicBool>,
    language: String,

    // Processing components
    audio_processor: Arc<Mutex<SileroVad>>,

    // Data storage and visualization
    transcript_history: Arc<RwLock<String>>,
    audio_visualization_data: Arc<RwLock<AudioVisualizationData>>,

    // Communication channels for sub-components
    segment_tx: mpsc::Sender<AudioSegment>,
    segment_rx: Option<mpsc::Receiver<AudioSegment>>,
    transcription_done_tx: mpsc::UnboundedSender<()>,
    _transcription_done_rx: Option<mpsc::UnboundedReceiver<()>>,

    // Statistics
    transcription_stats: Arc<Mutex<TranscriptionStats>>,
    stats_reporter: Option<StatsReporter>,

    // Task handles for graceful shutdown
    transcription_handle: Option<tokio::task::JoinHandle<()>>,
    audio_handle: Option<tokio::task::JoinHandle<()>>,

    // Audio processor reference for manual transcription
    audio_processor_ref: Option<Arc<crate::audio_processor::AudioProcessor>>,

    // Manual mode specific fields
    transcription_mode: Arc<AtomicU8>,
    current_manual_session: Arc<Mutex<Option<ManualSession>>>,
    processing_manual_session: Arc<Mutex<Option<ManualProcessingSession>>>,
    finalizing_manual_session: Arc<AtomicBool>,
    manual_session_tx: mpsc::Sender<ManualSessionCommand>,
    manual_session_rx: Option<mpsc::Receiver<ManualSessionCommand>>,

    // Sound effects
    sound_player: Option<Arc<crate::sound_player::SoundPlayer>>,

    // Enhancement mode
    magic_mode_enabled: Arc<AtomicBool>,

    // Backend management
    pub backend_status: Arc<RwLock<BackendStatus>>,
    backend_command_tx: Option<mpsc::UnboundedSender<BackendCommand>>,
}

impl RealTimeTranscriber {
    /// Creates a new RealTimeTranscriber instance
    ///
    /// # Arguments
    /// * `model_path` - Path to the Whisper model file
    /// * `app_config` - Application configuration
    /// * `sound_player` - Optional sound player for audio feedback
    ///
    /// # Returns
    /// Result containing the new instance or an error
    pub fn new(
        model_path: PathBuf,
        app_config: AppConfig,
        sound_player: Option<Arc<crate::sound_player::SoundPlayer>>,
    ) -> Result<Self, anyhow::Error> {
        // Use bounded channels with larger capacities for better performance
        let (tx, rx) = mpsc::channel(400);
        let (transcript_tx, transcript_rx) = broadcast::channel(100);
        let (segment_tx, segment_rx) = mpsc::channel(400);
        let (transcription_done_tx, transcription_done_rx) = mpsc::unbounded_channel();
        let (manual_session_tx, manual_session_rx) = mpsc::channel(10);

        // Get the Silero model from the models directory
        let home_dir = std::env::var("HOME").with_context(|| "Failed to get HOME directory")?;
        let models_dir = PathBuf::from(format!("{}/.cache/sonori/models", home_dir));
        let silero_model_path = models_dir.join("silero_vad.onnx");

        if !silero_model_path.exists() {
            return Err(anyhow::anyhow!(
                "Silero VAD model not found at {}. Please run the application again to download it.",
                silero_model_path.display()
            ));
        }

        println!("Using Silero VAD model at: {:?}", silero_model_path);
        println!("Using Whisper model at: {:?}", model_path);

        let running = Arc::new(AtomicBool::new(true));
        let recording = Arc::new(AtomicBool::new(false));
        let transcript_history = Arc::new(RwLock::new(String::new()));
        let backend = Arc::new(Mutex::new(None));
        let transcription_stats = Arc::new(Mutex::new(TranscriptionStats::new()));

        let audio_visualization_data =
            Arc::new(RwLock::new(AudioVisualizationData::with_capacity(1024)));

        let audio_processor = match SileroVad::new(
            (
                app_config.vad_config.clone(),
                app_config.realtime_mode_config.clone(),
                app_config.audio_processor_config.buffer_size,
                crate::config::SAMPLE_RATE,
            )
                .into(),
            &silero_model_path,
        ) {
            Ok(vad) => Arc::new(Mutex::new(vad)),
            Err(e) => {
                eprintln!(
                    "Failed to initialize SileroVad: {}. Using default configuration might help.",
                    e
                );
                return Err(anyhow::anyhow!("VAD initialization failed: {}", e));
            }
        };

        let backend_clone = backend.clone();
        let backend_ready = Arc::new(AtomicBool::new(false));
        let backend_ready_for_task = backend_ready.clone();
        let model_path_clone = model_path.clone();
        let backend_type = app_config.backend_config.backend;
        let backend_config = app_config.backend_config.clone();

        // Create backend status (shared with UI status bar)
        let backend_name = match backend_type {
            BackendType::CTranslate2 => "CTranslate2",
            BackendType::WhisperCpp => "WhisperCpp",
            BackendType::Moonshine => "Moonshine",
            BackendType::Parakeet => "Parakeet",
        };
        let backend_status = Arc::new(RwLock::new(BackendStatus::new(
            backend_name.to_string(),
            app_config.general_config.model.clone(),
        )));

        // Create backend manager
        let mut backend_manager = BackendManager::new(
            backend.clone(),
            backend_ready.clone(),
            backend_status.clone(),
        );
        backend_manager.start();
        let backend_command_tx = backend_manager.command_sender();

        // Set initial processing state for loading
        {
            let mut audio_data = audio_visualization_data.write();
            audio_data.set_processing_state(ProcessingState::Loading);
        }
        {
            let mut s = backend_status.write();
            s.state = BackendStatusState::Loading("Initializing...".to_string());
        }

        let backend_status_for_load = backend_status.clone();
        let audio_visualization_data_for_load = audio_visualization_data.clone();
        tokio::spawn(async move {
            println!(
                "INFO: Loading {} backend with model at {:?}",
                backend_type, model_path_clone
            );
            println!(
                "Backend config: threads={}, gpu_enabled={}, quantization={:?}",
                backend_config.threads,
                backend_config.gpu_enabled,
                backend_config.quantization_level
            );

            match create_backend(backend_type, &model_path_clone, &backend_config).await {
                Ok(b) => {
                    println!("{} backend loaded successfully!", backend_type);
                    let capabilities = b.capabilities();
                    println!(
                        "Backend capabilities: name={}, max_audio_duration={:?}, streaming={}",
                        capabilities.name,
                        capabilities.max_audio_duration,
                        capabilities.supports_streaming
                    );
                    *backend_clone.lock() = Some(Arc::new(b));
                    backend_ready_for_task.store(true, Ordering::Relaxed);
                    println!("Backend ready for transcription");

                    // Set processing state to idle after successful load
                    let mut audio_data = audio_visualization_data_for_load.write();
                    audio_data.set_processing_state(ProcessingState::Idle);

                    // Update backend status to ready
                    let mut s = backend_status_for_load.write();
                    s.state = BackendStatusState::Ready;
                }
                Err(e) => {
                    eprintln!("ERROR: Failed to load backend: {}", e);
                    eprintln!("Backend will not be available for transcription");

                    // Set processing state to error on load failure
                    let mut audio_data = audio_visualization_data_for_load.write();
                    audio_data.set_processing_state(ProcessingState::Error);

                    // Update backend status to error
                    let mut s = backend_status_for_load.write();
                    s.state = BackendStatusState::Error(format!("{}", e));
                    s.error_time = Some(std::time::Instant::now());
                }
            }
        });

        // Initialize transcription mode from config
        let transcription_mode = Arc::new(AtomicU8::new(
            TranscriptionMode::from(app_config.general_config.transcription_mode.as_str()).as_u8(),
        ));
        let current_manual_session = Arc::new(Mutex::new(None));
        let processing_manual_session = Arc::new(Mutex::new(None));

        Ok(Self {
            audio_capture: Arc::new(Mutex::new(AudioCapture::new())),
            tx,
            rx: Some(rx),
            transcript_tx,
            transcript_rx,
            running,
            recording,
            backend,
            backend_ready,
            language: app_config.general_config.language.clone(),
            audio_processor,
            transcript_history,
            audio_visualization_data,
            segment_tx,
            segment_rx: Some(segment_rx),
            transcription_done_tx,
            _transcription_done_rx: Some(transcription_done_rx),
            transcription_stats,
            stats_reporter: None,
            transcription_handle: None,
            audio_handle: None,
            audio_processor_ref: None,

            // Manual mode fields
            transcription_mode,
            current_manual_session,
            processing_manual_session,
            finalizing_manual_session: Arc::new(AtomicBool::new(false)),
            manual_session_tx,
            manual_session_rx: Some(manual_session_rx),

            // Sound effects
            sound_player,

            // Enhancement mode (always starts inactive, user toggles via UI button)
            magic_mode_enabled: Arc::new(AtomicBool::new(false)),

            // Backend management
            backend_status,
            backend_command_tx: Some(backend_command_tx),
        })
    }

    /// Starts the audio capture and transcription process
    ///
    /// Sets up PortAudio for capturing audio and spawns worker tasks for processing
    ///
    /// # Returns
    /// Result indicating success or an error with detailed message
    pub fn start(&mut self) -> Result<(), anyhow::Error> {
        // Ensure recording is initially set to false
        self.recording.store(false, Ordering::Relaxed);

        // Set running to true
        self.running.store(true, Ordering::Relaxed);

        // Start audio capture
        self.audio_capture.lock().start(
            self.tx.clone(),
            self.running.clone(),
            self.recording.clone(),
            self.transcription_stats.clone(),
        )?;

        // Initialize statistics reporter
        let stats_reporter =
            StatsReporter::new(self.transcription_stats.clone(), self.running.clone());
        stats_reporter.start_periodic_reporting();
        self.stats_reporter = Some(stats_reporter);

        // Initialize transcription processor
        let transcription_processor = TranscriptionProcessor::new(
            self.backend.clone(),
            self.backend_ready.clone(),
            self.language.clone(),
            self.running.clone(),
            self.transcription_done_tx.clone(),
            self.transcription_stats.clone(),
            self.audio_visualization_data.clone(),
            self.magic_mode_enabled.clone(),
        );

        // Get config
        let config = read_app_config();

        // Initialize audio processor
        let audio_processor = Arc::new(AudioProcessor::new(
            self.running.clone(),
            self.recording.clone(),
            self.transcript_history.clone(),
            self.audio_processor.clone(),
            self.audio_visualization_data.clone(),
            self.segment_tx.clone(),
            self.transcription_mode.clone(),
            self.transcription_stats.clone(),
            self.manual_session_tx.clone(),
            config,
        ));

        // Store reference for manual transcription
        self.audio_processor_ref = Some(audio_processor.clone());

        // Take ownership of the receivers and pass them to the processors
        if let (Some(segment_rx), Some(rx), Some(manual_session_rx)) = (
            self.segment_rx.take(),
            self.rx.take(),
            self.manual_session_rx.take(),
        ) {
            self.transcription_handle =
                Some(transcription_processor.start(segment_rx, self.transcript_tx.clone()));
            self.audio_handle = Some(audio_processor.start(rx));
            self.start_recording_monitor();
            self.start_manual_session_processor(manual_session_rx);
        } else {
            return Err(anyhow::anyhow!(
                "Failed to take ownership of receivers for processors"
            ));
        }

        Ok(())
    }

    /// Starts the manual session command processor
    fn start_manual_session_processor(
        &self,
        mut manual_session_rx: mpsc::Receiver<ManualSessionCommand>,
    ) {
        let current_manual_session = self.current_manual_session.clone();
        let processing_manual_session = self.processing_manual_session.clone();
        let finalizing_manual_session = self.finalizing_manual_session.clone();
        let transcription_mode = self.transcription_mode.clone(); // Shared reference
        let running = self.running.clone();
        let recording = self.recording.clone();
        let transcript_history = self.transcript_history.clone();
        let audio_visualization_data = self.audio_visualization_data.clone();
        let audio_processor_ref = self.audio_processor_ref.clone();
        let audio_capture = self.audio_capture.clone(); // Share audio capture for stream control
        let sound_player = self.sound_player.clone(); // Clone sound player for audio feedback
        let transcription_stats = self.transcription_stats.clone(); // For drain detection
        let app_config = read_app_config();
        let manual_mode_config = app_config.manual_mode_config.clone();
        let sample_rate = crate::config::SAMPLE_RATE;

        tokio::spawn(async move {
            while running.load(Ordering::Relaxed) {
                tokio::select! {
                    command = manual_session_rx.recv() => {
                        if let Some(cmd) = command {
                            match cmd {
                                ManualSessionCommand::StartSession { mut responder } => {
                                    let current_mode = TranscriptionMode::from_u8(transcription_mode.load(Ordering::Relaxed));
                                    if current_mode == TranscriptionMode::Manual {
                                        // Check if a session is currently finalizing
                                        if finalizing_manual_session.load(Ordering::Relaxed) {
                                            eprintln!("Cannot start new session: previous session is still finalizing");
                                            if let Some(responder) = responder.take() {
                                                let _ = responder.send(Err(anyhow::anyhow!(
                                                    "Cannot start new session while previous session is finalizing"
                                                )));
                                            }
                                            continue;
                                        }

                                        let max_duration = manual_mode_config.max_recording_duration_secs;
                                        let new_session = ManualSession::new(max_duration);
                                        let session_id = new_session.session_id.clone();

                                        let can_start = {
                                            let mut session_lock = current_manual_session.lock();
                                            if let Some(existing) = session_lock.as_ref() {
                                                // Check both the session state AND the actual recording flag
                                                // The recording flag may have been cleared by buffer overflow
                                                let actually_recording = recording.load(Ordering::Relaxed);
                                                if existing.is_recording && actually_recording {
                                                    eprintln!(
                                                        "Cannot start new session: existing session {} is still recording",
                                                        existing.session_id
                                                    );
                                                    if let Some(responder) = responder.take() {
                                                        let _ = responder.send(Err(anyhow::anyhow!(
                                                            "Cannot start new session: existing session {} is still recording",
                                                            existing.session_id
                                                        )));
                                                    }
                                                    false
                                                } else {
                                                    // Session exists but recording stopped (e.g. buffer overflow)
                                                    // Clear the old session and start fresh
                                                    if existing.is_recording && !actually_recording {
                                                        println!(
                                                            "Clearing stale session {} (recording flag is false)",
                                                            existing.session_id
                                                        );
                                                    }
                                                    *session_lock = Some(new_session);
                                                    true
                                                }
                                            } else {
                                                *session_lock = Some(new_session);
                                                true
                                            }
                                        };

                                        if !can_start {
                                            continue;
                                        }

                                        // Atomically clear buffer and set session ID under both locks
                                        if let Some(ref audio_processor) = audio_processor_ref {
                                            audio_processor.start_new_manual_session(session_id.clone());
                                            println!("Set session ID to: {}", session_id);
                                        }

                                        // Clear transcript if configured to do so
                                        if manual_mode_config.clear_on_new_session {
                                            let mut transcript_history_lock = transcript_history.write();
                                            transcript_history_lock.clear();

                                            let mut audio_data = audio_visualization_data.write();
                                            audio_data.transcript.clear();
                                            audio_data.reset_requested = true;
                                        }

                                        // Start the audio capture stream FIRST
                                        if let Err(e) = audio_capture.lock().start_recording() {
                                            eprintln!("Warning: Failed to start audio recording: {}", e);
                                        }

                                        // Then set recording flag (prevents dropping initial audio)
                                        recording.store(true, Ordering::Relaxed);

                                        // Play session start sound
                                        if let Some(player) = &sound_player {
                                            player.play(crate::sound_generator::SoundType::SessionStart);
                                        }

                                        if let Some(responder) = responder.take() {
                                            let _ = responder.send(Ok(session_id));
                                        }
                                    } else {
                                        eprintln!("Cannot start manual session when not in manual mode");
                                        if let Some(responder) = responder.take() {
                                            let _ = responder.send(Err(anyhow::anyhow!(
                                                "Cannot start manual session when not in manual mode"
                                            )));
                                        }
                                    }
                                }
                                ManualSessionCommand::StopSession { mut responder } => {
                                    let current_mode = TranscriptionMode::from_u8(transcription_mode.load(Ordering::Relaxed));
                                    if current_mode == TranscriptionMode::Manual {
                                        // Move active session into processing state so a new one can begin immediately
                                        let session_id_opt = Self::move_session_to_processing(
                                            &current_manual_session,
                                            &processing_manual_session,
                                        );

                                        let session_id_opt = match session_id_opt {
                                            Some(session_id) => {
                                                // Stop the audio stream FIRST (no more audio captured)
                                                if let Err(e) = audio_capture.lock().stop_recording() {
                                                    eprintln!("Warning: Failed to stop audio recording: {}", e);
                                                }

                                                // Keep recording=true to allow channel to drain into manual_audio_buffer
                                                // Will be set to false inside async task after sleep

                                                Some(session_id)
                                            }
                                            None => {
                                                let processing_id =
                                                    Self::get_processing_session_id(&processing_manual_session);
                                                if let Some(session_id) = processing_id.clone() {
                                                    // Stop the audio stream FIRST
                                                    if let Err(e) = audio_capture.lock().stop_recording() {
                                                        eprintln!("Warning: Failed to stop audio recording: {}", e);
                                                    }

                                                    // Keep recording=true to allow channel to drain into manual_audio_buffer
                                                    // Will be set to false inside async task after sleep

                                                    Some(session_id)
                                                } else {
                                                    eprintln!("No active session to stop");
                                                    if let Some(responder) = responder.take() {
                                                        let _ = responder
                                                            .send(Err(anyhow::anyhow!("No active manual session to stop")));
                                                    }
                                                    None
                                                }
                                            }
                                        };

                                        if let Some(session_id) = session_id_opt.clone() {
                                            if let Some(audio_processor) = &audio_processor_ref {
                                                let audio_processor = audio_processor.clone();
                                                let processing_manual_session =
                                                    processing_manual_session.clone();
                                                let sound_player_for_task = sound_player.clone();
                                                let audio_viz_data = audio_visualization_data.clone();
                                                let captured_session_id = session_id.clone(); // Capture before spawning
                                                let recording_for_task = recording.clone();
                                                let audio_capture_for_drain = audio_capture.clone();
                                                let transcription_stats_for_drain = transcription_stats.clone();
                                                let finalizing_flag = finalizing_manual_session.clone();
                                                tokio::spawn(async move {
                                                    // Guard lives for the entire async task duration
                                                    let _guard = FinalizingGuard::new(finalizing_flag);

                                                    // Wait for channel to drain using sample counters
                                                    let sent_count = audio_capture_for_drain.lock().get_samples_sent_count();
                                                    let received_count = audio_processor.get_samples_received_count();

                                                    let start_time = tokio::time::Instant::now();
                                                    let max_wait = Duration::from_millis(2000); // 2-second timeout

                                                    loop {
                                                        let sent = sent_count.load(Ordering::Acquire);
                                                        let received = received_count.load(Ordering::Acquire);

                                                        if received >= sent {
                                                            println!("Channel drained: {} samples processed", received);
                                                            break;
                                                        }

                                                        if start_time.elapsed() > max_wait {
                                                            let lost = sent - received;
                                                            eprintln!(
                                                                "Warning: Channel drain timeout - {} samples still in flight",
                                                                lost
                                                            );
                                                            // Update stats
                                                            if let Some(mut stats) = transcription_stats_for_drain.try_lock() {
                                                                stats.record_audio_drop(lost as u64);
                                                            }
                                                            break;
                                                        }

                                                        tokio::time::sleep(Duration::from_millis(10)).await;
                                                    }

                                                    // NOW set recording=false after channel has drained
                                                    recording_for_task.store(false, Ordering::Relaxed);

                                                    // Drop the guard immediately after drain completes so new
                                                    // sessions can start while transcription is still running
                                                    drop(_guard);

                                                    // Set processing state to transcribing for manual session
                                                    {
                                                        let mut audio_data = audio_viz_data.write();
                                                        audio_data.set_processing_state(ProcessingState::Transcribing);
                                                    }

                                                    let transcription_result = audio_processor
                                                        .trigger_manual_transcription(sample_rate, Some(captured_session_id.clone()))
                                                        .await;

                                                    if let Err(e) = transcription_result {
                                                        eprintln!(
                                                            "Failed to trigger manual transcription for session {}: {}",
                                                            session_id, e
                                                        );
                                                        Self::mark_processing_session_failed(
                                                            &processing_manual_session,
                                                            &session_id,
                                                        );

                                                        // Set processing state to error on manual transcription failure
                                                        {
                                                            let mut audio_data = audio_viz_data.write();
                                                            audio_data.set_processing_state(ProcessingState::Error);
                                                        }
                                                    } else if Self::clear_processing_session_if_matches(
                                                        &processing_manual_session,
                                                        &session_id,
                                                    ) {
                                                        // Session completed successfully - play completion sound
                                                        if let Some(player) = &sound_player_for_task {
                                                            player.play(crate::sound_generator::SoundType::SessionComplete);
                                                        }

                                                        // Set processing state to completed on successful manual transcription
                                                        {
                                                            let mut audio_data = audio_viz_data.write();
                                                            audio_data.set_processing_state(ProcessingState::Completed);
                                                        }

                                                        // Brief delay then return to idle
                                                        tokio::time::sleep(Duration::from_millis(2000)).await;
                                                        {
                                                            let mut audio_data = audio_viz_data.write();
                                                            audio_data.set_processing_state(ProcessingState::Idle);
                                                        }
                                                    }
                                                });
                                                if let Some(responder) = responder.take() {
                                                    let _ = responder.send(Ok(()));
                                                }
                                            } else {
                                                eprintln!("Audio processor reference not available for manual transcription");
                                                Self::mark_processing_session_failed(
                                                    &processing_manual_session,
                                                    &session_id,
                                                );
                                                if let Some(responder) = responder.take() {
                                                    let _ = responder.send(Err(anyhow::anyhow!(
                                                        "Audio processor unavailable for manual transcription"
                                                    )));
                                                }
                                            }
                                        } else {
                                            if let Some(responder) = responder.take() {
                                                let _ = responder.send(Err(anyhow::anyhow!(
                                                    "No active manual session to stop"
                                                )));
                                            }
                                        }
                                    } else {
                                        eprintln!("Cannot stop manual session when not in manual mode");
                                        if let Some(responder) = responder.take() {
                                            let _ = responder.send(Err(anyhow::anyhow!(
                                                "Cannot stop manual session when not in manual mode"
                                            )));
                                        }
                                    }
                                }
                                ManualSessionCommand::CancelSession { mut responder } => {
                                    let current_mode = TranscriptionMode::from_u8(transcription_mode.load(Ordering::Relaxed));
                                    if current_mode == TranscriptionMode::Manual {
                                        let mut cancelled = false;

                                        {
                                            let mut session_lock = current_manual_session.lock();
                                            if let Some(_session) = session_lock.take() {
                                                cancelled = true;
                                            }
                                        }

                                        {
                                            let mut processing_lock = processing_manual_session.lock();
                                            if let Some(_processing) = processing_lock.take() {
                                                cancelled = true;
                                            }
                                        }

                                        if cancelled {
                                            // Stop stream first, then clear flag
                                            if let Err(e) = audio_capture.lock().stop_recording() {
                                                eprintln!("Warning: Failed to stop audio recording: {}", e);
                                            }

                                            recording.store(false, Ordering::Relaxed);

                                            // Play session cancel sound
                                            if let Some(player) = &sound_player {
                                                player.play(crate::sound_generator::SoundType::SessionCancel);
                                            }

                                            if let Some(responder) = responder.take() {
                                                let _ = responder.send(Ok(()));
                                            }
                                        } else {
                                            eprintln!("No active session to cancel");
                                            if let Some(responder) = responder.take() {
                                                let _ = responder.send(Err(anyhow::anyhow!(
                                                    "No manual session to cancel"
                                                )));
                                            }
                                        }
                                    } else {
                                        eprintln!("Cannot cancel manual session when not in manual mode");
                                        if let Some(responder) = responder.take() {
                                            let _ = responder.send(Err(anyhow::anyhow!(
                                                "Cannot cancel manual session when not in manual mode"
                                            )));
                                        }
                                    }
                                }
                                ManualSessionCommand::GetSessionStatus => {
                                    // This is a query command - status is returned via the getter methods
                                    // The UI can call get_manual_session_status() directly
                                }
                                ManualSessionCommand::SwitchMode(new_mode) => {
                                    let old_mode = TranscriptionMode::from_u8(transcription_mode.load(Ordering::Relaxed));
                                    transcription_mode.store(new_mode.as_u8(), Ordering::Relaxed);

                                    // The UI will detect this change and update the button layout automatically

                                    // Handle mode-specific cleanup
                                    match (old_mode, new_mode) {
                                        // Switching FROM RealTime TO Manual - stop realtime recording
                                        (TranscriptionMode::RealTime, TranscriptionMode::Manual) => {
                                            // Update session ID to None (will be set when session starts)
                                            if let Some(ref audio_processor) = audio_processor_ref {
                                                audio_processor.set_session_id(None);
                                                audio_processor.reset_vad_state();
                                                println!("Cleared session ID and reset VAD for manual mode");
                                            }

                                            // Stop the audio capture stream to ensure clean state
                                            if let Err(e) = audio_capture.lock().stop_recording() {
                                                eprintln!("Warning: Failed to stop audio recording during mode switch: {}", e);
                                            }

                                            recording.store(false, Ordering::Relaxed);

                                            // Clear visualization to show we're now in manual mode
                                            let mut audio_data = audio_visualization_data.write();
                                            audio_data.is_speaking = false;
                                        }
                                        // Switching FROM Manual TO RealTime - cancel any manual session and start realtime recording
                                        (TranscriptionMode::Manual, TranscriptionMode::RealTime) => {
                                            // Promote any active session into processing state
                                            let session_id_opt = Self::move_session_to_processing(
                                                &current_manual_session,
                                                &processing_manual_session,
                                            );

                                            // Clear manual audio buffer BEFORE spawning task
                                            if let Some(audio_processor) = &audio_processor_ref {
                                                audio_processor.clear_manual_buffer();
                                                audio_processor.reset_vad_state();
                                            }

                                            if let Some(audio_processor) = &audio_processor_ref {
                                                if let Some(session_id) = session_id_opt {
                                                    let audio_processor = audio_processor.clone();
                                                    let processing_manual_session = processing_manual_session.clone();
                                                    let captured_session_id = session_id.clone(); // Capture before spawning
                                                    tokio::spawn(async move {
                                                        // Wait for any in-flight audio samples to be processed
                                                        tokio::time::sleep(Duration::from_millis(100)).await;

                                                        if let Err(e) =
                                                            audio_processor.trigger_manual_transcription(sample_rate, Some(captured_session_id.clone())).await
                                                        {
                                                            eprintln!(
                                                                "Failed to process final manual session during mode switch: {}",
                                                                e
                                                            );
                                                            Self::mark_processing_session_failed(
                                                                &processing_manual_session,
                                                                &session_id,
                                                            );
                                                        } else if Self::clear_processing_session_if_matches(
                                                            &processing_manual_session,
                                                            &session_id,
                                                        ) {
                                                            // Session completed during mode switch
                                                        }

                                                        // Update to realtime AFTER transcription completes
                                                        audio_processor.set_session_id(Some("realtime".to_string()));
                                                        println!("Set session ID to: realtime (after manual transcription completed)");
                                                    });
                                                } else {
                                                    // No pending session, update immediately
                                                    audio_processor.set_session_id(Some("realtime".to_string()));
                                                    println!("Set session ID to: realtime");
                                                }
                                            }

                                            // RealTime mode should start recording by default
                                            // First ensure clean state by stopping any existing stream
                                            if let Err(e) = audio_capture.lock().stop_recording() {
                                                eprintln!("Warning: Failed to stop audio recording during mode switch: {}", e);
                                            }

                                            // Now start recording in RealTime mode - stream first, then flag
                                            if let Err(e) = audio_capture.lock().start_recording() {
                                                eprintln!("Warning: Failed to start audio recording for RealTime mode: {}", e);
                                            }
                                            recording.store(true, Ordering::Relaxed);
                                        }
                                        // Same mode - no action needed
                                        _ => {}
                                    }
                                }
                            }
                        } else {
                            // Channel closed, exit loop
                            break;
                        }
                    }
                    _ = tokio::time::sleep(Duration::from_millis(100)) => {
                        // Periodic check for running flag
                        if !running.load(Ordering::Relaxed) {
                            break;
                        }
                    }
                }
            }
        });
    }

    fn move_session_to_processing(
        current_manual_session: &Arc<Mutex<Option<ManualSession>>>,
        processing_manual_session: &Arc<Mutex<Option<ManualProcessingSession>>>,
    ) -> Option<String> {
        let mut session_lock = current_manual_session.lock();
        if let Some(session) = session_lock.take() {
            let session_id = session.session_id.clone();
            let mut processing_lock = processing_manual_session.lock();
            let _ = processing_lock.replace(ManualProcessingSession::new(session));
            Some(session_id)
        } else {
            None
        }
    }

    fn get_processing_session_id(
        processing_manual_session: &Arc<Mutex<Option<ManualProcessingSession>>>,
    ) -> Option<String> {
        processing_manual_session
            .lock()
            .as_ref()
            .map(|processing| processing.session_id().to_string())
    }

    fn clear_processing_session_if_matches(
        processing_manual_session: &Arc<Mutex<Option<ManualProcessingSession>>>,
        session_id: &str,
    ) -> bool {
        let mut processing_lock = processing_manual_session.lock();
        if processing_lock
            .as_ref()
            .map(|processing| processing.session_id() == session_id)
            .unwrap_or(false)
        {
            processing_lock.take();
            true
        } else {
            false
        }
    }

    fn mark_processing_session_failed(
        processing_manual_session: &Arc<Mutex<Option<ManualProcessingSession>>>,
        session_id: &str,
    ) {
        let mut processing_lock = processing_manual_session.lock();
        if let Some(processing) = processing_lock.as_mut() {
            if processing.session_id() == session_id {
                processing.session.is_processing = false;
            }
        }
    }

    /// Starts a monitoring task that watches for recording state changes
    /// and manages the audio stream accordingly
    fn start_recording_monitor(&mut self) {
        let running = self.running.clone();
        let recording = self.recording.clone();

        // We need a way to communicate with the audio capture from the monitoring task
        // For now, we'll create a simple polling mechanism
        tokio::spawn(async move {
            let mut last_recording_state = false;
            let mut check_interval = tokio::time::interval(tokio::time::Duration::from_millis(100));

            while running.load(Ordering::Relaxed) {
                check_interval.tick().await;

                let current_recording_state = recording.load(Ordering::Relaxed);

                // If recording state changed, we need to log it
                // The actual audio stream management is now handled by the audio capture callback
                if current_recording_state != last_recording_state {
                    println!(
                        "Recording state changed: {} -> {}",
                        last_recording_state, current_recording_state
                    );
                    last_recording_state = current_recording_state;
                }
            }

            println!("Recording monitor task stopped");
        });
    }

    /// Stops the audio capture and transcription process
    ///
    /// Terminates all audio processing and releases resources
    ///
    /// # Returns
    /// Result indicating success or an error with detailed message
    pub async fn stop(&mut self) -> Result<(), anyhow::Error> {
        // Stop the audio stream to save CPU when not recording
        if let Err(e) = self.audio_capture.lock().stop_recording() {
            eprintln!("Warning: Failed to stop audio recording: {}", e);
        }

        self.recording.store(false, Ordering::Relaxed);

        Ok(())
    }

    /// Resumes audio processing after it has been stopped
    ///
    /// # Returns
    /// Result indicating success or error
    pub async fn resume(&mut self) -> Result<(), anyhow::Error> {
        if let Err(e) = self.audio_capture.lock().resume() {
            eprintln!("Failed to resume audio capture: {}", e);
            // Even if resume fails, we proceed since state is now "recording"
        }

        self.recording.store(true, Ordering::Relaxed);
        Ok(())
    }

    /// Completely shuts down the audio capture and transcription process
    ///
    /// Terminates all audio processing and releases resources
    ///
    /// # Returns
    /// Result indicating success or an error with detailed message
    pub async fn shutdown(&mut self) -> Result<(), anyhow::Error> {
        println!("Shutting down transcriber...");
        self.running.store(false, Ordering::Relaxed);
        self.recording.store(false, Ordering::Relaxed);

        // Shutdown backend manager
        if let Some(tx) = self.backend_command_tx.take() {
            let _ = tx.send(BackendCommand::Shutdown);
        }

        // Stop audio capture
        self.audio_capture.lock().stop();

        let shutdown_timeout = Duration::from_secs(3);

        // Wait for the audio processor to finish
        if let Some(handle) = self.audio_handle.take() {
            match tokio::time::timeout(shutdown_timeout, handle).await {
                Ok(Err(e)) => eprintln!("Audio processor task panicked: {:?}", e),
                Err(_) => eprintln!("Audio processor shutdown timed out"),
                _ => {}
            }
        }

        // Wait for the transcription processor to finish
        if let Some(handle) = self.transcription_handle.take() {
            match tokio::time::timeout(shutdown_timeout, handle).await {
                Ok(Err(e)) => eprintln!("Transcription processor task panicked: {:?}", e),
                Err(_) => eprintln!("Transcription processor shutdown timed out"),
                _ => {}
            }
        }

        // Clean up whisper model
        *self.backend.lock() = None;

        println!("Transcriber shut down successfully.");
        Ok(())
    }

    /// Toggles the recording state between active and paused
    /// This method is completely non-blocking - audio threads detect the change asynchronously
    pub fn toggle_recording(&mut self) {
        let was_recording = self.recording.load(Ordering::Relaxed);
        let new_state = !was_recording;

        // Control audio stream with correct ordering to prevent dropping audio
        if was_recording {
            // Stopping: stop stream first, then clear flag
            if let Err(e) = self.audio_capture.lock().stop_recording() {
                eprintln!("Warning: Failed to stop audio recording: {}", e);
            }
            self.recording.store(false, Ordering::Relaxed);
        } else {
            // Starting: start stream first, then set flag
            if let Err(e) = self.audio_capture.lock().start_recording() {
                eprintln!("Warning: Failed to start audio recording: {}", e);
            }
            self.recording.store(true, Ordering::Relaxed);
        }

        println!("Recording toggled: {} -> {}", was_recording, new_state);

        // Play sound feedback
        if let Some(sound_player) = &self.sound_player {
            if new_state {
                sound_player.play(crate::sound_generator::SoundType::RecordStart);
            } else {
                sound_player.play(crate::sound_generator::SoundType::RecordStop);
            }
        }

        // All transcription processing will detect the atomic state change via polling
        // UI remains responsive while audio/transcription systems adapt asynchronously
    }

    /// Returns the current transcript history
    ///
    /// # Returns
    /// A string containing all transcribed text so far
    pub fn get_transcript(&self) -> String {
        match self.transcript_history.try_read() {
            Some(history) => history.clone(),
            None => self.transcript_history.read().clone(),
        }
    }

    /// Returns the transcription statistics
    ///
    /// # Returns
    /// A formatted string containing transcription performance statistics
    pub fn get_stats_report(&self) -> String {
        match self.transcription_stats.try_lock() {
            Some(stats) => stats.report(),
            None => self.transcription_stats.lock().report(),
        }
    }

    /// Prints the current transcription statistics to console
    ///
    /// Useful for debugging or on-demand performance reporting
    pub fn print_stats(&self) {
        if let Some(stats_reporter) = &self.stats_reporter {
            stats_reporter.print_stats();
        }
    }

    /// Get the audio visualization data reference
    pub fn get_audio_visualization_data(&self) -> Arc<RwLock<AudioVisualizationData>> {
        self.audio_visualization_data.clone()
    }

    /// Get the running state reference
    pub fn get_running(&self) -> Arc<AtomicBool> {
        self.running.clone()
    }

    /// Get the recording state reference
    pub fn get_recording(&self) -> Arc<AtomicBool> {
        self.recording.clone()
    }

    /// Get the magic mode (LFM enhancement) state reference
    pub fn get_magic_mode_enabled(&self) -> Arc<AtomicBool> {
        self.magic_mode_enabled.clone()
    }

    /// Get the transcript history reference
    pub fn get_transcript_history(&self) -> Arc<RwLock<String>> {
        self.transcript_history.clone()
    }

    /// Get the transcript receiver for listening to new transcriptions
    pub fn get_transcript_rx(&self) -> broadcast::Receiver<TranscriptionMessage> {
        self.transcript_tx.subscribe()
    }

    /// Get the current transcription mode
    pub fn get_transcription_mode(&self) -> TranscriptionMode {
        TranscriptionMode::from_u8(self.transcription_mode.load(Ordering::Relaxed))
    }

    /// Set processing state in the audio visualization data
    pub fn set_processing_state(&self, state: ProcessingState) {
        let mut audio_data = self.audio_visualization_data.write();
        audio_data.set_processing_state(state);
    }

    pub fn get_manual_session_sender(&self) -> mpsc::Sender<ManualSessionCommand> {
        self.manual_session_tx.clone()
    }

    pub fn get_transcription_mode_ref(&self) -> Arc<AtomicU8> {
        self.transcription_mode.clone()
    }

    /// Get audio processor reference for direct control
    pub fn get_audio_processor(&self) -> Option<Arc<AudioProcessor>> {
        self.audio_processor_ref.clone()
    }

    /// Get the shared backend status reference
    pub fn get_backend_status(&self) -> Arc<RwLock<BackendStatus>> {
        self.backend_status.clone()
    }

    /// Get a command sender for backend operations (reload, shutdown)
    pub fn backend_command_sender(&self) -> Option<mpsc::UnboundedSender<BackendCommand>> {
        self.backend_command_tx.clone()
    }

    /// Set the transcription mode
    pub fn set_transcription_mode(&mut self, mode: TranscriptionMode) {
        self.transcription_mode
            .store(mode.as_u8(), Ordering::Relaxed);
        println!("Transcription mode changed to: {:?}", mode);
    }

    /// Start a new manual transcription session
    pub async fn start_manual_session(&self) -> Result<String, anyhow::Error> {
        if TranscriptionMode::from_u8(self.transcription_mode.load(Ordering::Relaxed))
            != TranscriptionMode::Manual
        {
            return Err(anyhow::anyhow!(
                "Cannot start manual session when not in manual mode"
            ));
        }

        let (tx, rx) = oneshot::channel();
        self.manual_session_tx
            .send(ManualSessionCommand::StartSession {
                responder: Some(tx),
            })
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send start session command: {}", e))?;

        rx.await
            .map_err(|_| anyhow::anyhow!("Start session response channel closed"))?
    }

    /// Stop the current manual transcription session and trigger processing
    pub async fn stop_manual_session(&self) -> Result<(), anyhow::Error> {
        if TranscriptionMode::from_u8(self.transcription_mode.load(Ordering::Relaxed))
            != TranscriptionMode::Manual
        {
            return Err(anyhow::anyhow!(
                "Cannot stop manual session when not in manual mode"
            ));
        }

        let (tx, rx) = oneshot::channel();
        self.manual_session_tx
            .send(ManualSessionCommand::StopSession {
                responder: Some(tx),
            })
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send stop session command: {}", e))?;

        rx.await
            .map_err(|_| anyhow::anyhow!("Stop session response channel closed"))?
    }

    /// Cancel the current manual transcription session
    pub async fn cancel_manual_session(&self) -> Result<(), anyhow::Error> {
        if TranscriptionMode::from_u8(self.transcription_mode.load(Ordering::Relaxed))
            != TranscriptionMode::Manual
        {
            return Err(anyhow::anyhow!(
                "Cannot cancel manual session when not in manual mode"
            ));
        }

        let (tx, rx) = oneshot::channel();
        self.manual_session_tx
            .send(ManualSessionCommand::CancelSession {
                responder: Some(tx),
            })
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send cancel session command: {}", e))?;

        rx.await
            .map_err(|_| anyhow::anyhow!("Cancel session response channel closed"))?
    }

    /// Get the status of the current manual session
    pub fn get_manual_session_status(&self) -> Option<ManualSessionStatus> {
        if TranscriptionMode::from_u8(self.transcription_mode.load(Ordering::Relaxed))
            != TranscriptionMode::Manual
        {
            return None;
        }

        {
            let current_session = self.current_manual_session.lock();
            if let Some(session) = current_session.as_ref() {
                return Some(session.get_status());
            }
        }

        let processing_session = self.processing_manual_session.lock();
        processing_session
            .as_ref()
            .map(|session| session.get_status())
    }

    /// Check if there's an active manual session
    pub fn has_active_manual_session(&self) -> bool {
        if TranscriptionMode::from_u8(self.transcription_mode.load(Ordering::Relaxed))
            != TranscriptionMode::Manual
        {
            return false;
        }

        {
            let current_session = self.current_manual_session.lock();
            if let Some(session) = current_session.as_ref() {
                if session.is_recording || session.is_processing {
                    return true;
                }
            }
        }

        self.processing_manual_session.lock().is_some()
    }

    /// Get the current manual session accumulated audio for processing
    pub fn get_manual_session_audio(&self) -> Option<Vec<f32>> {
        if TranscriptionMode::from_u8(self.transcription_mode.load(Ordering::Relaxed))
            != TranscriptionMode::Manual
        {
            return None;
        }

        let current_session = self.current_manual_session.lock();
        current_session
            .as_ref()
            .map(|session| session.accumulated_audio.clone())
    }

    /// Add audio data to the current manual session
    pub fn add_audio_to_manual_session(&self, audio_data: &[f32]) -> Result<(), anyhow::Error> {
        if TranscriptionMode::from_u8(self.transcription_mode.load(Ordering::Relaxed))
            != TranscriptionMode::Manual
        {
            return Ok(()); // Silently ignore if not in manual mode
        }

        let mut current_session = self.current_manual_session.lock();
        if let Some(session) = current_session.as_mut() {
            if session.is_recording {
                // Check if session has expired
                if session.is_expired() {
                    let session_id = session.session_id.clone();
                    session.is_recording = false;
                    session.is_processing = true;
                    println!(
                        "Manual session {} has reached maximum duration, stopping recording",
                        session_id
                    );

                    // Complete the session immediately (mark as done and clear it)
                    println!("Manual session {} completed and cleared", session_id);
                    *current_session = None; // Clear the session so new ones can start

                    // Stop recording
                    self.recording.store(false, Ordering::Relaxed);
                } else {
                    // Add audio data to accumulated buffer
                    session.accumulated_audio.extend_from_slice(audio_data);
                }
            }
        }

        Ok(())
    }

    /// Mark the current manual session as completed
    pub fn complete_manual_session(&self) -> Result<(), anyhow::Error> {
        {
            let mut current_session = self.current_manual_session.lock();
            if let Some(session) = current_session.as_mut() {
                session.is_processing = false;
                return Ok(());
            }
        }

        {
            let mut processing_session = self.processing_manual_session.lock();
            if let Some(session) = processing_session.as_mut() {
                session.session.is_processing = false;
            }
        }

        Ok(())
    }
}
