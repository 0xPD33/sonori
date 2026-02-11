use std::fs::OpenOptions;
use std::io::Write;

/// Stores statistics about transcription performance
#[derive(Default, Clone)]
pub struct TranscriptionStats {
    pub segments_processed: usize,
    pub total_audio_duration: f32,
    pub total_inference_time: f32,
    pub total_processing_time: f32,
    pub min_rtf: f32,
    pub max_rtf: f32,
    pub avg_rtf: f32,
    pub audio_channel_drops: u64,
    pub segment_channel_drops: u64,
}

impl TranscriptionStats {
    pub fn new() -> Self {
        Self {
            segments_processed: 0,
            total_audio_duration: 0.0,
            total_inference_time: 0.0,
            total_processing_time: 0.0,
            min_rtf: f32::MAX,
            max_rtf: 0.0,
            avg_rtf: 0.0,
            audio_channel_drops: 0,
            segment_channel_drops: 0,
        }
    }

    pub fn update(&mut self, segment_duration: f32, inference_time: f32, processing_time: f32) {
        let rtf = inference_time / segment_duration;

        self.segments_processed += 1;
        self.total_audio_duration += segment_duration;
        self.total_inference_time += inference_time;
        self.total_processing_time += processing_time;

        self.min_rtf = self.min_rtf.min(rtf);
        self.max_rtf = self.max_rtf.max(rtf);
        self.avg_rtf = self.total_inference_time / self.total_audio_duration;
    }

    pub fn record_audio_drop(&mut self, count: u64) -> u64 {
        self.audio_channel_drops += count;
        self.audio_channel_drops
    }

    pub fn record_segment_drop(&mut self, count: u64) -> u64 {
        self.segment_channel_drops += count;
        self.segment_channel_drops
    }

    pub fn report(&self) -> String {
        format!(
            "Transcription Statistics:\n\
             - Segments processed: {}\n\
             - Total audio duration: {:.2}s\n\
             - Total inference time: {:.2}s\n\
             - Total processing time: {:.2}s\n\
             - Average real-time factor (RTF): {:.2}x\n\
             - Min RTF: {:.2}x\n\
             - Max RTF: {:.2}x\n\
             - Audio channel drops: {}\n\
             - Segment channel drops: {}",
            self.segments_processed,
            self.total_audio_duration,
            self.total_inference_time,
            self.total_processing_time,
            self.avg_rtf,
            if self.min_rtf == f32::MAX {
                0.0
            } else {
                self.min_rtf
            },
            self.max_rtf,
            self.audio_channel_drops,
            self.segment_channel_drops
        )
    }

    /// Logs the statistics to a file
    pub fn log_to_file(&self, is_final: bool) {
        if self.segments_processed > 0 {
            let stats_report = self.report();

            // Write to file
            let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
            let report_type = if is_final {
                "Final Report"
            } else {
                "Periodic Report"
            };
            let file_content = format!(
                "\n--- {} ({}) ---\n{}\n",
                timestamp, report_type, stats_report
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
        }
    }
}
