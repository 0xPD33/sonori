use crate::backend::onnx_utils::{load_session, OnnxSessionOptions};
use anyhow::Result;
use ort::session::Session;
use parking_lot::Mutex;
use std::path::Path;

use super::mel::MelSpectrogram;

pub struct ParakeetModel {
    pub encoder: Mutex<Session>,
    pub decoder: Mutex<Session>,
    pub joiner: Mutex<Session>,
    pub mel: MelSpectrogram,
}

impl ParakeetModel {
    pub fn load(model_dir: impl AsRef<Path>, options: &OnnxSessionOptions) -> Result<Self> {
        let model_dir = model_dir.as_ref();

        let encoder_path =
            resolve_model_file(model_dir, "encoder", &["encoder.int8.onnx", "encoder.onnx"])?;
        let decoder_path =
            resolve_model_file(model_dir, "decoder", &["decoder.int8.onnx", "decoder.onnx"])?;
        let joiner_path =
            resolve_model_file(model_dir, "joiner", &["joiner.int8.onnx", "joiner.onnx"])?;

        println!(
            "Loading Parakeet encoder from: {}",
            encoder_path.display()
        );
        let encoder = load_session(&encoder_path, options)?;
        println!(
            "Loading Parakeet decoder from: {}",
            decoder_path.display()
        );
        let decoder = load_session(&decoder_path, options)?;
        println!("Loading Parakeet joiner from: {}", joiner_path.display());
        let joiner = load_session(&joiner_path, options)?;

        let mel = MelSpectrogram::new();

        Ok(Self {
            encoder: Mutex::new(encoder),
            decoder: Mutex::new(decoder),
            joiner: Mutex::new(joiner),
            mel,
        })
    }

    pub fn validate_model_dir(model_dir: impl AsRef<Path>) -> Result<()> {
        let model_dir = model_dir.as_ref();

        let has_encoder =
            model_dir.join("encoder.int8.onnx").exists() || model_dir.join("encoder.onnx").exists();
        if !has_encoder {
            return Err(anyhow::anyhow!(
                "Missing Parakeet encoder model (encoder.int8.onnx or encoder.onnx)"
            ));
        }

        let has_decoder =
            model_dir.join("decoder.int8.onnx").exists() || model_dir.join("decoder.onnx").exists();
        if !has_decoder {
            return Err(anyhow::anyhow!(
                "Missing Parakeet decoder model (decoder.int8.onnx or decoder.onnx)"
            ));
        }

        let has_joiner =
            model_dir.join("joiner.int8.onnx").exists() || model_dir.join("joiner.onnx").exists();
        if !has_joiner {
            return Err(anyhow::anyhow!(
                "Missing Parakeet joiner model (joiner.int8.onnx or joiner.onnx)"
            ));
        }

        if !model_dir.join("tokens.txt").exists() {
            return Err(anyhow::anyhow!("Missing Parakeet tokens.txt"));
        }

        Ok(())
    }
}

fn resolve_model_file(
    model_dir: &Path,
    component: &str,
    candidates: &[&str],
) -> Result<std::path::PathBuf> {
    for candidate in candidates {
        let path = model_dir.join(candidate);
        if path.exists() {
            return Ok(path);
        }
    }
    Err(anyhow::anyhow!(
        "Missing Parakeet {} model. Tried: {}",
        component,
        candidates.join(", ")
    ))
}
