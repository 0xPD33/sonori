use crate::backend::moonshine::model::{cached_input_shape, MoonshineFlavor, MoonshineLayout, MoonshineModel};
use crate::backend::moonshine::tokenizer::MoonshineTokenizer;
use crate::backend::onnx_utils::OnnxSessionOptions;
use crate::backend::{traits::TranscriptionError, BackendCapabilities, BackendConfig};
use crate::config::{CommonTranscriptionOptions, MoonshineOptions};
use ndarray::{Array1, Array2, ArrayD, Axis, IxDyn};
use ort::session::{SessionInputs, SessionInputValue};
use ort::tensor::TensorElementType;
use ort::value::{DynValue, Tensor, ValueType};
use std::path::Path;
use std::cmp::Ordering;

pub struct MoonshineBackend {
    model: MoonshineModel,
    tokenizer: MoonshineTokenizer,
    config: BackendConfig,
}

impl MoonshineBackend {
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

        let model = MoonshineModel::load(&model_path, &session_options).map_err(|e| {
            TranscriptionError::ModelNotAvailable(format!("Moonshine model load failed: {}", e))
        })?;

        let tokenizer = MoonshineTokenizer::from_dir(&model_path)?;

        Ok(Self {
            model,
            tokenizer,
            config: config.clone(),
        })
    }

    pub fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            name: "Moonshine",
            max_audio_duration: None,
            supported_languages: None,
            supports_streaming: true,
            gpu_available: self.config.gpu_enabled,
        }
    }

    pub fn transcribe(
        &self,
        samples: &[f32],
        _language: &str,
        _common_options: &CommonTranscriptionOptions,
        options: &MoonshineOptions,
        sample_rate: usize,
    ) -> Result<String, TranscriptionError> {
        let expected_rate = self
            .model
            .preprocessor_config
            .as_ref()
            .map(|config| config.sampling_rate)
            .unwrap_or(16_000);

        if sample_rate != expected_rate {
            return Err(TranscriptionError::InvalidAudio(format!(
                "Moonshine expects {}Hz audio, got {}Hz",
                expected_rate, sample_rate
            )));
        }

        let (encoder_inputs, attention_mask) = match self.model.layout {
            MoonshineLayout::Merged => self.prepare_encoder_inputs(samples)?,
            MoonshineLayout::Legacy => (self.preprocess(samples)?, None),
        };

        let max_tokens = self.max_tokens_for_audio(samples.len());
        let encoder_states = self.encode(encoder_inputs, attention_mask.clone())?;
        let token_ids = if options.enable_cache {
            self.greedy_decode_cached(&encoder_states, attention_mask, max_tokens)?
        } else {
            self.greedy_decode(&encoder_states, attention_mask, max_tokens)?
        };
        self.tokenizer.decode(&token_ids)
    }
}

impl MoonshineBackend {
    fn preprocess(&self, samples: &[f32]) -> Result<ArrayD<f32>, TranscriptionError> {
        let preprocess = self.model.preprocess.as_ref().ok_or_else(|| {
            TranscriptionError::InferenceError("Moonshine preprocess session missing".to_string())
        })?;
        let input = Array2::from_shape_vec((1, samples.len()), samples.to_vec()).map_err(|e| {
            TranscriptionError::InvalidAudio(format!("Invalid audio buffer shape: {}", e))
        })?;
        let input_tensor = Tensor::from_array(input).map_err(|e| {
            TranscriptionError::InferenceError(format!("Failed to build input tensor: {}", e))
        })?;

        let mut session = preprocess.lock();
        let outputs = session
            .run(ort::inputs! { self.model.preprocess_input.as_str() => input_tensor })
            .map_err(|e| {
                TranscriptionError::InferenceError(format!("Preprocess failed: {}", e))
            })?;

        outputs
            .get(self.model.preprocess_output.as_str())
            .ok_or_else(|| {
                TranscriptionError::InferenceError("Missing preprocess output".to_string())
            })?
            .try_extract_array::<f32>()
            .map(|arr| arr.to_owned())
            .map_err(|e| {
                TranscriptionError::InferenceError(format!("Preprocess output error: {}", e))
            })
    }

