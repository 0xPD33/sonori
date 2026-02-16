use crate::backend::onnx_utils::{load_session, OnnxSessionOptions};
use anyhow::{Context, Result};
use ort::session::{Input, Output, Session};
use ort::value::ValueType;
use parking_lot::Mutex;
use serde::Deserialize;
use std::path::{Path, PathBuf};

const PREPROCESS_ONNX: &str = "preprocess.onnx";
const ENCODER_ONNX: &str = "encode.onnx";
const DECODER_UNCACHED_ONNX: &str = "uncached_decode.onnx";
const DECODER_CACHED_ONNX: &str = "cached_decode.onnx";
const TOKENIZER_FILENAME: &str = "tokenizer.json";
const MERGED_ENCODER_ONNX: &str = "encoder_model.onnx";
const MERGED_DECODER_ONNX: &str = "decoder_model_merged.onnx";
const MERGED_PREPROCESSOR_CONFIG: &str = "preprocessor_config.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoonshineLayout {
    Legacy,
    Merged,
}

pub struct MoonshineModel {
    pub layout: MoonshineLayout,
    pub preprocessor_config: Option<MoonshinePreprocessorConfig>,
    pub flavor: Option<MoonshineFlavor>,
    pub preprocess: Option<Mutex<Session>>,
    pub encoder: Mutex<Session>,
    pub decoder: Mutex<Session>,
    pub decoder_cached: Option<Mutex<Session>>,
    pub preprocess_input: String,
    pub preprocess_output: String,
    pub encoder_input: String,
    pub encoder_attention_mask: Option<String>,
    pub encoder_output: String,
    pub decoder_input_ids: String,
    pub decoder_encoder_states: String,
    pub decoder_encoder_attention_mask: Option<String>,
    pub decoder_logits: String,
    pub decoder_use_cache_branch: Option<(String, ort::tensor::TensorElementType)>,
    pub decoder_cached_input_ids: Option<String>,
    pub decoder_cached_encoder_states: Option<String>,
    pub decoder_cached_past_inputs: Vec<String>,
    pub decoder_cached_logits: Option<String>,
    pub decoder_cached_present_outputs: Vec<String>,
}

pub struct MoonshineModelPaths {
    pub preprocess: PathBuf,
    pub encoder: PathBuf,
    pub decoder: PathBuf,
    pub decoder_cached: PathBuf,
    pub preprocessor_config: PathBuf,
}

impl MoonshineModel {
    pub fn resolve_paths(model_dir: impl AsRef<Path>) -> Result<MoonshineModelPaths> {
        let model_dir = model_dir.as_ref();
        let preprocess = model_dir.join(PREPROCESS_ONNX);
        let encoder = model_dir.join(ENCODER_ONNX);
        let decoder = model_dir.join(DECODER_UNCACHED_ONNX);
        let decoder_cached = model_dir.join(DECODER_CACHED_ONNX);
        let preprocessor_config = model_dir.join(MERGED_PREPROCESSOR_CONFIG);

        Ok(MoonshineModelPaths {
            preprocess,
            encoder,
            decoder,
            decoder_cached,
            preprocessor_config,
        })
    }

