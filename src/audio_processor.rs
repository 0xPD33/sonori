use hound;
use parking_lot::{Mutex, RwLock};
use std::collections::VecDeque;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::Duration;

use crate::config::{AppConfig, AudioProcessorConfig};
use crate::real_time_transcriber::TranscriptionMode;
use crate::silero_audio_processor::{AudioSegment, SileroVad, VadState};
use crate::transcription_stats::TranscriptionStats;
use crate::ui::common::AudioVisualizationData;

/// Handles audio processing and voice activity detection
pub struct AudioProcessor {
    running: Arc<AtomicBool>,
    recording: Arc<AtomicBool>,
    transcript_history: Arc<RwLock<String>>,
    audio_processor: Arc<Mutex<SileroVad>>,
    audio_visualization_data: Arc<RwLock<AudioVisualizationData>>,
    segment_tx: mpsc::Sender<AudioSegment>,
    buffer_size: usize,
    config: AudioProcessorConfig,
    transcription_stats: Arc<Mutex<TranscriptionStats>>,

    // Manual mode fields
    transcription_mode: Arc<Mutex<TranscriptionMode>>,
    manual_audio_buffer: Arc<Mutex<Vec<f32>>>,
    manual_buffer_max_size: usize,
    manual_session_tx: mpsc::Sender<crate::real_time_transcriber::ManualSessionCommand>,

    // Session tracking for preventing cross-session contamination
    current_session_id: Arc<RwLock<Option<String>>>,

    // Debug configuration
    debug_config: crate::config::DebugConfig,

    sample_rate: usize,
}

impl AudioProcessor {
    pub fn new(
        running: Arc<AtomicBool>,
        recording: Arc<AtomicBool>,
        transcript_history: Arc<RwLock<String>>,
        audio_processor: Arc<Mutex<SileroVad>>,
        audio_visualization_data: Arc<RwLock<AudioVisualizationData>>,
        segment_tx: mpsc::Sender<AudioSegment>,
        transcription_mode: Arc<Mutex<TranscriptionMode>>,
        transcription_stats: Arc<Mutex<TranscriptionStats>>,
        manual_session_tx: mpsc::Sender<crate::real_time_transcriber::ManualSessionCommand>,
        app_config: AppConfig,
    ) -> Self {
        // Calculate manual buffer size from max_recording_duration_secs and sample_rate
        let manual_buffer_max_size = (app_config.manual_mode_config.max_recording_duration_secs
            as usize)
            * app_config.audio_processor_config.sample_rate;
        let manual_audio_buffer = Arc::new(Mutex::new(Vec::with_capacity(manual_buffer_max_size)));

        // Initialize session ID based on transcription mode
        let initial_session_id = match *transcription_mode.lock() {
            TranscriptionMode::RealTime => Some("realtime".to_string()),
            TranscriptionMode::Manual => None, // Will be set when session starts
        };

        Self {
            running,
            recording,
            transcript_history,
            audio_processor,
            audio_visualization_data,
            segment_tx,
            buffer_size: app_config.audio_processor_config.buffer_size,
            config: app_config.audio_processor_config.clone(),
            transcription_stats,
            transcription_mode,
            manual_audio_buffer,
            manual_buffer_max_size,
            manual_session_tx,
            current_session_id: Arc::new(RwLock::new(initial_session_id)),
            debug_config: app_config.debug_config.clone(),
            sample_rate: app_config.audio_processor_config.sample_rate,
        }
    }

