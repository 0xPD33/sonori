use ndarray::{s, Array, Array2, ArrayBase, ArrayD, Dim, IxDynImpl, OwnedRepr};
use ort::session::builder::GraphOptimizationLevel;
use ort::session::{Session, SessionInputs};
use ort::value::Tensor;
use std::collections::VecDeque;
use std::path::Path;
use std::time::Duration;

/// Voice Activity Detection states
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VadState {
    Silence,
    PossibleSpeech,
    Speech,
    PossibleSilence,
}

/// Audio segment containing speech
#[derive(Debug, Clone)]
pub struct AudioSegment {
    pub samples: Vec<f32>,
    pub start_time: f64,
    pub end_time: f64,
}

/// Configuration for Voice Activity Detection
#[derive(Debug, Clone)]
pub struct VadConfig {
    /// Probability threshold for speech detection (0.0-1.0)
    pub threshold: f32,
    /// Size of audio frames in samples
    pub frame_size: usize,
    /// Audio sample rate in Hz (8000 or 16000)
    pub sample_rate: usize,
    /// Number of frames before confirming speech
    pub hangbefore_frames: usize,
    /// Number of frames after speech before silence
    pub hangover_frames: usize,
    /// Maximum buffer size in samples
    pub max_buffer_duration: usize,
    /// Maximum number of segments to process at once
    pub max_segment_count: usize,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            threshold: 0.2,
            frame_size: 1024,            // ~64ms at 16kHz
            sample_rate: 16000,          // 16kHz (supported by Silero VAD)
            hangbefore_frames: 1,        // Frames before confirming speech
            hangover_frames: 15,         // Frames after speech before silence
            max_buffer_duration: 480000, // 30 seconds at 16kHz
            max_segment_count: 20,       // Maximum segments to keep in memory
        }
    }
}

/// Represents the sample rate for the Silero VAD model
#[derive(Debug, Clone, Copy)]
pub enum SampleRate {
    EightkHz,
    SixteenkHz,
}

impl From<SampleRate> for i64 {
    fn from(value: SampleRate) -> Self {
        match value {
            SampleRate::EightkHz => 8000,
            SampleRate::SixteenkHz => 16000,
        }
    }
}

impl From<SampleRate> for usize {
    fn from(value: SampleRate) -> Self {
        match value {
            SampleRate::EightkHz => 8000,
            SampleRate::SixteenkHz => 16000,
        }
    }
}

impl From<usize> for SampleRate {
    fn from(value: usize) -> Self {
        match value {
            8000 => SampleRate::EightkHz,
            _ => SampleRate::SixteenkHz,
        }
    }
}

#[derive(Debug)]
pub struct SileroVad {
    session: Session,
    sample_rate: ArrayBase<OwnedRepr<i64>, Dim<[usize; 1]>>,
    state: ArrayBase<OwnedRepr<f32>, Dim<IxDynImpl>>,
    config: VadConfig,
    buffer: VecDeque<f32>,
    speeches: Vec<AudioSegment>,
    current_state: VadState,
    frames_in_state: usize,
    silence_frames: usize,
    current_time: f64,
    time_offset: f64,
    speech_start_time: Option<f64>,
    sample_buffer: Vec<f32>,
    frame_buffer: Array2<f32>,
    sample_rate_f64: f64,
    segment_buffer: Vec<f32>,
    frame_counter: usize,
    buffer_check_interval: usize,
    samples_since_trim: usize,
    trim_threshold: usize,
}

impl SileroVad {
    pub fn new(config: VadConfig, model_path: impl AsRef<Path>) -> Result<Self, ort::Error> {
        let sample_rate: SampleRate = config.sample_rate.into();
        let frame_size = config.frame_size;

        // Create ONNX session with optimized settings and limited threading
        let session = Session::builder()?
            .with_optimization_level(GraphOptimizationLevel::Level3)?
            .with_intra_threads(1)?  // Single thread for individual operations
            .with_inter_threads(1)?  // Single thread for operator parallelism
            .commit_from_file(model_path)?;

        // Initialize model state
        let state = ArrayD::<f32>::zeros([2, 1, 128].as_slice());
        let sample_rate_arr = Array::from_shape_vec([1], vec![i64::from(sample_rate)]).unwrap();

        let frame_buffer = Array2::<f32>::zeros((1, frame_size));

        // Precompute derived values
        let sample_rate_f64 = config.sample_rate as f64;
        let max_buffer_duration = config.max_buffer_duration;
        let max_segment_count = config.max_segment_count;

        let buffer_check_interval = 30; // Check buffer every 30 frames
        let trim_threshold = frame_size * 60; // Check for trim after ~60 frames of data

        // Pre-allocate buffers with capacity
        let buffer = VecDeque::with_capacity(frame_size * 2);
        let speeches = Vec::with_capacity(max_segment_count);
        let sample_buffer = Vec::with_capacity(max_buffer_duration);
        let segment_buffer = Vec::with_capacity(max_buffer_duration / 2); // Half max_buffer for segments

        Ok(Self {
            session,
            sample_rate: sample_rate_arr,
            state,
            config,
            buffer,
            speeches,
            current_state: VadState::Silence,
            frames_in_state: 0,
            silence_frames: 0,
            current_time: 0.0,
            time_offset: 0.0,
            speech_start_time: None,
            sample_buffer,
            frame_buffer,
            sample_rate_f64,
            segment_buffer,
            frame_counter: 0,
            buffer_check_interval,
            samples_since_trim: 0,
            trim_threshold,
        })
    }