    fn prepare_encoder_inputs(
        &self,
        samples: &[f32],
    ) -> Result<(ArrayD<f32>, Option<ArrayD<i64>>), TranscriptionError> {
        let mut input = samples.to_vec();
        let config = self.model.preprocessor_config.as_ref();
        if let Some(config) = config {
            if config.do_normalize {
                let mean = input.iter().copied().sum::<f32>() / input.len().max(1) as f32;
                let var = input
                    .iter()
                    .map(|v| {
                        let diff = v - mean;
                        diff * diff
                    })
                    .sum::<f32>()
                    / input.len().max(1) as f32;
                let std = var.sqrt().max(1e-6);
                for value in &mut input {
                    *value = (*value - mean) / std;
                }
            }
        }

        let input_values =
            Array2::from_shape_vec((1, input.len()), input).map_err(|e| {
                TranscriptionError::InvalidAudio(format!("Invalid audio buffer shape: {}", e))
            })?;

        let attention_mask = if config.map(|c| c.return_attention_mask).unwrap_or(false) {
            Some(
                Array2::from_shape_vec((1, input_values.shape()[1]), vec![1i64; input_values.shape()[1]])
                    .map_err(|e| {
                        TranscriptionError::InferenceError(format!(
                            "Failed to build attention mask: {}",
                            e
                        ))
                    })?
                    .into_dyn(),
            )
        } else {
            None
        };

        Ok((input_values.into_dyn(), attention_mask))
    }

    fn encode(
        &self,
        features: ArrayD<f32>,
        attention_mask: Option<ArrayD<i64>>,
    ) -> Result<ArrayD<f32>, TranscriptionError> {
        let features_tensor = Tensor::from_array(features).map_err(|e| {
            TranscriptionError::InferenceError(format!("Failed to build features tensor: {}", e))
        })?;

        let mut session = self.model.encoder.lock();
        let mut inputs: Vec<(String, SessionInputValue)> =
            vec![(self.model.encoder_input.clone(), features_tensor.into())];

        if let (Some(mask), Some(mask_name)) = (attention_mask, self.model.encoder_attention_mask.as_ref()) {
            let mask_tensor = Tensor::from_array(mask).map_err(|e| {
                TranscriptionError::InferenceError(format!(
                    "Failed to build attention mask tensor: {}",
                    e
                ))
            })?;
            inputs.push((mask_name.clone(), mask_tensor.into()));
        }

        let outputs = session
            .run(SessionInputs::from(inputs))
            .map_err(|e| TranscriptionError::InferenceError(format!("Encoder failed: {}", e)))?;

        outputs
            .get(self.model.encoder_output.as_str())
            .ok_or_else(|| TranscriptionError::InferenceError("Missing encoder output".to_string()))?
            .try_extract_array::<f32>()
            .map(|arr| arr.to_owned())
            .map_err(|e| TranscriptionError::InferenceError(format!("Encoder output error: {}", e)))
    }

