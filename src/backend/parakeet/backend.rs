use crate::backend::onnx_utils::OnnxSessionOptions;
use crate::backend::{traits::TranscriptionError, BackendCapabilities, BackendConfig};
use crate::config::{CommonTranscriptionOptions, ParakeetOptions};
use ort::value::Tensor;
use std::path::Path;

use super::decoder::greedy_decode_tdt;
use super::model::ParakeetModel;
use super::tokenizer::ParakeetTokenizer;

pub struct ParakeetBackend {
    model: ParakeetModel,
    tokenizer: ParakeetTokenizer,
    config: BackendConfig,
}

impl ParakeetBackend {
    pub fn new(
        model_path: impl AsRef<Path>,
        config: &BackendConfig,
    ) -> Result<Self, TranscriptionError> {
        let session_options = OnnxSessionOptions {
            intra_threads: config.threads,
            inter_threads: 1,
            execution_provider: if config.gpu_enabled {
                crate::backend::onnx_utils::ExecutionProviderPreference::PreferGpu
            } else {
                crate::backend::onnx_utils::ExecutionProviderPreference::CpuOnly
            },
        };

        let model = ParakeetModel::load(&model_path, &session_options).map_err(|e| {
            TranscriptionError::ModelNotAvailable(format!("Parakeet model load failed: {}", e))
        })?;

        let tokenizer = ParakeetTokenizer::from_dir(&model_path)?;

        Ok(Self {
            model,
            tokenizer,
            config: config.clone(),
        })
    }

    pub fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            name: "Parakeet TDT",
            max_audio_duration: Some(90.0), // TDT decoder limited to ~10K steps ≈ ~100s; 90s safe margin
            supported_languages: Some(vec![
                "en".into(),
                "de".into(),
                "es".into(),
                "fr".into(),
                "it".into(),
                "pt".into(),
                "nl".into(),
                "pl".into(),
                "ro".into(),
                "sv".into(),
                "da".into(),
                "fi".into(),
                "no".into(),
                "cs".into(),
                "sk".into(),
                "hu".into(),
                "el".into(),
                "bg".into(),
                "hr".into(),
                "sl".into(),
                "lt".into(),
                "lv".into(),
                "et".into(),
                "uk".into(),
                "ca".into(),
            ]),
            supports_streaming: false,
            gpu_available: self.config.gpu_enabled,
        }
    }

    pub fn transcribe(
        &self,
        samples: &[f32],
        _language: &str,
        _common_options: &CommonTranscriptionOptions,
        _options: &ParakeetOptions,
        sample_rate: usize,
    ) -> Result<String, TranscriptionError> {
        if sample_rate != 16000 {
            return Err(TranscriptionError::InvalidAudio(format!(
                "Parakeet expects 16000Hz audio, got {}Hz",
                sample_rate
            )));
        }

        // 1. Compute mel spectrogram -> [1, 128, T]
        let mel_features = self.model.mel.compute(samples);

        // 2. Run encoder
        // Encoder inputs: audio_signal [B, 128, T] (float), length [B] (int64)
        let num_frames = mel_features.shape()[2] as i64;
        let mel_tensor = Tensor::from_array(mel_features).map_err(|e| {
            TranscriptionError::InferenceError(format!("Failed to build mel tensor: {}", e))
        })?;

        let length = ndarray::Array1::from(vec![num_frames]);
        let length_tensor = Tensor::from_array(length).map_err(|e| {
            TranscriptionError::InferenceError(format!("Failed to build length tensor: {}", e))
        })?;

        // Encoder outputs: outputs [B, 1024, T'], encoded_lengths [B]
        let (encoder_out, encoded_length) = {
            let mut encoder = self.model.encoder.lock();
            let encoder_outputs = encoder
                .run(ort::inputs! {
                    "audio_signal" => mel_tensor,
                    "length" => length_tensor
                })
                .map_err(|e| {
                    TranscriptionError::InferenceError(format!("Encoder failed: {}", e))
                })?;

            let out = encoder_outputs
                .get("outputs")
                .ok_or_else(|| {
                    TranscriptionError::InferenceError(
                        "Missing encoder 'outputs'".to_string(),
                    )
                })?
                .try_extract_array::<f32>()
                .map(|a| a.to_owned())
                .map_err(|e| {
                    TranscriptionError::InferenceError(format!("Encoder output error: {}", e))
                })?;

            let enc_len = encoder_outputs
                .get("encoded_lengths")
                .ok_or_else(|| {
                    TranscriptionError::InferenceError(
                        "Missing encoder 'encoded_lengths'".to_string(),
                    )
                })?
                .try_extract_array::<i64>()
                .map(|a| a.to_owned())
                .map_err(|e| {
                    TranscriptionError::InferenceError(format!(
                        "Encoder encoded_lengths error: {}",
                        e
                    ))
                })?;

            let t_enc = enc_len.iter().next().copied().unwrap_or(0) as usize;
            (out, t_enc)
        };

        // 3. Run greedy TDT decode (decoder + joiner loop)
        let token_ids = greedy_decode_tdt(
            &encoder_out,
            encoded_length,
            &self.model.decoder,
            &self.model.joiner,
            self.tokenizer.vocab_size(),
            self.tokenizer.blank_id(),
            10000,
        )?;

        // 4. Decode tokens to string
        Ok(self.tokenizer.decode(&token_ids))
    }
}
