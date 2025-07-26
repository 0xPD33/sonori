use parking_lot::{Mutex, RwLock};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::Duration;

use crate::config::{AppConfig, AudioProcessorConfig};
use crate::silero_audio_processor::{AudioSegment, SileroVad, VadState};
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
}

impl AudioProcessor {
    pub fn new(
        running: Arc<AtomicBool>,
        recording: Arc<AtomicBool>,
        transcript_history: Arc<RwLock<String>>,
        audio_processor: Arc<Mutex<SileroVad>>,
        audio_visualization_data: Arc<RwLock<AudioVisualizationData>>,
        segment_tx: mpsc::Sender<AudioSegment>,
        app_config: AppConfig,
    ) -> Self {
        Self {
            running,
            recording,
            transcript_history,
            audio_processor,
            audio_visualization_data,
            segment_tx,
            buffer_size: app_config.buffer_size,
            config: app_config.audio_processor_config,
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

        // Create thread-local buffer
        let mut audio_buffer = Vec::with_capacity(buffer_size);
        let max_vis_samples = config.max_vis_samples;

        // Start audio processing task
        tokio::spawn(async move {
            let mut _last_vad_state = VadState::Silence;
            let mut latest_is_speaking = false;

            while running.load(Ordering::Relaxed) {
                // Check if we should be processing audio
                if !recording.load(Ordering::Relaxed) {
                    // When not recording, clear visualization data and wait for recording to start
                    if let Some(mut audio_data) = audio_visualization_data.try_write() {
                        if !audio_data.samples.is_empty() {
                            audio_data.samples.clear();
                            audio_data.is_speaking = false;
                        }
                    }
                    
                    // Use tokio::task::yield_now() to avoid blocking completely but be more efficient
                    // than polling. This allows other tasks to run and only wakes up when scheduled.
                    tokio::task::yield_now().await;
                    continue;
                }

                // When recording is active, block on receiving audio data
                match rx.recv().await {
                    Some(samples) => {
                        // Reuse buffer by clearing and extending
                        audio_buffer.clear();
                        audio_buffer.extend_from_slice(&samples);

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
                            match processor.process_audio(&audio_buffer) {
                                Ok(segments) => {
                                    latest_is_speaking = processor.is_speaking();
                                    audio_data.is_speaking = latest_is_speaking;

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
                    None => {
                        // Channel closed
                        println!("Audio channel disconnected");
                        break;
                    }
                }
            }
        })
    }
}
