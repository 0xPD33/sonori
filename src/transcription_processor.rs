use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{broadcast, mpsc};

use crate::backend::TranscriptionBackend;
use crate::config::read_app_config;
use crate::post_processor;
use crate::silero_audio_processor::AudioSegment;
use crate::transcription_stats::TranscriptionStats;

/// Handles the processing of audio segments for transcription
pub struct TranscriptionProcessor {
    backend: Arc<Mutex<Option<TranscriptionBackend>>>,
    language: String,
    running: Arc<AtomicBool>,
    transcription_done_tx: mpsc::UnboundedSender<()>,
    transcription_stats: Arc<Mutex<TranscriptionStats>>,
}

impl TranscriptionProcessor {
    pub fn new(
        backend: Arc<Mutex<Option<TranscriptionBackend>>>,
        language: String,
        running: Arc<AtomicBool>,
        transcription_done_tx: mpsc::UnboundedSender<()>,
        transcription_stats: Arc<Mutex<TranscriptionStats>>,
    ) -> Self {
        Self {
            backend,
            language,
            running,
            transcription_done_tx,
            transcription_stats,
        }
    }

    /// Transcribe an audio segment using the backend
    fn transcribe_segment(
        backend: &Arc<Mutex<Option<TranscriptionBackend>>>,
        segment: &AudioSegment,
        language: &str,
        stats: &Arc<Mutex<TranscriptionStats>>,
    ) -> String {
        let app_config = read_app_config();
        let log_stats_enabled = app_config.debug_config.log_stats_enabled;

        if log_stats_enabled {
            println!(
                "Transcribing segment from {:.2}s to {:.2}s",
                segment.start_time, segment.end_time
            );
        }

        let start_time = Instant::now();
        let segment_duration = (segment.end_time - segment.start_time) as f32;

        // Get a lock on the backend and check if it's available
        let mut backend_lock = backend.lock();

        if backend_lock.is_none() {
            let total_duration = start_time.elapsed();

            if log_stats_enabled {
                println!(
                    "Backend not available (checked in {:.2}s)",
                    total_duration.as_secs_f32()
                );
            }

            return "[backend not available]".to_string();
        }

        // Generate with the backend while still holding the lock
        // Use backend-specific options from config
        let backend_ref = backend_lock.as_ref().unwrap();
        let inference_start = Instant::now();

        let result = match backend_ref {
            crate::backend::TranscriptionBackend::CTranslate2(ct2_backend) => {
                ct2_backend.transcribe(&segment.samples, language, &app_config.common_transcription_options, &app_config.ctranslate2_options, segment.sample_rate)
            }
            crate::backend::TranscriptionBackend::WhisperCpp(whisper_cpp_backend) => {
                whisper_cpp_backend.transcribe(&segment.samples, language, &app_config.common_transcription_options, &app_config.whisper_cpp_options, segment.sample_rate)
            }
            crate::backend::TranscriptionBackend::Parakeet => {
                Err(crate::backend::TranscriptionError::BackendNotImplemented(
                    "Parakeet backend not yet implemented".to_string()
                ))
            }
        };

        let result = match result {
            Ok(transcription) => {
                let inference_duration = inference_start.elapsed();
                let total_duration = start_time.elapsed();
                let inference_secs = inference_duration.as_secs_f32();
                let total_secs = total_duration.as_secs_f32();

                // Update statistics
                if let Some(mut stats_lock) = stats.try_lock() {
                    stats_lock.update(segment_duration, inference_secs, total_secs);
                }

                if log_stats_enabled {
                    println!(
                        "Transcription timing: Segment length: {:.2}s, Inference time: {:.2}s, Total processing time: {:.2}s, RTF: {:.2}",
                        segment_duration,
                        inference_secs,
                        total_secs,
                        inference_secs / segment_duration
                    );

                    println!("Transcription (raw): '{}'", transcription);
                }

                // Apply post-processing
                let processed_transcription = post_processor::post_process_text(transcription, &app_config.post_process_config);

                if log_stats_enabled {
                    println!("Transcription (processed): '{}'", processed_transcription);
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

                format!("[transcription error: {}]", e)
            }
        };

        drop(backend_lock);

        result
    }

    pub fn start(
        &self,
        mut segment_rx: mpsc::Receiver<AudioSegment>,
        transcript_tx: broadcast::Sender<crate::real_time_transcriber::TranscriptionMessage>,
    ) -> tokio::task::JoinHandle<()> {
        let backend = self.backend.clone();
        let language = self.language.clone();
        let running = self.running.clone();
        let transcription_done_tx = self.transcription_done_tx.clone();
        let transcription_stats = self.transcription_stats.clone();

        let app_config = read_app_config();
        let log_stats_enabled = app_config.debug_config.log_stats_enabled;

        // Spawn a dedicated task for transcription
        tokio::spawn(async move {
            println!("Transcription task started");

            // When recording is false, no segments are received from AudioProcessor,
            // so this task naturally idles until recording is resumed
            loop {
                // Check if we should shut down
                if !running.load(Ordering::Relaxed) {
                    // Before shutting down, process any remaining segments
                    while let Ok(segment) = segment_rx.try_recv() {
                        let segment_info = format!(
                            "Segment {:.2}s-{:.2}s",
                            segment.start_time, segment.end_time
                        );

                        let thread_start_time = Instant::now();

                        // Process remaining segments
                        let backend_clone = backend.clone();
                        let language_clone = language.clone();
                        let stats_clone = transcription_stats.clone();
                        let tx_clone = transcript_tx.clone();
                        let session_id = segment.session_id.clone();

                        tokio::task::spawn_blocking(move || {
                            let transcription = Self::transcribe_segment(
                                &backend_clone,
                                &segment,
                                &language_clone,
                                &stats_clone,
                            );

                            if !transcription.is_empty() {
                                let message = crate::real_time_transcriber::TranscriptionMessage {
                                    text: transcription,
                                    session_id,
                                };
                                if let Err(e) = tx_clone.send(message) {
                                    eprintln!("Failed to send transcription: {}", e);
                                }
                            }
                        });

                        let thread_processing_time = thread_start_time.elapsed();

                        if log_stats_enabled {
                            println!(
                                "Task processing started for {} - Setup time: {:.2}s",
                                segment_info,
                                thread_processing_time.as_secs_f32()
                            );
                        }
                    }
                    break;
                }

                // Block on receiving segments without timeout - this is much more efficient
                match segment_rx.recv().await {
                    Some(segment) => {
                        let segment_info = format!(
                            "Segment {:.2}s-{:.2}s",
                            segment.start_time, segment.end_time
                        );

                        let thread_start_time = Instant::now();

                        // Check if this is a manual mode segment (larger duration indicates batch processing)
                        let is_manual_segment = segment.end_time - segment.start_time > 5.0;

                        // Process in a separate task to avoid blocking
                        let backend_clone = backend.clone();
                        let language_clone = language.clone();
                        let stats_clone = transcription_stats.clone();
                        let tx_clone = transcript_tx.clone();
                        let session_id = segment.session_id.clone();

                        if is_manual_segment {
                            // Handle manual mode segments with longer timeout and batch processing
                            tokio::task::spawn_blocking(move || {
                                let transcription = Self::process_manual_segment(
                                    &backend_clone,
                                    &segment,
                                    &language_clone,
                                    &stats_clone,
                                );

                                if !transcription.is_empty() {
                                    let message = crate::real_time_transcriber::TranscriptionMessage {
                                        text: transcription,
                                        session_id,
                                    };
                                    if let Err(e) = tx_clone.send(message) {
                                        eprintln!("Failed to send manual transcription: {}", e);
                                    }
                                } else {
                                    println!("Manual transcription resulted in empty text");
                                }
                            });
                        } else {
                            // Handle real-time segments (existing behavior)
                            let session_id_rt = session_id.clone();
                            tokio::task::spawn_blocking(move || {
                                let transcription = Self::transcribe_segment(
                                    &backend_clone,
                                    &segment,
                                    &language_clone,
                                    &stats_clone,
                                );

                                if !transcription.is_empty() {
                                    let message = crate::real_time_transcriber::TranscriptionMessage {
                                        text: transcription,
                                        session_id: session_id_rt,
                                    };
                                    if let Err(e) = tx_clone.send(message) {
                                        eprintln!("Failed to send transcription: {}", e);
                                    }
                                }
                            });
                        }

                        let thread_processing_time = thread_start_time.elapsed();

                        if log_stats_enabled {
                            println!(
                                "Task processing started for {} - Setup time: {:.2}s",
                                segment_info,
                                thread_processing_time.as_secs_f32()
                            );
                        }
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
        backend: &Arc<Mutex<Option<TranscriptionBackend>>>,
        segment: &AudioSegment,
        language: &str,
        stats: &Arc<Mutex<TranscriptionStats>>,
    ) -> String {
        let app_config = read_app_config();
        let start_time = Instant::now();
        let duration = segment.end_time - segment.start_time;

        println!("Processing manual segment: {:.2}s of audio", duration);

        // Check if user wants to disable chunking entirely (experimental mode)
        if app_config.manual_mode_config.disable_chunking {
            println!("EXPERIMENTAL: Processing entire recording as single segment (chunking disabled)");
            let result = Self::transcribe_segment(backend, segment, language, stats);
            let processing_time = start_time.elapsed();
            println!(
                "Manual segment processing completed in {:.2}s",
                processing_time.as_secs_f32()
            );
            return result;
        }

        // For very long segments, we might want to split them into smaller chunks
        // to avoid memory issues and improve processing reliability
        if duration > 30.0 {
            println!("Large manual segment detected, processing in chunks...");
            return Self::process_large_manual_segment(
                backend,
                segment,
                language,
                stats,
            );
        }

        // Process normally for smaller manual segments
        let result = Self::transcribe_segment(backend, segment, language, stats);

        let processing_time = start_time.elapsed();
        println!(
            "Manual segment processing completed in {:.2}s",
            processing_time.as_secs_f32()
        );

        result
    }

    /// Process very large manual segments by splitting into chunks
    fn process_large_manual_segment(
        backend: &Arc<Mutex<Option<TranscriptionBackend>>>,
        segment: &AudioSegment,
        language: &str,
        stats: &Arc<Mutex<TranscriptionStats>>,
    ) -> String {
        let app_config = read_app_config();
        let sample_rate = segment.sample_rate;
        let chunk_duration_samples = 30 * sample_rate; // 30 seconds per chunk (Whisper training window)

        // Get overlap settings from config
        let use_overlap = app_config.manual_mode_config.enable_chunk_overlap;
        let overlap_seconds = app_config.manual_mode_config.chunk_overlap_seconds;
        let overlap_samples = if use_overlap {
            (overlap_seconds * sample_rate as f32) as usize
        } else {
            0
        };

        if use_overlap {
            println!(
                "Using {:.1}s overlap between chunks (config: enable_chunk_overlap=true, chunk_overlap_seconds={:.1})",
                overlap_seconds, overlap_seconds
            );
        } else {
            println!("No overlap between chunks (config: enable_chunk_overlap=false)");
        }

        let mut transcriptions = Vec::new();
        let mut start_idx = 0;

        while start_idx < segment.samples.len() {
            let end_idx = (start_idx + chunk_duration_samples).min(segment.samples.len());

            // Create chunk segment
            let chunk_audio = segment.samples[start_idx..end_idx].to_vec();
            let chunk_start_time = start_idx as f64 / sample_rate as f64;
            let chunk_end_time = end_idx as f64 / sample_rate as f64;

            let chunk_segment = AudioSegment {
                samples: chunk_audio,
                start_time: chunk_start_time,
                end_time: chunk_end_time,
                sample_rate,
                session_id: segment.session_id.clone(), // Inherit session ID from parent segment
            };

            println!(
                "Processing chunk {:.1}s - {:.1}s",
                chunk_start_time, chunk_end_time
            );

            // Transcribe chunk
            let chunk_transcription =
                Self::transcribe_segment(backend, &chunk_segment, language, stats);

            if !chunk_transcription.is_empty() {
                transcriptions.push(chunk_transcription.trim().to_string());
            }

            // Move to next chunk (with optional overlap for boundary word handling)
            // Industry best practice: use small overlap (0.5-1.0s) to catch boundary words
            // while avoiding the hallucination caused by large overlaps (2+ seconds)
            if end_idx >= segment.samples.len() {
                break;
            }

            // Calculate next start position based on overlap setting
            if use_overlap {
                start_idx = (end_idx as isize - overlap_samples as isize).max(0) as usize;
            } else {
                start_idx = end_idx;
            }
        }

        // Combine all chunk transcriptions
        transcriptions.join(" ")
    }
}