    /// Starts audio processing
    pub fn start(&self, mut rx: mpsc::Receiver<Vec<f32>>) -> tokio::task::JoinHandle<()> {
        let running = self.running.clone();
        let recording = self.recording.clone();
        let transcript_history = self.transcript_history.clone();
        let audio_processor = self.audio_processor.clone();
        let audio_visualization_data = self.audio_visualization_data.clone();
        let segment_tx = self.segment_tx.clone();
        let config = self.config.clone();
        let buffer_size = self.buffer_size;
        let transcription_mode = self.transcription_mode.clone();
        let manual_audio_buffer = self.manual_audio_buffer.clone();
        let manual_buffer_max_size = self.manual_buffer_max_size;
        let transcription_stats = self.transcription_stats.clone();
        let session_id_ref = self.current_session_id.clone();
        let sample_rate = self.sample_rate;

        // Create thread-local buffer
        let mut audio_buffer = Vec::with_capacity(buffer_size);
        let preroll_max_samples = ((sample_rate * 150) / 1000).max(buffer_size);
        let mut preroll_buffer: VecDeque<f32> = VecDeque::with_capacity(preroll_max_samples);

        // Start audio processing task
        tokio::spawn(async move {
            let mut _last_vad_state = VadState::Silence;
            let mut latest_is_speaking = false;

            while running.load(Ordering::Relaxed) {
                // Check if we should be processing audio or just doing decay animation
                let is_recording = recording.load(Ordering::Relaxed);

                if !is_recording {
                    // When paused, decay spectrogram to zero instead of clearing immediately.
                    // Only keep the tight 60 Hz loop while there is data to animate.
                    let sleep_duration = {
                        // Use minimal lock scope for performance
                        let mut audio_data = audio_visualization_data.write();

                        let should_animate = if !audio_data.samples.is_empty() {
                            // Gradually decay samples toward zero for smooth fade-out effect
                            for sample in &mut audio_data.samples {
                                *sample *= 0.95; // Exponential decay factor
                            }

                            // Check if samples are essentially zero (below threshold)
                            let max_amplitude = audio_data
                                .samples
                                .iter()
                                .map(|&x| x.abs())
                                .fold(0.0, f32::max);

                            if max_amplitude < 0.001 {
                                // Samples have decayed enough, clear them
                                audio_data.samples.clear();
                                false
                            } else {
                                // Keep animating while decay is visible
                                true
                            }
                        } else {
                            false
                        };

                        audio_data.is_speaking = false; // No longer speaking when paused

                        // Return appropriate sleep duration based on animation state
                        if should_animate {
                            Duration::from_millis(16) // 60 FPS for smooth animation
                        } else {
                            Duration::from_millis(100) // Slower poll when idle
                        }
                    }; // Release lock before sleep

                    preroll_buffer.clear();
                    tokio::time::sleep(sleep_duration).await;
                    continue;
                }

                // When recording is active, try to receive audio data with timeout
                match tokio::time::timeout(Duration::from_millis(50), rx.recv()).await {
                    Ok(Some(samples)) => {
                        // Reuse buffer by clearing and extending
                        audio_buffer.clear();
                        audio_buffer.extend_from_slice(&samples);

                        // Maintain rolling history for leading context
                        preroll_buffer.extend(samples.iter().copied());
                        if preroll_buffer.len() > preroll_max_samples {
                            let excess = preroll_buffer.len() - preroll_max_samples;
                            preroll_buffer.drain(0..excess);
                        }

                        // Route to appropriate processing based on current mode
                        let current_mode = *transcription_mode.lock();
                        match current_mode {
                            TranscriptionMode::RealTime => {
                                Self::process_realtime_audio(
                                    &audio_buffer,
                                    &audio_processor,
                                    &audio_visualization_data,
                                    &segment_tx,
                                    &transcript_history,
                                    &transcription_stats,
                                    buffer_size,
                                    &mut latest_is_speaking,
                                    &mut preroll_buffer,
                                    preroll_max_samples,
                                    sample_rate,
                                    &session_id_ref,
                                )
                                .await;
                            }
                            TranscriptionMode::Manual => {
                                Self::process_manual_audio(
                                    &audio_buffer,
                                    &manual_audio_buffer,
                                    &audio_visualization_data,
                                    buffer_size,
                                    manual_buffer_max_size,
                                    &recording,
                                    sample_rate,
                                )
                                .await;
                            }
                        }
                    }
                    Ok(None) => {
                        // Channel closed
                        break;
                    }
                    Err(_) => {
                        // Timeout - continue loop to check recording flag again
                        // This allows the decay logic to run when recording stops
                        continue;
                    }
                }
            }
        })
    }

