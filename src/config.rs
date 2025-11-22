use crate::backend::{BackendConfig, BackendType};
use crate::silero_audio_processor::VadConfig as SileroVadConfig;
use ct2rs::WhisperOptions;
use serde::{Deserialize, Serialize};
use winit::keyboard::KeyCode;

/// Audio processor configuration parameters for general audio processing
/// This is separate from the VAD-specific settings
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioProcessorConfig {
    /// Audio sample rate in Hz (must be 8000 or 16000 for Silero VAD)
    /// This value is used throughout the application for audio processing
    pub sample_rate: usize,
    /// The global buffer size used throughout the application
    /// This is the fundamental audio processing block size in samples
    /// Also used for visualization sample count
    pub buffer_size: usize,
}

impl Default for AudioProcessorConfig {
    fn default() -> Self {
        Self {
            sample_rate: 16000,
            buffer_size: 1024,
        }
    }
}

/// Configuration for general core settings
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    /// Main model to use for transcription
    pub model: String,
    /// Language for transcription
    pub language: String,
    /// Transcription mode: "realtime" or "manual"
    pub transcription_mode: String,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            model: "small.en".to_string(),
            language: "en".to_string(),
            transcription_mode: "manual".to_string(),
        }
    }
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

/// Configuration for real-time transcription mode
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RealtimeModeConfig {
    /// Maximum audio buffer duration in seconds for VAD history
    pub max_buffer_duration_sec: f32,

    /// Maximum number of speech segments to keep in buffer
    pub max_segment_count: usize,
}

impl Default for RealtimeModeConfig {
    fn default() -> Self {
        Self {
            max_buffer_duration_sec: 30.0,
            max_segment_count: 20,
        }
    }
}

/// Configuration for manual transcription mode
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ManualModeConfig {
    /// Maximum recording duration in seconds (default: 120)
    /// Buffer size is calculated as: max_recording_duration_secs * sample_rate
    pub max_recording_duration_secs: u32,

    /// Whether to clear previous transcript when starting new session
    pub clear_on_new_session: bool,

    /// Duration of each chunk in seconds (default: 29.0)
    /// Note: 29s avoids edge case where duration == chunk_size hits token limits
    pub chunk_duration_seconds: f32,

    /// Whether to enable chunk overlap for manual mode transcription (default: true)
    /// When enabled, uses small overlap between chunks to catch boundary words
    /// Overlap amount is controlled by chunk_overlap_seconds
    pub enable_chunk_overlap: bool,

    /// Overlap duration in seconds between chunks (default: 0.5)
    /// Only used when enable_chunk_overlap is true
    /// Recommended range: 0.1 to 1.0 seconds (avoid 2+ seconds due to hallucination)
    pub chunk_overlap_seconds: f32,

    /// EXPERIMENTAL: Disable chunking for manual mode transcription (default: false)
    /// When enabled, processes entire recording as single segment (no chunk limit)
    /// Note: May consume more memory for very long recordings
    /// Note: Whisper model was trained on 30-second chunks, very long audio may have issues
    pub disable_chunking: bool,
}

impl Default for PortalConfig {
    fn default() -> Self {
        Self {
            enable_xdg_portal: true, // Default to enabled for better UX
            enable_global_shortcuts: true,
            manual_toggle_accelerator: "<Super>backslash".to_string(),
            application_id: "dev.sonori".to_string(),
            paste_shortcut: "ctrl_shift_v".to_string(), // Default: Ctrl+Shift+V (works in terminals)
        }
    }
}

impl Default for ManualModeConfig {
    fn default() -> Self {
        Self {
            max_recording_duration_secs: 120,
            clear_on_new_session: true,
            chunk_duration_seconds: 29.0, // 29s avoids edge case at exactly 30s boundary
            enable_chunk_overlap: true,   // Enable overlap by default
            chunk_overlap_seconds: 2.0,   // 2.0 second overlap (matches packaged config)
            disable_chunking: false,      // Chunking enabled by default
        }
    }
}

/// Configuration for debugging and development
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DebugConfig {
    /// Whether to log statistics
    pub log_stats_enabled: bool,
    /// Whether to save manual mode audio to WAV files for debugging
    pub save_manual_audio_debug: bool,
    /// Directory to save debug recordings (default: "recordings")
    pub recording_dir: String,
}

/// Configuration for sound settings
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundConfig {
    /// Enable sound feedback
    pub enabled: bool,
    /// Sound volume (0.0-1.0)
    pub volume: f32,
}

impl Default for DebugConfig {
    fn default() -> Self {
        Self {
            log_stats_enabled: false,
            save_manual_audio_debug: false,
            recording_dir: "recordings".to_string(),
        }
    }
}

