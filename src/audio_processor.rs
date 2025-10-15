use parking_lot::{Mutex, RwLock};
use std::collections::VecDeque;
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
        app_config: AppConfig,
    ) -> Self {
        let manual_buffer_max_size = app_config.manual_mode_config.manual_buffer_size;
        let manual_audio_buffer = Arc::new(Mutex::new(Vec::with_capacity(manual_buffer_max_size)));

        Self {
            running,
            recording,
            transcript_history,
            audio_processor,
            audio_visualization_data,
            segment_tx,
            buffer_size: app_config.buffer_size,
            config: app_config.audio_processor_config,
            transcription_stats,
            transcription_mode,
            manual_audio_buffer,
            manual_buffer_max_size,
            sample_rate: app_config.sample_rate,
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
        let sample_rate = self.sample_rate;

        // Create thread-local buffer
        let mut audio_buffer = Vec::with_capacity(buffer_size);
        let max_vis_samples = config.max_vis_samples;
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
                    let mut sleep_duration = Duration::from_millis(100);
                    {
                        let mut audio_data = audio_visualization_data.write(); // Blocking write to ensure decay happens
                        if !audio_data.samples.is_empty() {
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
                            } else {
                                // Keep animating while decay is visible
                                sleep_duration = Duration::from_millis(16);
                            }
                        }
                        audio_data.is_speaking = false; // No longer speaking when paused
                    } // Release lock here

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
                                    max_vis_samples,
                                    &mut latest_is_speaking,
                                    &mut preroll_buffer,
                                    preroll_max_samples,
                                    sample_rate,
                                )
                                .await;
                            }
                            TranscriptionMode::Manual => {
                                Self::process_manual_audio(
                                    &audio_buffer,
                                    &manual_audio_buffer,
                                    &audio_visualization_data,
                                    max_vis_samples,
                                    manual_buffer_max_size,
                                    &recording,
                                )
                                .await;
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
        transcription_stats: &Arc<Mutex<TranscriptionStats>>,
        max_vis_samples: usize,
        latest_is_speaking: &mut bool,
        preroll_buffer: &mut VecDeque<f32>,
        preroll_max_samples: usize,
        sample_rate: usize,
    ) {
        let (segments_result, vad_speaking) = {
            let mut processor = audio_processor.lock();
            let result = processor.process_audio(audio_buffer);
            let speaking = processor.is_speaking();
            (result, speaking)
        };

        let new_samples: Vec<f32> = audio_buffer.iter().take(max_vis_samples).copied().collect();
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
        max_vis_samples: usize,
        manual_buffer_max_size: usize,
        recording: &Arc<AtomicBool>,
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

        // Accumulate audio in manual buffer with overflow protection
        // Use lock() instead of try_lock() to guarantee no audio samples are dropped
        let mut manual_buffer = manual_audio_buffer.lock();
        let current_size = manual_buffer.len();
        let new_size = current_size + audio_buffer.len();

        // Check if adding this audio would exceed the buffer limit
        if new_size > manual_buffer_max_size {
            eprintln!(
                "Manual buffer overflow: current={}, new={}, max={}. Auto-stopping recording.",
                current_size, new_size, manual_buffer_max_size
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
    pub async fn process_accumulated_manual_audio(
        &self,
        sample_rate: usize,
    ) -> Result<(), anyhow::Error> {
        let accumulated_audio = {
            let mut manual_buffer = self.manual_audio_buffer.lock();
            if manual_buffer.is_empty() {
                return Ok(()); // Nothing to process
            }
            // Use mem::take instead of clone to avoid copying the entire buffer
            std::mem::take(&mut *manual_buffer)
        };

        let original_duration = accumulated_audio.len() as f64 / sample_rate as f64;
        println!(
            "Processing accumulated manual audio: {} samples ({:.2}s)",
            accumulated_audio.len(),
            original_duration
        );

        // Apply VAD-based silence removal to improve transcription quality
        let speech_only_audio = {
            let mut vad = self.audio_processor.lock();
            match vad.process_audio(&accumulated_audio) {
                Ok(speech_segments) => {
                    if speech_segments.is_empty() {
                        println!("No speech detected in manual recording");
                        return Ok(()); // Nothing to transcribe
                    }

                    // Concatenate all speech segments, removing silence
                    let total_speech_samples: usize =
                        speech_segments.iter().map(|seg| seg.samples.len()).sum();

                    let mut concatenated = Vec::with_capacity(total_speech_samples);
                    for segment in speech_segments {
                        concatenated.extend_from_slice(&segment.samples);
                    }

                    let speech_duration = concatenated.len() as f64 / sample_rate as f64;
                    let silence_removed = original_duration - speech_duration;
                    println!(
                        "VAD preprocessing: {:.2}s speech, {:.2}s silence removed ({:.1}% reduction)",
                        speech_duration,
                        silence_removed,
                        (silence_removed / original_duration) * 100.0
                    );

                    concatenated
                }
                Err(e) => {
                    eprintln!("VAD preprocessing failed: {}, using original audio", e);
                    accumulated_audio // Fallback to original audio if VAD fails
                }
            }
        };

        // Create a single audio segment for transcription
        let duration_secs = speech_only_audio.len() as f64 / sample_rate as f64;
        let segment = AudioSegment {
            samples: speech_only_audio,
            start_time: 0.0,
            end_time: duration_secs,
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

    /// Get the current manual buffer size
    pub fn get_manual_buffer_size(&self) -> usize {
        self.manual_audio_buffer.lock().len()
    }

    /// Clear the manual audio buffer
    pub fn clear_manual_buffer(&self) {
        self.manual_audio_buffer.lock().clear();
    }

    /// Trigger manual transcription of accumulated audio
    pub async fn trigger_manual_transcription(
        &self,
        sample_rate: usize,
    ) -> Result<(), anyhow::Error> {
        self.process_accumulated_manual_audio(sample_rate).await
    }
}
