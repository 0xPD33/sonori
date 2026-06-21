use serde::{Deserialize, Serialize};
use speechcore::{BackendConfig, BackendType};

/// Audio sample rate in Hz - hardcoded to 16000 (required by Silero VAD)
pub const SAMPLE_RATE: usize = 16000;

/// Audio processor configuration parameters for general audio processing
/// This is separate from the VAD-specific settings
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioProcessorConfig {
    /// The global buffer size used throughout the application
    /// This is the fundamental audio processing block size in samples
    /// Also used for visualization sample count
    pub buffer_size: usize,
}

impl Default for AudioProcessorConfig {
    fn default() -> Self {
        Self { buffer_size: 1024 }
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

/// Application ID for portal registration - hardcoded app identifier
pub const APPLICATION_ID: &str = "dev.sonori";

/// Shortcut activation mode for manual transcription
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum ShortcutMode {
    /// Press to start, press again to stop (default)
    #[default]
    Toggle,
    /// Hold to record, release to stop (push-to-talk)
    PushToTalk,
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
    /// Shortcut activation mode: Toggle (press to start/stop) or PushToTalk (hold to record)
    pub shortcut_mode: ShortcutMode,
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
    /// Note: some transcription models are trained on short chunks, so very long audio may have issues
    pub disable_chunking: bool,
}

impl Default for PortalConfig {
    fn default() -> Self {
        Self {
            enable_xdg_portal: true, // Default to enabled for better UX
            enable_global_shortcuts: true,
            manual_toggle_accelerator: "<Super>backslash".to_string(),
            shortcut_mode: ShortcutMode::default(),
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

/// Get the default transcript history path for the current user
fn default_transcript_history_path() -> String {
    let path = if let Some(cache_home) = std::env::var_os("XDG_CACHE_HOME") {
        std::path::PathBuf::from(cache_home)
            .join("sonori")
            .join("transcript_history.txt")
    } else if let Some(home) = std::env::var_os("HOME") {
        std::path::PathBuf::from(home)
            .join(".cache")
            .join("sonori")
            .join("transcript_history.txt")
    } else {
        std::path::PathBuf::from("transcript_history.txt")
    };
    path.to_string_lossy().to_string()
}

/// Check if a transcript history path matches the default for the current user
fn is_default_transcript_history_path(path: &str) -> bool {
    path == default_transcript_history_path()
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
    /// Whether to save transcript history to a persistent file
    pub save_transcript_history: bool,
    /// Path to transcript history file (default: ~/.cache/sonori/transcript_history.txt)
    /// Skipped during serialization if using the default value to allow per-user paths
    #[serde(skip_serializing_if = "is_default_transcript_history_path")]
    pub transcript_history_path: String,
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
            save_transcript_history: false,
            transcript_history_path: default_transcript_history_path(),
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

/// Configuration for transcription enhancement ("Magic Mode")
/// Uses llama.cpp with GGUF models for GPU-accelerated inference
pub const DEFAULT_ENHANCEMENT_SYSTEM_PROMPT: &str = "Rewrite the transcript into clean, natural text while preserving the speaker's meaning. Fix obvious transcription artifacts, punctuation, and casing. Do not add facts, explanations, or commentary.";

fn default_enhancement_system_prompt() -> Option<String> {
    Some(DEFAULT_ENHANCEMENT_SYSTEM_PROMPT.to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EnhancementConfig {
    /// Enable enhancement by default when magic mode is toggled
    pub enabled: bool,
    /// Model identifier (HuggingFace format): "owner/repo/filename.gguf"
    pub model: Option<String>,
    /// Custom system prompt for the enhancement model
    #[serde(default = "default_enhancement_system_prompt")]
    pub system_prompt: Option<String>,
    /// Maximum tokens to generate (default: 256)
    pub max_tokens: usize,
}

impl Default for EnhancementConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model: None,
            system_prompt: default_enhancement_system_prompt(),
            max_tokens: 256,
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

/// VAD sensitivity presets for different acoustic environments
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum VadSensitivity {
    /// Less sensitive - reduces false positives in noisy environments
    Low,
    /// Balanced - good for most environments (default)
    #[default]
    Medium,
    /// More sensitive - catches quiet speech, may trigger on background noise
    High,
}

impl VadSensitivity {
    /// Get the speech detection threshold for this sensitivity level
    pub fn threshold(&self) -> f32 {
        match self {
            VadSensitivity::Low => 0.15,
            VadSensitivity::Medium => 0.10,
            VadSensitivity::High => 0.05,
        }
    }

    /// Get the speech end threshold (hysteresis) for this sensitivity level
    pub fn speech_end_threshold(&self) -> f32 {
        match self {
            VadSensitivity::Low => 0.12,
            VadSensitivity::Medium => 0.08,
            VadSensitivity::High => 0.03,
        }
    }
}

/// Window position presets for layer-shell anchoring
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum WindowPosition {
    BottomLeft,
    #[default]
    BottomCenter,
    BottomRight,
    TopLeft,
    TopCenter,
    TopRight,
    MiddleLeft,
    MiddleCenter,
    MiddleRight,
    Custom,
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
            WindowPosition::Custom => Anchor::TOP | Anchor::LEFT,
        }
    }
}

/// Pixel position for a user-dragged overlay window.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct CustomWindowPosition {
    pub x: i32,
    pub y: i32,
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
    /// TopLeft, TopCenter, TopRight, MiddleLeft, MiddleCenter, MiddleRight, Custom
    pub window_position: WindowPosition,

    /// Position used when window_position is Custom.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_window_position: Option<CustomWindowPosition>,
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
            custom_window_position: None,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum VisualThemePreset {
    #[default]
    Focus,
    Pulse,
    Terminal,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum SpectrogramSkin {
    #[default]
    Bars,
    Waveform,
    Meter,
}

#[derive(Debug, Clone, Copy)]
pub struct ResolvedVisualTheme {
    pub speaking_color: [f32; 4],
    pub idle_color: [f32; 4],
    pub recording_indicator_color: [f32; 4],
    pub typewriter_default: bool,
}

impl VisualThemePreset {
    pub fn resolve(self) -> ResolvedVisualTheme {
        match self {
            VisualThemePreset::Focus => ResolvedVisualTheme {
                speaking_color: [0.1, 0.9, 0.5, 1.0],
                idle_color: [1.0, 0.85, 0.15, 1.0],
                recording_indicator_color: [0.9, 0.2, 0.2, 1.0],
                typewriter_default: false,
            },
            VisualThemePreset::Pulse => ResolvedVisualTheme {
                speaking_color: [0.14, 0.95, 0.72, 1.0],
                idle_color: [0.82, 0.88, 1.0, 1.0],
                recording_indicator_color: [1.0, 0.18, 0.32, 1.0],
                typewriter_default: false,
            },
            VisualThemePreset::Terminal => ResolvedVisualTheme {
                speaking_color: [0.40, 1.0, 0.52, 1.0],
                idle_color: [0.70, 0.95, 0.72, 1.0],
                recording_indicator_color: [0.40, 1.0, 0.52, 1.0],
                typewriter_default: true,
            },
        }
    }
}

/// Configuration for UI appearance settings
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    /// Curated visual theme preset
    pub visual_theme: VisualThemePreset,

    /// Spectrogram rendering skin
    pub spectrogram_skin: SpectrogramSkin,

    /// Base font size for transcript text (default: 10.0)
    /// Actual rendered size is font_size * display_scale
    pub font_size: f32,

    /// Text color when speaking/actively transcribing [r, g, b, a] (0.0-1.0)
    pub speaking_color: [f32; 4],

    /// Text color when idle/not speaking [r, g, b, a] (0.0-1.0)
    pub idle_color: [f32; 4],

    /// Recording indicator dot color [r, g, b, a] (0.0-1.0)
    pub recording_indicator_color: [f32; 4],

    /// Whether to show the pulsing recording indicator
    pub show_recording_indicator: bool,

    /// Whether to enable typewriter effect when transcription completes (manual mode)
    pub typewriter_effect: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            visual_theme: VisualThemePreset::Focus,
            spectrogram_skin: SpectrogramSkin::Bars,
            font_size: 10.0,
            speaking_color: [0.1, 0.9, 0.5, 1.0], // Teal-green
            idle_color: [1.0, 0.85, 0.15, 1.0],   // Gold
            recording_indicator_color: [0.9, 0.2, 0.2, 1.0], // Red
            show_recording_indicator: true,
            typewriter_effect: false,
        }
    }
}

impl UiConfig {
    pub fn effective_speaking_color(&self) -> [f32; 4] {
        match self.visual_theme {
            VisualThemePreset::Focus => self.speaking_color,
            _ => self.visual_theme.resolve().speaking_color,
        }
    }

    pub fn effective_idle_color(&self) -> [f32; 4] {
        match self.visual_theme {
            VisualThemePreset::Focus => self.idle_color,
            _ => self.visual_theme.resolve().idle_color,
        }
    }

    pub fn effective_recording_indicator_color(&self) -> [f32; 4] {
        match self.visual_theme {
            VisualThemePreset::Focus => self.recording_indicator_color,
            _ => self.visual_theme.resolve().recording_indicator_color,
        }
    }

    pub fn effective_spectrogram_color(&self) -> [f32; 4] {
        match self.visual_theme {
            VisualThemePreset::Focus => [1.0, 1.0, 1.0, 1.0],
            VisualThemePreset::Pulse => [0.18, 0.95, 0.72, 1.0],
            VisualThemePreset::Terminal => [0.40, 1.0, 0.52, 1.0],
        }
    }

    pub fn effective_typewriter_enabled(&self) -> bool {
        self.typewriter_effect || self.visual_theme.resolve().typewriter_default
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
            "Auto" => {
                // Auto mode: prefer Fifo, but accept whatever is available
                return available_modes
                    .first()
                    .copied()
                    .unwrap_or(wgpu::PresentMode::Fifo);
            }
            _ => {
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

    /// Moonshine-specific options
    pub moonshine_options: MoonshineOptions,

    /// Parakeet TDT-specific options
    pub parakeet_options: ParakeetOptions,

    /// Nemotron 3.5 ASR-specific options
    pub nemotron_options: NemotronOptions,

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

    /// LFM enhancement configuration ("Magic Mode")
    pub enhancement_config: EnhancementConfig,

    /// UI appearance configuration
    pub ui_config: UiConfig,

    /// Deprecated legacy field - use backend_config instead
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compute_type: Option<String>,

    /// Deprecated legacy field - use backend_config instead
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        let backend_config = BackendConfig {
            backend: BackendType::WhisperCpp,
            ..BackendConfig::default()
        };

        Self {
            general_config: GeneralConfig::default(),
            backend_config,
            audio_processor_config: AudioProcessorConfig::default(),
            realtime_mode_config: RealtimeModeConfig::default(),
            manual_mode_config: ManualModeConfig::default(),
            vad_config: VadConfigSerde::default(),
            common_transcription_options: CommonTranscriptionOptions::default(),
            ctranslate2_options: CT2Options::default(),
            whisper_cpp_options: WhisperCppOptions::default(),
            moonshine_options: MoonshineOptions::default(),
            parakeet_options: ParakeetOptions::default(),
            nemotron_options: NemotronOptions::default(),
            portal_config: PortalConfig::default(),
            display_config: DisplayConfig::default(),
            window_behavior_config: WindowBehaviorConfig::default(),
            sound_config: SoundConfig::default(),
            debug_config: DebugConfig::default(),
            post_process_config: PostProcessConfig::default(),
            enhancement_config: EnhancementConfig::default(),
            ui_config: UiConfig::default(),
            compute_type: None,
            device: None,
        }
    }
}

impl From<AppConfig> for speechcore::SpeechConfig {
    fn from(config: AppConfig) -> Self {
        Self {
            general_config: speechcore::config::GeneralConfig {
                model: config.general_config.model,
                language: config.general_config.language,
                transcription_mode: config.general_config.transcription_mode,
            },
            backend_config: config.backend_config,
            audio_processor_config: speechcore::config::AudioProcessorConfig {
                buffer_size: config.audio_processor_config.buffer_size,
            },
            realtime_mode_config: speechcore::config::RealtimeModeConfig {
                max_buffer_duration_sec: config.realtime_mode_config.max_buffer_duration_sec,
                max_segment_count: config.realtime_mode_config.max_segment_count,
            },
            manual_mode_config: speechcore::config::ManualModeConfig {
                max_recording_duration_secs: config.manual_mode_config.max_recording_duration_secs,
                clear_on_new_session: config.manual_mode_config.clear_on_new_session,
                chunk_duration_seconds: config.manual_mode_config.chunk_duration_seconds,
                enable_chunk_overlap: config.manual_mode_config.enable_chunk_overlap,
                chunk_overlap_seconds: config.manual_mode_config.chunk_overlap_seconds,
                disable_chunking: config.manual_mode_config.disable_chunking,
            },
            vad_config: speechcore::config::VadConfigSerde {
                sensitivity: config.vad_config.sensitivity.into(),
                hangbefore_frames: config.vad_config.hangbefore_frames,
                hangover_frames: config.vad_config.hangover_frames,
                silence_tolerance_frames: config.vad_config.silence_tolerance_frames,
                speech_prob_smoothing: config.vad_config.speech_prob_smoothing,
            },
            common_transcription_options: speechcore::config::CommonTranscriptionOptions {
                beam_size: config.common_transcription_options.beam_size,
                patience: config.common_transcription_options.patience,
            },
            ctranslate2_options: speechcore::config::CT2Options {
                repetition_penalty: config.ctranslate2_options.repetition_penalty,
            },
            whisper_cpp_options: speechcore::config::WhisperCppOptions {
                temperature: config.whisper_cpp_options.temperature,
                suppress_blank: config.whisper_cpp_options.suppress_blank,
                no_context: config.whisper_cpp_options.no_context,
                max_tokens: config.whisper_cpp_options.max_tokens,
                initial_prompt: config.whisper_cpp_options.initial_prompt,
            },
            moonshine_options: speechcore::config::MoonshineOptions {
                enable_cache: config.moonshine_options.enable_cache,
            },
            parakeet_options: speechcore::config::ParakeetOptions::default(),
            nemotron_options: speechcore::config::NemotronOptions {
                language: config.nemotron_options.language,
            },
            debug_config: speechcore::config::DebugConfig {
                log_stats_enabled: config.debug_config.log_stats_enabled,
                save_manual_audio_debug: config.debug_config.save_manual_audio_debug,
                recording_dir: config.debug_config.recording_dir,
            },
            post_process_config: speechcore::config::PostProcessConfig {
                enabled: config.post_process_config.enabled,
                remove_leading_dashes: config.post_process_config.remove_leading_dashes,
                remove_trailing_dashes: config.post_process_config.remove_trailing_dashes,
                normalize_whitespace: config.post_process_config.normalize_whitespace,
            },
            compute_type: config.compute_type,
            device: config.device,
        }
    }
}

impl From<VadSensitivity> for speechcore::config::VadSensitivity {
    fn from(sensitivity: VadSensitivity) -> Self {
        match sensitivity {
            VadSensitivity::Low => Self::Low,
            VadSensitivity::Medium => Self::Medium,
            VadSensitivity::High => Self::High,
        }
    }
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

/// Whisper.cpp internal thresholds - hardcoded to whisper.cpp defaults
pub const WHISPER_ENTROPY_THOLD: f32 = 2.4;
pub const WHISPER_LOGPROB_THOLD: f32 = -1.0;
pub const WHISPER_NO_SPEECH_THOLD: f32 = 0.6;

/// Whisper.cpp-specific transcription options
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WhisperCppOptions {
    pub temperature: f32,
    pub suppress_blank: bool,
    pub no_context: bool,
    pub max_tokens: i32,
    /// Initial prompt to condition the model (used internally for chunk continuity)
    #[serde(skip)]
    pub initial_prompt: Option<String>,
}

impl Default for WhisperCppOptions {
    fn default() -> Self {
        Self {
            temperature: 0.2,     // Gentle sampling bump to match packaged config
            suppress_blank: true, // Skip blank segments
            no_context: true,     // Disable context to prevent double transcriptions
            max_tokens: 0,        // No limit
            initial_prompt: None, // Set dynamically for chunk continuity
        }
    }
}

/// Moonshine-specific options
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct MoonshineOptions {
    /// Whether to use cached decoder (prefill + decode steps) for faster inference
    pub enable_cache: bool,
}

/// Parakeet TDT-specific options
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ParakeetOptions {}

/// Nemotron 3.5 ASR-specific options
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NemotronOptions {
    /// Target language locale for the lang-ID prompt (e.g. "en-US", "de-DE",
    /// or "auto" for built-in language detection).
    pub language: String,
}

impl Default for NemotronOptions {
    fn default() -> Self {
        Self {
            language: "en-US".to_string(),
        }
    }
}

/// Configuration for Voice Activity Detection
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VadConfigSerde {
    /// VAD sensitivity preset for different acoustic environments
    /// Low: Reduces false positives in noisy environments
    /// Medium: Balanced for most environments (default)
    /// High: Catches quiet speech, may trigger on background noise
    pub sensitivity: VadSensitivity,
    /// Number of frames before confirming speech
    pub hangbefore_frames: usize,
    /// Number of frames after speech before ending segment
    pub hangover_frames: usize,
    /// Number of non-speech frames to tolerate in PossibleSpeech before giving up
    pub silence_tolerance_frames: usize,
    /// Exponential moving average smoothing factor (0.0-1.0)
    pub speech_prob_smoothing: f32,
}

impl Default for VadConfigSerde {
    fn default() -> Self {
        Self {
            sensitivity: VadSensitivity::default(), // Medium sensitivity (threshold: 0.10, speech_end: 0.08)
            hangbefore_frames: 5,                   // 50ms - capture more lead-in audio
            hangover_frames: 30,                    // 300ms - keep more trailing audio
            silence_tolerance_frames: 8,            // 80ms - tolerate more pauses
            speech_prob_smoothing: 0.3,             // EMA smoothing factor (production standard)
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

                #[cfg(feature = "backend-ctranslate2")]
                {
                    self.backend_config =
                        speechcore::migrate_legacy_ctranslate2_config(compute_type, device, None);
                    self.compute_type = None;
                    self.device = None;
                }

                #[cfg(not(feature = "backend-ctranslate2"))]
                {
                    println!(
                        "Skipping legacy CTranslate2 config migration because backend-ctranslate2 is disabled"
                    );
                }
            }
        }

        // Ensure whisper.cpp does not reuse context across sessions (prevents duplicate transcriptions)
        if !self.whisper_cpp_options.no_context {
            println!(
                "Enabling whisper_cpp_options.no_context to prevent cross-session duplication"
            );
            self.whisper_cpp_options.no_context = true;
        }

        // Bring legacy configs up to current default temperature if they were using the old default
        if (self.whisper_cpp_options.temperature - 0.0).abs() < f32::EPSILON {
            self.whisper_cpp_options.temperature = 0.2;
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
            println!(
                "Loading configuration from SONORI_CONFIG_PATH: {}",
                path.display()
            );
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
        let path = PathBuf::from(config_home)
            .join("sonori")
            .join("config.toml");
        if path.exists() {
            return Some(path);
        }
    } else if let Some(home) = std::env::var_os("HOME") {
        let path = PathBuf::from(home)
            .join(".config")
            .join("sonori")
            .join("config.toml");
        if path.exists() {
            return Some(path);
        }
    }

    // 2. No config found
    None
}

fn user_config_path() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;

    if let Some(config_home) = std::env::var_os("XDG_CONFIG_HOME") {
        Some(
            PathBuf::from(config_home)
                .join("sonori")
                .join("config.toml"),
        )
    } else {
        std::env::var_os("HOME").map(|home| {
            PathBuf::from(home)
                .join(".config")
                .join("sonori")
                .join("config.toml")
        })
    }
}

/// Create default config in user config directory on first run
fn ensure_user_config() {
    let user_config_path = match user_config_path() {
        Some(path) => path,
        None => return,
    };

    let user_config_dir = match user_config_path.parent() {
        Some(dir) => dir,
        None => return,
    };

    // Skip if user config already exists
    if user_config_path.exists() {
        return;
    }

    // Create config directory
    if let Err(e) = std::fs::create_dir_all(user_config_dir) {
        eprintln!("Failed to create config directory: {}", e);
        return;
    }

    // Write default config as TOML
    let default_config = AppConfig::default();
    match toml::to_string_pretty(&default_config) {
        Ok(toml_string) => match std::fs::write(&user_config_path, toml_string) {
            Ok(_) => println!("Created default config at: {}", user_config_path.display()),
            Err(e) => eprintln!("Failed to write default config: {}", e),
        },
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
                    println!(
                        "Failed to read config from {}: {}. Using default configuration.",
                        path.display(),
                        e
                    );
                    return (AppConfig::default(), None);
                }
            }
        }
        None => {
            println!("No config.toml found. Using default configuration.");
            return (AppConfig::default(), None);
        }
    };

    let config = match build_config_with_defaults(&config_str) {
        Ok((mut config, updated_toml)) => {
            config.migrate_legacy_config();

            if let (Some(path), Some(updated_toml)) = (config_path.as_ref(), updated_toml) {
                if let Err(e) = std::fs::write(path, updated_toml) {
                    eprintln!("Failed to update config with new defaults: {}", e);
                }
            }

            config
        }
        Err(e) => {
            println!(
                "Failed to parse config.toml: {}. Using default configuration.",
                e
            );
            AppConfig::default()
        }
    };

    (config, config_path)
}

pub fn write_app_config(config: &AppConfig) -> Result<(), String> {
    let config_path = find_config_path()
        .or_else(user_config_path)
        .ok_or_else(|| {
            "Unable to determine config path. Set SONORI_CONFIG_PATH or HOME.".to_string()
        })?;

    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create config directory: {}", e))?;
    }

    let toml_string = toml::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    std::fs::write(&config_path, toml_string)
        .map_err(|e| format!("Failed to write config: {}", e))?;

    Ok(())
}

fn build_config_with_defaults(
    config_str: &str,
) -> Result<(AppConfig, Option<String>), toml::de::Error> {
    let mut default_value = toml::Value::Table(Default::default());
    if let Ok(default_toml) = toml::to_string(&AppConfig::default()) {
        if let Ok(value) = toml::from_str::<toml::Value>(&default_toml) {
            default_value = value;
        }
    }

    let user_value = toml::from_str::<toml::Value>(config_str)?;
    let mut merged_value = default_value;
    merge_toml(&mut merged_value, user_value.clone());

    let updated_toml = if merged_value != user_value {
        toml::to_string_pretty(&merged_value).ok()
    } else {
        None
    };

    let merged_string = toml::to_string(&merged_value).unwrap_or_default();
    let config = toml::from_str::<AppConfig>(&merged_string)?;

    Ok((config, updated_toml))
}

fn merge_toml(base: &mut toml::Value, overlay: toml::Value) {
    match (base, overlay) {
        (toml::Value::Table(base_table), toml::Value::Table(overlay_table)) => {
            for (key, value) in overlay_table {
                match base_table.get_mut(&key) {
                    Some(existing) => merge_toml(existing, value),
                    None => {
                        base_table.insert(key, value);
                    }
                }
            }
        }
        (base_value, overlay_value) => {
            *base_value = overlay_value;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_theme_defaults_to_focus() {
        let config = AppConfig::default();

        assert_eq!(config.ui_config.visual_theme, VisualThemePreset::Focus);
        assert_eq!(config.ui_config.spectrogram_skin, SpectrogramSkin::Bars);
        assert_eq!(
            config.ui_config.effective_speaking_color(),
            VisualThemePreset::Focus.resolve().speaking_color
        );
    }

    #[test]
    fn default_backend_is_whisper_cpp() {
        let config = AppConfig::default();

        assert_eq!(config.backend_config.backend, BackendType::WhisperCpp);
    }

    #[test]
    fn focus_theme_respects_existing_custom_ui_colors() {
        let mut config = AppConfig::default();
        config.ui_config.visual_theme = VisualThemePreset::Focus;
        config.ui_config.speaking_color = [0.2, 0.3, 0.4, 1.0];
        config.ui_config.idle_color = [0.5, 0.6, 0.7, 1.0];
        config.ui_config.recording_indicator_color = [0.8, 0.1, 0.2, 1.0];

        assert_eq!(
            config.ui_config.effective_speaking_color(),
            [0.2, 0.3, 0.4, 1.0]
        );
        assert_eq!(
            config.ui_config.effective_idle_color(),
            [0.5, 0.6, 0.7, 1.0]
        );
        assert_eq!(
            config.ui_config.effective_recording_indicator_color(),
            [0.8, 0.1, 0.2, 1.0]
        );
    }

    #[test]
    fn missing_enhancement_prompt_uses_default() {
        let toml = r#"
[enhancement_config]
enabled = true
"#;

        let (config, _) = build_config_with_defaults(toml).expect("config should parse");

        assert_eq!(
            config.enhancement_config.system_prompt.as_deref(),
            Some(DEFAULT_ENHANCEMENT_SYSTEM_PROMPT)
        );
    }
}
