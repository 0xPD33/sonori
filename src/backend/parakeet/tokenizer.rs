use crate::backend::traits::TranscriptionError;
use std::collections::HashMap;
use std::path::Path;

pub struct ParakeetTokenizer {
    id_to_token: HashMap<u32, String>,
    blank_id: u32,
    vocab_size: usize,
}

impl ParakeetTokenizer {
    pub fn from_dir(model_dir: impl AsRef<Path>) -> Result<Self, TranscriptionError> {
        let tokens_path = model_dir.as_ref().join("tokens.txt");
        if !tokens_path.exists() {
            return Err(TranscriptionError::ModelNotAvailable(format!(
                "Parakeet tokens.txt not found: {}",
                tokens_path.display()
            )));
        }

        let contents = std::fs::read_to_string(&tokens_path).map_err(|e| {
            TranscriptionError::IoError(format!(
                "Failed to read {}: {}",
                tokens_path.display(),
                e
            ))
        })?;

        let mut id_to_token = HashMap::new();
        let mut max_id: u32 = 0;

        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            // sherpa-onnx format: "token id" (space-separated)
            // Find the last space to split on (token may contain spaces)
            let last_space = line.rfind(' ').ok_or_else(|| {
                TranscriptionError::ModelNotAvailable(format!(
                    "Invalid tokens.txt line (no space): {}",
                    line
                ))
            })?;

            let token = &line[..last_space];
            let id_str = &line[last_space + 1..];
            let id: u32 = id_str.parse().map_err(|e| {
                TranscriptionError::ModelNotAvailable(format!(
                    "Invalid token ID '{}': {}",
                    id_str, e
                ))
            })?;

            if id > max_id {
                max_id = id;
            }

            id_to_token.insert(id, token.to_string());
        }

        let vocab_size = id_to_token.len();
        let blank_id = max_id; // blank is typically the last entry

        Ok(Self {
            id_to_token,
            blank_id,
            vocab_size,
        })
    }

    pub fn decode(&self, ids: &[u32]) -> String {
        let text: String = ids
            .iter()
            .filter_map(|&id| self.id_to_token.get(&id))
            .cloned()
            .collect::<Vec<_>>()
            .join("");

        // Replace SentencePiece marker with space
        let text = text.replace('\u{2581}', " ");
        text.trim().to_string()
    }

    pub fn blank_id(&self) -> u32 {
        self.blank_id
    }

    pub fn vocab_size(&self) -> usize {
        self.vocab_size
    }
}