    /// Reset the model state
    pub fn reset(&mut self) {
        self.state = ArrayD::<f32>::zeros([2, 1, 128].as_slice());
        self.buffer.clear();
        self.speeches.clear();
        self.current_state = VadState::Silence;
        self.frames_in_state = 0;
        self.silence_frames = 0;
        self.current_time = 0.0;
        self.time_offset = 0.0;
        self.speech_start_time = None;
        self.sample_buffer.clear();
        self.frame_counter = 0;
        self.samples_since_trim = 0;
        println!("SileroVad state has been reset");
    }

    /// Calculate speech probability for an audio frame
    fn calc_speech_prob(&mut self, audio_frame: &[f32]) -> Result<f32, ort::Error> {
        // Silero model expects frames of exactly 512 samples (and internal slicing to 480)
        let frame_len = audio_frame.len().min(512);

        if frame_len == audio_frame.len() {
            // Only fill the portion of the buffer we'll use
            for i in 0..frame_len {
                self.frame_buffer[[0, i]] = audio_frame[i];
            }
        } else {
            // We need to adjust the frame size
            for i in 0..frame_len {
                self.frame_buffer[[0, i]] = if i < audio_frame.len() {
                    audio_frame[i]
                } else {
                    0.0
                };
            }
        }

        // Slice to the correct length
        let frame = self.frame_buffer.slice(s![.., ..frame_len]);

        // Convert ndarrays to ort tensors
        let frame_tensor = Tensor::from_array(frame.to_owned())?;
        let state_tensor = Tensor::from_array(std::mem::take(&mut self.state))?;
        let sample_rate_tensor = Tensor::from_array(self.sample_rate.to_owned())?;

        // Run inference
        let inps = ort::inputs![
            frame_tensor,
            state_tensor,
            sample_rate_tensor,
        ];

        let res = self.session.run(SessionInputs::ValueSlice::<3>(&inps))?;

        // Update internal state
        self.state = res["stateN"].try_extract_array().unwrap().to_owned();

        // Extract and return the speech probability
        let output_tensor = res["output"].try_extract_tensor::<f32>().unwrap();
        Ok(output_tensor.1[0])
    }

    /// Process a frame of audio samples and update VAD state
    pub fn process_frame(&mut self, frame: &[f32]) -> Result<VadState, ort::Error> {
        let speech_prob = self.calc_speech_prob(frame)?;

        // Update VAD state first, before changing time or buffer
        self.update_vad_state(speech_prob);

        // Update current time
        let time_increment = frame.len() as f64 / self.sample_rate_f64;
        self.current_time += time_increment;

        // Add frame to sample buffer
        self.sample_buffer.extend_from_slice(frame);

        // Track number of samples added since last trim
        self.samples_since_trim += frame.len();

        // Only check buffer size every N frames
        self.frame_counter += 1;
        if self.frame_counter >= self.buffer_check_interval {
            self.frame_counter = 0;
            self.trim_buffer_if_needed();
        }

        Ok(self.current_state)
    }

    /// Trim the buffer if it exceeds the maximum size
    fn trim_buffer_if_needed(&mut self) {
        if self.sample_buffer.len() <= self.config.max_buffer_duration {
            return;
        }

        // Calculate trim parameters
        let excess = self.sample_buffer.len() - self.config.max_buffer_duration;
        let time_trimmed = excess as f64 / self.sample_rate_f64;
        let new_time_offset = self.time_offset + time_trimmed;

        // This function does the actual trimming work, reused for both trim cases
        self.trim_buffer(excess, new_time_offset);
    }

