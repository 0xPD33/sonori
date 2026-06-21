mod llama_model;

pub use llama_model::LlamaCppModel;

use crate::config::{EnhancementConfig, DEFAULT_ENHANCEMENT_SYSTEM_PROMPT};
use futures_util::StreamExt;
use parking_lot::Mutex;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;

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

pub struct MagicModeEnhancer {
    config: EnhancementConfig,
    enabled: Arc<AtomicBool>,
    model: Mutex<Option<Box<dyn EnhancementModel>>>,
}

impl MagicModeEnhancer {
    pub fn new(config: EnhancementConfig, enabled: Arc<AtomicBool>) -> Self {
        Self {
            config,
            enabled,
            model: Mutex::new(None),
        }
    }

    fn model_path(&self) -> Result<Option<PathBuf>, EnhancementError> {
        let Some(model) = self.config.model.as_deref() else {
            eprintln!("Magic Mode enabled but no enhancement model is configured");
            return Ok(None);
        };

        if is_enhancement_gguf_available(model) {
            return get_enhancement_gguf_path(model).map(Some);
        }

        eprintln!("Enhancement model not found, attempting download: {model}");
        download_enhancement_gguf_blocking(model).map(Some)
    }

    fn load_model_if_needed(&self) -> Result<(), EnhancementError> {
        let mut model = self.model.lock();
        if model.is_some() {
            return Ok(());
        }

        let Some(path) = self.model_path()? else {
            return Ok(());
        };

        if !is_model_available(&path) {
            return Err(EnhancementError::ModelNotAvailable(format!(
                "GGUF model not found: {}",
                path.display()
            )));
        }

        println!("Loading Sonori Magic Mode model from: {}", path.display());
        *model = Some(Box::new(LlamaCppModel::from_file_with_options(
            &path,
            self.config.max_tokens,
            2048,
        )?));
        Ok(())
    }
}

impl MagicModeEnhancer {
    pub fn enhance(&self, transcription: &str) -> anyhow::Result<String> {
        if !self.enabled.load(Ordering::Relaxed) || transcription.trim().is_empty() {
            return Ok(transcription.to_string());
        }

        self.load_model_if_needed()?;

        let model = self.model.lock();
        let Some(model) = model.as_ref() else {
            return Ok(transcription.to_string());
        };

        let system_prompt = self
            .config
            .system_prompt
            .as_deref()
            .or(Some(DEFAULT_ENHANCEMENT_SYSTEM_PROMPT));

        match model.enhance(transcription, system_prompt) {
            Ok(enhanced) => Ok(enhanced),
            Err(e) => {
                eprintln!("Magic Mode enhancement failed: {e}");
                Ok(transcription.to_string())
            }
        }
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

fn enhancement_model_dir() -> Result<PathBuf, EnhancementError> {
    let model_dir = if let Ok(path) = std::env::var("SONORI_ENHANCEMENT_MODEL_DIR") {
        PathBuf::from(path)
    } else if let Some(cache_home) = std::env::var_os("XDG_CACHE_HOME") {
        PathBuf::from(cache_home)
            .join("sonori")
            .join("models")
            .join("enhancement")
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home)
            .join(".cache")
            .join("sonori")
            .join("models")
            .join("enhancement")
    } else {
        PathBuf::from("enhancement")
    };

    std::fs::create_dir_all(&model_dir)?;
    Ok(model_dir)
}

fn get_enhancement_gguf_path(model: &str) -> Result<PathBuf, EnhancementError> {
    let filename = model.split('/').next_back().unwrap_or(model);
    Ok(enhancement_model_dir()?.join(filename))
}

fn is_enhancement_gguf_available(model: &str) -> bool {
    get_enhancement_gguf_path(model).is_ok_and(|path| path.exists())
}

fn download_enhancement_gguf_blocking(model: &str) -> Result<PathBuf, EnhancementError> {
    let runtime = tokio::runtime::Runtime::new().map_err(EnhancementError::IoError)?;
    runtime.block_on(download_enhancement_gguf(model))
}

async fn download_enhancement_gguf(model: &str) -> Result<PathBuf, EnhancementError> {
    let parts: Vec<&str> = model.splitn(3, '/').collect();
    if parts.len() < 3 {
        return Err(EnhancementError::ModelNotAvailable(
            "Invalid model format. Expected owner/repo/filename.gguf".to_string(),
        ));
    }

    let repo = format!("{}/{}", parts[0], parts[1]);
    let filename = parts[2];
    let output_path = enhancement_model_dir()?.join(filename);
    if output_path.exists() {
        return Ok(output_path);
    }

    let url = format!("https://huggingface.co/{repo}/resolve/main/{filename}");
    println!("Downloading Sonori Magic Mode model from: {url}");

    let response = reqwest::get(&url)
        .await
        .map_err(|e| EnhancementError::InferenceError(format!("download request failed: {e}")))?
        .error_for_status()
        .map_err(|e| EnhancementError::InferenceError(format!("download failed: {e}")))?;

    let mut file = tokio::fs::File::create(&output_path)
        .await
        .map_err(EnhancementError::IoError)?;
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk =
            chunk.map_err(|e| EnhancementError::InferenceError(format!("download failed: {e}")))?;
        file.write_all(&chunk)
            .await
            .map_err(EnhancementError::IoError)?;
    }

    Ok(output_path)
}