/// Configuration for transcription post-processing
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PostProcessConfig {
    /// Enable post-processing of transcriptions
    pub enabled: bool,
    /// Remove leading dashes from transcriptions
    pub remove_leading_dashes: bool,
    /// Remove trailing dashes from transcriptions
    pub remove_trailing_dashes: bool,
    /// Normalize whitespace (collapse multiple spaces, remove leading/trailing)
    pub normalize_whitespace: bool,
}

impl Default for PostProcessConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            remove_leading_dashes: true,
            remove_trailing_dashes: true,
            normalize_whitespace: true,
        }
    }
}

impl Default for SoundConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            volume: 0.5,
        }
    }
}

/// Window position presets for layer-shell anchoring
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum WindowPosition {
    BottomLeft,
    BottomCenter,
    BottomRight,
    TopLeft,
    TopCenter,
    TopRight,
    MiddleLeft,
    MiddleCenter,
    MiddleRight,
}

impl Default for WindowPosition {
    fn default() -> Self {
        WindowPosition::BottomCenter
    }
}

impl WindowPosition {
    /// Convert window position to Wayland layer-shell anchor flags
    /// Returns the anchor flags used to position the window at the desired location
    #[cfg(target_os = "linux")]
    pub fn to_wayland_anchor(&self) -> winit::platform::wayland::Anchor {
        use winit::platform::wayland::Anchor;

        match self {
            WindowPosition::BottomLeft => Anchor::BOTTOM | Anchor::LEFT,
            WindowPosition::BottomCenter => Anchor::BOTTOM,
            WindowPosition::BottomRight => Anchor::BOTTOM | Anchor::RIGHT,
            WindowPosition::TopLeft => Anchor::TOP | Anchor::LEFT,
            WindowPosition::TopCenter => Anchor::TOP,
            WindowPosition::TopRight => Anchor::TOP | Anchor::RIGHT,
            WindowPosition::MiddleLeft => Anchor::LEFT,
            WindowPosition::MiddleCenter => Anchor::empty(), // No anchors = centered
            WindowPosition::MiddleRight => Anchor::RIGHT,
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

    /// Window position on screen (layer-shell anchor configuration)
    /// Available positions: BottomLeft, BottomCenter, BottomRight,
    /// TopLeft, TopCenter, TopRight, MiddleLeft, MiddleCenter, MiddleRight
    pub window_position: WindowPosition,
}

/// Configuration for system tray behavior
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowBehaviorConfig {
    /// Whether to show the application icon in the system tray
    pub show_in_system_tray: bool,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            vsync_mode: "Enabled".to_string(), // Default to traditional vsync
            target_fps: 60,                    // Cap at 60 FPS when vsync disabled
            window_position: WindowPosition::default(),
        }
    }
}

impl Default for WindowBehaviorConfig {
    fn default() -> Self {
        Self {
            show_in_system_tray: true, // Show tray icon by default
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// General core configuration
    pub general_config: GeneralConfig,

    /// Backend configuration (includes backend selection)
    pub backend_config: BackendConfig,

    /// Audio processing configuration
    pub audio_processor_config: AudioProcessorConfig,

    /// Real-time transcription mode configuration
    pub realtime_mode_config: RealtimeModeConfig,

    /// Manual transcription mode configuration
    pub manual_mode_config: ManualModeConfig,

    /// Voice Activity Detection configuration
    pub vad_config: VadConfigSerde,

    /// Common transcription options shared across all backends
    pub common_transcription_options: CommonTranscriptionOptions,

    /// CTranslate2-specific options
    pub ctranslate2_options: CT2Options,

    /// Whisper.cpp-specific options
    pub whisper_cpp_options: WhisperCppOptions,

    /// XDG Desktop Portal configuration
    pub portal_config: PortalConfig,

    /// Display and rendering configuration
    pub display_config: DisplayConfig,

    /// Window visibility and system tray configuration
    pub window_behavior_config: WindowBehaviorConfig,

    /// Sound effects configuration
    pub sound_config: SoundConfig,

    /// Debug and development configuration
    pub debug_config: DebugConfig,

    /// Transcription post-processing configuration
    pub post_process_config: PostProcessConfig,

    /// Deprecated legacy field - use backend_config instead
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compute_type: Option<String>,

    /// Deprecated legacy field - use backend_config instead
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
}

/// Common transcription options shared across all backends
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CommonTranscriptionOptions {
    /// Beam search width (1 = greedy/fastest, higher = more accurate but slower)
    pub beam_size: usize,
    /// Beam search patience factor
    pub patience: f32,
}

impl Default for CommonTranscriptionOptions {
    fn default() -> Self {
        Self {
            beam_size: 5,
            patience: 1.0,
        }
    }
}

/// CTranslate2-specific transcription options
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CT2Options {
    /// Penalty for repeated tokens
    pub repetition_penalty: f32,
}

impl Default for CT2Options {
    fn default() -> Self {
        Self {
            repetition_penalty: 1.25,
        }
    }
}

/// Whisper.cpp-specific transcription options
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WhisperCppOptions {
    pub temperature: f32,
    pub suppress_blank: bool,
    pub no_context: bool,
    pub max_tokens: i32,
    pub entropy_thold: f32,
    pub logprob_thold: f32,
    pub no_speech_thold: f32,
}

impl Default for WhisperCppOptions {
    fn default() -> Self {
        Self {
            temperature: 0.2,     // Gentle sampling bump to match packaged config
            suppress_blank: true, // Skip blank segments
            no_context: true,     // Disable context to prevent double transcriptions
            max_tokens: 0,        // No limit
            entropy_thold: 2.4,   // Default whisper.cpp value
            logprob_thold: -1.0,  // Default whisper.cpp value
            no_speech_thold: 0.6, // Default whisper.cpp value
        }
    }
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
            threshold: 0.10,             // Lower threshold to detect quieter speech
            hangbefore_frames: 5,        // Increased to 50ms - capture more lead-in audio
            hangover_frames: 30,         // Increased to 300ms - keep more trailing audio
            silence_tolerance_frames: 8, // Increased to 80ms - tolerate more pauses
            speech_end_threshold: 0.08,  // Lower threshold for continuation
            speech_prob_smoothing: 0.3,  // EMA smoothing factor (production standard)
        }
    }
}