    /// Process audio in real-time mode (existing behavior)
    async fn process_realtime_audio(
        audio_buffer: &[f32],
        audio_processor: &Arc<Mutex<SileroVad>>,
        audio_visualization_data: &Arc<RwLock<AudioVisualizationData>>,
        segment_tx: &mpsc::Sender<AudioSegment>,
        transcript_history: &Arc<RwLock<String>>,
        transcription_stats: &Arc<Mutex<TranscriptionStats>>,
        buffer_size: usize,
        latest_is_speaking: &mut bool,
        preroll_buffer: &mut VecDeque<f32>,
        preroll_max_samples: usize,
        sample_rate: usize,
        session_id_ref: &Arc<RwLock<Option<String>>>,
    ) {
        let (segments_result, vad_speaking) = {
            let mut processor = audio_processor.lock();
            let result = processor.process_audio(audio_buffer);
            let speaking = processor.is_speaking();
            (result, speaking)
        };

        let new_samples: Vec<f32> = audio_buffer.iter().take(buffer_size).copied().collect();
        let was_speaking = *latest_is_speaking;

        let mut reset_history = false;
        {
            let mut audio_data = audio_visualization_data.write();

            if audio_data.samples != new_samples {
                audio_data.samples = new_samples;
            }

            match &segments_result {
                Ok(_) => {
                    *latest_is_speaking = vad_speaking;
                    audio_data.is_speaking = *latest_is_speaking;
                }
                Err(_) => {
                    *latest_is_speaking = false;
                    audio_data.is_speaking = false;
                }
            }

            if audio_data.reset_requested {
                audio_data.reset_requested = false;
                audio_data.transcript.clear();
                reset_history = true;
            }
        }

        if reset_history {
            transcript_history.write().clear();
        }

        match segments_result {
            Ok(segments) => {
                if segments.is_empty() {
                    return;
                }

                let total = segments.len();
                let mut prepend_preroll = !was_speaking;
                let current_session_id = session_id_ref.read().clone();
                for (idx, mut segment) in segments.into_iter().enumerate() {
                    if prepend_preroll && !preroll_buffer.is_empty() {
                        let mut combined =
                            Vec::with_capacity(preroll_buffer.len() + segment.samples.len());
                        combined.extend(preroll_buffer.iter().copied());
                        combined.extend_from_slice(&segment.samples);

                        let preroll_duration = preroll_buffer.len() as f64 / sample_rate as f64;
                        segment.samples = combined;
                        segment.start_time = (segment.start_time - preroll_duration).max(0.0);
                        prepend_preroll = false;
                    }

                    // Set session ID for this segment
                    segment.session_id = current_session_id.clone();

                    if let Err(e) = segment_tx.send(segment).await {
                        eprintln!("Failed to send audio segment: {}", e);
                        let dropped = (total - idx) as u64;
                        if dropped > 0 {
                            if let Some(mut stats) = transcription_stats.try_lock() {
                                let total_drops = stats.record_segment_drop(dropped);
                                eprintln!(
                                    "Dropped {} audio segments (total: {}) due to channel error",
                                    dropped, total_drops
                                );
                            } else {
                                eprintln!(
                                    "Dropped {} audio segments but could not update stats",
                                    dropped
                                );
                            }
                        }
                        break;
                    }
                }

                if preroll_buffer.len() > preroll_max_samples {
                    let excess = preroll_buffer.len() - preroll_max_samples;
                    preroll_buffer.drain(0..excess);
                }
            }
            Err(e) => {
                eprintln!("Error processing audio: {}", e);
            }
        }
    }

    /// Process audio in manual mode (accumulate for batch processing)
    async fn process_manual_audio(
        audio_buffer: &[f32],
        manual_audio_buffer: &Arc<Mutex<Vec<f32>>>,
        audio_visualization_data: &Arc<RwLock<AudioVisualizationData>>,
        buffer_size: usize,
        manual_buffer_max_size: usize,
        recording: &Arc<AtomicBool>,
        sample_rate: usize,
    ) {
        // Update visualization data
        if let Some(mut audio_data) = audio_visualization_data.try_write() {
            let new_samples: Vec<f32> = audio_buffer.iter().take(buffer_size).copied().collect();
            if audio_data.samples != new_samples {
                audio_data.samples = new_samples;
            }
            // In manual mode, show recording indicator
            audio_data.is_speaking = true;
        }

        // Accumulate audio in manual buffer with overflow protection
        // Use lock() instead of try_lock() to guarantee no audio samples are dropped
        let mut manual_buffer = manual_audio_buffer.lock();
        let current_size = manual_buffer.len();
        let new_size = current_size + audio_buffer.len();

        // Check if adding this audio would exceed the buffer limit
        if new_size > manual_buffer_max_size {
            let current_duration = current_size as f64 / sample_rate as f64;
            let max_duration = manual_buffer_max_size as f64 / sample_rate as f64;

            eprintln!(
                "Manual buffer full ({:.1}s / {:.1}s). Recording stopped.",
                current_duration, max_duration
            );

            // Stop recording to prevent buffer overflow
            recording.store(false, Ordering::Relaxed);

            // Add as much as we can without exceeding the limit
            let space_remaining = manual_buffer_max_size.saturating_sub(current_size);
            if space_remaining > 0 {
                manual_buffer.extend_from_slice(&audio_buffer[..space_remaining]);
            }
        } else {
            manual_buffer.extend_from_slice(audio_buffer);
        }
    }

