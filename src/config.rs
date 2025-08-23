// use crate::audio_processor::VadConfig;
use crate::silero_audio_processor::VadConfig as SileroVadConfig;
use ct2rs::WhisperOptions;
use serde::{Deserialize, Serialize};
use winit::keyboard::KeyCode;

/// Audio processor configuration parameters for general audio processing
/// This is separate from the VAD-specific settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioProcessorConfig {
    /// Maximum number of samples to store for visualization
    /// Controls the detail level of the audio waveform display
    pub max_vis_samples: usize,
}

impl Default for AudioProcessorConfig {
    fn default() -> Self {
        Self {
            max_vis_samples: 1024, // Number of samples to display in visualization
        }
    }
}

/// Configuration for keyboard shortcuts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyboardShortcuts {
    /// Key to copy transcript (with Ctrl modifier)
    pub copy_transcript: String,
    /// Key to reset transcript (with Ctrl modifier)
    pub reset_transcript: String,
    /// Key to toggle recording
    pub toggle_recording: String,
    /// Key to exit application
    pub exit_application: String,
}

/// Configuration for XDG Desktop Portal features
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PortalConfig {
    /// Whether to enable XDG Desktop Portal for input injection
    /// When enabled, allows the application to inject keystrokes via portal
    pub enable_xdg_portal: bool,
    /// Whether to enable xdg-desktop-portal Global Shortcuts
    pub enable_global_shortcuts: bool,
    /// Accelerator string for manual toggle (e.g., "<Super>Tab")
    pub manual_toggle_accelerator: String,
    /// Application ID used to register with xdg-desktop-portal (stable name)
    pub application_id: String,
}

/// Configuration for manual transcription mode
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManualModeConfig {
    /// Maximum recording duration in seconds (default: 60)
    pub max_recording_duration_secs: u32,
    
    /// Audio buffer size for manual sessions (default: 16000 * 60 = 1 min at 16kHz)
    pub manual_buffer_size: usize,
    
    /// Whether to auto-start a new session after completing one
    pub auto_restart_sessions: bool,
    
    /// Whether to clear previous transcript when starting new session
    pub clear_on_new_session: bool,
    
    /// Processing timeout in seconds
    pub processing_timeout_secs: u32,
}

impl Default for KeyboardShortcuts {
    fn default() -> Self {
        Self {
            copy_transcript: "KeyC".to_string(),    // Default: Ctrl+C
            reset_transcript: "KeyR".to_string(),   // Default: Ctrl+R
            toggle_recording: "Space".to_string(),  // Default: Space
            exit_application: "Escape".to_string(), // Default: Escape
        }
    }
}

impl Default for PortalConfig {
    fn default() -> Self {
        Self {
            enable_xdg_portal: true, // Default to enabled for better UX
            enable_global_shortcuts: true,
            manual_toggle_accelerator: "<Super>Tab".to_string(),
            application_id: "dev.paddy.sonori".to_string(),
        }
    }
}

impl Default for ManualModeConfig {
    fn default() -> Self {
        Self {
            max_recording_duration_secs: 60,
            manual_buffer_size: 16000 * 60, // 1 minute at 16kHz
            auto_restart_sessions: false,
            clear_on_new_session: true,
            processing_timeout_secs: 30,
        }
    }
}

