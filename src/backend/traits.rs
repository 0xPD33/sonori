//! Common traits and error types for transcription backends

use std::error::Error;
use std::fmt;

/// Unified error type for transcription operations
#[derive(Debug, Clone)]
pub enum TranscriptionError {
    /// Model not loaded or initialization failed
    ModelNotAvailable(String),

    /// Backend-specific inference error
    InferenceError(String),

    /// Invalid audio format or parameters
    InvalidAudio(String),

    /// Unsupported language for this backend
    UnsupportedLanguage(String),

    /// Backend not yet implemented
    BackendNotImplemented(String),

    /// Configuration error
    ConfigurationError(String),

    /// I/O error (model loading, etc.)
    IoError(String),
}

impl fmt::Display for TranscriptionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TranscriptionError::ModelNotAvailable(msg) => {
                write!(f, "Model not available: {}", msg)
            }
            TranscriptionError::InferenceError(msg) => {
                write!(f, "Inference error: {}", msg)
            }
            TranscriptionError::InvalidAudio(msg) => {
                write!(f, "Invalid audio: {}", msg)
            }
            TranscriptionError::UnsupportedLanguage(msg) => {
                write!(f, "Unsupported language: {}", msg)
            }
            TranscriptionError::BackendNotImplemented(msg) => {
                write!(f, "Backend not implemented: {}", msg)
            }
            TranscriptionError::ConfigurationError(msg) => {
                write!(f, "Configuration error: {}", msg)
            }
            TranscriptionError::IoError(msg) => {
                write!(f, "I/O error: {}", msg)
            }
        }
    }
}

impl Error for TranscriptionError {}

/// Convert anyhow errors (used by ct2rs) to TranscriptionError
impl From<anyhow::Error> for TranscriptionError {
    fn from(err: anyhow::Error) -> Self {
        TranscriptionError::InferenceError(err.to_string())
    }
}

/// Convert std::io::Error to TranscriptionError
impl From<std::io::Error> for TranscriptionError {
    fn from(err: std::io::Error) -> Self {
        TranscriptionError::IoError(err.to_string())
    }
}

/// Standardized transcription result
/// Allows backends to provide additional metadata in the future
#[derive(Debug, Clone)]
pub struct TranscriptionResult {
    /// The transcribed text
    pub text: String,

    /// Optional confidence score (0.0-1.0)
    pub confidence: Option<f32>,

    /// Optional language detected (if different from requested)
    pub detected_language: Option<String>,
}

impl TranscriptionResult {
    /// Create a simple result with just text
    pub fn new(text: String) -> Self {
        Self {
            text,
            confidence: None,
            detected_language: None,
        }
    }

    /// Create a result with confidence score
    pub fn with_confidence(text: String, confidence: f32) -> Self {
        Self {
            text,
            confidence: Some(confidence),
            detected_language: None,
        }
    }
}
