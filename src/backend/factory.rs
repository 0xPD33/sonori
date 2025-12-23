//! Backend factory for creating transcription backends based on configuration

use super::ctranslate2::CT2Backend;
use super::traits::TranscriptionError;
use super::whisper_cpp::WhisperCppBackend;
use super::{BackendConfig, BackendType, TranscriptionBackend};
use std::path::{Path, PathBuf};

/// Create a transcription backend based on type and configuration
///
/// This function handles model provisioning automatically:
/// - For CT2: Uses provided model path (directory)
/// - For WhisperCpp: Downloads GGML model if not present
///
/// # Arguments
/// * `backend_type` - Which backend implementation to create
/// * `model_path` - Path to the model (CT2 uses this directly, whisper.cpp may download)
/// * `backend_config` - Backend-agnostic configuration
///
/// # Returns
/// Initialized TranscriptionBackend or error
pub async fn create_backend(
    backend_type: BackendType,
    model_path: impl AsRef<Path>,
    backend_config: &BackendConfig,
) -> Result<TranscriptionBackend, TranscriptionError> {
    match backend_type {
        BackendType::CTranslate2 => {
            // CT2 backend: use model path as-is (directory)
            let ct2_backend = CT2Backend::new(model_path, backend_config)?;
            Ok(TranscriptionBackend::CTranslate2(ct2_backend))
        }

        BackendType::WhisperCpp => {
            // WhisperCpp backend: auto-download GGML model if needed
            let model_path_ref = model_path.as_ref();

            // If model doesn't exist, try to download it
            if !model_path_ref.exists() {
                // Extract model name from path (e.g., "/path/ggml-base.en.bin" -> "base.en")
                let model_name = extract_whisper_cpp_model_name(model_path_ref)?;

                println!("WhisperCpp model not found, downloading: {}", model_name);

                // Download the model (this uses the quantization from backend_config)
                crate::download::download_whisper_cpp_model(
                    &model_name,
                    &backend_config.quantization_level,
                )
                .await
                .map_err(|e| {
                    TranscriptionError::ModelNotAvailable(format!(
                        "Failed to download whisper.cpp model: {}",
                        e
                    ))
                })?;
            }

            // Create the backend with the model
            let whisper_cpp_backend = WhisperCppBackend::new(model_path, backend_config)?;
            Ok(TranscriptionBackend::WhisperCpp(whisper_cpp_backend))
        }

        BackendType::Parakeet => Err(TranscriptionError::BackendNotImplemented(
            "Parakeet backend not yet implemented. Please use CTranslate2 backend.".to_string(),
        )),

        BackendType::Moonshine => {
             let moonshine_backend = super::moonshine::MoonshineBackend::new(model_path, backend_config)?;
             Ok(TranscriptionBackend::Moonshine(moonshine_backend))
        }
    }
}

/// Extract model name from a whisper.cpp model path
///
/// Converts paths like:
/// - "/path/to/ggml-base.en.bin" -> "base.en"
/// - "/path/to/ggml-small-q5_1.bin" -> "small"
fn extract_whisper_cpp_model_name(path: &Path) -> Result<String, TranscriptionError> {
    let filename = path.file_stem().and_then(|s| s.to_str()).ok_or_else(|| {
        TranscriptionError::ConfigurationError(format!("Invalid model path: {}", path.display()))
    })?;

    // Remove "ggml-" prefix if present
    let name = filename.strip_prefix("ggml-").unwrap_or(filename);

    // Remove quantization suffix if present (e.g., "-q5_1", "-q4_0", "-q8_0")
    // But keep model names like "large-v3-turbo" intact
    let name = if let Some(pos) = name.rfind("-q") {
        // Check if what follows "-q" looks like a quantization (e.g., "5_1", "8_0")
        let after_q = &name[pos + 2..];
        if after_q.chars().next().map_or(false, |c| c.is_ascii_digit()) {
            &name[..pos]
        } else {
            name
        }
    } else {
        name
    };

    Ok(name.to_string())
}

/// Validate that a model path is compatible with a backend
///
/// # Arguments
/// * `backend_type` - The backend type to validate for
/// * `model_path` - Path to check
///
/// # Returns
/// Ok if compatible, Err with explanation if not
pub fn validate_model_path(
    backend_type: BackendType,
    model_path: impl AsRef<Path>,
) -> Result<(), TranscriptionError> {
    let path = model_path.as_ref();

    if !path.exists() {
        return Err(TranscriptionError::IoError(format!(
            "Model path does not exist: {}",
            path.display()
        )));
    }

    match backend_type {
        BackendType::CTranslate2 => {
            // CT2 models are directories containing multiple files
            if !path.is_dir() {
                return Err(TranscriptionError::ConfigurationError(
                    "CTranslate2 models must be directories containing model files".to_string(),
                ));
            }

            // Check for essential CT2 files
            let model_bin = path.join("model.bin");
            if !model_bin.exists() {
                return Err(TranscriptionError::ConfigurationError(
                    "CTranslate2 model directory missing model.bin file".to_string(),
                ));
            }

            Ok(())
        }

        BackendType::WhisperCpp => {
            // whisper.cpp models are single .bin files (GGML format)
            if !path.is_file() {
                return Err(TranscriptionError::ConfigurationError(
                    "whisper.cpp models must be .bin files in GGML format".to_string(),
                ));
            }

            if path.extension().and_then(|s| s.to_str()) != Some("bin") {
                return Err(TranscriptionError::ConfigurationError(
                    "whisper.cpp models must have .bin extension".to_string(),
                ));
            }

            Ok(())
        }

        BackendType::Parakeet => Err(TranscriptionError::BackendNotImplemented(
            "Parakeet backend not yet implemented".to_string(),
        )),

        BackendType::Moonshine => {
            if !path.is_dir() {
                return Err(TranscriptionError::ConfigurationError(
                    "Moonshine models must be directories".to_string(),
                ));
            }
            super::moonshine::model::MoonshineModel::validate_model_dir(path).map_err(|e| {
                TranscriptionError::ConfigurationError(format!(
                    "Moonshine model directory invalid: {}",
                    e
                ))
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_type_display() {
        assert_eq!(BackendType::CTranslate2.to_string(), "ctranslate2");
        assert_eq!(BackendType::WhisperCpp.to_string(), "whisper_cpp");
        assert_eq!(BackendType::Parakeet.to_string(), "parakeet");
    }
}