    /// Trim buffer by specified number of samples, updating time offset
    fn trim_buffer(&mut self, trim_samples: usize, new_time_offset: f64) {
        if trim_samples == 0 {
            return;
        }

        if let Some(start_time) = self.speech_start_time {
            if start_time < new_time_offset {
                if cfg!(debug_assertions) {
                    println!("Speech crosses buffer at {:.2}s", new_time_offset);
                }

                // Create a segment for the part being trimmed
                let segment = AudioSegment {
                    samples: self.extract_speech_segment(start_time, new_time_offset),
                    start_time,
                    end_time: new_time_offset,
                };

                if !segment.samples.is_empty() {
                    self.speeches.push(segment);

                    if self.speeches.len() > self.config.max_segment_count {
                        self.speeches.remove(0);
                    }
                }

                self.speech_start_time = Some(new_time_offset);
            }
        }

        // Use drain for efficiency
        self.sample_buffer.drain(0..trim_samples);
        self.time_offset = new_time_offset;
    }

    /// Update the VAD state based on speech probability
    fn update_vad_state(&mut self, speech_prob: f32) {
        let threshold = self.config.threshold;
        let is_speech = speech_prob > threshold;

        let hangbefore_frames = self.config.hangbefore_frames;
        let hangover_frames = self.config.hangover_frames;

        match self.current_state {
            VadState::Silence => {
                if is_speech {
                    self.current_state = VadState::PossibleSpeech;
                    self.frames_in_state = 1;
                    if cfg!(debug_assertions) {
                        println!("Silence → PossibleSpeech");
                    }
                }
            }
            VadState::PossibleSpeech => {
                if is_speech {
                    self.frames_in_state += 1;
                    self.silence_frames = 0;

                    if self.frames_in_state >= hangbefore_frames {
                        // Precompute values needed for start time calculation
                        let frames_to_time = hangbefore_frames as f64
                            * self.config.frame_size as f64
                            / self.sample_rate_f64;

                        // Set speech start time, accounting for the hangbefore frames
                        let start_time = self.current_time - frames_to_time;

                        self.speech_start_time = Some(start_time);
                        self.current_state = VadState::Speech;
                        self.frames_in_state = 0;

                        if cfg!(debug_assertions) {
                            println!("PossibleSpeech → Speech (start: {:.2}s)", start_time);
                        }
                    }
                } else {
                    // Add tolerance for noise fluctuations
                    self.silence_frames += 1;
                    let silence_tolerance: usize = 2;

                    if self.silence_frames >= silence_tolerance {
                        self.current_state = VadState::Silence;
                        self.frames_in_state = 0;
                        self.silence_frames = 0;
                        if cfg!(debug_assertions) {
                            println!("PossibleSpeech → Silence (after tolerance)");
                        }
                    } else if cfg!(debug_assertions) {
                        println!(
                            "PossibleSpeech → Still in possible speech (silence: {}/{})",
                            self.silence_frames, silence_tolerance
                        );
                    }
                }
            }
            VadState::Speech => {
                if !is_speech {
                    self.current_state = VadState::PossibleSilence;
                    self.frames_in_state = 1;
                    if cfg!(debug_assertions) {
                        println!("Speech → PossibleSilence");
                    }
                }
            }
            VadState::PossibleSilence => {
                if !is_speech {
                    self.frames_in_state += 1;
                    if self.frames_in_state >= hangover_frames {
                        self.current_state = VadState::Silence;
                        self.frames_in_state = 0;

                        if cfg!(debug_assertions) {
                            println!("PossibleSilence → Silence (end: {:.2}s)", self.current_time);
                        }

                        // Finalize the speech segment if we have one
                        self.finalize_speech_segment();
                    }
                } else {
                    self.current_state = VadState::Speech;
                    self.frames_in_state = 0;
                    if cfg!(debug_assertions) {
                        println!("PossibleSilence → Speech");
                    }
                }
            }
        }
    }

    /// Finalize a speech segment when transitioning to silence
    fn finalize_speech_segment(&mut self) {
        if let Some(start_time) = self.speech_start_time.take() {
            let segment = AudioSegment {
                samples: self.extract_speech_segment(start_time, self.current_time),
                start_time,
                end_time: self.current_time,
            };

            if !segment.samples.is_empty() {
                // Add the segment and cap the total number
                self.speeches.push(segment);
                if self.speeches.len() > self.config.max_segment_count {
                    self.speeches.remove(0);
                }
            }
        }
    }

