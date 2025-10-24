use anyhow::Context;
use chrono::Utc;
use parking_lot::{Mutex, RwLock};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc, oneshot};

// Use local modules
use crate::audio_capture::AudioCapture;
use crate::audio_processor::AudioProcessor;
use crate::backend::{create_backend, TranscriptionBackend};
use crate::config::{read_app_config, AppConfig};
use crate::silero_audio_processor::{AudioSegment, SileroVad};
use crate::stats_reporter::StatsReporter;
use crate::transcription_processor::TranscriptionProcessor;
use crate::transcription_stats::TranscriptionStats;
use crate::ui::common::AudioVisualizationData;

/// Transcription mode enumeration
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TranscriptionMode {
    /// Continuous real-time transcription (existing behavior)
    RealTime,
    /// On-demand manual transcription sessions
    Manual,
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

/// Main transcription coordinator that integrates all components
pub struct RealTimeTranscriber {
    // Audio capture (wrapped in Arc<Mutex> for sharing with command processor)
    audio_capture: Arc<Mutex<AudioCapture>>,

    // Audio processing
    tx: mpsc::Sender<Vec<f32>>,
    rx: Option<mpsc::Receiver<Vec<f32>>>,

    // Transcription
    pub transcript_tx: broadcast::Sender<String>,
    pub transcript_rx: broadcast::Receiver<String>,

    // State control
    running: Arc<AtomicBool>,
    recording: Arc<AtomicBool>,

    // Model and parameters
    backend: Arc<Mutex<Option<TranscriptionBackend>>>,
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
    transcription_done_rx: Option<mpsc::UnboundedReceiver<()>>,

    // Statistics
    transcription_stats: Arc<Mutex<TranscriptionStats>>,
    stats_reporter: Option<StatsReporter>,

    // Task handles for graceful shutdown
    transcription_handle: Option<tokio::task::JoinHandle<()>>,
    audio_handle: Option<tokio::task::JoinHandle<()>>,

    // Audio processor reference for manual transcription
    audio_processor_ref: Option<Arc<crate::audio_processor::AudioProcessor>>,

    // Manual mode specific fields
    transcription_mode: Arc<Mutex<TranscriptionMode>>,
    current_manual_session: Arc<Mutex<Option<ManualSession>>>,
    processing_manual_session: Arc<Mutex<Option<ManualProcessingSession>>>,
    manual_session_tx: mpsc::Sender<ManualSessionCommand>,
    manual_session_rx: Option<mpsc::Receiver<ManualSessionCommand>>,

    // Sound effects
    sound_player: Option<Arc<crate::sound_player::SoundPlayer>>,
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

        let audio_visualization_data = Arc::new(RwLock::new(AudioVisualizationData {
            samples: Vec::new(),
            is_speaking: false,
            transcript: String::new(),
            reset_requested: false,
        }));

        let audio_processor = match SileroVad::new(
            (
                app_config.vad_config.clone(),
                app_config.audio_processor_config.buffer_size,
                app_config.audio_processor_config.sample_rate,
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
        let model_path_clone = model_path.clone();
        let backend_type = app_config.backend_config.backend.clone();
        let backend_config = app_config.backend_config.clone();

        tokio::spawn(async move {
            println!(
                "INFO: Loading {} backend with model at {:?}",
                backend_type, model_path_clone
            );
            println!(
                "Backend config: threads={}, gpu_enabled={}, quantization={:?}",
                backend_config.threads, backend_config.gpu_enabled, backend_config.quantization_level
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
                    *backend_clone.lock() = Some(b);
                }
                Err(e) => {
                    eprintln!("ERROR: Failed to load backend: {}", e);
                }
            }
        });

        // Initialize transcription mode from config
        let transcription_mode = Arc::new(Mutex::new(TranscriptionMode::from(
            app_config.general_config.transcription_mode.as_str(),
        )));
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
            language: app_config.general_config.language.clone(),
            audio_processor,
            transcript_history,
            audio_visualization_data,
            segment_tx,
            segment_rx: Some(segment_rx),
            transcription_done_tx,
            transcription_done_rx: Some(transcription_done_rx),
            transcription_stats,
            stats_reporter: None,
            transcription_handle: None,
            audio_handle: None,
            audio_processor_ref: None,

            // Manual mode fields
            transcription_mode,
            current_manual_session,
            processing_manual_session,
            manual_session_tx,
            manual_session_rx: Some(manual_session_rx),

            // Sound effects
            sound_player,
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
            self.language.clone(),
            self.running.clone(),
            self.transcription_done_tx.clone(),
            self.transcription_stats.clone(),
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
        let transcription_mode = self.transcription_mode.clone(); // Shared reference
        let running = self.running.clone();
        let recording = self.recording.clone();
        let transcript_history = self.transcript_history.clone();
        let audio_visualization_data = self.audio_visualization_data.clone();
        let audio_processor_ref = self.audio_processor_ref.clone();
        let audio_capture = self.audio_capture.clone(); // Share audio capture for stream control
        let sound_player = self.sound_player.clone(); // Clone sound player for audio feedback
        let app_config = read_app_config();
        let manual_mode_config = app_config.manual_mode_config.clone();
        let sample_rate = app_config.audio_processor_config.sample_rate;

        tokio::spawn(async move {
            while running.load(Ordering::Relaxed) {
                tokio::select! {
                    command = manual_session_rx.recv() => {
                        if let Some(cmd) = command {
                            match cmd {
                                ManualSessionCommand::StartSession { mut responder } => {
                                    let current_mode = *transcription_mode.lock();
                                    if current_mode == TranscriptionMode::Manual {
                                        let max_duration = manual_mode_config.max_recording_duration_secs;
                                        let new_session = ManualSession::new(max_duration);
                                        let session_id = new_session.session_id.clone();

                                        let can_start = {
                                            let mut session_lock = current_manual_session.lock();
                                            if let Some(existing) = session_lock.as_ref() {
                                                if existing.is_recording {
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

                                        // Clear transcript if configured to do so
                                        if manual_mode_config.clear_on_new_session {
                                            let mut transcript_history_lock = transcript_history.write();
                                            transcript_history_lock.clear();

                                            let mut audio_data = audio_visualization_data.write();
                                            audio_data.transcript.clear();
                                            audio_data.reset_requested = true;
                                        }

                                        recording.store(true, Ordering::Relaxed);

                                        // Start the audio capture stream
                                        if let Err(e) = audio_capture.lock().start_recording() {
                                            eprintln!("Warning: Failed to start audio recording: {}", e);
                                        }

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
                                    let current_mode = *transcription_mode.lock();
                                    if current_mode == TranscriptionMode::Manual {
                                        // Move active session into processing state so a new one can begin immediately
                                        let session_id_opt = Self::move_session_to_processing(
                                            &current_manual_session,
                                            &processing_manual_session,
                                        );

                                        let session_id_opt = match session_id_opt {
                                            Some(session_id) => {
                                                recording.store(false, Ordering::Relaxed);

                                                if let Err(e) = audio_capture.lock().stop_recording() {
                                                    eprintln!("Warning: Failed to stop audio recording: {}", e);
                                                }

                                                Some(session_id)
                                            }
                                            None => {
                                                let processing_id =
                                                    Self::get_processing_session_id(&processing_manual_session);
                                                if let Some(session_id) = processing_id.clone() {
                                                    recording.store(false, Ordering::Relaxed);

                                                    if let Err(e) = audio_capture.lock().stop_recording() {
                                                        eprintln!("Warning: Failed to stop audio recording: {}", e);
                                                    }

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
                                                tokio::spawn(async move {
                                                    if let Err(e) = audio_processor
                                                        .trigger_manual_transcription(sample_rate)
                                                        .await
                                                    {
                                                        eprintln!(
                                                            "Failed to trigger manual transcription for session {}: {}",
                                                            session_id, e
                                                        );
                                                        Self::mark_processing_session_failed(
                                                            &processing_manual_session,
                                                            &session_id,
                                                        );
                                                    } else if Self::clear_processing_session_if_matches(
                                                        &processing_manual_session,
                                                        &session_id,
                                                    ) {
                                                        // Session completed successfully - play completion sound
                                                        if let Some(player) = &sound_player_for_task {
                                                            player.play(crate::sound_generator::SoundType::SessionComplete);
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
                                        } else if let Some(responder) = responder.take() {
                                            let _ = responder.send(Err(anyhow::anyhow!(
                                                "No active manual session to stop"
                                            )));
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
                                    let current_mode = *transcription_mode.lock();
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
                                            recording.store(false, Ordering::Relaxed);

                                            if let Err(e) = audio_capture.lock().stop_recording() {
                                                eprintln!("Warning: Failed to stop audio recording: {}", e);
                                            }

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
                                    let old_mode = *transcription_mode.lock();
                                    *transcription_mode.lock() = new_mode;

                                    // The UI will detect this change and update the button layout automatically

                                    // Handle mode-specific cleanup
                                    match (old_mode, new_mode) {
                                        // Switching FROM RealTime TO Manual - stop realtime recording
                                        (TranscriptionMode::RealTime, TranscriptionMode::Manual) => {
                                            recording.store(false, Ordering::Relaxed);

                                            // Stop the audio capture stream to ensure clean state
                                            if let Err(e) = audio_capture.lock().stop_recording() {
                                                eprintln!("Warning: Failed to stop audio recording during mode switch: {}", e);
                                            }

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

                                            if let Some(audio_processor) = &audio_processor_ref {
                                                if let Some(session_id) = session_id_opt {
                                                    let audio_processor = audio_processor.clone();
                                                    let processing_manual_session = processing_manual_session.clone();
                                                    tokio::spawn(async move {
                                                        if let Err(e) =
                                                            audio_processor.trigger_manual_transcription(sample_rate).await
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
                                                    });
                                                }
                                            }

                                            // Clear manual audio buffer
                                            if let Some(audio_processor) = &audio_processor_ref {
                                                audio_processor.clear_manual_buffer();
                                            }

                                            // RealTime mode should start recording by default
                                            // First ensure clean state by stopping any existing stream
                                            if let Err(e) = audio_capture.lock().stop_recording() {
                                                eprintln!("Warning: Failed to stop audio recording during mode switch: {}", e);
                                            }

                                            // Now start recording in RealTime mode
                                            recording.store(true, Ordering::Relaxed);
                                            if let Err(e) = audio_capture.lock().start_recording() {
                                                eprintln!("Warning: Failed to start audio recording for RealTime mode: {}", e);
                                            }
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
        self.recording.store(false, Ordering::Relaxed);

        // Stop the audio stream to save CPU when not recording
        if let Err(e) = self.audio_capture.lock().stop_recording() {
            eprintln!("Warning: Failed to stop audio recording: {}", e);
        }

        Ok(())
    }

    /// Resumes audio processing after it has been stopped
    ///
    /// # Returns
    /// Result indicating success or error
    pub async fn resume(&mut self) -> Result<(), anyhow::Error> {
        self.recording.store(true, Ordering::Relaxed);

        if let Err(e) = self.audio_capture.lock().resume() {
            eprintln!("Failed to resume audio capture: {}", e);
            // Even if resume fails, we proceed since state is now "recording"
        }
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

        // Stop audio capture
        self.audio_capture.lock().stop();

        // Wait for the audio processor to finish
        if let Some(handle) = self.audio_handle.take() {
            if let Err(e) = handle.await {
                eprintln!("Audio processor task panicked: {:?}", e);
            }
        }

        // Wait for the transcription processor to finish
        if let Some(handle) = self.transcription_handle.take() {
            if let Err(e) = handle.await {
                eprintln!("Transcription processor task panicked: {:?}", e);
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
        // IMMEDIATE: Atomic state toggle (non-blocking)
        let was_recording = self.recording.load(Ordering::Relaxed);
        let new_state = !was_recording;
        self.recording.store(new_state, Ordering::Relaxed);

        println!(
            "Recording toggled atomically: {} -> {} (transcription threads will detect change)",
            was_recording, new_state
        );

        // Play sound feedback
        if let Some(sound_player) = &self.sound_player {
            if new_state {
                sound_player.play(crate::sound_generator::SoundType::RecordStart);
            } else {
                sound_player.play(crate::sound_generator::SoundType::RecordStop);
            }
        }

        // ASYNC: Control audio stream in background task to avoid blocking
        if was_recording {
            // We were recording, now stopping - stop the stream to save CPU
            if let Err(e) = self.audio_capture.lock().stop_recording() {
                eprintln!("Warning: Failed to stop audio recording: {}", e);
            }
        } else {
            // We were stopped, now recording - start the stream
            if let Err(e) = self.audio_capture.lock().start_recording() {
                eprintln!("Warning: Failed to start audio recording: {}", e);
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

    /// Get the transcript history reference
    pub fn get_transcript_history(&self) -> Arc<RwLock<String>> {
        self.transcript_history.clone()
    }

    /// Get the transcript receiver for listening to new transcriptions
    pub fn get_transcript_rx(&self) -> broadcast::Receiver<String> {
        self.transcript_tx.subscribe()
    }

    /// Get the current transcription mode
    pub fn get_transcription_mode(&self) -> TranscriptionMode {
        *self.transcription_mode.lock()
    }

    pub fn get_manual_session_sender(&self) -> mpsc::Sender<ManualSessionCommand> {
        self.manual_session_tx.clone()
    }

    pub fn get_transcription_mode_ref(&self) -> Arc<Mutex<TranscriptionMode>> {
        self.transcription_mode.clone()
    }

    /// Set the transcription mode
    pub fn set_transcription_mode(&mut self, mode: TranscriptionMode) {
        *self.transcription_mode.lock() = mode;
        println!("Transcription mode changed to: {:?}", mode);
    }

    /// Start a new manual transcription session
    pub async fn start_manual_session(&self) -> Result<String, anyhow::Error> {
        if *self.transcription_mode.lock() != TranscriptionMode::Manual {
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
        if *self.transcription_mode.lock() != TranscriptionMode::Manual {
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
        if *self.transcription_mode.lock() != TranscriptionMode::Manual {
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
        if *self.transcription_mode.lock() != TranscriptionMode::Manual {
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
        if *self.transcription_mode.lock() != TranscriptionMode::Manual {
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
        if *self.transcription_mode.lock() != TranscriptionMode::Manual {
            return None;
        }

        let current_session = self.current_manual_session.lock();
        current_session
            .as_ref()
            .map(|session| session.accumulated_audio.clone())
    }

    /// Add audio data to the current manual session
    pub fn add_audio_to_manual_session(&self, audio_data: &[f32]) -> Result<(), anyhow::Error> {
        if *self.transcription_mode.lock() != TranscriptionMode::Manual {
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