    pub fn load(model_dir: impl AsRef<Path>, options: &OnnxSessionOptions) -> Result<Self> {
        let model_dir = model_dir.as_ref();
        let paths = Self::resolve_paths(model_dir)?;
        let flavor = resolve_flavor(model_dir);

        let merged_encoder = model_dir.join(MERGED_ENCODER_ONNX);
        let merged_decoder = model_dir.join(MERGED_DECODER_ONNX);
        let merged_preprocessor = paths.preprocessor_config.clone();

        let (layout, encoder_path, decoder_path, preprocess_path, preprocessor_config) =
            if merged_encoder.exists() && merged_decoder.exists() {
                (
                    MoonshineLayout::Merged,
                    merged_encoder,
                    merged_decoder,
                    None,
                    Some(load_preprocessor_config(&merged_preprocessor)?),
                )
            } else {
                if !paths.preprocess.exists() {
                    return Err(anyhow::anyhow!(
                        "Missing Moonshine preprocessor model: {}",
                        paths.preprocess.display()
                    ));
                }
                if !paths.encoder.exists() {
                    return Err(anyhow::anyhow!(
                        "Missing Moonshine encoder model: {}",
                        paths.encoder.display()
                    ));
                }
                if !paths.decoder.exists() {
                    return Err(anyhow::anyhow!(
                        "Missing Moonshine decoder model: {}",
                        paths.decoder.display()
                    ));
                }
                (
                    MoonshineLayout::Legacy,
                    paths.encoder.clone(),
                    paths.decoder.clone(),
                    Some(paths.preprocess.clone()),
                    None,
                )
            };
        let tokenizer_path = model_dir.join(TOKENIZER_FILENAME);
        if !tokenizer_path.exists() {
            return Err(anyhow::anyhow!(
                "Missing Moonshine tokenizer: {}",
                tokenizer_path.display()
            ));
        }

        let preprocess = match preprocess_path {
            Some(path) => Some(
                load_session(&path, options)
                    .with_context(|| "Failed to load Moonshine preprocess.onnx")?,
            ),
            None => None,
        };
        let encoder = load_session(&encoder_path, options).with_context(|| {
            if layout == MoonshineLayout::Merged {
                "Failed to load Moonshine encoder_model.onnx"
            } else {
                "Failed to load Moonshine encode.onnx"
            }
        })?;
        let decoder = load_session(&decoder_path, options).with_context(|| {
            if layout == MoonshineLayout::Merged {
                "Failed to load Moonshine decoder_model_merged.onnx"
            } else {
                "Failed to load Moonshine uncached_decode.onnx"
            }
        })?;

        let decoder_cached = if paths.decoder_cached.exists() {
            Some(
                load_session(&paths.decoder_cached, options)
                    .with_context(|| "Failed to load Moonshine cached_decode.onnx")?,
            )
        } else {
            None
        };

        let (preprocess_input, preprocess_output) = if let Some(ref preprocess) = preprocess {
            (
                resolve_input_name(
                    &preprocess.inputs,
                    &["input_values", "audio", "input"],
                    "preprocess input",
                )?,
                resolve_output_name(
                    &preprocess.outputs,
                    &["input_features", "features", "output"],
                    "preprocess output",
                )?,
            )
        } else {
            ("input_values".to_string(), "input_features".to_string())
        };

        let encoder_input = resolve_input_name(
            &encoder.inputs,
            &["input_features", "input_values", "features", "input"],
            "encoder input",
        )?;
        let encoder_attention_mask =
            resolve_optional_input_name(&encoder.inputs, &["attention_mask"]);
        let encoder_output = resolve_output_name(
            &encoder.outputs,
            &["encoder_hidden_states", "last_hidden_state", "output"],
            "encoder output",
        )?;

        let decoder_input_ids = resolve_input_name(
            &decoder.inputs,
            &["input_ids", "tokens", "decoder_input_ids"],
            "decoder input_ids",
        )?;
        let decoder_encoder_states = resolve_input_name(
            &decoder.inputs,
            &[
                "encoder_hidden_states",
                "encoder_outputs",
                "encoder_hidden_state",
            ],
            "decoder encoder_hidden_states",
        )?;
        let decoder_encoder_attention_mask =
            resolve_optional_input_name(&decoder.inputs, &["encoder_attention_mask"]);
        let decoder_use_cache_branch =
            resolve_optional_input_type(&decoder.inputs, &["use_cache_branch", "use_cache"]);
        let decoder_logits =
            resolve_output_name(&decoder.outputs, &["logits", "output"], "decoder logits")?;

        let (
            decoder_cached_input_ids,
            decoder_cached_encoder_states,
            decoder_cached_past_inputs,
            decoder_cached_logits,
            decoder_cached_present_outputs,
        ) = if let Some(ref cached_session) = decoder_cached {
            let cached_session = cached_session;
            let cached_input_ids = resolve_input_name(
                &cached_session.inputs,
                &["input_ids", "tokens", "decoder_input_ids"],
                "cached decoder input_ids",
            )?;
            let cached_encoder_states = resolve_input_name(
                &cached_session.inputs,
                &[
                    "encoder_hidden_states",
                    "encoder_outputs",
                    "encoder_hidden_state",
                ],
                "cached decoder encoder_hidden_states",
            )?;

            let past_inputs = cached_session
                .inputs
                .iter()
                .filter(|input| is_cache_tensor(&input.name))
                .map(|input| input.name.clone())
                .collect::<Vec<_>>();

            let present_outputs = cached_session
                .outputs
                .iter()
                .filter(|output| is_present_tensor(&output.name))
                .map(|output| output.name.clone())
                .collect::<Vec<_>>();

            let cached_logits = resolve_output_name(
                &cached_session.outputs,
                &["logits", "output"],
                "cached decoder logits",
            )?;

            (
                Some(cached_input_ids),
                Some(cached_encoder_states),
                past_inputs,
                Some(cached_logits),
                present_outputs,
            )
        } else {
            (None, None, Vec::new(), None, Vec::new())
        };

        Ok(Self {
            layout,
            preprocessor_config,
            flavor,
            preprocess: preprocess.map(Mutex::new),
            encoder: Mutex::new(encoder),
            decoder: Mutex::new(decoder),
            decoder_cached: decoder_cached.map(Mutex::new),
            preprocess_input,
            preprocess_output,
            encoder_input,
            encoder_attention_mask,
            encoder_output,
            decoder_input_ids,
            decoder_encoder_states,
            decoder_encoder_attention_mask,
            decoder_logits,
            decoder_use_cache_branch,
            decoder_cached_input_ids,
            decoder_cached_encoder_states,
            decoder_cached_past_inputs,
            decoder_cached_logits,
            decoder_cached_present_outputs,
        })
    }

