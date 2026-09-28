mod backend;
mod theme;

pub use backend::*;
pub use theme::*;

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
    /// Microphone to record from: part of its name, matched without case.
    /// The system default input when unset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_device: Option<String>,
}

impl Default for AudioProcessorConfig {
    fn default() -> Self {
        Self {
            buffer_size: 1024,
            input_device: None,
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
    /// What happens with a finished transcript
    pub output_mode: OutputMode,
}

/// What happens with a finished transcript.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum OutputMode {
    /// Copy to the clipboard, then send the paste shortcut.
    #[default]
    Paste,
    /// Type the text with wtype or dotool. The clipboard stays untouched.
    Type,
    /// Copy to the clipboard only.
    Clipboard,
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
    /// Maximum recording duration in seconds (default: 600)
    /// Buffer size is calculated as: max_recording_duration_secs * sample_rate
    pub max_recording_duration_secs: u32,

    /// Whether to clear previous transcript when starting new session
    pub clear_on_new_session: bool,

    /// Duration of each chunk in seconds (default: 29.0)
    /// Note: 29s avoids edge case where duration == chunk_size hits token limits
    pub chunk_duration_seconds: f32,

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
            output_mode: OutputMode::default(),
        }
    }
}

impl Default for ManualModeConfig {
    fn default() -> Self {
        Self {
            max_recording_duration_secs: 600,
            clear_on_new_session: true,
            chunk_duration_seconds: 29.0, // 29s avoids edge case at exactly 30s boundary
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
}

/// Configuration for the persistent transcript history file
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TranscriptHistoryConfig {
    /// Append every final transcript to the history file
    pub enabled: bool,
    /// History file (default: ~/.cache/sonori/transcript_history.txt). Skipped
    /// when serializing the default, so each user keeps their own home path.
    #[serde(skip_serializing_if = "is_default_transcript_history_path")]
    pub path: String,
}

impl Default for TranscriptHistoryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            path: default_transcript_history_path(),
        }
    }
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
    /// Drop standalone filler words such as "um" and "uh"
    pub remove_fillers: bool,
    /// Collapse an immediately repeated word ("the the" -> "the").
    /// Off by default: doubles like "had had" and "that that" are legitimate.
    pub collapse_repeated_words: bool,
    /// Capitalize the first letter of each sentence.
    /// Off by default: every current backend already capitalizes, and it is
    /// wrong when dictating shell commands.
    pub capitalize_sentences: bool,
    /// Append a full stop when the text ends without terminal punctuation.
    /// Off by default for the same reason as `capitalize_sentences`.
    pub ensure_terminal_punctuation: bool,
    /// Whole-word replacements, matched without case: `"nix os" = "NixOS"`.
    /// Fixes names and jargon for every backend.
    pub replacements: std::collections::BTreeMap<String, String>,
}

impl Default for PostProcessConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            remove_leading_dashes: true,
            remove_trailing_dashes: true,
            normalize_whitespace: true,
            remove_fillers: true,
            collapse_repeated_words: false,
            capitalize_sentences: false,
            ensure_terminal_punctuation: false,
            replacements: Default::default(),
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
    /// Maximum tokens to generate (default: 1024)
    pub max_tokens: usize,
    /// Whether Magic Mode is on. Kept across restarts.
    pub active: bool,
}

