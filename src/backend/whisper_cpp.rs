//! whisper.cpp backend implementation via whisper-rs bindings
//!
//! Provides an alternative transcription backend using whisper.cpp (GGML models)
//! instead of CTranslate2.

use super::traits::TranscriptionError;
use super::{BackendCapabilities, BackendConfig};
use parking_lot::Mutex;
use std::path::Path;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState};

/// whisper.cpp backend wrapper
pub struct WhisperCppBackend {
    /// The underlying whisper-rs context (holds loaded model)
    context: WhisperContext,

    /// Reusable state for transcription (eliminates per-call allocation overhead)
    state: Mutex<WhisperState>,

    /// Configuration used to create this backend
    config: BackendConfig,
}

impl WhisperCppBackend {
    /// Create a new WhisperCpp backend from a model path and configuration
    ///
    /// # Arguments
    /// * `model_path` - Path to the GGML model file (.bin)
    /// * `backend_config` - Backend-agnostic configuration to interpret
    ///
    /// # Returns
    /// Result containing the initialized backend or an error
    pub fn new(
        model_path: impl AsRef<Path>,
        backend_config: &BackendConfig,
    ) -> Result<Self, TranscriptionError> {
        // Create context parameters
        let mut ctx_params = WhisperContextParameters::default();
        ctx_params.use_gpu = backend_config.gpu_enabled;

        // Get model path as string
        let model_path_str = model_path
            .as_ref()
            .to_str()
            .ok_or_else(|| {
                TranscriptionError::ConfigurationError("Invalid model path encoding".to_string())
            })?;

        // Load the GGML model
        let context = WhisperContext::new_with_params(model_path_str, ctx_params).map_err(|e| {
            TranscriptionError::ModelNotAvailable(format!("Failed to load GGML model: {:?}", e))
        })?;

        println!("whisper.cpp model loaded successfully!");
        println!("  Model is multilingual: {}", context.is_multilingual());

        // Create initial state for reuse across transcriptions
        let state = context.create_state().map_err(|e| {
            TranscriptionError::ModelNotAvailable(format!("Failed to create whisper state: {:?}", e))
        })?;

        Ok(Self {
            context,
            state: Mutex::new(state),
            config: backend_config.clone(),
        })
    }

    /// Get backend capabilities
    pub fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            name: "whisper.cpp",
            max_audio_duration: None, // No hard limit - processes audio of any length
            supported_languages: None,       // Supports all Whisper languages
            supports_streaming: false,       // Standard whisper.cpp doesn't stream
            gpu_available: self.config.gpu_enabled,
        }
    }

    /// Transcribe audio samples
    ///
    /// # Arguments
    /// * `samples` - Audio samples (f32, mono)
    /// * `language` - Language code (e.g., "en", "es")
    /// * `common_options` - Common transcription options (beam_size, patience)
    /// * `options` - Whisper.cpp-specific options
    /// * `sample_rate` - Audio sample rate in Hz (from config)
    ///
    /// # Returns
    /// Transcribed text or error
    pub fn transcribe(
        &self,
        samples: &[f32],
        language: &str,
        common_options: &crate::config::CommonTranscriptionOptions,
        options: &crate::config::WhisperCppOptions,
        sample_rate: usize,
    ) -> Result<String, TranscriptionError> {
        // Lock the reusable state (eliminates per-transcription allocation overhead)
        let mut state = self.state.lock();

        // Build FullParams based on beam size from common options
        let mut params = if common_options.beam_size > 1 {
            FullParams::new(SamplingStrategy::BeamSearch {
                beam_size: common_options.beam_size as i32,
                patience: common_options.patience,
            })
        } else {
            // True greedy decoding (no best_of for speed)
            FullParams::new(SamplingStrategy::Greedy { best_of: 1 })
        };

        // Configure transcription parameters
        params.set_n_threads(self.config.threads as i32);
        params.set_language(Some(language));

        // Disable console output (we handle our own logging)
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);

        // Apply whisper.cpp-specific options from config
        params.set_temperature(options.temperature);
        params.set_suppress_blank(options.suppress_blank);
        params.set_no_context(options.no_context);
        if options.max_tokens > 0 {
            params.set_max_tokens(options.max_tokens);
        }
        params.set_entropy_thold(options.entropy_thold);
        params.set_logprob_thold(options.logprob_thold);
        params.set_no_speech_thold(options.no_speech_thold);

        // Adaptive segmentation: Use single_segment for short audio (<=30s), multi-segment for longer
        // The Whisper model was trained on 30-second chunks, so longer audio benefits from chunking
        const SEGMENT_THRESHOLD_SECONDS: usize = 30;
        let audio_duration_secs = samples.len() / sample_rate;
        let use_single_segment = audio_duration_secs <= SEGMENT_THRESHOLD_SECONDS;

        params.set_single_segment(use_single_segment);
        params.set_no_timestamps(false);   // We need timestamps for segment boundaries

        if !use_single_segment {
            println!(
                "Using multi-segment transcription for {:.1}s audio (threshold: {}s)",
                audio_duration_secs as f32, SEGMENT_THRESHOLD_SECONDS
            );
        }

        // Note: repetition_penalty not supported by whisper-rs
        // whisper.cpp has built-in repetition handling, so this is okay

        // Run the transcription using the reused state
        state.full(params, samples).map_err(|e| {
            TranscriptionError::InferenceError(format!("Transcription failed: {:?}", e))
        })?;

        // Extract and concatenate all segment text
        let mut full_text = String::new();
        for segment in state.as_iter() {
            let segment_text = segment.to_str().map_err(|e| {
                TranscriptionError::InferenceError(format!(
                    "Failed to extract segment text: {:?}",
                    e
                ))
            })?;
            full_text.push_str(segment_text);
        }

        Ok(full_text)
    }
}

/// Map whisper-rs errors to TranscriptionError
impl From<whisper_rs::WhisperError> for TranscriptionError {
    fn from(err: whisper_rs::WhisperError) -> Self {
        use whisper_rs::WhisperError;

        match err {
            WhisperError::InitError => {
                TranscriptionError::ModelNotAvailable(format!("Initialization error: {:?}", err))
            }
            WhisperError::NoSamples | WhisperError::InvalidThreadCount => {
                TranscriptionError::InvalidAudio(format!("Invalid input: {:?}", err))
            }
            _ => TranscriptionError::InferenceError(format!("whisper-rs error: {:?}", err)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_interpretation() {
        let config = BackendConfig {
            threads: 4,
            gpu_enabled: false,
            quantization_level: super::super::QuantizationLevel::Medium,
        };

        // Just verify the config structure
        assert_eq!(config.threads, 4);
        assert!(!config.gpu_enabled);
    }

    #[test]
    fn test_capabilities() {
        // Can't create a real backend without a model file,
        // but we can test the structure
        let config = BackendConfig::default();
        let caps = BackendCapabilities {
            name: "whisper.cpp",
            max_audio_duration: None, // No hard limit
            supported_languages: None,
            supports_streaming: false,
            gpu_available: config.gpu_enabled,
        };

        assert_eq!(caps.name, "whisper.cpp");
        assert_eq!(caps.max_audio_duration, None);
    }
}
