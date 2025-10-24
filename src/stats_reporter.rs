use anyhow::Result;
use chrono;
use parking_lot::Mutex;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::config::read_app_config;
use crate::transcription_stats::TranscriptionStats;

const STATS_INTERVAL_SECS: u64 = 10;

/// Handles reporting of transcription statistics
pub struct StatsReporter {
    transcription_stats: Arc<Mutex<TranscriptionStats>>,
    running: Arc<AtomicBool>,
}

impl StatsReporter {
    /// Creates a new StatsReporter
    pub fn new(
        transcription_stats: Arc<Mutex<TranscriptionStats>>,
        running: Arc<AtomicBool>,
    ) -> Self {
        Self {
            transcription_stats,
            running,
        }
    }

    /// Start periodic reporting with specified interval
    pub fn start_periodic_reporting(&self) {
        // Get configuration options
        let app_config = read_app_config();
        let log_stats_enabled = app_config.debug_config.log_stats_enabled;

        // Exit early if stats logging is not enabled
        if !log_stats_enabled {
            println!("Stats reporting disabled - no statistics will be logged");
            return;
        }

        println!(
            "Stats reporting enabled - will report every {} seconds",
            STATS_INTERVAL_SECS
        );

        let transcription_stats = self.transcription_stats.clone();
        let running = self.running.clone();

        // Create stats file
        println!("Stats logging enabled - will write to console and transcription_stats.log");

        // Create or truncate the stats file
        if let Err(e) = File::create("transcription_stats.log") {
            eprintln!("Failed to create stats file: {}", e);
        }

        // Spawn an async task to periodically report transcription statistics
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(STATS_INTERVAL_SECS));
            while running.load(Ordering::Relaxed) {
                interval.tick().await;
                if let Some(stats) = transcription_stats.try_lock() {
                    if stats.segments_processed > 0 {
                        let stats_report = stats.report();
                        println!("\n--- Periodic Transcription Statistics ---");
                        println!("{}", stats_report);
                        println!("------------------------------------------\n");
                        let timestamp =
                            chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
                        let file_content = format!("\n--- {} ---\n{}\n", timestamp, stats_report);
                        match std::fs::OpenOptions::new()
                            .append(true)
                            .create(true)
                            .open("transcription_stats.log")
                        {
                            Ok(mut file) => {
                                if let Err(e) = writeln!(file, "{}", file_content) {
                                    eprintln!("Failed to write to stats file: {}", e);
                                }
                            }
                            Err(e) => eprintln!("Failed to open stats file: {}", e),
                        }
                    }
                }
            }
            println!("Stats reporting stopped");
        });
    }

    /// Print current statistics on demand
    pub fn print_stats(&self) {
        // Get configuration options
        let app_config = read_app_config();
        let log_stats_enabled = app_config.debug_config.log_stats_enabled;

        // Exit early if stats logging is not enabled
        if !log_stats_enabled {
            println!("Stats reporting disabled - no statistics will be logged on demand");
            return;
        }

        if let Some(stats) = self.transcription_stats.try_lock() {
            if stats.segments_processed > 0 {
                let stats_report = stats.report();

                // Log to console
                println!("\n--- Current Transcription Statistics ---");
                println!("{}", stats_report);
                println!("-----------------------------------------\n");

                // Write to file
                let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
                let file_content = format!(
                    "\n--- {} (On-Demand Report) ---\n{}\n",
                    timestamp, stats_report
                );

                match OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open("transcription_stats.log")
                {
                    Ok(mut file) => {
                        if let Err(e) = writeln!(file, "{}", file_content) {
                            eprintln!("Failed to write to stats file: {}", e);
                        }
                    }
                    Err(e) => eprintln!("Failed to open stats file: {}", e),
                }
            } else {
                println!("No transcription statistics available yet.");
            }
        } else {
            println!("Could not access transcription statistics (locked).");
        }
    }
}
