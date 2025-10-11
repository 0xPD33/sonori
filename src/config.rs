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
    /// Paste shortcut to use: "ctrl_shift_v" (default, works in terminals) or "ctrl_v"
    pub paste_shortcut: String,
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
            paste_shortcut: "ctrl_shift_v".to_string(), // Default: Ctrl+Shift+V (works in terminals)
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

/// Configuration for display and rendering settings
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DisplayConfig {
    /// VSync mode: "Auto", "Enabled", "Adaptive", "Disabled", or "Mailbox"
    /// - Auto: Use first available present mode (default behavior)
    /// - Enabled: Traditional vsync (Fifo) - waits for vertical blank, no tearing
    /// - Adaptive: Adaptive vsync (FifoRelaxed) - vsync when above refresh rate, immediate when below
    /// - Disabled: No vsync (Immediate) - lowest latency, potential tearing
    /// - Mailbox: Triple-buffered vsync - no tearing, lowest latency with vsync
    pub vsync_mode: String,

    /// Target FPS when vsync is disabled (prevents unbounded frame rates)
    pub target_fps: u32,
}

/// Configuration for window visibility and system tray behavior
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowBehaviorConfig {
    /// Whether to automatically hide the window when idle
    pub hide_when_idle: bool,

    /// Delay in milliseconds before auto-hiding after recording stops
    pub auto_hide_delay_ms: u64,

    /// Whether to start the application with the window hidden
    pub start_hidden: bool,

    /// Whether to show the application icon in the system tray
    pub show_in_system_tray: bool,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            vsync_mode: "Enabled".to_string(), // Default to traditional vsync
            target_fps: 60,                    // Cap at 60 FPS when vsync disabled
        }
    }
}

impl Default for WindowBehaviorConfig {
    fn default() -> Self {
        Self {
            hide_when_idle: false,        // Don't auto-hide by default
            auto_hide_delay_ms: 2000,     // 2 seconds delay
            start_hidden: false,          // Start visible by default
            show_in_system_tray: true,    // Show tray icon by default
        }
    }
}

impl DisplayConfig {
    /// Convert string vsync_mode to wgpu::PresentMode, with fallback logic
    pub fn to_present_mode(&self, available_modes: &[wgpu::PresentMode]) -> wgpu::PresentMode {
        let preferred = match self.vsync_mode.as_str() {
            "Enabled" => wgpu::PresentMode::Fifo,
            "Adaptive" => wgpu::PresentMode::FifoRelaxed,
            "Disabled" => wgpu::PresentMode::Immediate,
            "Mailbox" => wgpu::PresentMode::Mailbox,
            "Auto" | _ => {
                // Auto mode: prefer Fifo, but accept whatever is available
                return available_modes
                    .first()
                    .copied()
                    .unwrap_or(wgpu::PresentMode::Fifo);
            }
        };

        // Check if preferred mode is available
        if available_modes.contains(&preferred) {
            preferred
        } else {
            // Fallback to Fifo (guaranteed to be available), or first available
            if available_modes.contains(&wgpu::PresentMode::Fifo) {
                println!(
                    "Warning: Preferred vsync mode '{}' not available, falling back to Fifo",
                    self.vsync_mode
                );
                wgpu::PresentMode::Fifo
            } else {
                println!(
                    "Warning: Preferred vsync mode '{}' not available, using first available mode",
                    self.vsync_mode
                );
                available_modes
                    .first()
                    .copied()
                    .unwrap_or(wgpu::PresentMode::Fifo)
            }
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
#[serde(default)]
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
    /// Display and rendering configuration
    pub display_config: DisplayConfig,
    /// Window visibility and system tray configuration
    pub window_behavior_config: WindowBehaviorConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhisperOptionsSerde {
    pub beam_size: usize,
    pub patience: f32,
    pub repetition_penalty: f32,
}

/// Configuration for Voice Activity Detection
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VadConfigSerde {
    /// Probability threshold for speech detection (0.0-1.0)
    pub threshold: f32,
    /// Number of frames before confirming speech
    pub hangbefore_frames: usize,
    /// Number of frames after speech before ending segment
    pub hangover_frames: usize,
    /// Number of samples to advance between frames
    pub hop_samples: usize,
    /// Maximum buffer size in seconds
    pub max_buffer_duration_sec: f32,
    /// Maximum number of segments to keep
    pub max_segment_count: usize,
    /// Number of non-speech frames to tolerate in PossibleSpeech before giving up
    pub silence_tolerance_frames: usize,
    /// Lower threshold for speech continuation (hysteresis)
    pub speech_end_threshold: f32,
    /// Exponential moving average smoothing factor (0.0-1.0)
    pub speech_prob_smoothing: f32,
}

impl Default for VadConfigSerde {
    fn default() -> Self {
        Self {
            threshold: 0.2,                // Silero uses probability threshold (0.0-1.0)
            hangbefore_frames: 3,          // Wait 30ms (3 frames) before confirming speech
            hangover_frames: 20, // Wait 200ms (20 frames) of silence before ending segment
            hop_samples: 160,    // 10ms hop for overlapping windows
            max_buffer_duration_sec: 30.0, // Maximum buffer size in seconds
            max_segment_count: 20, // Maximum number of segments to keep
            silence_tolerance_frames: 5, // 50ms tolerance in PossibleSpeech (5 frames @ 10ms)
            speech_end_threshold: 0.15, // Lower threshold for speech continuation (hysteresis)
            speech_prob_smoothing: 0.3, // EMA smoothing factor (production standard)
        }
    }
}

impl SileroVadConfig {
    pub fn from_config(
        vad_config: &VadConfigSerde,
        _buffer_size: usize,
        sample_rate: usize,
    ) -> Self {
        Self {
            threshold: vad_config.threshold,
            frame_size: 512,
            sample_rate,
            hangbefore_frames: vad_config.hangbefore_frames,
            hangover_frames: vad_config.hangover_frames,
            hop_samples: vad_config.hop_samples,
            max_buffer_duration: (vad_config.max_buffer_duration_sec * sample_rate as f32) as usize,
            max_segment_count: vad_config.max_segment_count,
            silence_tolerance_frames: vad_config.silence_tolerance_frames,
            speech_end_threshold: vad_config.speech_end_threshold,
            speech_prob_smoothing: vad_config.speech_prob_smoothing,
        }
    }
}

impl From<(VadConfigSerde, usize, usize)> for SileroVadConfig {
    fn from((config, _buffer_size, sample_rate): (VadConfigSerde, usize, usize)) -> Self {
        Self {
            threshold: config.threshold,
            frame_size: 512,
            sample_rate,
            hangbefore_frames: config.hangbefore_frames,
            hangover_frames: config.hangover_frames,
            hop_samples: config.hop_samples,
            max_buffer_duration: (config.max_buffer_duration_sec * sample_rate as f32) as usize,
            max_segment_count: config.max_segment_count,
            silence_tolerance_frames: config.silence_tolerance_frames,
            speech_end_threshold: config.speech_end_threshold,
            speech_prob_smoothing: config.speech_prob_smoothing,
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
            display_config: DisplayConfig::default(),
            window_behavior_config: WindowBehaviorConfig::default(),
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