impl KeyboardShortcuts {
    /// Convert a key string to a KeyCode
    pub fn to_key_code(&self, key_str: &str) -> Option<KeyCode> {
        match key_str {
            "KeyA" => Some(KeyCode::KeyA),
            "KeyB" => Some(KeyCode::KeyB),
            "KeyC" => Some(KeyCode::KeyC),
            "KeyD" => Some(KeyCode::KeyD),
            "KeyE" => Some(KeyCode::KeyE),
            "KeyF" => Some(KeyCode::KeyF),
            "KeyG" => Some(KeyCode::KeyG),
            "KeyH" => Some(KeyCode::KeyH),
            "KeyI" => Some(KeyCode::KeyI),
            "KeyJ" => Some(KeyCode::KeyJ),
            "KeyK" => Some(KeyCode::KeyK),
            "KeyL" => Some(KeyCode::KeyL),
            "KeyM" => Some(KeyCode::KeyM),
            "KeyN" => Some(KeyCode::KeyN),
            "KeyO" => Some(KeyCode::KeyO),
            "KeyP" => Some(KeyCode::KeyP),
            "KeyQ" => Some(KeyCode::KeyQ),
            "KeyR" => Some(KeyCode::KeyR),
            "KeyS" => Some(KeyCode::KeyS),
            "KeyT" => Some(KeyCode::KeyT),
            "KeyU" => Some(KeyCode::KeyU),
            "KeyV" => Some(KeyCode::KeyV),
            "KeyW" => Some(KeyCode::KeyW),
            "KeyX" => Some(KeyCode::KeyX),
            "KeyY" => Some(KeyCode::KeyY),
            "KeyZ" => Some(KeyCode::KeyZ),
            "Digit0" => Some(KeyCode::Digit0),
            "Digit1" => Some(KeyCode::Digit1),
            "Digit2" => Some(KeyCode::Digit2),
            "Digit3" => Some(KeyCode::Digit3),
            "Digit4" => Some(KeyCode::Digit4),
            "Digit5" => Some(KeyCode::Digit5),
            "Digit6" => Some(KeyCode::Digit6),
            "Digit7" => Some(KeyCode::Digit7),
            "Digit8" => Some(KeyCode::Digit8),
            "Digit9" => Some(KeyCode::Digit9),
            "Space" => Some(KeyCode::Space),
            "Escape" => Some(KeyCode::Escape),
            "Enter" => Some(KeyCode::Enter),
            "Tab" => Some(KeyCode::Tab),
            "F1" => Some(KeyCode::F1),
            "F2" => Some(KeyCode::F2),
            "F3" => Some(KeyCode::F3),
            "F4" => Some(KeyCode::F4),
            "F5" => Some(KeyCode::F5),
            "F6" => Some(KeyCode::F6),
            "F7" => Some(KeyCode::F7),
            "F8" => Some(KeyCode::F8),
            "F9" => Some(KeyCode::F9),
            "F10" => Some(KeyCode::F10),
            "F11" => Some(KeyCode::F11),
            "F12" => Some(KeyCode::F12),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Main model to use for transcription
    pub model: String,
    /// Language for transcription
    pub language: String,
    /// Compute type for model inference
    pub compute_type: String,
    /// Device for model inference
    pub device: String,
    /// Whether to log statistics
    pub log_stats_enabled: bool,
    /// The global buffer size used throughout the application
    /// This is the fundamental audio processing block size in samples
    pub buffer_size: usize,
    /// Audio sample rate in Hz (must be 8000 or 16000 for Silero VAD)
    /// This value is used throughout the application for audio processing
    pub sample_rate: usize,
    /// Transcription mode: "realtime" or "manual"
    pub transcription_mode: String,
    /// Whisper model configuration
    pub whisper_options: WhisperOptionsSerde,
    /// Voice Activity Detection configuration
    pub vad_config: VadConfigSerde,
    /// Audio processor configuration
    pub audio_processor_config: AudioProcessorConfig,
    /// Keyboard shortcuts configuration
    pub keyboard_shortcuts: KeyboardShortcuts,
    /// XDG Desktop Portal configuration
    pub portal_config: PortalConfig,
    /// Manual transcription mode configuration
    pub manual_mode_config: ManualModeConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhisperOptionsSerde {
    pub beam_size: usize,
    pub patience: f32,
    pub repetition_penalty: f32,
}

/// Configuration for Voice Activity Detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VadConfigSerde {
    /// Probability threshold for speech detection (0.0-1.0)
    pub threshold: f32,
    /// Number of frames before confirming speech
    pub hangbefore_frames: usize,
    /// Number of frames after speech before ending segment
    pub hangover_frames: usize,
    /// Maximum buffer size in seconds
    pub max_buffer_duration_sec: f32,
    /// Maximum number of segments to keep
    pub max_segment_count: usize,
}

impl Default for VadConfigSerde {
    fn default() -> Self {
        Self {
            threshold: 0.2,                // Silero uses probability threshold (0.0-1.0)
            hangbefore_frames: 1,          // Wait for this many frames before confirming speech
            hangover_frames: 15, // Wait for this many frames of silence before ending segment
            max_buffer_duration_sec: 30.0, // Maximum buffer size in seconds
            max_segment_count: 20, // Maximum number of segments to keep
        }
    }
}

impl SileroVadConfig {
    pub fn from_config(
        vad_config: &VadConfigSerde,
        buffer_size: usize,
        sample_rate: usize,
    ) -> Self {
        Self {
            threshold: vad_config.threshold,
            frame_size: buffer_size,
            sample_rate,
            hangbefore_frames: vad_config.hangbefore_frames,
            hangover_frames: vad_config.hangover_frames,
            max_buffer_duration: (vad_config.max_buffer_duration_sec * sample_rate as f32) as usize,
            max_segment_count: vad_config.max_segment_count,
        }
    }
}

impl From<(VadConfigSerde, usize, usize)> for SileroVadConfig {
    fn from((config, buffer_size, sample_rate): (VadConfigSerde, usize, usize)) -> Self {
        Self {
            threshold: config.threshold,
            frame_size: buffer_size,
            sample_rate,
            hangbefore_frames: config.hangbefore_frames,
            hangover_frames: config.hangover_frames,
            max_buffer_duration: (config.max_buffer_duration_sec * sample_rate as f32) as usize,
            max_segment_count: config.max_segment_count,
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            model: "openai/whisper-base.en".to_string(),
            language: "en".to_string(),
            compute_type: "INT8".to_string(),
            device: "CPU".to_string(),
            log_stats_enabled: true,
            buffer_size: 1024,
            sample_rate: 16000,
            transcription_mode: "realtime".to_string(), // Default to realtime for backward compatibility
            whisper_options: WhisperOptionsSerde {
                beam_size: 5,
                patience: 1.0,
                repetition_penalty: 1.25,
            },
            vad_config: VadConfigSerde::default(),
            audio_processor_config: AudioProcessorConfig::default(),
            keyboard_shortcuts: KeyboardShortcuts::default(),
            portal_config: PortalConfig::default(),
            manual_mode_config: ManualModeConfig::default(),
        }
    }
}

impl WhisperOptionsSerde {
    pub fn to_whisper_options(&self) -> WhisperOptions {
        WhisperOptions {
            beam_size: self.beam_size,
            patience: self.patience,
            repetition_penalty: self.repetition_penalty,
            ..Default::default()
        }
    }
}

/// Helper function to read the application configuration
pub fn read_app_config() -> AppConfig {
    match std::fs::read_to_string("config.toml") {
        Ok(config_str) => match toml::from_str(&config_str) {
            Ok(config) => config,
            Err(e) => {
                println!(
                    "Failed to parse config.toml: {}. Using default configuration.",
                    e
                );
                AppConfig::default()
            }
        },
        Err(e) => {
            println!(
                "Failed to read config.toml: {}. Using default configuration.",
                e
            );
            AppConfig::default()
        }
    }
}