    fn greedy_decode(
        &self,
        encoder_states: &ArrayD<f32>,
        attention_mask: Option<ArrayD<i64>>,
        max_tokens: usize,
    ) -> Result<Vec<u32>, TranscriptionError> {
        let encoder_tensor = Tensor::from_array(encoder_states.clone()).map_err(|e| {
            TranscriptionError::InferenceError(format!("Failed to build encoder tensor: {}", e))
        })?;

        let encoder_attention_mask = match attention_mask {
            Some(mask) => Some(
                Tensor::from_array(mask).map_err(|e| {
                    TranscriptionError::InferenceError(format!(
                        "Failed to build attention mask tensor: {}",
                        e
                    ))
                })?,
            ),
            None => None,
        };

        let mut tokens: Vec<u32> = vec![self.tokenizer.bos_token_id()];
        for _ in 0..max_tokens {
            let input_ids = tokens.iter().map(|id| i64::from(*id)).collect::<Vec<_>>();
            let input_ids = Array2::from_shape_vec((1, input_ids.len()), input_ids).map_err(|e| {
                TranscriptionError::InferenceError(format!("Failed to build input IDs: {}", e))
            })?;
            let input_ids_tensor = Tensor::from_array(input_ids).map_err(|e| {
                TranscriptionError::InferenceError(format!("Failed to build input IDs tensor: {}", e))
            })?;

            let mut inputs: Vec<(String, SessionInputValue)> = vec![
                (self.model.decoder_input_ids.clone(), input_ids_tensor.into()),
                (
                    self.model.decoder_encoder_states.clone(),
                    (&encoder_tensor).into(),
                ),
            ];
            if let (Some(mask), Some(mask_name)) = (
                encoder_attention_mask.as_ref(),
                self.model.decoder_encoder_attention_mask.as_ref(),
            ) {
                inputs.push((mask_name.clone(), mask.into()));
            }
            if let Some((name, element_type)) = &self.model.decoder_use_cache_branch {
                let value = build_scalar_bool(false, *element_type)?;
                inputs.push((name.clone(), value.into()));
            }

            let mut session = self.model.decoder.lock();
            let outputs = session
                .run(SessionInputs::from(inputs))
                .map_err(|e| TranscriptionError::InferenceError(format!("Decoder failed: {}", e)))?;

            let logits = outputs
                .get(self.model.decoder_logits.as_str())
                .ok_or_else(|| TranscriptionError::InferenceError("Missing decoder logits".to_string()))?
                .try_extract_array::<f32>()
                .map_err(|e| {
                    TranscriptionError::InferenceError(format!("Decoder logits error: {}", e))
                })?;

            let next_token = select_next_token(logits.to_owned())?;
            if next_token == self.tokenizer.eos_token_id() {
                break;
            }
            tokens.push(next_token);
        }

        Ok(tokens)
    }

    fn greedy_decode_cached(
        &self,
        encoder_states: &ArrayD<f32>,
        attention_mask: Option<ArrayD<i64>>,
        max_tokens: usize,
    ) -> Result<Vec<u32>, TranscriptionError> {
        let decoder_cached = self.model.decoder_cached.as_ref().ok_or_else(|| {
            TranscriptionError::BackendNotImplemented(
                "Moonshine cached decoder not available".to_string(),
            )
        })?;

        let input_ids_name = self
            .model
            .decoder_cached_input_ids
            .as_ref()
            .ok_or_else(|| {
                TranscriptionError::InferenceError(
                    "Moonshine cached decoder missing input_ids".to_string(),
                )
            })?;
        let encoder_states_name = self
            .model
            .decoder_cached_encoder_states
            .as_ref()
            .ok_or_else(|| {
                TranscriptionError::InferenceError(
                    "Moonshine cached decoder missing encoder_hidden_states".to_string(),
                )
            })?;
        let logits_name = self
            .model
            .decoder_cached_logits
            .as_ref()
            .ok_or_else(|| {
                TranscriptionError::InferenceError(
                    "Moonshine cached decoder missing logits output".to_string(),
                )
            })?;

        let past_names = &self.model.decoder_cached_past_inputs;
        let present_names = &self.model.decoder_cached_present_outputs;

        if !past_names.is_empty() && past_names.len() != present_names.len() {
            return Err(TranscriptionError::InferenceError(format!(
                "Moonshine cached decoder past/present mismatch ({} vs {})",
                past_names.len(),
                present_names.len()
            )));
        }

        let encoder_tensor = Tensor::from_array(encoder_states.clone()).map_err(|e| {
            TranscriptionError::InferenceError(format!("Failed to build encoder tensor: {}", e))
        })?;
        let encoder_attention_mask = match attention_mask {
            Some(mask) => Some(
                Tensor::from_array(mask).map_err(|e| {
                    TranscriptionError::InferenceError(format!(
                        "Failed to build attention mask tensor: {}",
                        e
                    ))
                })?,
            ),
            None => None,
        };

        let mut tokens: Vec<u32> = vec![self.tokenizer.bos_token_id()];
        let mut past_cache: Option<Vec<DynValue>> = None;

        for _ in 0..max_tokens {
            let last_token = *tokens.last().unwrap_or(&self.tokenizer.bos_token_id());
            let input_ids = Array2::from_shape_vec((1, 1), vec![i64::from(last_token)]).map_err(|e| {
                TranscriptionError::InferenceError(format!("Failed to build input IDs: {}", e))
            })?;
            let input_ids_tensor = Tensor::from_array(input_ids).map_err(|e| {
                TranscriptionError::InferenceError(format!(
                    "Failed to build input IDs tensor: {}",
                    e
                ))
            })?;

            let mut inputs: Vec<(String, SessionInputValue)> = Vec::new();
            inputs.push((input_ids_name.clone(), input_ids_tensor.into()));
            inputs.push((encoder_states_name.clone(), (&encoder_tensor).into()));

            if let (Some(mask), Some(mask_name)) = (
                encoder_attention_mask.as_ref(),
                self.model.decoder_encoder_attention_mask.as_ref(),
            ) {
                inputs.push((mask_name.clone(), mask.into()));
            }

            if let Some((name, element_type)) = &self.model.decoder_use_cache_branch {
                let value = build_scalar_bool(true, *element_type)?;
                inputs.push((name.clone(), value.into()));
            }

            if !past_names.is_empty() {
                if past_cache.is_none() {
                    past_cache = Some(self.init_past_cache(decoder_cached, past_names, self.model.flavor)?);
                }

                if let Some(ref cache_values) = past_cache {
                    for (name, value) in past_names.iter().zip(cache_values.iter()) {
                        inputs.push((name.clone(), value.into()));
                    }
                }
            }

            let mut session = decoder_cached.lock();
            let mut outputs = session
                .run(SessionInputs::from(inputs))
                .map_err(|e| TranscriptionError::InferenceError(format!("Decoder failed: {}", e)))?;

            let logits = outputs
                .get(logits_name.as_str())
                .ok_or_else(|| TranscriptionError::InferenceError("Missing decoder logits".to_string()))?
                .try_extract_array::<f32>()
                .map_err(|e| {
                    TranscriptionError::InferenceError(format!("Decoder logits error: {}", e))
                })?
                .to_owned();

            if !present_names.is_empty() {
                let mut new_cache = Vec::with_capacity(present_names.len());
                for name in present_names {
                    let value = outputs.remove(name).ok_or_else(|| {
                        TranscriptionError::InferenceError(format!(
                            "Missing cached decoder output: {}",
                            name
                        ))
                    })?;
                    new_cache.push(value);
                }
                past_cache = Some(new_cache);
            }

            let next_token = select_next_token(logits)?;
            if next_token == self.tokenizer.eos_token_id() {
                break;
            }
            tokens.push(next_token);
        }

        Ok(tokens)
    }

