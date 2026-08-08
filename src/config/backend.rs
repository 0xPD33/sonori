use serde::{Deserialize, Serialize};
use speechcore;


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