impl SileroVadConfig {
    pub fn from_config(
        vad_config: &VadConfigSerde,
        realtime_config: &RealtimeModeConfig,
        _buffer_size: usize,
        sample_rate: usize,
    ) -> Self {
        Self {
            threshold: vad_config.threshold,
            frame_size: 512,
            sample_rate,
            hangbefore_frames: vad_config.hangbefore_frames,
            hangover_frames: vad_config.hangover_frames,
            hop_samples: (sample_rate as f32 * 0.01) as usize, // 10ms hop calculated from sample_rate
            max_buffer_duration: (realtime_config.max_buffer_duration_sec * sample_rate as f32)
                as usize,
            max_segment_count: realtime_config.max_segment_count,
            silence_tolerance_frames: vad_config.silence_tolerance_frames,
            speech_end_threshold: vad_config.speech_end_threshold,
            speech_prob_smoothing: vad_config.speech_prob_smoothing,
        }
    }
}

impl From<(VadConfigSerde, RealtimeModeConfig, usize, usize)> for SileroVadConfig {
    fn from(
        (config, realtime_config, _buffer_size, sample_rate): (
            VadConfigSerde,
            RealtimeModeConfig,
            usize,
            usize,
        ),
    ) -> Self {
        Self {
            threshold: config.threshold,
            frame_size: 512,
            sample_rate,
            hangbefore_frames: config.hangbefore_frames,
            hangover_frames: config.hangover_frames,
            hop_samples: (sample_rate as f32 * 0.01) as usize, // 10ms hop calculated from sample_rate
            max_buffer_duration: (realtime_config.max_buffer_duration_sec * sample_rate as f32)
                as usize,
            max_segment_count: realtime_config.max_segment_count,
            silence_tolerance_frames: config.silence_tolerance_frames,
            speech_end_threshold: config.speech_end_threshold,
            speech_prob_smoothing: config.speech_prob_smoothing,
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            general_config: GeneralConfig::default(),
            backend_config: BackendConfig::default(),
            audio_processor_config: AudioProcessorConfig::default(),
            realtime_mode_config: RealtimeModeConfig::default(),
            manual_mode_config: ManualModeConfig::default(),
            vad_config: VadConfigSerde::default(),
            common_transcription_options: CommonTranscriptionOptions::default(),
            ctranslate2_options: CT2Options::default(),
            whisper_cpp_options: WhisperCppOptions::default(),
            portal_config: PortalConfig::default(),
            display_config: DisplayConfig::default(),
            window_behavior_config: WindowBehaviorConfig::default(),
            sound_config: SoundConfig::default(),
            debug_config: DebugConfig::default(),
            post_process_config: PostProcessConfig::default(),
            compute_type: None,
            device: None,
        }
    }
}