impl Default for EnhancementConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model: None,
            system_prompt: default_enhancement_system_prompt(),
            max_tokens: 1024,
            active: false,
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

    /// Persistent transcript history
    pub history_config: TranscriptHistoryConfig,

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
            history_config: TranscriptHistoryConfig::default(),
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
                input_device: config.audio_processor_config.input_device,
            },
            realtime_mode_config: speechcore::config::RealtimeModeConfig {
                max_buffer_duration_sec: config.realtime_mode_config.max_buffer_duration_sec,
                max_segment_count: config.realtime_mode_config.max_segment_count,
            },
            manual_mode_config: speechcore::config::ManualModeConfig {
                max_recording_duration_secs: config.manual_mode_config.max_recording_duration_secs,
                clear_on_new_session: config.manual_mode_config.clear_on_new_session,
                chunk_duration_seconds: config.manual_mode_config.chunk_duration_seconds,
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
                remove_fillers: config.post_process_config.remove_fillers,
                collapse_repeated_words: config.post_process_config.collapse_repeated_words,
                capitalize_sentences: config.post_process_config.capitalize_sentences,
                ensure_terminal_punctuation: config.post_process_config.ensure_terminal_punctuation,
                replacements: config.post_process_config.replacements,
            },
            compute_type: config.compute_type,
            device: config.device,
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

        // 120 s was the old cap; long recordings now transcribe while recording.
        if self.manual_mode_config.max_recording_duration_secs == 120 {
            self.manual_mode_config.max_recording_duration_secs = 600;
        }

        // 256 was the old default and cut longer dictations short.
        if self.enhancement_config.max_tokens == 256 {
            self.enhancement_config.max_tokens = 1024;
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

/// The config file Sonori reads and writes, for display, with `~` for the home directory.
pub fn config_path_for_display() -> String {
    let Some(path) = find_config_path().or_else(user_config_path) else {
        return "none (set HOME)".to_string();
    };
    match std::env::var_os("HOME").and_then(|home| path.strip_prefix(home).ok()) {
        Some(relative) => format!("~/{}", relative.display()),
        None => path.display().to_string(),
    }
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
        Ok((mut config, updated_toml, unknown_keys)) => {
            config.migrate_legacy_config();

            if !unknown_keys.is_empty() {
                eprintln!(
                    "Warning: unknown config keys are ignored: {}",
                    unknown_keys.join(", ")
                );
            }

            if let (Some(path), Some(updated_toml)) = (config_path.as_ref(), updated_toml) {
                if let Err(e) = write_config_file(path, &updated_toml) {
                    eprintln!("Failed to update config with new defaults: {}", e);
                }
            }

            config
        }
        Err(e) => {
            eprintln!(
                "Failed to parse config.toml ({}). Using default configuration; saving settings is blocked until the file is fixed.",
                describe_toml_error(&config_str, &e)
            );
            if let Some(path) = config_path.as_ref() {
                let backup = path.with_extension("toml.bak");
                match std::fs::copy(path, &backup) {
                    Ok(_) => eprintln!("Backed up the invalid config to {}", backup.display()),
                    Err(e) => eprintln!("Failed to back up the invalid config: {}", e),
                }
            }
            AppConfig::default()
        }
    };

    (config, config_path)
}

/// A user-facing problem with the config file on disk: a parse error or unknown keys.
pub fn config_file_problem() -> Option<String> {
    let content = std::fs::read_to_string(find_config_path()?).ok()?;
    match build_config_with_defaults(&content) {
        Err(e) => Some(format!(
            "config.toml invalid ({}); using defaults",
            describe_toml_error(&content, &e)
        )),
        Ok((_, _, unknown_keys)) if !unknown_keys.is_empty() => Some(format!(
            "Unknown config keys ignored: {}",
            unknown_keys.join(", ")
        )),
        Ok(_) => None,
    }
}

pub fn write_app_config(config: &AppConfig) -> Result<(), String> {
    let config_path = find_config_path()
        .or_else(user_config_path)
        .ok_or_else(|| {
            "Unable to determine config path. Set SONORI_CONFIG_PATH or HOME.".to_string()
        })?;

    // The running config fell back to defaults; saving it would erase the user's file.
    if let Ok(content) = std::fs::read_to_string(&config_path) {
        if let Err(e) = build_config_with_defaults(&content) {
            return Err(format!(
                "config.toml has an error ({}); fix it before saving settings",
                describe_toml_error(&content, &e)
            ));
        }
    }

    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create config directory: {}", e))?;
    }

    let toml_string = toml::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    write_config_file(&config_path, &toml_string)
        .map_err(|e| format!("Failed to write config: {}", e))?;

    Ok(())
}