    /// Extract speech segment from the sample history
    fn extract_speech_segment(&mut self, start_time: f64, end_time: f64) -> Vec<f32> {
        // Precompute constants once
        let context_duration = 0.03; // 30ms context
        let context_samples = (context_duration * self.sample_rate_f64) as usize;

        // Check if we're potentially losing the beginning of speech due to buffer limits
        if start_time < self.time_offset && cfg!(debug_assertions) {
            println!(
                "Warning: Speech segment starts before buffer window: start={:.3}s, buffer_start={:.3}s",
                start_time, self.time_offset
            );
        }

        // Adjust times for the current buffer window - doing calculations only once
        let adjusted_start = (start_time - self.time_offset - context_duration).max(0.0);
        let adjusted_end = (end_time - self.time_offset).max(0.0);

        // Convert to sample indices within buffer bounds, with context window
        // Cache the sample rate conversion to avoid repeated multiplication
        let sample_idx_converter = |time: f64| -> usize { (time * self.sample_rate_f64) as usize };

        let start_idx = sample_idx_converter(adjusted_start)
            .saturating_sub(context_samples)
            .min(self.sample_buffer.len());

        let end_idx = sample_idx_converter(adjusted_end).min(self.sample_buffer.len());

        // Check for valid indices
        if start_idx >= end_idx || start_idx >= self.sample_buffer.len() {
            if cfg!(debug_assertions) {
                println!(
                    "Invalid segment: {:.2}s-{:.2}s (offset: {:.2}s)",
                    start_time, end_time, self.time_offset
                );
            }
            return Vec::new();
        }

        // Get a slice of the buffer and convert to Vec directly
        self.sample_buffer[start_idx..end_idx].to_vec()
    }

    /// Process a batch of audio samples
    pub fn process_audio(&mut self, samples: &[f32]) -> Result<Vec<AudioSegment>, ort::Error> {
        if samples.is_empty() {
            return Ok(Vec::new());
        }

        // Pre-allocate frame vector once and reuse it
        let frame_size = self.config.frame_size;
        let mut frame = Vec::with_capacity(frame_size);

        // Add the new samples to our buffer
        self.buffer.extend(samples);

        // Process as many full frames as we can
        while self.buffer.len() >= frame_size {
            frame.clear();

            // Extract exactly frame_size samples into our frame buffer
            let drain_size = frame_size.min(self.buffer.len());
            frame.extend(self.buffer.drain(0..drain_size));

            // Process this frame
            self.process_frame(&frame)?;
        }

        // Only process partial frames if they are at least 1/4 of a frame
        // This reduces CPU impact while still capturing significant speech
        let partial_threshold = frame_size / 4;

        if !self.buffer.is_empty() && self.buffer.len() >= partial_threshold {
            frame.clear();
            frame.resize(frame_size, 0.0); // Fill with zeros to complete the frame

            // Copy the remaining samples into the frame
            let remaining = self.buffer.len();
            frame[0..remaining].copy_from_slice(&self.buffer.make_contiguous()[0..remaining]);
            self.buffer.clear();

            // Process this partial frame
            self.process_frame(&frame)?;
        }

        // Only check for proactive trimming when we've added enough
        // new samples to potentially require it
        if self.samples_since_trim >= self.trim_threshold {
            self.samples_since_trim = 0;

            // Proactively trim sample buffer to prevent excessive memory growth
            let max_buffer = self.config.max_buffer_duration;
            let current_size = self.sample_buffer.len();

            // If buffer exceeds 75% of max, trim it to 50% for headroom
            if current_size > max_buffer * 3 / 4 {
                let target_size = max_buffer / 2;
                let excess = current_size - target_size;

                // Calculate time offset change
                let time_trimmed = excess as f64 / self.sample_rate_f64;
                let new_time_offset = self.time_offset + time_trimmed;

                // Use the common trim function
                self.trim_buffer(excess, new_time_offset);
            }
        }

        // If we have any speeches, return them and clear our buffer
        if self.speeches.is_empty() {
            Ok(Vec::new())
        } else {
            let speeches = std::mem::take(&mut self.speeches);
            Ok(speeches)
        }
    }

    /// Get current VAD state
    #[inline]
    pub fn get_state(&self) -> VadState {
        self.current_state
    }

    /// Check if currently in speech state
    #[inline]
    pub fn is_speaking(&self) -> bool {
        self.current_state == VadState::Speech || self.current_state == VadState::PossibleSpeech
    }

    /// Get duration of current speech if any
    #[inline]
    pub fn get_current_speech_duration(&self) -> Option<Duration> {
        self.speech_start_time.map(|start| {
            let duration_secs = self.current_time - start;
            Duration::from_secs_f64(duration_secs)
        })
    }

    /// Get detected speech segments
    #[inline]
    pub fn get_speeches(&self) -> &[AudioSegment] {
        &self.speeches
    }

    /// Drain detected speech segments
    #[inline]
    pub fn drain_speeches(&mut self) -> Vec<AudioSegment> {
        std::mem::take(&mut self.speeches)
    }

    /// Get current speech segment if active
    pub fn get_current_speech(&mut self) -> Option<AudioSegment> {
        if self.is_speaking() && self.speech_start_time.is_some() {
            let start_time = self.speech_start_time.unwrap();
            Some(AudioSegment {
                samples: self.extract_speech_segment(start_time, self.current_time),
                start_time,
                end_time: self.current_time,
            })
        } else {
            None
        }
    }
}
