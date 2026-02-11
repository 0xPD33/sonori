//! CTranslate2 backend implementation
//!
//! Wraps the existing ct2rs::Whisper implementation with the unified backend interface.

use super::traits::TranscriptionError;
use super::{BackendCapabilities, BackendConfig, BackendType, QuantizationLevel};
use ct2rs::{ComputeType, Config, Device, Whisper};
use std::path::Path;

/// CTranslate2 backend wrapper
pub struct CT2Backend {
    /// The underlying ct2rs Whisper model
    whisper: Whisper,

    /// Configuration used to create this backend
    config: BackendConfig,
}

impl CT2Backend {
    /// Create a new CT2 backend from a model path and configuration
    ///
    /// # Arguments
    /// * `model_path` - Path to the CT2-converted Whisper model directory
    /// * `backend_config` - Backend-agnostic configuration to interpret
    ///
    /// # Returns
    /// Result containing the initialized backend or an error
    pub fn new(
        model_path: impl AsRef<Path>,
        backend_config: &BackendConfig,
    ) -> Result<Self, TranscriptionError> {
        // Map backend config to CT2-specific config
        let ct2_config = Self::map_config(backend_config);

        // Initialize the CT2 Whisper model
        let whisper = Whisper::new(model_path.as_ref(), ct2_config)
            .map_err(|e| TranscriptionError::ModelNotAvailable(e.to_string()))?;

        Ok(Self {
            whisper,
            config: backend_config.clone(),
        })
    }

    /// Map backend-agnostic config to CT2-specific configuration
    fn map_config(backend_config: &BackendConfig) -> Config {
        // Map GPU setting to Device
        let device = if backend_config.gpu_enabled {
            Device::CUDA
        } else {
            Device::CPU
        };

        // Map quantization level to ComputeType
        let compute_type = match backend_config.quantization_level {
            QuantizationLevel::High => ComputeType::FLOAT16,
            QuantizationLevel::Medium => ComputeType::INT8,
            QuantizationLevel::Low => ComputeType::INT8,
        };

        Config {
            device,
            device_indices: vec![0], // Use first GPU if GPU enabled
            compute_type,
            tensor_parallel: false,
            num_threads_per_replica: backend_config.threads,
            max_queued_batches: 0, // Automatic
            cpu_core_offset: -1,   // No offset
        }
    }

    /// Get backend capabilities
    pub fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            name: "CTranslate2",
            max_audio_duration: Some(60.0), // CT2 handles up to 60s segments well
            supported_languages: None,      // Supports all Whisper languages
            supports_streaming: false,      // CT2 doesn't support streaming
            gpu_available: self.config.gpu_enabled,
        }
    }

    /// Transcribe audio samples
    ///
    /// # Arguments
    /// * `samples` - Audio samples (f32, mono)
    /// * `language` - Language code (e.g., "en", "es")
    /// * `common_options` - Common transcription options (beam_size, patience)
    /// * `options` - CTranslate2-specific options
    /// * `sample_rate` - Audio sample rate in Hz (from config, for API consistency)
    ///
    /// # Returns
    /// Transcribed text or error
    pub fn transcribe(
        &self,
        samples: &[f32],
        language: &str,
        common_options: &crate::config::CommonTranscriptionOptions,
        options: &crate::config::CT2Options,
        _sample_rate: usize,
    ) -> Result<String, TranscriptionError> {
        // Convert CT2Options to ct2rs::WhisperOptions, combining with common options
        let ct2_options = options.to_whisper_options(common_options);

        // Call CT2 generate
        let result = self
            .whisper
            .generate(samples, Some(language), false, &ct2_options)?;

        // Extract first result (CT2 returns Vec<String>)
        let transcription = result
            .first()
            .map(|s| s.to_string())
            .unwrap_or_else(|| String::new());

        Ok(transcription)
    }
}

/// Legacy configuration migration
/// Maps old CT2-specific config fields to BackendConfig
pub fn migrate_legacy_config(
    compute_type_str: &str,
    device_str: &str,
    threads: Option<usize>,
) -> BackendConfig {
    // Map compute_type string to QuantizationLevel
    let quantization_level = match compute_type_str.to_uppercase().as_str() {
        "FLOAT32" | "FLOAT16" | "AUTO" => QuantizationLevel::High,
        "INT8" => QuantizationLevel::Medium,
        "INT16" => QuantizationLevel::Low,
        _ => QuantizationLevel::Medium, // Default to medium
    };

    // Map device string to gpu_enabled
    let gpu_enabled =
        device_str.to_uppercase().contains("CUDA") || device_str.to_uppercase().contains("GPU");

    BackendConfig {
        backend: BackendType::CTranslate2,
        threads: threads.unwrap_or_else(|| num_cpus::get().min(4)),
        gpu_enabled,
        quantization_level,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_mapping() {
        let backend_config = BackendConfig {
            backend: BackendType::CTranslate2,
            threads: 4,
            gpu_enabled: false,
            quantization_level: QuantizationLevel::Medium,
        };

        let ct2_config = CT2Backend::map_config(&backend_config);

        assert_eq!(ct2_config.device, Device::CPU);
        assert_eq!(ct2_config.compute_type, ComputeType::INT8);
        assert_eq!(ct2_config.num_threads_per_replica, 4);
    }

    #[test]
    fn test_legacy_migration() {
        let backend_config = migrate_legacy_config("INT8", "CPU", Some(4));

        assert!(!backend_config.gpu_enabled);
        assert_eq!(backend_config.quantization_level, QuantizationLevel::Medium);
        assert_eq!(backend_config.threads, 4);
    }

    #[test]
    fn test_gpu_config() {
        let backend_config = BackendConfig {
            backend: BackendType::CTranslate2,
            threads: 4,
            gpu_enabled: true,
            quantization_level: QuantizationLevel::High,
        };

        let ct2_config = CT2Backend::map_config(&backend_config);

        assert_eq!(ct2_config.device, Device::CUDA);
        assert_eq!(ct2_config.compute_type, ComputeType::FLOAT16);
    }
}
