use anyhow::Context;
use chrono::{DateTime, Utc};
use ct2rs::{ComputeType, Config, Device, Whisper, WhisperOptions};
use parking_lot::{Mutex, RwLock};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc};

// Use local modules
use crate::audio_capture::AudioCapture;
use crate::audio_processor::AudioProcessor;
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

/// Commands for manual session management
#[derive(Debug)]
pub enum ManualSessionCommand {
    StartSession,
    StopSession,
    CancelSession,
    GetSessionStatus,
    SwitchMode(TranscriptionMode),
}

/// Main transcription coordinator that integrates all components
pub struct RealTimeTranscriber {
    // Audio capture
    audio_capture: AudioCapture,

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
    whisper: Arc<Mutex<Option<Whisper>>>,
    language: String,
    options: WhisperOptions,

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
    manual_session_tx: mpsc::Sender<ManualSessionCommand>,
    manual_session_rx: Option<mpsc::Receiver<ManualSessionCommand>>,
}

impl RealTimeTranscriber {
    /// Creates a new RealTimeTranscriber instance
    ///
    /// # Arguments
    /// * `model_path` - Path to the Whisper model file
    /// * `app_config` - Application configuration
    ///
    /// # Returns
    /// Result containing the new instance or an error
    pub fn new(model_path: PathBuf, app_config: AppConfig) -> Result<Self, anyhow::Error> {
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
        let whisper = Arc::new(Mutex::new(None));
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
                app_config.buffer_size,
                app_config.sample_rate,
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

        let whisper_clone = whisper.clone();
        let model_path_clone = model_path.clone();
        let options = app_config.whisper_options.to_whisper_options();

        tokio::spawn(async move {
            let mut config = Config::default();
            let app_config = read_app_config(); // Read config once

            // Always use CPU for inference
            config.device = Device::CPU;
            println!("INFO: Loading model on CPU.");

            // Use configured compute type for CPU
            let compute_type_str = app_config.compute_type.as_str();
            config.compute_type = match compute_type_str {
                "FLOAT16" => ComputeType::FLOAT16,
                "INT8" => ComputeType::INT8,
                _ => ComputeType::DEFAULT, // Safe fallback
            };

            // Configure CPU threads
            let cpu_cores = std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4);
            config.num_threads_per_replica = (cpu_cores / 2).max(2).min(4);

            println!(
                "Attempting to load Whisper model with config: Device={:?}, ComputeType={:?}, CPU threads={}",
                config.device, config.compute_type, config.num_threads_per_replica
            );

            match Whisper::new(&model_path_clone, config) {
                Ok(w) => {
                    println!("Whisper model loaded successfully!");
                    *whisper_clone.lock() = Some(w);
                }
                Err(e) => {
                    eprintln!("ERROR: Failed to load Whisper model: {}", e);
                }
            }
        });

        // Initialize transcription mode from config
        let transcription_mode = Arc::new(Mutex::new(TranscriptionMode::from(
            app_config.transcription_mode.as_str(),
        )));
        let current_manual_session = Arc::new(Mutex::new(None));

