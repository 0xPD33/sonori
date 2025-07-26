use anyhow::Context;
use ct2rs::{ComputeType, Config, Device, Whisper, WhisperOptions};
use parking_lot::{Mutex, RwLock};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
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
        let (tx, rx) = mpsc::channel(50);
        let (transcript_tx, transcript_rx) = broadcast::channel(100);
        let (segment_tx, segment_rx) = mpsc::channel(50);
        let (transcription_done_tx, transcription_done_rx) = mpsc::unbounded_channel();

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

            // Determine device based on feature flag and user config
            #[cfg(feature = "cuda")]
            {
                if app_config.device.to_uppercase() == "CUDA" {
                    println!("INFO: CUDA feature is enabled and config is set to CUDA. Attempting to load model on GPU.");
                    config.device = Device::CUDA;
                } else {
                    println!("INFO: CUDA feature is enabled, but config is set to CPU. Loading model on CPU.");
                    config.device = Device::CPU;
                }
            }
            #[cfg(not(feature = "cuda"))]
            {
                if app_config.device.to_uppercase() == "CUDA" {
                    println!("WARN: Config specifies CUDA, but the application was not compiled with the 'cuda' feature.");
                    println!("WARN: Falling back to CPU. To use CUDA, recompile with '--features cuda'.");
                } else {
                    println!("INFO: CUDA feature not enabled. Loading model on CPU.");
                }
                config.device = Device::CPU;
            }

            // On GPU, FLOAT16 is often faster and uses less VRAM with minimal quality loss.
            // On CPU, INT8 is usually best.
            let compute_type_str = if config.device == Device::CUDA {
                "FLOAT16" // Good default for GPU
            } else {
                app_config.compute_type.as_str() // Use configured value for CPU
            };

            config.compute_type = match compute_type_str {
                "FLOAT16" => ComputeType::FLOAT16,
                "INT8" => ComputeType::INT8,
                _ => ComputeType::DEFAULT, // Safe fallback
            };

            // This setting is still relevant for CPU-based operations even when the device is CUDA.
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
                    #[cfg(feature = "cuda")]
                    if app_config.device.to_uppercase() == "CUDA" {
                        eprintln!("HINT: If using CUDA, ensure NVIDIA drivers, CUDA Toolkit, and cuDNN are correctly installed and accessible in your PATH.");
                    }
                }
            }
        });

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
        let audio_processor = AudioProcessor::new(
            self.running.clone(),
            self.recording.clone(),
            self.transcript_history.clone(),
            self.audio_processor.clone(),
            self.audio_visualization_data.clone(),
            self.segment_tx.clone(),
            config,
        );

        // Take ownership of the receivers and pass them to the processors
        if let (Some(segment_rx), Some(rx)) = (self.segment_rx.take(), self.rx.take()) {
            self.transcription_handle =
                Some(transcription_processor.start(segment_rx, self.transcript_tx.clone()));
            self.audio_handle = Some(audio_processor.start(rx));
            self.start_recording_monitor();
        } else {
            return Err(anyhow::anyhow!(
                "Failed to take ownership of receivers for processors"
            ));
        }

        Ok(())
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
                    println!("Recording state changed: {} -> {}", last_recording_state, current_recording_state);
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
    pub fn toggle_recording(&mut self) {
        let was_recording = self.recording.load(Ordering::Relaxed);
        self.recording.store(!was_recording, Ordering::Relaxed);

        // Control the audio stream based on the new recording state
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

        println!("Recording toggled to: {}", !was_recording);
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
}