    pub fn validate_model_dir(model_dir: impl AsRef<Path>) -> Result<()> {
        let paths = Self::resolve_paths(model_dir.as_ref())?;

        let merged_encoder = model_dir.as_ref().join(MERGED_ENCODER_ONNX);
        let merged_decoder = model_dir.as_ref().join(MERGED_DECODER_ONNX);
        let merged_preprocessor = model_dir.as_ref().join(MERGED_PREPROCESSOR_CONFIG);

        if merged_encoder.exists() && merged_decoder.exists() {
            if !merged_preprocessor.exists() {
                return Err(anyhow::anyhow!(
                    "Missing Moonshine preprocessor config: {}",
                    merged_preprocessor.display()
                ));
            }
        } else {
            if !paths.preprocess.exists() {
                return Err(anyhow::anyhow!(
                    "Missing Moonshine preprocessor model: {}",
                    paths.preprocess.display()
                ));
            }
            if !paths.encoder.exists() {
                return Err(anyhow::anyhow!(
                    "Missing Moonshine encoder model: {}",
                    paths.encoder.display()
                ));
            }
            if !paths.decoder.exists() {
                return Err(anyhow::anyhow!(
                    "Missing Moonshine decoder model: {}",
                    paths.decoder.display()
                ));
            }
        }

        let tokenizer_path = model_dir.as_ref().join(TOKENIZER_FILENAME);
        if !tokenizer_path.exists() {
            return Err(anyhow::anyhow!(
                "Missing Moonshine tokenizer: {}",
                tokenizer_path.display()
            ));
        }

        Ok(())
    }
}

fn resolve_input_name(inputs: &[Input], candidates: &[&str], label: &str) -> Result<String> {
    resolve_name(
        inputs
            .iter()
            .map(|input| input.name.as_str())
            .collect::<Vec<_>>(),
        candidates,
        label,
    )
}

fn resolve_output_name(outputs: &[Output], candidates: &[&str], label: &str) -> Result<String> {
    resolve_name(
        outputs
            .iter()
            .map(|output| output.name.as_str())
            .collect::<Vec<_>>(),
        candidates,
        label,
    )
}

fn resolve_optional_input_name(inputs: &[Input], candidates: &[&str]) -> Option<String> {
    for candidate in candidates {
        for input in inputs {
            if input.name.eq_ignore_ascii_case(candidate) {
                return Some(input.name.clone());
            }
        }
    }
    for candidate in candidates {
        let candidate_lower = candidate.to_lowercase();
        for input in inputs {
            if input.name.to_lowercase().contains(&candidate_lower) {
                return Some(input.name.clone());
            }
        }
    }
    None
}

