use parking_lot::{Mutex, RwLock};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::Duration;

use crate::config::{AppConfig, AudioProcessorConfig};
use crate::silero_audio_processor::{AudioSegment, SileroVad, VadState};
use crate::ui::common::AudioVisualizationData;
use crate::real_time_transcriber::TranscriptionMode;

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
    
    // Manual mode fields
    transcription_mode: Arc<Mutex<TranscriptionMode>>,
    manual_audio_buffer: Arc<Mutex<Vec<f32>>>,
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
        app_config: AppConfig,
    ) -> Self {
        let manual_audio_buffer = Arc::new(Mutex::new(Vec::with_capacity(app_config.manual_mode_config.manual_buffer_size)));
        
        Self {
            running,
            recording,
            transcript_history,
            audio_processor,
            audio_visualization_data,
            segment_tx,
            buffer_size: app_config.buffer_size,
            config: app_config.audio_processor_config,
            transcription_mode,
            manual_audio_buffer,
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

        // Create thread-local buffer
        let mut audio_buffer = Vec::with_capacity(buffer_size);
        let max_vis_samples = config.max_vis_samples;

        // Start audio processing task
        tokio::spawn(async move {
            let mut _last_vad_state = VadState::Silence;
            let mut latest_is_speaking = false;

            while running.load(Ordering::Relaxed) {
                // Check if we should be processing audio or just doing decay animation
                let is_recording = recording.load(Ordering::Relaxed);
                
                if !is_recording {
                    // When paused, decay spectrogram to zero instead of clearing immediately
                    {
                        let mut audio_data = audio_visualization_data.write(); // Blocking write to ensure decay happens
                        if !audio_data.samples.is_empty() {
                            println!("Decaying spectrogram: {} samples", audio_data.samples.len());
                            // Gradually decay samples toward zero for smooth fade-out effect
                            for sample in &mut audio_data.samples {
                                *sample *= 0.95; // Exponential decay factor
                            }
                            
                            // Check if samples are essentially zero (below threshold)
                            let max_amplitude = audio_data.samples.iter()
                                .map(|&x| x.abs())
                                .fold(0.0, f32::max);
                            
                            if max_amplitude < 0.001 {
                                // Samples have decayed enough, clear them
                                audio_data.samples.clear();
                                println!("Spectrogram decay completed - samples cleared");
                            }
                        }
                        audio_data.is_speaking = false; // No longer speaking when paused
                    } // Release lock here
                    
                    // Continue the fade-out animation at ~60fps
                    tokio::time::sleep(Duration::from_millis(16)).await;
                    continue;
                }

                // When recording is active, try to receive audio data with timeout
                match tokio::time::timeout(Duration::from_millis(50), rx.recv()).await {
                    Ok(Some(samples)) => {
                        // Reuse buffer by clearing and extending
                        audio_buffer.clear();
                        audio_buffer.extend_from_slice(&samples);

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
                                    max_vis_samples,
                                    &mut latest_is_speaking,
                                ).await;
                            }
                            TranscriptionMode::Manual => {
                                Self::process_manual_audio(
                                    &audio_buffer,
                                    &manual_audio_buffer,
                                    &audio_visualization_data,
                                    max_vis_samples,
                                ).await;
                            }
                        }
                    }
                    Ok(None) => {
                        // Channel closed
                        println!("Audio channel disconnected");
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
        max_vis_samples: usize,
        latest_is_speaking: &mut bool,
    ) {
        // Process audio only if we can get both locks
        if let (Some(mut processor), Some(mut audio_data)) = (
            audio_processor.try_lock(),
            audio_visualization_data.try_write(),
        ) {
            // Only update visualization data if it's different from current samples
            let new_samples: Vec<f32> =
                audio_buffer.iter().take(max_vis_samples).copied().collect();
            if audio_data.samples != new_samples {
                audio_data.samples = new_samples;
            }

            // Process audio with the processor
            match processor.process_audio(audio_buffer) {
                Ok(segments) => {
                    *latest_is_speaking = processor.is_speaking();
                    audio_data.is_speaking = *latest_is_speaking;

                    // Handle reset request if present
                    if audio_data.reset_requested {
                        audio_data.reset_requested = false;
                        audio_data.transcript.clear();

                        if let Some(mut history) = transcript_history.try_write() {
                            history.clear();
                        }
                    }

                    // Send segments for transcription
                    for segment in segments {
                        if let Err(e) = segment_tx.try_send(segment) {
                            eprintln!("Failed to send audio segment: {}", e);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error processing audio: {}", e);
                    audio_data.is_speaking = false;
                }
            }
        }
    }

    /// Process audio in manual mode (accumulate for batch processing)
    async fn process_manual_audio(
        audio_buffer: &[f32],
        manual_audio_buffer: &Arc<Mutex<Vec<f32>>>,
        audio_visualization_data: &Arc<RwLock<AudioVisualizationData>>,
        max_vis_samples: usize,
    ) {
        // Update visualization data
        if let Some(mut audio_data) = audio_visualization_data.try_write() {
            let new_samples: Vec<f32> =
                audio_buffer.iter().take(max_vis_samples).copied().collect();
            if audio_data.samples != new_samples {
                audio_data.samples = new_samples;
            }
            // In manual mode, show recording indicator
            audio_data.is_speaking = true;
        }

        // Accumulate audio in manual buffer
        if let Some(mut manual_buffer) = manual_audio_buffer.try_lock() {
            manual_buffer.extend_from_slice(audio_buffer);
        }
    }

    /// Process accumulated manual audio when session ends
    pub async fn process_accumulated_manual_audio(
        &self, 
        sample_rate: usize
    ) -> Result<(), anyhow::Error> {
        let accumulated_audio = {
            let mut manual_buffer = self.manual_audio_buffer.lock();
            if manual_buffer.is_empty() {
                return Ok(()); // Nothing to process
            }
            let audio = manual_buffer.clone();
            manual_buffer.clear();
            audio
        };

        println!("Processing accumulated manual audio: {} samples", accumulated_audio.len());

        // Create a single large audio segment for the entire manual session
        let duration_secs = accumulated_audio.len() as f64 / sample_rate as f64;
        let segment = AudioSegment {
            samples: accumulated_audio,
            start_time: 0.0,
            end_time: duration_secs,
        };

        // Send to transcription processor
        if let Err(e) = self.segment_tx.try_send(segment) {
            eprintln!("Failed to send manual audio segment: {}", e);
            return Err(anyhow::anyhow!("Failed to send manual audio segment: {}", e));
        }

        // Update visualization to show processing state
        if let Some(mut audio_data) = self.audio_visualization_data.try_write() {
            audio_data.is_speaking = false;
            // Clear visualization samples since recording stopped
            audio_data.samples.clear();
        }

        Ok(())
    }

    /// Get the current manual buffer size
    pub fn get_manual_buffer_size(&self) -> usize {
        self.manual_audio_buffer.lock().len()
    }

    /// Clear the manual audio buffer
    pub fn clear_manual_buffer(&self) {
        self.manual_audio_buffer.lock().clear();
    }

    /// Trigger manual transcription of accumulated audio
    pub async fn trigger_manual_transcription(&self, sample_rate: usize) -> Result<(), anyhow::Error> {
        self.process_accumulated_manual_audio(sample_rate).await
    }
}
