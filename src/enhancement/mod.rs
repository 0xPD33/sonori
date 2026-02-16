mod llama_model;

pub use llama_model::LlamaCppModel;

use std::fmt;
use std::path::Path;

#[derive(Debug)]
pub enum EnhancementError {
    ModelNotAvailable(String),
    TokenizerError(String),
    InferenceError(String),
    IoError(std::io::Error),
}

impl fmt::Display for EnhancementError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ModelNotAvailable(msg) => write!(f, "Model not available: {}", msg),
            Self::TokenizerError(msg) => write!(f, "Tokenizer error: {}", msg),
            Self::InferenceError(msg) => write!(f, "Inference error: {}", msg),
            Self::IoError(e) => write!(f, "IO error: {}", e),
        }
    }
}

impl std::error::Error for EnhancementError {}

impl From<std::io::Error> for EnhancementError {
    fn from(e: std::io::Error) -> Self {
        Self::IoError(e)
    }
}

/// Trait for text enhancement models
/// Implement this trait to add support for new LLM backends
pub trait EnhancementModel: Send + Sync {
    /// Enhance a transcription, optionally with a custom system prompt
    fn enhance(
        &self,
        transcription: &str,
        system_prompt: Option<&str>,
    ) -> Result<String, EnhancementError>;

    /// Get the model's name/identifier
    fn name(&self) -> &str;
}

/// Check if model files are available at the given path
pub fn is_model_available(model_path: impl AsRef<Path>) -> bool {
    LlamaCppModel::is_available(&model_path)
}

/// Load a model from the given path
pub fn load_model(
    model_path: impl AsRef<Path>,
) -> Result<Box<dyn EnhancementModel>, EnhancementError> {
    Ok(Box::new(LlamaCppModel::from_file(model_path)?))
}

// Implement the trait for LlamaCppModel
impl EnhancementModel for LlamaCppModel {
    fn enhance(
        &self,
        transcription: &str,
        system_prompt: Option<&str>,
    ) -> Result<String, EnhancementError> {
        self.enhance(transcription, system_prompt)
    }

    fn name(&self) -> &str {
        "LlamaCpp"
    }
}