fn resolve_optional_input_type(
    inputs: &[Input],
    candidates: &[&str],
) -> Option<(String, ort::tensor::TensorElementType)> {
    for candidate in candidates {
        for input in inputs {
            if input.name.eq_ignore_ascii_case(candidate) {
                if let ValueType::Tensor { ty, .. } = input.input_type {
                    return Some((input.name.clone(), ty));
                }
            }
        }
    }
    for candidate in candidates {
        let candidate_lower = candidate.to_lowercase();
        for input in inputs {
            if input.name.to_lowercase().contains(&candidate_lower) {
                if let ValueType::Tensor { ty, .. } = input.input_type {
                    return Some((input.name.clone(), ty));
                }
            }
        }
    }
    None
}

fn resolve_name(names: Vec<&str>, candidates: &[&str], label: &str) -> Result<String> {
    if names.len() == 1 {
        return Ok(names[0].to_string());
    }

    for candidate in candidates {
        for name in &names {
            if name.eq_ignore_ascii_case(candidate) {
                return Ok((*name).to_string());
            }
        }
    }

    for candidate in candidates {
        let candidate_lower = candidate.to_lowercase();
        for name in &names {
            if name.to_lowercase().contains(&candidate_lower) {
                return Ok((*name).to_string());
            }
        }
    }

    Err(anyhow::anyhow!(
        "Unable to resolve {} (candidates: {:?}, available: {:?})",
        label,
        candidates,
        names
    ))
}

fn is_cache_tensor(name: &str) -> bool {
    let name = name.to_lowercase();
    name.contains("past") || name.contains("key_values")
}

fn is_present_tensor(name: &str) -> bool {
    let name = name.to_lowercase();
    name.contains("present") || name.contains("key_values")
}

pub fn cached_input_shape(inputs: &[Input], name: &str) -> Result<Vec<i64>> {
    let input = inputs
        .iter()
        .find(|input| input.name == name)
        .ok_or_else(|| anyhow::anyhow!("Cached decoder missing input: {}", name))?;

    match &input.input_type {
        ValueType::Tensor { shape, .. } => Ok(shape.to_vec()),
        _ => Err(anyhow::anyhow!(
            "Cached decoder input is not a tensor: {}",
            name
        )),
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct MoonshinePreprocessorConfig {
    pub do_normalize: bool,
    pub feature_extractor_type: String,
    pub feature_size: usize,
    pub padding_side: String,
    pub padding_value: f32,
    pub return_attention_mask: bool,
    pub sampling_rate: usize,
}

fn load_preprocessor_config(path: &Path) -> Result<MoonshinePreprocessorConfig> {
    let contents = std::fs::read_to_string(path).with_context(|| {
        format!(
            "Failed to read Moonshine preprocessor config: {}",
            path.display()
        )
    })?;
    let config: MoonshinePreprocessorConfig =
        serde_json::from_str(&contents).with_context(|| {
            format!(
                "Failed to parse Moonshine preprocessor config: {}",
                path.display()
            )
        })?;
    Ok(config)
}

#[derive(Debug, Clone, Copy)]
pub struct MoonshineFlavor {
    pub token_rate: usize,
    pub num_layers: usize,
    pub num_key_value_heads: usize,
    pub head_dim: usize,
}

fn resolve_flavor(model_dir: &Path) -> Option<MoonshineFlavor> {
    let model_id = model_dir.file_name()?.to_str()?;
    let model_id = model_id
        .strip_prefix("moonshine-")
        .and_then(|name| name.strip_suffix("-onnx"))
        .unwrap_or(model_id);

    match model_id {
        "tiny" => Some(MoonshineFlavor {
            token_rate: 6,
            num_layers: 6,
            num_key_value_heads: 8,
            head_dim: 36,
        }),
        "base" => Some(MoonshineFlavor {
            token_rate: 6,
            num_layers: 8,
            num_key_value_heads: 8,
            head_dim: 52,
        }),
        "tiny-ar" | "tiny-zh" | "tiny-ja" | "tiny-ko" | "tiny-vi" => Some(MoonshineFlavor {
            token_rate: 13,
            num_layers: 6,
            num_key_value_heads: 8,
            head_dim: 36,
        }),
        "tiny-uk" => Some(MoonshineFlavor {
            token_rate: 8,
            num_layers: 6,
            num_key_value_heads: 8,
            head_dim: 36,
        }),
        "base-es" => Some(MoonshineFlavor {
            token_rate: 6,
            num_layers: 8,
            num_key_value_heads: 8,
            head_dim: 52,
        }),
        _ => None,
    }
}
