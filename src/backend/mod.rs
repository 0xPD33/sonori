//! Multi-backend transcription abstraction
//!
//! This module provides a unified interface for different transcription backends,
//! allowing runtime selection between CTranslate2, whisper.cpp, and future backends.
//!
//! # Architecture
//! - Enum-based dispatch for zero-cost abstraction
//! - Backend-agnostic configuration with capability negotiation
//! - Single backend loaded at runtime
//! - Maintains all current features (stats, options, error handling)

pub mod ctranslate2;
pub mod factory;
pub mod moonshine;
pub mod onnx_utils;
pub mod parakeet;
pub mod traits;
pub mod whisper_cpp;

use serde::{Deserialize, Serialize};
use std::fmt;

/// Identifies which backend implementation to use
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendType {
    /// CTranslate2 backend (current default)
    #[serde(alias = "ct2", alias = "ctranslate2")]
    CTranslate2,

    /// whisper.cpp backend via whisper-rs bindings
    #[serde(alias = "whisper-cpp", alias = "whispercpp")]
    WhisperCpp,

    /// Moonshine ONNX backend
    #[serde(alias = "moonshine")]
    Moonshine,

    /// NVIDIA Parakeet backend (future)
    Parakeet,
}

impl Default for BackendType {
    fn default() -> Self {
        BackendType::WhisperCpp
    }
}

impl fmt::Display for BackendType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BackendType::CTranslate2 => write!(f, "ctranslate2"),
            BackendType::WhisperCpp => write!(f, "whisper_cpp"),
            BackendType::Moonshine => write!(f, "moonshine"),
            BackendType::Parakeet => write!(f, "parakeet"),
        }
    }
}

/// Quantization level for model inference
/// Backends interpret this based on their capabilities
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QuantizationLevel {
    /// Highest precision, slower inference
    /// - CTranslate2: FLOAT32 or FLOAT16
    /// - whisper.cpp: f32 or f16
    High,

    /// Balanced precision and speed (default)
    /// - CTranslate2: FLOAT16 or INT8
    /// - whisper.cpp: q5_1 or q8_0
    Medium,

    /// Lowest precision, fastest inference
    /// - CTranslate2: INT8
    /// - whisper.cpp: q4_0 or q5_0
    Low,
}

impl Default for QuantizationLevel {
    fn default() -> Self {
        QuantizationLevel::Medium
    }
}

/// Backend-agnostic configuration for transcription
/// Each backend interprets these generic options based on its capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BackendConfig {
    /// Backend type to use (ctranslate2, whisper_cpp, parakeet)
    pub backend: BackendType,

    /// Number of CPU threads to use for inference
    /// Backends may adjust based on their threading model
    pub threads: usize,

    /// Whether to enable GPU acceleration if available
    /// - CTranslate2: CUDA support
    /// - whisper.cpp: CUDA/Metal/Vulkan support
    pub gpu_enabled: bool,

    /// Quantization level for model weights
    /// Backends translate this to their specific quantization formats
    pub quantization_level: QuantizationLevel,
}

impl Default for BackendConfig {
    fn default() -> Self {
        Self {
            backend: BackendType::default(),
            threads: num_cpus::get().min(4), // Cap at 4 threads for efficiency
            gpu_enabled: true,               // Default to GPU for better performance
            quantization_level: QuantizationLevel::Medium,
        }
    }
}

/// Backend capabilities reported by each implementation
/// Allows UI and config validation to adapt to backend features
#[derive(Debug, Clone)]
pub struct BackendCapabilities {
    /// Backend name for display
    pub name: &'static str,

    /// Maximum audio duration this backend can handle in seconds
    pub max_audio_duration: Option<f32>,

    /// Supported languages (None = all languages)
    pub supported_languages: Option<Vec<String>>,

    /// Whether this backend supports streaming transcription
    pub supports_streaming: bool,

    /// Whether GPU acceleration is available on this system
    pub gpu_available: bool,
}

/// The unified transcription backend enum
/// Uses enum dispatch for zero-cost abstraction
pub enum TranscriptionBackend {
    /// CTranslate2 backend
    CTranslate2(ctranslate2::CT2Backend),

    /// whisper.cpp backend
    WhisperCpp(whisper_cpp::WhisperCppBackend),

    /// Moonshine backend
    Moonshine(moonshine::MoonshineBackend),

    /// Parakeet TDT backend
    Parakeet(parakeet::ParakeetBackend),
}

impl TranscriptionBackend {
    /// Get the backend type
    pub fn backend_type(&self) -> BackendType {
        match self {
            TranscriptionBackend::CTranslate2(_) => BackendType::CTranslate2,
            TranscriptionBackend::WhisperCpp(_) => BackendType::WhisperCpp,
            TranscriptionBackend::Moonshine(_) => BackendType::Moonshine,
            TranscriptionBackend::Parakeet(_) => BackendType::Parakeet,
        }
    }

    /// Get backend capabilities
    pub fn capabilities(&self) -> BackendCapabilities {
        match self {
            TranscriptionBackend::CTranslate2(backend) => backend.capabilities(),
            TranscriptionBackend::WhisperCpp(backend) => backend.capabilities(),
            TranscriptionBackend::Moonshine(backend) => backend.capabilities(),
            TranscriptionBackend::Parakeet(backend) => backend.capabilities(),
        }
    }
}

// Re-export commonly used types
pub use factory::create_backend;
pub use traits::TranscriptionError;
