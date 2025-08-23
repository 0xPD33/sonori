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

                        // Check if this is a manual mode segment (larger duration indicates batch processing)
                        let is_manual_segment = segment.end_time - segment.start_time > 5.0;

                        // Process in a separate task to avoid blocking
                        let whisper_clone = whisper.clone();
                        let language_clone = language.clone();
                        let options_clone = options.clone();
                        let stats_clone = transcription_stats.clone();
                        let tx_clone = transcript_tx.clone();

                        if is_manual_segment {
                            // Handle manual mode segments with longer timeout and batch processing
                            tokio::task::spawn_blocking(move || {
                                let transcription = Self::process_manual_segment(
                                    &whisper_clone,
                                    &segment,
                                    &language_clone,
                                    &options_clone,
                                    &stats_clone,
                                );

                                if !transcription.is_empty() {
                                    if let Err(e) = tx_clone.send(transcription) {
                                        eprintln!("Failed to send manual transcription: {}", e);
                                    }
                                } else {
                                    println!("Manual transcription resulted in empty text");
                                }
                            });
                        } else {
                            // Handle real-time segments (existing behavior)
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
        whisper: &Arc<Mutex<Option<Whisper>>>,
        segment: &AudioSegment,
        language: &str,
        options: &WhisperOptions,
        stats: &Arc<Mutex<TranscriptionStats>>,
    ) -> String {
        let start_time = Instant::now();
        let duration = segment.end_time - segment.start_time;
        
        println!("Processing manual segment: {:.2}s of audio", duration);
        
        // For very long segments, we might want to split them into smaller chunks
        // to avoid memory issues and improve processing reliability
        if duration > 30.0 {
            println!("Large manual segment detected, processing in chunks...");
            // For manual segments, we need to estimate sample rate from the segment data
            let estimated_sample_rate = (segment.samples.len() as f64 / duration) as usize;
            return Self::process_large_manual_segment(whisper, segment, language, options, stats, estimated_sample_rate);
        }
        
        // Process normally for smaller manual segments
        let result = transcribe_with_whisper(whisper, segment, language, options, stats);
        
        let processing_time = start_time.elapsed();
        println!("Manual segment processing completed in {:.2}s", processing_time.as_secs_f32());
        
        result
    }

    /// Process very large manual segments by splitting into chunks
    fn process_large_manual_segment(
        whisper: &Arc<Mutex<Option<Whisper>>>,
        segment: &AudioSegment,
        language: &str,
        options: &WhisperOptions,
        stats: &Arc<Mutex<TranscriptionStats>>,
        sample_rate: usize,
    ) -> String {
        let chunk_duration_samples = 30 * sample_rate; // 30 seconds per chunk
        let overlap_samples = 2 * sample_rate; // 2 seconds overlap to avoid cutting words
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
            };
            
            println!("Processing chunk {:.1}s - {:.1}s", chunk_start_time, chunk_end_time);
            
            // Transcribe chunk
            let chunk_transcription = transcribe_with_whisper(
                whisper, 
                &chunk_segment, 
                language, 
                options, 
                stats
            );
            
            if !chunk_transcription.is_empty() {
                transcriptions.push(chunk_transcription.trim().to_string());
            }
            
            // Move to next chunk with overlap to avoid cutting words
            if end_idx >= segment.samples.len() {
                break;
            }
            start_idx = end_idx - overlap_samples;
        }
        
        // Combine all chunk transcriptions
        let combined = transcriptions.join(" ");
        
        // Clean up potential word duplicates from overlapping chunks
        Self::clean_overlap_duplicates(&combined)
    }

    /// Clean up potential word duplicates from overlapping chunk processing
    fn clean_overlap_duplicates(text: &str) -> String {
        let words: Vec<&str> = text.split_whitespace().collect();
        if words.is_empty() {
            return String::new();
        }
        
        let mut cleaned_words = vec![words[0]];
        
        for i in 1..words.len() {
            let current_word = words[i].to_lowercase();
            let previous_word = words[i-1].to_lowercase();
            
            // Skip if current word is the same as previous (likely overlap duplicate)
            if current_word != previous_word {
                cleaned_words.push(words[i]);
            }
        }
        
        cleaned_words.join(" ")
    }
}