    /// Process accumulated manual audio when session ends
    /// Manual mode transcribes the entire buffer without VAD filtering to guarantee
    /// that no speech is missed. User controls start/stop explicitly.
    pub async fn process_accumulated_manual_audio(
        &self,
        sample_rate: usize,
        expected_session_id: Option<String>,
    ) -> Result<(), anyhow::Error> {
        let accumulated_audio = {
            let mut manual_buffer = self.manual_audio_buffer.lock();
            if manual_buffer.is_empty() {
                return Ok(()); // Nothing to process
            }
            // Use mem::take instead of clone to avoid copying the entire buffer
            std::mem::take(&mut *manual_buffer)
        };

        let duration_secs = accumulated_audio.len() as f64 / sample_rate as f64;
        println!(
            "Processing manual audio: {} samples ({:.2}s)",
            accumulated_audio.len(),
            duration_secs
        );

        // Save audio to WAV file for debugging
        self.save_audio_to_wav(&accumulated_audio, sample_rate as u32);

        // Create a single audio segment for transcription with ALL audio
        // No VAD filtering - guarantees every sample is transcribed
        let segment = AudioSegment {
            samples: accumulated_audio,
            start_time: 0.0,
            end_time: duration_secs,
            sample_rate,
            session_id: expected_session_id,
            is_manual: true, // Manual mode segment
        };

        // Send to transcription processor
        if let Err(e) = self.segment_tx.send(segment).await {
            eprintln!("Failed to send manual audio segment: {}", e);
            if let Some(mut stats) = self.transcription_stats.try_lock() {
                let total = stats.record_segment_drop(1);
                eprintln!(
                    "Manual segment dropped (total drops: {}) due to channel error",
                    total
                );
            }
            return Err(anyhow::anyhow!(
                "Failed to send manual audio segment: {}",
                e
            ));
        }

        // Update visualization to show processing state
        if let Some(mut audio_data) = self.audio_visualization_data.try_write() {
            audio_data.is_speaking = false;
            // Clear visualization samples since recording stopped
            audio_data.samples.clear();
        }

        Ok(())
    }

    /// Get the current size of the manual audio buffer
    pub fn get_manual_buffer_size(&self) -> usize {
        self.manual_audio_buffer.lock().len()
    }

    /// Clear the manual audio buffer
    pub fn clear_manual_buffer(&self) {
        // Use mem::take() to reuse buffer capacity (avoids reallocation)
        let mut buffer = self.manual_audio_buffer.lock();
        let _ = std::mem::take(&mut *buffer);
    }

    /// Atomically start a new manual session by clearing buffer and setting session ID
    /// This prevents race conditions where audio could be processed with inconsistent state
    pub fn start_new_manual_session(&self, session_id: String) {
        // Acquire both locks in consistent order to prevent race conditions
        let mut buffer = self.manual_audio_buffer.lock();
        let mut session_id_lock = self.current_session_id.write();

        // Use mem::take() to reuse buffer capacity (avoids reallocation)
        let _ = std::mem::take(&mut *buffer);
        *session_id_lock = Some(session_id);
    }

    /// Reset the VAD state for a new session
    /// This clears all internal buffers and state to prevent audio from previous sessions
    /// from leaking into new sessions
    pub fn reset_vad_state(&self) {
        let mut vad = self.audio_processor.lock();
        vad.reset();
    }

    /// Trigger manual transcription of accumulated audio
    pub async fn trigger_manual_transcription(
        &self,
        sample_rate: usize,
        expected_session_id: Option<String>,
    ) -> Result<(), anyhow::Error> {
        self.process_accumulated_manual_audio(sample_rate, expected_session_id)
            .await
    }

    /// Update the current session ID
    pub fn set_session_id(&self, session_id: Option<String>) {
        *self.current_session_id.write() = session_id;
    }

    /// Get the current session ID
    pub fn get_session_id(&self) -> Option<String> {
        self.current_session_id.read().clone()
    }

    /// Get a reference to the current session ID (for cloning into async tasks)
    pub fn get_session_id_ref(&self) -> Arc<RwLock<Option<String>>> {
        self.current_session_id.clone()
    }

    /// Save audio to WAV file in the configured directory (only if debug flag is enabled)
    pub fn save_audio_to_wav(&self, audio_samples: &[f32], sample_rate: u32) {
        // Only save if debug option is enabled
        if !self.debug_config.save_manual_audio_debug {
            return;
        }

        let recording_dir = &self.debug_config.recording_dir;

        // Create directory if it doesn't exist
        if let Err(e) = fs::create_dir_all(recording_dir) {
            eprintln!("Failed to create recording directory '{}': {}", recording_dir, e);
            return;
        }

        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let filename = format!("recording_{}.wav", timestamp);
        let path = Path::new(recording_dir).join(&filename);

        // Create WAV writer
        match hound::WavWriter::create(
            &path,
            hound::WavSpec {
                channels: 1,
                sample_rate,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        ) {
            Ok(mut writer) => {
                // Convert f32 samples to i16
                for &sample in audio_samples {
                    let sample_i16 = (sample * 32767.0).clamp(-32768.0, 32767.0) as i16;
                    if let Err(e) = writer.write_sample(sample_i16) {
                        eprintln!("Error writing WAV sample: {}", e);
                        return;
                    }
                }

                match writer.finalize() {
                    Ok(_) => println!("Audio saved to: {}", path.display()),
                    Err(e) => eprintln!("Error finalizing WAV file: {}", e),
                }
            }
            Err(e) => eprintln!("Failed to create WAV file: {}", e),
        }
    }
}
