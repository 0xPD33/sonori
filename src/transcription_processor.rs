use parking_lot::{Mutex, RwLock};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{broadcast, mpsc};

use crate::backend::TranscriptionBackend;
use crate::config::read_app_config;
use crate::post_processor;
use crate::silero_audio_processor::{AudioSegment, SileroVad, VadConfig, VadState};
use crate::transcription_stats::TranscriptionStats;
use crate::ui::common::{AudioVisualizationData, ProcessingState};

/// Extract the last N words from text for use as a prompt
fn extract_prompt_context(text: &str, max_words: usize) -> String {
    let word_count = text.split_whitespace().count();
    if word_count <= max_words {
        text.to_string()
    } else {
        text.split_whitespace()
            .skip(word_count - max_words)
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Find natural pause points in audio using VAD.
/// Returns sample indices where pauses occur (good places to split chunks).
fn find_pause_points(samples: &[f32], sample_rate: usize) -> Vec<usize> {
    let home_dir = match std::env::var("HOME") {
        Ok(h) => h,
        Err(_) => {
            println!("Could not get HOME directory, falling back to time-based chunking");
            return Vec::new();
        }
    };
    let model_path =
        std::path::PathBuf::from(format!("{}/.cache/sonori/models/silero_vad.onnx", home_dir));

    if !model_path.exists() {
        println!(
            "VAD model not found at {:?}, falling back to time-based chunking",
            model_path
        );
        return Vec::new();
    }

    // Create VAD with config tuned for finding pauses
    let config = VadConfig {
        threshold: 0.3, // Slightly higher threshold for clearer boundaries
        speech_end_threshold: 0.2,
        frame_size: 512,
        sample_rate,
        hangbefore_frames: 3,
        hangover_frames: 15, // Shorter hangover to detect pauses faster
        hop_samples: 160,
        max_buffer_duration: samples.len() + 1024,
        max_segment_count: 1000,
        silence_tolerance_frames: 3,
        speech_prob_smoothing: 0.3,
    };

    let mut vad = match SileroVad::new(config, &model_path) {
        Ok(v) => v,
        Err(e) => {
            println!(
                "Failed to initialize VAD: {:?}, falling back to time-based chunking",
                e
            );
            return Vec::new();
        }
    };

    // Process audio through VAD to track state transitions
    // Only consider pauses that last at least this long (filters out brief hesitations)
    let min_pause_duration_ms = 300;
    let min_pause_samples = (sample_rate * min_pause_duration_ms) / 1000;

    let mut pause_points = Vec::new();
    let frame_size = 512;
    let hop_samples = 160;
    let mut current_sample = 0;
    let mut was_speaking = false;
    let mut pause_start: Option<usize> = None;

    // Process in frames
    let mut frame = vec![0.0f32; frame_size];
    let mut buffer_pos = 0;

    for &sample in samples {
        frame[buffer_pos] = sample;
        buffer_pos += 1;

        if buffer_pos >= frame_size {
            if let Ok(state) = vad.process_frame(&frame, hop_samples) {
                let is_speaking = matches!(state, VadState::Speech | VadState::PossibleSpeech);

                // Detect transition from speech to silence (potential pause start)
                if was_speaking && !is_speaking {
                    pause_start = Some(current_sample);
                }

                // Detect transition from silence to speech (pause ended)
                // Only record if pause was long enough
                if !was_speaking && is_speaking {
                    if let Some(start) = pause_start {
                        let pause_duration = current_sample.saturating_sub(start);
                        if pause_duration >= min_pause_samples {
                            // Use the midpoint of the pause as the split point
                            pause_points.push(start + pause_duration / 2);
                        }
                    }
                    pause_start = None;
                }

                was_speaking = is_speaking;
            }

            // Slide the frame
            frame.copy_within(hop_samples.., 0);
            buffer_pos = frame_size - hop_samples;
            current_sample += hop_samples;
        }
    }

    // Handle trailing pause (audio ends in silence)
    if let Some(start) = pause_start {
        let pause_duration = current_sample.saturating_sub(start);
        if pause_duration >= min_pause_samples {
            pause_points.push(start + pause_duration / 2);
        }
    }

    pause_points
}

/// Handles the processing of audio segments for transcription
pub struct TranscriptionProcessor {
    backend: Arc<Mutex<Option<Arc<TranscriptionBackend>>>>,
    backend_ready: Arc<AtomicBool>,
    language: String,
    running: Arc<AtomicBool>,
    transcription_done_tx: mpsc::UnboundedSender<()>,
    transcription_stats: Arc<Mutex<TranscriptionStats>>,
    audio_visualization_data: Arc<RwLock<AudioVisualizationData>>,
    magic_mode_enabled: Arc<AtomicBool>,
    enhancement_model: Arc<Mutex<Option<Box<dyn crate::enhancement::EnhancementModel>>>>,
}

impl TranscriptionProcessor {
    pub fn new(
        backend: Arc<Mutex<Option<Arc<TranscriptionBackend>>>>,
        backend_ready: Arc<AtomicBool>,
        language: String,
        running: Arc<AtomicBool>,
        transcription_done_tx: mpsc::UnboundedSender<()>,
        transcription_stats: Arc<Mutex<TranscriptionStats>>,
        audio_visualization_data: Arc<RwLock<AudioVisualizationData>>,
        magic_mode_enabled: Arc<AtomicBool>,
    ) -> Self {
        Self {
            backend,
            backend_ready,
            language,
            running,
            transcription_done_tx,
            transcription_stats,
            audio_visualization_data,
            magic_mode_enabled,
            enhancement_model: Arc::new(Mutex::new(None)),
        }
    }

    /// Transcribe an audio segment using the backend.
    /// Optionally accepts an initial prompt for chunk continuity (whisper.cpp only; CT2 ignores it).
    fn transcribe_segment(
        backend: &Arc<Mutex<Option<Arc<TranscriptionBackend>>>>,
        segment: &AudioSegment,
        language: &str,
        stats: &Arc<Mutex<TranscriptionStats>>,
        audio_visualization_data: &Arc<RwLock<AudioVisualizationData>>,
        initial_prompt: Option<&str>,
    ) -> String {
        let mut app_config = read_app_config();
        let log_stats_enabled = app_config.debug_config.log_stats_enabled;

        // Set processing state to transcribing
        {
            let mut audio_data = audio_visualization_data.write();
            audio_data.set_processing_state(ProcessingState::Transcribing);
        }

        if log_stats_enabled {
            println!(
                "Transcribing segment from {:.2}s to {:.2}s{}",
                segment.start_time,
                segment.end_time,
                if initial_prompt.is_some() {
                    " (with prompt)"
                } else {
                    ""
                }
            );
        }

        let start_time = Instant::now();
        let segment_duration = (segment.end_time - segment.start_time) as f32;

        let backend_arc = {
            let lock = backend.lock();
            lock.as_ref().map(Arc::clone)
        }; // lock dropped here

        let Some(backend_ref) = backend_arc.as_ref() else {
            let total_duration = start_time.elapsed();
            if log_stats_enabled {
                println!(
                    "Backend not available (checked in {:.2}s)",
                    total_duration.as_secs_f32()
                );
            }
            {
                let mut audio_data = audio_visualization_data.write();
                audio_data.set_processing_state(ProcessingState::Idle);
            }
            return "[backend not available]".to_string();
        };

        // Apply initial prompt for whisper.cpp backend (if provided)
        if let Some(prompt) = initial_prompt {
            app_config.whisper_cpp_options.initial_prompt = Some(prompt.to_string());
        }
        let inference_start = Instant::now();

        let result = match &**backend_ref {
            crate::backend::TranscriptionBackend::CTranslate2(ct2_backend) => ct2_backend
                .transcribe(
                    &segment.samples,
                    language,
                    &app_config.common_transcription_options,
                    &app_config.ctranslate2_options,
                    segment.sample_rate,
                ),
            crate::backend::TranscriptionBackend::WhisperCpp(whisper_cpp_backend) => {
                whisper_cpp_backend.transcribe(
                    &segment.samples,
                    language,
                    &app_config.common_transcription_options,
                    &app_config.whisper_cpp_options,
                    segment.sample_rate,
                )
            }
            crate::backend::TranscriptionBackend::Moonshine(moonshine_backend) => moonshine_backend
                .transcribe(
                    &segment.samples,
                    language,
                    &app_config.common_transcription_options,
                    &app_config.moonshine_options,
                    segment.sample_rate,
                ),
            crate::backend::TranscriptionBackend::Parakeet => {
                Err(crate::backend::TranscriptionError::BackendNotImplemented(
                    "Parakeet backend not yet implemented".to_string(),
                ))
            }
        };

        let result = match result {
            Ok(transcription) => {
                let inference_duration = inference_start.elapsed();
                let total_duration = start_time.elapsed();
                let inference_secs = inference_duration.as_secs_f32();
                let total_secs = total_duration.as_secs_f32();

                if let Some(mut stats_lock) = stats.try_lock() {
                    stats_lock.update(segment_duration, inference_secs, total_secs);
                }

                if log_stats_enabled {
                    println!(
                        "Transcription timing: Segment length: {:.2}s, Inference time: {:.2}s, Total: {:.2}s, RTF: {:.2}",
                        segment_duration, inference_secs, total_secs, inference_secs / segment_duration
                    );
                    println!("Transcription (raw): '{}'", transcription);
                }

                let processed_transcription = post_processor::post_process_text(
                    transcription,
                    &app_config.post_process_config,
                );

                if log_stats_enabled {
                    println!("Transcription (processed): '{}'", processed_transcription);
                }

                {
                    let mut audio_data = audio_visualization_data.write();
                    audio_data.set_processing_state(ProcessingState::Idle);
                }

                processed_transcription
            }
            Err(e) => {
                let total_duration = start_time.elapsed();
                if log_stats_enabled {
                    println!(
                        "Transcription error after {:.2}s: {}",
                        total_duration.as_secs_f32(),
                        e
                    );
                }
                {
                    let mut audio_data = audio_visualization_data.write();
                    audio_data.set_processing_state(ProcessingState::Error);
                }
                format!("[transcription error: {}]", e)
            }
        };

        result
    }

    async fn process_segment(
        segment: AudioSegment,
        backend: Arc<Mutex<Option<Arc<TranscriptionBackend>>>>,
        language: String,
        stats: Arc<Mutex<TranscriptionStats>>,
        audio_visualization_data: Arc<RwLock<AudioVisualizationData>>,
        transcript_tx: broadcast::Sender<crate::real_time_transcriber::TranscriptionMessage>,
        magic_mode_enabled: Arc<AtomicBool>,
        enhancement_model: Arc<Mutex<Option<Box<dyn crate::enhancement::EnhancementModel>>>>,
        log_stats_enabled: bool,
    ) {
        let segment_info = format!(
            "Segment {:.2}s-{:.2}s",
            segment.start_time, segment.end_time
        );
        let start_time = Instant::now();

        let processing_result = tokio::task::spawn_blocking(move || {
            let session_id = segment.session_id.clone();

            if segment.is_manual {
                let mut transcription = Self::process_manual_segment(
                    &backend,
                    &segment,
                    &language,
                    &stats,
                    &audio_visualization_data,
                );

                // Apply LFM enhancement if magic mode is enabled
                if !transcription.is_empty() && magic_mode_enabled.load(Ordering::Relaxed) {
                    transcription = Self::enhance_transcription(&transcription, &enhancement_model);
                }

                if transcription.is_empty() {
                    println!("Manual transcription resulted in empty text");
                    None
                } else {
                    Some(crate::real_time_transcriber::TranscriptionMessage {
                        text: transcription,
                        session_id,
                    })
                }
            } else {
                let transcription = Self::transcribe_segment(
                    &backend,
                    &segment,
                    &language,
                    &stats,
                    &audio_visualization_data,
                    None,
                );

                if transcription.is_empty() {
                    None
                } else {
                    Some(crate::real_time_transcriber::TranscriptionMessage {
                        text: transcription,
                        session_id,
                    })
                }
            }
        })
        .await;

        match processing_result {
            Ok(Some(message)) => {
                if let Err(e) = transcript_tx.send(message) {
                    eprintln!("Failed to send transcription: {}", e);
                }
            }
            Ok(None) => {}
            Err(e) => eprintln!("Transcription worker task failed: {}", e),
        }

        if log_stats_enabled {
            println!(
                "Segment processing finished for {} in {:.2}s",
                segment_info,
                start_time.elapsed().as_secs_f32()
            );
        }
    }

    pub fn start(
        &self,
        mut segment_rx: mpsc::Receiver<AudioSegment>,
        transcript_tx: broadcast::Sender<crate::real_time_transcriber::TranscriptionMessage>,
    ) -> tokio::task::JoinHandle<()> {
        let backend = self.backend.clone();
        let backend_ready = self.backend_ready.clone();
        let language = self.language.clone();
        let running = self.running.clone();
        let transcription_done_tx = self.transcription_done_tx.clone();
        let transcription_stats = self.transcription_stats.clone();
        let audio_visualization_data = self.audio_visualization_data.clone();
        let magic_mode_enabled = self.magic_mode_enabled.clone();
        let enhancement_model = self.enhancement_model.clone();

        let app_config = read_app_config();
        let log_stats_enabled = app_config.debug_config.log_stats_enabled;

        // Spawn a dedicated task for transcription
        tokio::spawn(async move {
            println!("Transcription task started");

            // Wait for backend to be ready before processing segments
            println!("Waiting for transcription backend to initialize...");
            let warn_interval = std::time::Duration::from_secs(10);
            let mut last_warn = std::time::Instant::now();

            while !backend_ready.load(Ordering::Relaxed) {
                if !running.load(Ordering::Relaxed) {
                    println!("Transcription task shutting down before backend initialization");
                    return;
                }

                if last_warn.elapsed() >= warn_interval {
                    eprintln!(
                        "Backend is still initializing (>{}s); continuing to wait",
                        warn_interval.as_secs()
                    );
                    last_warn = std::time::Instant::now();
                }

                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }

            println!("Backend ready, starting transcription processing");

            // When recording is false, no segments are received from AudioProcessor,
            // so this task naturally idles until recording is resumed
            loop {
                // Check if we should shut down
                if !running.load(Ordering::Relaxed) {
                    // Before shutting down, process any remaining segments
                    while let Ok(segment) = segment_rx.try_recv() {
                        Self::process_segment(
                            segment,
                            backend.clone(),
                            language.clone(),
                            transcription_stats.clone(),
                            audio_visualization_data.clone(),
                            transcript_tx.clone(),
                            magic_mode_enabled.clone(),
                            enhancement_model.clone(),
                            log_stats_enabled,
                        )
                        .await;
                    }
                    break;
                }

                // Block on receiving segments without timeout - this is much more efficient
                match segment_rx.recv().await {
                    Some(segment) => {
                        Self::process_segment(
                            segment,
                            backend.clone(),
                            language.clone(),
                            transcription_stats.clone(),
                            audio_visualization_data.clone(),
                            transcript_tx.clone(),
                            magic_mode_enabled.clone(),
                            enhancement_model.clone(),
                            log_stats_enabled,
                        )
                        .await;
                    }
                    None => {
                        // Channel closed
                        break;
                    }
                }
            }

            println!("Transcription task shutting down");
            let _ = transcription_done_tx.send(());
        })
    }

    /// Process a manual mode segment with specialized handling for longer audio
    fn process_manual_segment(
        backend: &Arc<Mutex<Option<Arc<TranscriptionBackend>>>>,
        segment: &AudioSegment,
        language: &str,
        stats: &Arc<Mutex<TranscriptionStats>>,
        audio_visualization_data: &Arc<RwLock<AudioVisualizationData>>,
    ) -> String {
        let app_config = read_app_config();
        let start_time = Instant::now();
        let duration = segment.end_time - segment.start_time;

        println!("Processing manual segment: {:.2}s of audio", duration);

        // Check if user wants to disable chunking entirely (experimental mode)
        if app_config.manual_mode_config.disable_chunking {
            println!(
                "EXPERIMENTAL: Processing entire recording as single segment (chunking disabled)"
            );
            let result = Self::transcribe_segment(
                backend,
                segment,
                language,
                stats,
                audio_visualization_data,
                None,
            );
            let processing_time = start_time.elapsed();
            println!(
                "Manual segment processing completed in {:.2}s",
                processing_time.as_secs_f32()
            );
            return result;
        }

        // For very long segments, we might want to split them into smaller chunks
        // to avoid memory issues and improve processing reliability
        let chunk_threshold = app_config.manual_mode_config.chunk_duration_seconds as f64;
        if duration >= chunk_threshold {
            println!("Large manual segment detected, processing in chunks...");
            return Self::process_large_manual_segment(
                backend,
                segment,
                language,
                stats,
                audio_visualization_data,
            );
        }

        // Process normally for smaller manual segments
        let result = Self::transcribe_segment(
            backend,
            segment,
            language,
            stats,
            audio_visualization_data,
            None,
        );

        let processing_time = start_time.elapsed();
        println!(
            "Manual segment processing completed in {:.2}s",
            processing_time.as_secs_f32()
        );

        result
    }

    /// Process very large manual segments by splitting into chunks using VAD-guided boundaries.
    /// Finds natural pauses in speech to split at, avoiding mid-word cuts.
    /// Falls back to time-based splitting if no pauses found or continuous speech exceeds limits.
    fn process_large_manual_segment(
        backend: &Arc<Mutex<Option<Arc<TranscriptionBackend>>>>,
        segment: &AudioSegment,
        language: &str,
        stats: &Arc<Mutex<TranscriptionStats>>,
        audio_visualization_data: &Arc<RwLock<AudioVisualizationData>>,
    ) -> String {
        let app_config = read_app_config();
        let sample_rate = segment.sample_rate;
        let max_chunk_seconds = app_config.manual_mode_config.chunk_duration_seconds;
        let max_chunk_samples = (max_chunk_seconds as f64 * sample_rate as f64).round() as usize;
        let total_len = segment.samples.len();

        println!(
            "Processing {:.1}s of audio with VAD-guided chunking (max chunk: {:.1}s)",
            total_len as f64 / sample_rate as f64,
            max_chunk_seconds
        );

        // Find natural pause points using VAD
        let pause_points = find_pause_points(&segment.samples, sample_rate);

        if !pause_points.is_empty() {
            println!(
                "Found {} natural pause point(s) in audio",
                pause_points.len()
            );
        } else {
            println!("No natural pauses detected, using time-based chunking");
        }

        // Build chunk ranges using pause points as preferred boundaries
        let chunk_ranges =
            Self::build_vad_guided_chunks(total_len, max_chunk_samples, &pause_points, sample_rate);

        println!("Split into {} chunk(s)", chunk_ranges.len());

        // Transcribe each chunk with prompt conditioning for continuity
        let mut transcriptions: Vec<String> = Vec::new();
        let mut previous_text = String::new();
        const PROMPT_CONTEXT_WORDS: usize = 30;

        for (chunk_idx, (start_idx, end_idx)) in chunk_ranges.iter().enumerate() {
            let chunk_audio = segment.samples[*start_idx..*end_idx].to_vec();
            let chunk_start_time = segment.start_time + (*start_idx as f64 / sample_rate as f64);
            let chunk_end_time = segment.start_time + (*end_idx as f64 / sample_rate as f64);

            let chunk_segment = AudioSegment {
                samples: chunk_audio,
                start_time: chunk_start_time,
                end_time: chunk_end_time,
                sample_rate,
                session_id: segment.session_id.clone(),
                is_manual: segment.is_manual,
            };

            println!(
                "Processing chunk {}/{} ({:.1}s - {:.1}s, {:.1}s duration)",
                chunk_idx + 1,
                chunk_ranges.len(),
                chunk_start_time,
                chunk_end_time,
                chunk_end_time - chunk_start_time
            );

            // Use previous transcription as prompt for continuity (whisper.cpp only)
            let prompt = if !previous_text.is_empty() {
                Some(extract_prompt_context(&previous_text, PROMPT_CONTEXT_WORDS))
            } else {
                None
            };

            let chunk_transcription = Self::transcribe_segment(
                backend,
                &chunk_segment,
                language,
                stats,
                audio_visualization_data,
                prompt.as_deref(),
            );

            if !chunk_transcription.is_empty() {
                let trimmed = chunk_transcription.trim().to_string();
                if !trimmed.is_empty() {
                    transcriptions.push(trimmed.clone());
                    previous_text = trimmed;
                }
            }
        }

        // Combine all chunk transcriptions
        transcriptions.join(" ")
    }

    /// Build chunk ranges using VAD pause points as preferred split locations.
    /// Tries to split at natural pauses, falls back to time-based if needed.
    fn build_vad_guided_chunks(
        total_len: usize,
        max_chunk_samples: usize,
        pause_points: &[usize],
        sample_rate: usize,
    ) -> Vec<(usize, usize)> {
        let mut chunk_ranges: Vec<(usize, usize)> = Vec::new();
        let mut start_idx = 0;

        // Minimum chunk size (avoid tiny fragments)
        let min_chunk_samples = sample_rate * 2; // 2 seconds minimum

        while start_idx < total_len {
            let remaining = total_len - start_idx;

            // If remaining audio fits in one chunk, take it all
            if remaining <= max_chunk_samples {
                chunk_ranges.push((start_idx, total_len));
                break;
            }

            // Find the best pause point within our max chunk size
            let max_end = start_idx + max_chunk_samples;
            let best_pause = pause_points
                .iter()
                .filter(|&&p| p > start_idx + min_chunk_samples && p <= max_end)
                .max(); // Take the latest pause within range (largest chunk)

            let end_idx = if let Some(&pause) = best_pause {
                // Split at the natural pause
                println!(
                    "  Splitting at pause at {:.1}s",
                    pause as f64 / sample_rate as f64
                );
                pause
            } else {
                // No pause found, fall back to max chunk size
                max_end.min(total_len)
            };

            chunk_ranges.push((start_idx, end_idx));
            start_idx = end_idx;
        }

        // Merge trailing tiny chunk into previous if too short
        if chunk_ranges.len() > 1 {
            if let Some(&(last_start, last_end)) = chunk_ranges.last() {
                let last_len = last_end - last_start;
                if last_len < min_chunk_samples {
                    let len = chunk_ranges.len();
                    if let Some(prev_range) = chunk_ranges.get_mut(len - 2) {
                        let merged_len = last_end - prev_range.0;
                        // Only merge if result is within reasonable limits (45s)
                        if merged_len <= sample_rate * 45 {
                            prev_range.1 = last_end;
                            chunk_ranges.pop();
                        }
                    }
                }
            }
        }

        chunk_ranges
    }

    /// Enhance transcription using the configured enhancement model (llama.cpp/GGUF)
    /// Lazily loads the model if not already loaded
    fn enhance_transcription(
        transcription: &str,
        enhancement_model: &Arc<Mutex<Option<Box<dyn crate::enhancement::EnhancementModel>>>>,
    ) -> String {
        use crate::config::read_app_config;

        let config = read_app_config();
        let enhancement_config = &config.enhancement_config;

        // Try to get or load the enhancement model
        let mut model_guard = enhancement_model.lock();

        // If model is not loaded, try to load it based on config
        if model_guard.is_none() {
            // Get model path - download GGUF from HuggingFace if needed
            // Model format: "owner/repo/filename.gguf"
            let model_path = match &enhancement_config.model {
                Some(model) => {
                    if crate::download::is_enhancement_gguf_available(model) {
                        crate::download::get_enhancement_gguf_path(model).ok()
                    } else {
                        // Try to download (blocking - not ideal but works for now)
                        eprintln!("Enhancement model not found, attempting download...");
                        eprintln!("Model: {}", model);

                        // Use tokio runtime to download
                        let rt = tokio::runtime::Runtime::new().ok();
                        if let Some(rt) = rt {
                            match rt.block_on(crate::download::download_enhancement_gguf(model)) {
                                Ok(path) => Some(path),
                                Err(e) => {
                                    eprintln!("Failed to download enhancement model: {:?}", e);
                                    None
                                }
                            }
                        } else {
                            eprintln!("Failed to create tokio runtime for download");
                            None
                        }
                    }
                }
                None => {
                    eprintln!("No enhancement model configured");
                    eprintln!("Set [enhancement_config].model = \"owner/repo/filename.gguf\"");
                    None
                }
            };

            match model_path {
                Some(path) => {
                    if crate::enhancement::is_model_available(&path) {
                        println!("Loading enhancement model from: {:?}", path);
                        match crate::enhancement::load_model(&path) {
                            Ok(model) => {
                                println!(
                                    "Enhancement model '{}' loaded successfully",
                                    model.name()
                                );
                                *model_guard = Some(model);
                            }
                            Err(e) => {
                                eprintln!("Failed to load enhancement model: {:?}", e);
                                return transcription.to_string();
                            }
                        }
                    } else {
                        eprintln!("Enhancement model not found at: {:?}", path);
                        return transcription.to_string();
                    }
                }
                None => {
                    eprintln!("No enhancement model available!");
                    eprintln!("Configure [enhancement_config].model in config.toml");
                    return transcription.to_string();
                }
            }
        }

        // Now we have the model loaded, enhance the transcription
        if let Some(ref model) = *model_guard {
            let system_prompt = enhancement_config.system_prompt.as_deref();
            match model.enhance(transcription, system_prompt) {
                Ok(enhanced) => {
                    println!("Transcription enhanced successfully");
                    enhanced
                }
                Err(e) => {
                    eprintln!("Enhancement failed: {:?}", e);
                    transcription.to_string()
                }
            }
        } else {
            transcription.to_string()
        }
    }
}
