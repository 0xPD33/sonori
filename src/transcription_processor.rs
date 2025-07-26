use ct2rs::{Whisper, WhisperOptions};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc};

use crate::config::read_app_config;
use crate::silero_audio_processor::AudioSegment;
use crate::transcribe::transcribe_with_whisper;
use crate::transcription_stats::TranscriptionStats;

/// Handles the processing of audio segments for transcription
pub struct TranscriptionProcessor {
    whisper: Arc<Mutex<Option<Whisper>>>,
    language: String,
    options: WhisperOptions,
    running: Arc<AtomicBool>,
    transcription_done_tx: mpsc::UnboundedSender<()>,
    transcription_stats: Arc<Mutex<TranscriptionStats>>,
}

impl TranscriptionProcessor {
    pub fn new(
        whisper: Arc<Mutex<Option<Whisper>>>,
        language: String,
        options: WhisperOptions,
        running: Arc<AtomicBool>,
        transcription_done_tx: mpsc::UnboundedSender<()>,
        transcription_stats: Arc<Mutex<TranscriptionStats>>,
    ) -> Self {
        Self {
            whisper,
            language,
            options,
            running,
            transcription_done_tx,
            transcription_stats,
        }
    }

    pub fn start(
        &self,
        mut segment_rx: mpsc::Receiver<AudioSegment>,
        transcript_tx: broadcast::Sender<String>,
    ) -> tokio::task::JoinHandle<()> {
        let whisper = self.whisper.clone();
        let language = self.language.clone();
        let options = self.options.clone();
        let running = self.running.clone();
        let transcription_done_tx = self.transcription_done_tx.clone();
        let transcription_stats = self.transcription_stats.clone();

        let app_config = read_app_config();
        let log_stats_enabled = app_config.log_stats_enabled;

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
                        let whisper_clone = whisper.clone();
                        let language_clone = language.clone();
                        let options_clone = options.clone();
                        let stats_clone = transcription_stats.clone();
                        let tx_clone = transcript_tx.clone();

                        tokio::task::spawn_blocking(move || {
                            let transcription = transcribe_with_whisper(
                                &whisper_clone,
                                &segment,
                                &language_clone,
                                &options_clone,
                                &stats_clone,
                            );

                            if !transcription.is_empty() {
                                if let Err(e) = tx_clone.send(transcription) {
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

                        // Process in a separate task to avoid blocking
                        let whisper_clone = whisper.clone();
                        let language_clone = language.clone();
                        let options_clone = options.clone();
                        let stats_clone = transcription_stats.clone();
                        let tx_clone = transcript_tx.clone();

                        // Spawn a dedicated task for the actual transcription work
                        // Pass the segment by value to avoid extra allocation
                        tokio::task::spawn_blocking(move || {
                            let transcription = transcribe_with_whisper(
                                &whisper_clone,
                                &segment,
                                &language_clone,
                                &options_clone,
                                &stats_clone,
                            );

                            if !transcription.is_empty() {
                                if let Err(e) = tx_clone.send(transcription) {
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
}