impl AppConfig {
    /// Migrate legacy compute_type/device fields to new backend_config
    pub fn migrate_legacy_config(&mut self) {
        if let (Some(compute_type), Some(device)) = (&self.compute_type, &self.device) {
            let is_default_config = self.backend_config.threads == num_cpus::get().min(4)
                && !self.backend_config.gpu_enabled;

            if is_default_config {
                println!(
                    "Migrating legacy config fields (compute_type={}, device={}) to backend_config",
                    compute_type, device
                );

                self.backend_config =
                    crate::backend::ctranslate2::migrate_legacy_config(compute_type, device, None);
                self.compute_type = None;
                self.device = None;
            }
        }

        // Ensure whisper.cpp does not reuse context across sessions (prevents duplicate transcriptions)
        if !self.whisper_cpp_options.no_context {
            println!("Enabling whisper_cpp_options.no_context to prevent cross-session duplication");
            self.whisper_cpp_options.no_context = true;
        }

        // Bring legacy configs up to current default temperature if they were using the old default
        if (self.whisper_cpp_options.temperature - 0.0).abs() < f32::EPSILON {
            self.whisper_cpp_options.temperature = 0.2;
        }
    }
}

impl CT2Options {
    /// Convert to ct2rs::WhisperOptions, combining with common options
    pub fn to_whisper_options(
        &self,
        common_options: &CommonTranscriptionOptions,
    ) -> WhisperOptions {
        WhisperOptions {
            beam_size: common_options.beam_size,
            patience: common_options.patience,
            repetition_penalty: self.repetition_penalty,
            ..Default::default()
        }
    }
}

/// Helper function to find config file path
fn find_config_path() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;

    // 0. Explicit override for debugging or custom layouts
    if let Ok(custom_path) = std::env::var("SONORI_CONFIG_PATH") {
        let path = PathBuf::from(custom_path);
        if path.exists() {
            println!("Loading configuration from SONORI_CONFIG_PATH: {}", path.display());
            return Some(path);
        } else {
            eprintln!(
                "Warning: SONORI_CONFIG_PATH set to {} but file does not exist. Falling back to defaults.",
                path.display()
            );
            return None;
        }
    }

    // 1. Check ~/.config/sonori/config.toml (user config)
    if let Some(config_home) = std::env::var_os("XDG_CONFIG_HOME") {
        let path = PathBuf::from(config_home).join("sonori").join("config.toml");
        if path.exists() {
            return Some(path);
        }
    } else if let Some(home) = std::env::var_os("HOME") {
        let path = PathBuf::from(home).join(".config").join("sonori").join("config.toml");
        if path.exists() {
            return Some(path);
        }
    }

    // 2. No config found
    None
}

/// Create default config in user config directory on first run
fn ensure_user_config() {
    use std::path::PathBuf;

    // Determine user config path
    let user_config_dir = if let Some(config_home) = std::env::var_os("XDG_CONFIG_HOME") {
        PathBuf::from(config_home).join("sonori")
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home).join(".config").join("sonori")
    } else {
        return; // Can't determine config dir
    };

    let user_config_path = user_config_dir.join("config.toml");

    // Skip if user config already exists
    if user_config_path.exists() {
        return;
    }

    // Create config directory
    if let Err(e) = std::fs::create_dir_all(&user_config_dir) {
        eprintln!("Failed to create config directory: {}", e);
        return;
    }

    // Write default config as TOML
    let default_config = AppConfig::default();
    match toml::to_string_pretty(&default_config) {
        Ok(toml_string) => {
            match std::fs::write(&user_config_path, toml_string) {
                Ok(_) => println!("Created default config at: {}", user_config_path.display()),
                Err(e) => eprintln!("Failed to write default config: {}", e),
            }
        }
        Err(e) => eprintln!("Failed to serialize default config: {}", e),
    }
}

/// Helper function to read the application configuration
pub fn read_app_config() -> AppConfig {
    let (config, _path) = read_app_config_with_path();
    config
}

/// Helper function to read the application configuration and return the path used (if any)
pub fn read_app_config_with_path() -> (AppConfig, Option<std::path::PathBuf>) {
    // Ensure user has a config file (copy from system on first run)
    ensure_user_config();

    let config_path = find_config_path();

    let config_str = match config_path.as_ref() {
        Some(path) => {
            println!("Loading configuration from: {}", path.display());
            match std::fs::read_to_string(path) {
                Ok(content) => content,
                Err(e) => {
                    println!("Failed to read config from {}: {}. Using default configuration.", path.display(), e);
                    return (AppConfig::default(), None);
                }
            }
        }
        None => {
            println!("No config.toml found. Using default configuration.");
            return (AppConfig::default(), None);
        }
    };

    let config = match toml::from_str::<AppConfig>(&config_str) {
        Ok(mut config) => {
            // Migrate legacy configuration if needed
            config.migrate_legacy_config();
            config
        }
        Err(e) => {
            println!("Failed to parse config.toml: {}. Using default configuration.", e);
            AppConfig::default()
        }
    };

    (config, config_path)
}