        Ok(Self {
            audio_capture: AudioCapture::new(),
            tx,
            rx: Some(rx),
            transcript_tx,
            transcript_rx,
            running,
            recording,
            whisper,
            language: app_config.language,
            options,
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
            manual_session_tx,
            manual_session_rx: Some(manual_session_rx),
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
        self.audio_capture.start(
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
            self.whisper.clone(),
            self.language.clone(),
            self.options.clone(),
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
        let transcription_mode = self.transcription_mode.clone(); // Shared reference
        let running = self.running.clone();
        let recording = self.recording.clone();
        let transcript_history = self.transcript_history.clone();
        let audio_visualization_data = self.audio_visualization_data.clone();
        let audio_processor_ref = self.audio_processor_ref.clone();

        tokio::spawn(async move {
            while running.load(Ordering::Relaxed) {
                tokio::select! {
                    command = manual_session_rx.recv() => {
                        if let Some(cmd) = command {
                            match cmd {
                                ManualSessionCommand::StartSession => {
                                    let current_mode = *transcription_mode.lock();
                                    if current_mode == TranscriptionMode::Manual {
                                        let mut session_lock = current_manual_session.lock();

                                        // Check if there's already an active session
                                        if let Some(session) = session_lock.as_ref() {
                                            if session.is_recording || session.is_processing {
                                                eprintln!("Cannot start new session: existing session {} is still active", session.session_id);
                                                continue;
                                            }
                                        }

                                        // Create new session
                                        let max_duration = read_app_config().manual_mode_config.max_recording_duration_secs;
                                        let new_session = ManualSession::new(max_duration);
                                        let session_id = new_session.session_id.clone();

                                        // Clear transcript if configured to do so
                                        if read_app_config().manual_mode_config.clear_on_new_session {
                                            let mut transcript_history_lock = transcript_history.write();
                                            transcript_history_lock.clear();

                                            let mut audio_data = audio_visualization_data.write();
                                            audio_data.transcript.clear();
                                            audio_data.reset_requested = true;
                                        }

                                        *session_lock = Some(new_session);
                                        recording.store(true, Ordering::Relaxed);
                                        println!("Started manual recording session: {}", session_id);
                                    } else {
                                        eprintln!("Cannot start manual session when not in manual mode");
                                    }
                                }
                                ManualSessionCommand::StopSession => {
                                    let current_mode = *transcription_mode.lock();
                                    if current_mode == TranscriptionMode::Manual {
                                        // Check and update session state first, then drop the lock
                                        let session_id_opt = {
                                            let mut session_lock = current_manual_session.lock();
                                            if let Some(session) = session_lock.as_mut() {
                                                if session.is_recording {
                                                    session.is_recording = false;
                                                    session.is_processing = true;
                                                    recording.store(false, Ordering::Relaxed);
                                                    println!("Stopped manual recording session: {}", session.session_id);
                                                    Some(session.session_id.clone())
                                                } else {
                                                    eprintln!("No active recording session to stop");
                                                    None
                                                }
                                            } else {
                                                eprintln!("No active session to stop");
                                                None
                                            }
                                        }; // session_lock is dropped here

                                        // Now trigger transcription without holding the lock
                                        if let Some(session_id) = session_id_opt {
                                            if let Some(audio_processor) = &audio_processor_ref {
                                                let sample_rate = read_app_config().sample_rate;
                                                match audio_processor.trigger_manual_transcription(sample_rate).await {
                                                    Ok(()) => {
                                                        println!("Manual session {} transcription triggered successfully", session_id);
                                                    }
                                                    Err(e) => {
                                                        eprintln!("Failed to trigger manual transcription for session {}: {}", session_id, e);
                                                    }
                                                }
                                            } else {
                                                eprintln!("Audio processor reference not available for manual transcription");
                                            }

                                            // Complete the session (mark as done and clear it)
                                            let mut session_lock = current_manual_session.lock();
                                            println!("Manual session {} completed and cleared", session_id);
                                            *session_lock = None; // Clear the session so new ones can start
                                        }
                                    } else {
                                        eprintln!("Cannot stop manual session when not in manual mode");
                                    }
                                }
                                ManualSessionCommand::CancelSession => {
                                    let current_mode = *transcription_mode.lock();
                                    if current_mode == TranscriptionMode::Manual {
                                        let mut session_lock = current_manual_session.lock();
                                        if let Some(session) = session_lock.as_ref() {
                                            println!("Cancelled manual session: {}", session.session_id);
                                            *session_lock = None;
                                            recording.store(false, Ordering::Relaxed);
                                        } else {
                                            eprintln!("No active session to cancel");
                                        }
                                    } else {
                                        eprintln!("Cannot cancel manual session when not in manual mode");
                                    }
                                }
                                ManualSessionCommand::GetSessionStatus => {
                                    // This is a query command - status is returned via the getter methods
                                    // The UI can call get_manual_session_status() directly
                                }
                                ManualSessionCommand::SwitchMode(new_mode) => {
                                    let old_mode = *transcription_mode.lock();
                                    println!("Switching transcription mode from {:?} to {:?}", old_mode, new_mode);
                                    *transcription_mode.lock() = new_mode;

                                    // The UI will detect this change and update the button layout automatically

                                    // Handle mode-specific cleanup
                                    match (old_mode, new_mode) {
                                        // Switching FROM RealTime TO Manual - stop realtime recording
                                        (TranscriptionMode::RealTime, TranscriptionMode::Manual) => {
                                            println!("Stopping realtime recording for manual mode switch");
                                            recording.store(false, Ordering::Relaxed);

                                            // Clear visualization to show we're now in manual mode
                                            let mut audio_data = audio_visualization_data.write();
                                            audio_data.is_speaking = false;
                                        }
                                        // Switching FROM Manual TO RealTime - cancel any manual session
                                        (TranscriptionMode::Manual, TranscriptionMode::RealTime) => {
                                            // Check session state and spawn transcription task if needed
                                            let should_transcribe = {
                                                let mut session_lock = current_manual_session.lock();
                                                if let Some(session) = session_lock.as_ref() {
                                                    println!("Cancelled manual session {} due to mode switch to realtime", session.session_id);
                                                    let was_recording = session.is_recording;
                                                    *session_lock = None;
                                                    was_recording
                                                } else {
                                                    false
                                                }
                                            }; // session_lock is dropped here

                                            // If session was recording, trigger transcription before canceling
                                            if should_transcribe {
                                                if let Some(audio_processor) = &audio_processor_ref {
                                                    let sample_rate = read_app_config().sample_rate;
                                                    tokio::spawn({
                                                        let audio_processor = audio_processor.clone();
                                                        async move {
                                                            if let Err(e) = audio_processor.trigger_manual_transcription(sample_rate).await {
                                                                eprintln!("Failed to process final manual session during mode switch: {}", e);
                                                            }
                                                        }
                                                    });
                                                }
                                            }

                                            recording.store(false, Ordering::Relaxed); // Ensure recording is stopped

                                            // Clear manual audio buffer
                                            if let Some(audio_processor) = &audio_processor_ref {
                                                audio_processor.clear_manual_buffer();
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
        if let Err(e) = self.audio_capture.stop_recording() {
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

        if let Err(e) = self.audio_capture.resume() {
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
        self.audio_capture.stop();

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
        *self.whisper.lock() = None;

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

        // ASYNC: Control audio stream in background task to avoid blocking
        if was_recording {
            // We were recording, now stopping - stop the stream to save CPU
            if let Err(e) = self.audio_capture.stop_recording() {
                eprintln!("Warning: Failed to stop audio recording: {}", e);
            }
        } else {
            // We were stopped, now recording - start the stream
            if let Err(e) = self.audio_capture.start_recording() {
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
    pub fn start_manual_session(&mut self) -> Result<String, anyhow::Error> {
        if *self.transcription_mode.lock() != TranscriptionMode::Manual {
            return Err(anyhow::anyhow!(
                "Cannot start manual session when not in manual mode"
            ));
        }

        // Check if there's already an active session
        {
            let current_session = self.current_manual_session.lock();
            if let Some(session) = current_session.as_ref() {
                if session.is_recording || session.is_processing {
                    return Err(anyhow::anyhow!(
                        "Cannot start new session: existing session {} is still active",
                        session.session_id
                    ));
                }
            }
        }

        // Create new session with config parameters
        let max_duration = read_app_config()
            .manual_mode_config
            .max_recording_duration_secs;
        let new_session = ManualSession::new(max_duration);
        let session_id = new_session.session_id.clone();

        // Clear transcript if configured to do so
        if read_app_config().manual_mode_config.clear_on_new_session {
            let mut transcript_history = self.transcript_history.write();
            transcript_history.clear();

            let mut audio_data = self.audio_visualization_data.write();
            audio_data.transcript.clear();
            audio_data.reset_requested = true;
        }

        // Store the new session
        {
            let mut current_session = self.current_manual_session.lock();
            *current_session = Some(new_session);
        }

        // Set recording flag to true to start audio capture
        self.recording.store(true, Ordering::Relaxed);

        println!("Started manual transcription session: {}", session_id);
        Ok(session_id)
    }

    /// Stop the current manual transcription session and trigger processing
    pub fn stop_manual_session(&mut self) -> Result<(), anyhow::Error> {
        if *self.transcription_mode.lock() != TranscriptionMode::Manual {
            return Err(anyhow::anyhow!(
                "Cannot stop manual session when not in manual mode"
            ));
        }

        let session_id = {
            let mut current_session = self.current_manual_session.lock();
            if let Some(session) = current_session.as_mut() {
                if !session.is_recording {
                    return Err(anyhow::anyhow!("No active recording session to stop"));
                }

                session.is_recording = false;
                session.is_processing = true;
                session.session_id.clone()
            } else {
                return Err(anyhow::anyhow!("No manual session found"));
            }
        };

        // Stop recording audio
        self.recording.store(false, Ordering::Relaxed);

        println!(
            "Stopped manual transcription session: {} - processing...",
            session_id
        );

        // Trigger transcription through the session command system
        let sender = self.manual_session_tx.clone();
        tokio::spawn(async move {
            if let Err(e) = sender.send(ManualSessionCommand::StopSession).await {
                eprintln!("Failed to send stop session command: {}", e);
            }
        });

        Ok(())
    }

    /// Cancel the current manual transcription session
    pub fn cancel_manual_session(&mut self) -> Result<(), anyhow::Error> {
        if *self.transcription_mode.lock() != TranscriptionMode::Manual {
            return Err(anyhow::anyhow!(
                "Cannot cancel manual session when not in manual mode"
            ));
        }

        let session_id = {
            let mut current_session = self.current_manual_session.lock();
            if let Some(session) = current_session.take() {
                session.session_id
            } else {
                return Err(anyhow::anyhow!("No manual session to cancel"));
            }
        };

        // Stop recording audio
        self.recording.store(false, Ordering::Relaxed);

        println!("Cancelled manual transcription session: {}", session_id);
        Ok(())
    }

    /// Get the status of the current manual session
    pub fn get_manual_session_status(&self) -> Option<ManualSessionStatus> {
        if *self.transcription_mode.lock() != TranscriptionMode::Manual {
            return None;
        }

        let current_session = self.current_manual_session.lock();
        current_session.as_ref().map(|session| session.get_status())
    }

    /// Check if there's an active manual session
    pub fn has_active_manual_session(&self) -> bool {
        if *self.transcription_mode.lock() != TranscriptionMode::Manual {
            return false;
        }

        let current_session = self.current_manual_session.lock();
        current_session.as_ref().map_or(false, |session| {
            session.is_recording || session.is_processing
        })
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
        let mut current_session = self.current_manual_session.lock();
        if let Some(session) = current_session.as_mut() {
            session.is_processing = false;
            println!("Manual session {} completed", session.session_id);

            // Auto-restart if configured
            if read_app_config().manual_mode_config.auto_restart_sessions {
                // Note: Auto-restart would need to be implemented at a higher level
                // since this method doesn't have &mut self
                println!("Auto-restart is enabled - new session should be started");
            }
        }
        Ok(())
    }
}
