pub mod audio_capture;
pub mod audio_processor;
pub mod backend;
pub mod config;
pub mod copy;
pub mod download;
pub mod portal_input;
pub mod portal_tokens;
pub mod post_processor;
pub mod real_time_transcriber;
pub mod silero_audio_processor;
pub mod sound_generator;
pub mod sound_player;
pub mod stats_reporter;
pub mod system_tray;
pub mod transcript_writer;
pub mod transcription_processor;
pub mod transcription_stats;
pub mod ui;

// Re-export key components for easier access
pub use audio_capture::AudioCapture;
pub use audio_processor::AudioProcessor;
pub use config::read_app_config;
pub use real_time_transcriber::RealTimeTranscriber;
pub use stats_reporter::StatsReporter;
pub use transcription_processor::TranscriptionProcessor;
pub use transcription_stats::TranscriptionStats;