/// Writes through a temp file and a rename, so a crash never leaves a truncated
/// config. The rename targets the resolved path, so a symlinked config stays a symlink.
fn write_config_file(path: &std::path::Path, content: &str) -> std::io::Result<()> {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, &path)
}

fn describe_toml_error(config_str: &str, e: &toml::de::Error) -> String {
    let message = e.message().trim();
    match e.span().and_then(|span| config_str.get(..span.start)) {
        Some(before) => format!("line {}: {}", before.matches('\n').count() + 1, message),
        None => message.to_string(),
    }
}

/// Returns the merged config, the merged TOML when it differs from the user's
/// file, and the user's keys that no config field reads.
fn build_config_with_defaults(
    config_str: &str,
) -> Result<(AppConfig, Option<String>, Vec<String>), toml::de::Error> {
    let mut default_value = toml::Value::Table(Default::default());
    if let Ok(default_toml) = toml::to_string(&AppConfig::default()) {
        if let Ok(value) = toml::from_str::<toml::Value>(&default_toml) {
            default_value = value;
        }
    }

    let mut user_value = toml::from_str::<toml::Value>(config_str)?;
    migrate_legacy_keys(&mut user_value);
    // Typed parse of the user's own text, so error spans point into their file.
    let parsed = toml::from_str::<AppConfig>(config_str)?;

    // ponytail: a key whose value serde skips when serializing (an unset Option,
    // or the default transcript path) is reported only if the user writes that
    // exact skipped value, which the generated config never does.
    let mut unknown_keys = Vec::new();
    if let Ok(known) = toml::Value::try_from(&parsed) {
        collect_unknown_keys(&user_value, &known, "", &mut unknown_keys);
    }

    let mut merged_value = default_value;
    merge_toml(&mut merged_value, user_value.clone());

    let updated_toml = if merged_value != user_value {
        toml::to_string_pretty(&merged_value).ok()
    } else {
        None
    };

    let merged_string = toml::to_string(&merged_value).unwrap_or_default();
    let config = toml::from_str::<AppConfig>(&merged_string)?;

    Ok((config, updated_toml, unknown_keys))
}

/// Moves or drops keys that older versions wrote, so they are not reported as
/// unknown and the write-back removes them from the file.
fn migrate_legacy_keys(user: &mut toml::Value) {
    let Some(root) = user.as_table_mut() else {
        return;
    };
    if let Some(manual) = root
        .get_mut("manual_mode_config")
        .and_then(toml::Value::as_table_mut)
    {
        // Chunk overlap was never implemented.
        manual.remove("enable_chunk_overlap");
        manual.remove("chunk_overlap_seconds");
    }
    let legacy_path = root
        .get_mut("debug_config")
        .and_then(toml::Value::as_table_mut)
        .and_then(|debug| {
            // History is on by default now; the old default `false` is not a choice.
            debug.remove("save_transcript_history");
            debug.remove("transcript_history_path")
        });
    if let Some(path) = legacy_path {
        if let Some(history) = root
            .entry("history_config")
            .or_insert_with(|| toml::Value::Table(Default::default()))
            .as_table_mut()
        {
            history.entry("path").or_insert(path);
        }
    }
}

fn collect_unknown_keys(
    user: &toml::Value,
    known: &toml::Value,
    prefix: &str,
    out: &mut Vec<String>,
) {
    let (Some(user), Some(known)) = (user.as_table(), known.as_table()) else {
        return;
    };
    for (key, value) in user {
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        match known.get(key) {
            Some(known_value) => collect_unknown_keys(value, known_value, &path, out),
            None => out.push(path),
        }
    }
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

        let (config, _, _) = build_config_with_defaults(toml).expect("config should parse");

        assert_eq!(
            config.enhancement_config.system_prompt.as_deref(),
            Some(DEFAULT_ENHANCEMENT_SYSTEM_PROMPT)
        );
    }
}