    fn init_past_cache(
        &self,
        decoder_cached: &parking_lot::Mutex<ort::session::Session>,
        past_names: &[String],
        flavor: Option<MoonshineFlavor>,
    ) -> Result<Vec<DynValue>, TranscriptionError> {
        let mut cache_values = Vec::with_capacity(past_names.len());

        let session = decoder_cached.lock();

        for name in past_names {
            let shape = if let (Some(flavor), true) = (flavor, name.contains("past_key_values")) {
                vec![
                    0,
                    flavor.num_key_value_heads as i64,
                    1,
                    flavor.head_dim as i64,
                ]
            } else {
                cached_input_shape(&session.inputs, name).map_err(|e| {
                    TranscriptionError::InferenceError(format!(
                        "Failed to read cached decoder input shape: {}",
                        e
                    ))
                })?
            };

            let input = session
                .inputs
                .iter()
                .find(|input| input.name == *name)
                .ok_or_else(|| {
                    TranscriptionError::InferenceError(format!(
                        "Missing cached decoder input metadata: {}",
                        name
                    ))
                })?;

            let element_type = match &input.input_type {
                ValueType::Tensor { ty, .. } => *ty,
                _ => {
                    return Err(TranscriptionError::InferenceError(format!(
                        "Cached decoder input is not a tensor: {}",
                        name
                    )))
                }
            };

            if element_type != TensorElementType::Float32 {
                return Err(TranscriptionError::InferenceError(format!(
                    "Cached decoder input {} has unsupported type {:?}",
                    name, element_type
                )));
            }

            let dims = shape
                .iter()
                .enumerate()
                .map(|(idx, dim)| {
                    if *dim < 0 {
                        if idx == 0 {
                            1usize
                        } else {
                            0usize
                        }
                    } else {
                        *dim as usize
                    }
                })
                .collect::<Vec<_>>();

            let total = dims.iter().product::<usize>();
            let data = vec![0.0f32; total];
            let array = ArrayD::from_shape_vec(IxDyn(&dims), data).map_err(|e| {
                TranscriptionError::InferenceError(format!(
                    "Failed to allocate cache tensor {}: {}",
                    name, e
                ))
            })?;
            let tensor = Tensor::from_array(array).map_err(|e| {
                TranscriptionError::InferenceError(format!(
                    "Failed to build cache tensor {}: {}",
                    name, e
                ))
            })?;
            cache_values.push(tensor.into_dyn());
        }

        Ok(cache_values)
    }

