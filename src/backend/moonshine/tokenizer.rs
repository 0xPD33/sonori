use crate::backend::traits::TranscriptionError;
use std::path::Path;
use tokenizers::Tokenizer;

const TOKENIZER_FILENAME: &str = "tokenizer.json";

const BOS_TOKEN_CANDIDATES: [&str; 6] = [
    "<s>",
    "<|startoftranscript|>",
    "<|startoftext|>",
    "<sos>",
    "<bos>",
    "[BOS]",
];

const EOS_TOKEN_CANDIDATES: [&str; 6] = [
    "</s>",
    "<|endoftext|>",
    "<|endoftranscript|>",
    "<eos>",
    "[EOS]",
    "<|eot|>",
];

pub struct MoonshineTokenizer {
    tokenizer: Tokenizer,
    bos_token_id: u32,
    eos_token_id: u32,
}

impl MoonshineTokenizer {
    pub fn from_dir(model_dir: impl AsRef<Path>) -> Result<Self, TranscriptionError> {
        let tokenizer_path = model_dir.as_ref().join(TOKENIZER_FILENAME);
        if !tokenizer_path.exists() {
            return Err(TranscriptionError::ModelNotAvailable(format!(
                "Moonshine tokenizer not found: {}",
                tokenizer_path.display()
            )));
        }

        let tokenizer = Tokenizer::from_file(&tokenizer_path).map_err(|e| {
            TranscriptionError::ModelNotAvailable(format!(
                "Failed to load Moonshine tokenizer: {}",
                e
            ))
        })?;

        let bos_token_id =
            resolve_special_token(&tokenizer, &BOS_TOKEN_CANDIDATES).ok_or_else(|| {
                TranscriptionError::ModelNotAvailable(
                    "Moonshine tokenizer missing BOS token".to_string(),
                )
            })?;

        let eos_token_id =
            resolve_special_token(&tokenizer, &EOS_TOKEN_CANDIDATES).ok_or_else(|| {
                TranscriptionError::ModelNotAvailable(
                    "Moonshine tokenizer missing EOS token".to_string(),
                )
            })?;

        Ok(Self {
            tokenizer,
            bos_token_id,
            eos_token_id,
        })
    }

    pub fn decode(&self, ids: &[u32]) -> Result<String, TranscriptionError> {
        self.tokenizer
            .decode(ids, true)
            .map_err(|e| TranscriptionError::InferenceError(e.to_string()))
    }

    pub fn bos_token_id(&self) -> u32 {
        self.bos_token_id
    }

    pub fn eos_token_id(&self) -> u32 {
        self.eos_token_id
    }
}

fn resolve_special_token(tokenizer: &Tokenizer, candidates: &[&str]) -> Option<u32> {
    for token in candidates {
        if let Some(id) = tokenizer.token_to_id(token) {
            return Some(id);
        }
    }
    None
}
