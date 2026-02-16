use anyhow;
use portaudio as pa;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::config::read_app_config;
use crate::transcription_stats::TranscriptionStats;
use parking_lot::Mutex;

/// Manages audio capture using PortAudio
pub struct AudioCapture {
    pa_stream: Option<pa::Stream<pa::NonBlocking, pa::Input<f32>>>,
    pa: Option<pa::PortAudio>,
    input_settings: Option<pa::InputStreamSettings<f32>>,
    samples_sent: Arc<AtomicUsize>,
}

impl AudioCapture {
    /// Creates a new AudioCapture instance
    pub fn new() -> Self {
        Self {
            pa_stream: None,
            pa: None,
            input_settings: None,
            samples_sent: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Initializes PortAudio settings without starting the stream
    fn initialize_audio(&mut self) -> Result<(), anyhow::Error> {
        if self.pa.is_some() {
            return Ok(()); // Already initialized
        }

        let config = read_app_config();

        let pa = pa::PortAudio::new()
            .map_err(|e| anyhow::anyhow!("Failed to initialize PortAudio: {}", e))?;

        let input_params = pa
            .default_input_stream_params::<f32>(1)
            .map_err(|e| anyhow::anyhow!("Failed to get default input stream parameters: {}", e))?;

        let input_settings = pa::InputStreamSettings::new(
            input_params,
            crate::config::SAMPLE_RATE as f64,
            config.audio_processor_config.buffer_size as u32,
        );

        self.pa = Some(pa);
        self.input_settings = Some(input_settings);
        Ok(())
    }

    /// Starts audio capture
    ///
    /// # Arguments
    /// * `tx` - Channel sender for audio samples
    /// * `running` - Atomic flag indicating whether the app is running
    /// * `recording` - Atomic flag indicating whether recording is active
    ///
    /// # Returns
    /// Result indicating success or error
    pub fn start(
        &mut self,
        tx: mpsc::Sender<Vec<f32>>,
        running: Arc<AtomicBool>,
        recording: Arc<AtomicBool>,
        transcription_stats: Arc<Mutex<TranscriptionStats>>,
    ) -> Result<(), anyhow::Error> {
        self.initialize_audio()?;

        let pa = self
            .pa
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("PortAudio not initialized"))?;
        let input_settings = self
            .input_settings
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Audio input settings not initialized"))?
            .clone();

        // Clone the recording Arc before moving it into the closure
        let recording_for_callback = recording.clone();
        let stats_for_callback = transcription_stats.clone();
        let samples_sent_for_callback = self.samples_sent.clone();

        let callback = move |pa::InputStreamCallbackArgs { buffer, .. }| {
            // Only send samples when recording is active
            if recording_for_callback.load(Ordering::Relaxed) {
                let samples = buffer.to_vec();
                match tx.try_send(samples) {
                    Ok(_) => {
                        // Increment counter after successful send
                        samples_sent_for_callback.fetch_add(1, Ordering::Release);
                    }
                    Err(mpsc::error::TrySendError::Full(_)) => {
                        if let Some(mut stats) = stats_for_callback.try_lock() {
                            let total = stats.record_audio_drop(1);
                            eprintln!("Audio channel full, dropped samples (total: {})", total);
                        }
                    }
                    Err(mpsc::error::TrySendError::Closed(_)) => {
                        eprintln!("Failed to send samples: channel closed");
                        if let Some(mut stats) = stats_for_callback.try_lock() {
                            let total = stats.record_audio_drop(1);
                            eprintln!("Audio channel drop recorded (total: {})", total);
                        }
                    }
                }
            }

            // Check if we should continue based on running flag
            if running.load(Ordering::Relaxed) {
                pa::Continue
            } else {
                pa::Complete
            }
        };

        let mut stream = pa
            .open_non_blocking_stream(input_settings, callback)
            .map_err(|e| anyhow::anyhow!("Failed to open stream: {}", e))?;

        // Only start the stream if recording is active
        if recording.load(Ordering::Relaxed) {
            stream
                .start()
                .map_err(|e| anyhow::anyhow!("Failed to start stream: {}", e))?;
        }

        self.pa_stream = Some(stream);
        Ok(())
    }

    /// Starts the PortAudio stream when recording begins
    pub fn start_recording(&mut self) -> Result<(), anyhow::Error> {
        if let Some(stream) = &mut self.pa_stream {
            if !stream.is_active().unwrap_or(false) {
                stream
                    .start()
                    .map_err(|e| anyhow::anyhow!("Failed to start recording: {}", e))?;
            }
        }
        Ok(())
    }

    /// Stops the PortAudio stream when recording ends (but keeps stream object)
    pub fn stop_recording(&mut self) -> Result<(), anyhow::Error> {
        if let Some(stream) = &mut self.pa_stream {
            if stream.is_active().unwrap_or(false) {
                stream
                    .stop()
                    .map_err(|e| anyhow::anyhow!("Failed to stop recording: {}", e))?;
            }
        }
        Ok(())
    }

    /// Gets the count of audio samples sent through the channel
    pub fn get_samples_sent_count(&self) -> Arc<AtomicUsize> {
        self.samples_sent.clone()
    }

    /// Temporarily pauses audio capture without closing the stream
    /// This allows for resuming the stream later
    ///
    /// # Returns
    /// Result indicating success or error
    pub fn pause(&mut self) -> Result<(), anyhow::Error> {
        self.stop_recording()
    }

    /// Resumes a previously paused audio capture stream
    ///
    /// # Returns
    /// Result indicating success or error
    pub fn resume(&mut self) -> Result<(), anyhow::Error> {
        self.start_recording()
    }

    /// Completely stops and cleans up the audio capture
    /// This closes the stream and releases resources
    pub fn stop(&mut self) {
        if let Some(stream) = &mut self.pa_stream {
            if let Err(e) = stream.stop() {
                eprintln!("Failed to stop stream: {}", e);
            }
            if let Err(e) = stream.close() {
                eprintln!("Failed to close stream: {}", e);
            }
        }
        self.pa_stream = None;
        self.pa = None;
        self.input_settings = None;
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        self.stop();
    }
}