    fn max_tokens_for_audio(&self, audio_len: usize) -> usize {
        let rate = self
            .model
            .preprocessor_config
            .as_ref()
            .map(|config| config.sampling_rate)
            .unwrap_or(16_000) as f32;
        let token_rate = self
            .model
            .flavor
            .map(|flavor| flavor.token_rate as f32);

        if let Some(token_rate) = token_rate {
            let seconds = (audio_len as f32) / rate;
            (seconds * token_rate).ceil().max(1.0) as usize
        } else {
            256
        }
    }
}

fn build_scalar_bool(
    value: bool,
    element_type: TensorElementType,
) -> Result<DynValue, TranscriptionError> {
    match element_type {
        TensorElementType::Bool => {
            let array = Array1::from(vec![value]);
            let tensor = Tensor::from_array(array).map_err(|e| {
                TranscriptionError::InferenceError(format!(
                    "Failed to build bool scalar tensor: {}",
                    e
                ))
            })?;
            Ok(tensor.into_dyn())
        }
        TensorElementType::Int64 => {
            let array = Array1::from(vec![if value { 1i64 } else { 0i64 }]);
            let tensor = Tensor::from_array(array).map_err(|e| {
                TranscriptionError::InferenceError(format!(
                    "Failed to build int64 scalar tensor: {}",
                    e
                ))
            })?;
            Ok(tensor.into_dyn())
        }
        TensorElementType::Int32 => {
            let array = Array1::from(vec![if value { 1i32 } else { 0i32 }]);
            let tensor = Tensor::from_array(array).map_err(|e| {
                TranscriptionError::InferenceError(format!(
                    "Failed to build int32 scalar tensor: {}",
                    e
                ))
            })?;
            Ok(tensor.into_dyn())
        }
        other => Err(TranscriptionError::InferenceError(format!(
            "Unsupported use_cache_branch type: {:?}",
            other
        ))),
    }
}

fn select_next_token(logits: ArrayD<f32>) -> Result<u32, TranscriptionError> {
    let vector: Array1<f32> = match logits.ndim() {
        1 => logits
            .into_dimensionality()
            .map_err(|e| TranscriptionError::InferenceError(e.to_string()))?,
        2 => {
            let last_row = logits.shape()[0].saturating_sub(1);
            logits
                .index_axis(Axis(0), last_row)
                .to_owned()
                .into_dimensionality()
                .map_err(|e| TranscriptionError::InferenceError(e.to_string()))?
        }
        3 => {
            let batch = logits.index_axis(Axis(0), 0);
            let last_row = batch.shape()[0].saturating_sub(1);
            batch
                .index_axis(Axis(0), last_row)
                .to_owned()
                .into_dimensionality()
                .map_err(|e| TranscriptionError::InferenceError(e.to_string()))?
        }
        _ => {
            return Err(TranscriptionError::InferenceError(format!(
                "Unsupported logits shape: {:?}",
                logits.shape()
            )));
        }
    };

    let mut best_idx = 0usize;
    let mut best_val = f32::NEG_INFINITY;
    for (idx, value) in vector.iter().enumerate() {
        let value = *value;
        let ordering = value.partial_cmp(&best_val).unwrap_or(Ordering::Less);
        if ordering == Ordering::Greater {
            best_val = value;
            best_idx = idx;
        }
    }

    Ok(best_idx as u32)
}
