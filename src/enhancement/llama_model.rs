use super::EnhancementError;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel, Special};
use llama_cpp_2::sampling::LlamaSampler;
use std::num::NonZeroU32;
use std::path::Path;

const DEFAULT_MAX_TOKENS: usize = 64;
const DEFAULT_CONTEXT_SIZE: u32 = 2048;

/// System prompt for transforming transcriptions into clear prompts
const SYSTEM_PROMPT: &str = r#"You are a helpful assistant that transforms raw speech transcriptions into clear, well-structured text.
Fix grammar, remove filler words (um, uh, like), and preserve the original intent.
Output ONLY the improved text, nothing else."#;

pub struct LlamaCppModel {
    backend: LlamaBackend,
    model: LlamaModel,
    max_tokens: usize,
    context_size: u32,
}

// Safety: LlamaBackend and LlamaModel are thread-safe once initialized
unsafe impl Send for LlamaCppModel {}
unsafe impl Sync for LlamaCppModel {}

impl LlamaCppModel {
    pub fn from_file(model_path: impl AsRef<Path>) -> Result<Self, EnhancementError> {
        Self::from_file_with_options(model_path, DEFAULT_MAX_TOKENS, DEFAULT_CONTEXT_SIZE)
    }

    pub fn from_file_with_options(
        model_path: impl AsRef<Path>,
        max_tokens: usize,
        context_size: u32,
    ) -> Result<Self, EnhancementError> {
        let path = model_path.as_ref();
        if !path.exists() {
            return Err(EnhancementError::ModelNotAvailable(format!(
                "GGUF model not found: {}",
                path.display()
            )));
        }

        // Initialize backend
        let backend = LlamaBackend::init().map_err(|e| {
            EnhancementError::InferenceError(format!("Failed to initialize llama backend: {:?}", e))
        })?;

        // Configure model loading parameters (CPU only)
        let model_params = LlamaModelParams::default();

        let model = LlamaModel::load_from_file(&backend, path, &model_params).map_err(|e| {
            EnhancementError::InferenceError(format!("Failed to load GGUF model: {:?}", e))
        })?;

        println!("Loaded GGUF model from {} (CPU mode)", path.display());

        Ok(Self {
            backend,
            model,
            max_tokens,
            context_size,
        })
    }

    /// Check if a GGUF model file exists at the given path
    pub fn is_available(model_path: impl AsRef<Path>) -> bool {
        let path = model_path.as_ref();
        path.exists() && path.extension().map_or(false, |ext| ext == "gguf")
    }

    /// Enhance a transcription by transforming it into a clear prompt
    pub fn enhance(
        &self,
        transcription: &str,
        system_prompt: Option<&str>,
    ) -> Result<String, EnhancementError> {
        use std::time::Instant;

        let sys_prompt = system_prompt.unwrap_or(SYSTEM_PROMPT);

        // Build the chat prompt using ChatML format
        let prompt = format!(
            "<|im_start|>system\n{}<|im_end|>\n<|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant\n",
            sys_prompt, transcription
        );

        // Create context for this generation
        let ctx_params =
            LlamaContextParams::default().with_n_ctx(NonZeroU32::new(self.context_size));

        let mut ctx = self
            .model
            .new_context(&self.backend, ctx_params)
            .map_err(|e| {
                EnhancementError::InferenceError(format!("Failed to create context: {:?}", e))
            })?;

        // Tokenize the prompt
        let start = Instant::now();
        let tokens = self
            .model
            .str_to_token(&prompt, AddBos::Always)
            .map_err(|e| {
                EnhancementError::InferenceError(format!("Failed to tokenize: {:?}", e))
            })?;

        // Create batch and add tokens
        let mut batch = LlamaBatch::new(self.context_size as usize, 1);
        let last_index = tokens.len() - 1;
        for (i, token) in tokens.iter().enumerate() {
            batch
                .add(*token, i as i32, &[0], i == last_index)
                .map_err(|e| {
                    EnhancementError::InferenceError(format!(
                        "Failed to add token to batch: {:?}",
                        e
                    ))
                })?;
        }

        // Process the prompt (prefill)
        ctx.decode(&mut batch).map_err(|e| {
            EnhancementError::InferenceError(format!("Failed to decode prompt: {:?}", e))
        })?;

        println!(
            "[LlamaCpp] Prefill ({} tokens): {:?}",
            tokens.len(),
            start.elapsed()
        );

        // Set up sampler for generation
        let mut sampler = LlamaSampler::chain_simple([
            LlamaSampler::temp(0.7),
            LlamaSampler::top_p(0.9, 1),
            LlamaSampler::dist(42),
        ]);

        // Generate completion
        let decode_start = Instant::now();
        let mut output_tokens = Vec::new();
        let mut n_cur = tokens.len() as i32;

        for _ in 0..self.max_tokens {
            // Sample next token
            let token = sampler.sample(&ctx, batch.n_tokens() - 1);
            sampler.accept(token);

            // Check for end of generation
            if self.model.is_eog_token(token) {
                break;
            }

            // Decode token to string to check for stop sequences
            let token_str = self
                .model
                .token_to_str(token, Special::Tokenize)
                .unwrap_or_default();

            if token_str.contains("<|im_end|>") || token_str.contains("<|endoftext|>") {
                break;
            }

            output_tokens.push(token);

            // Prepare next batch
            batch.clear();
            batch.add(token, n_cur, &[0], true).map_err(|e| {
                EnhancementError::InferenceError(format!("Failed to add token: {:?}", e))
            })?;
            n_cur += 1;

            // Decode
            ctx.decode(&mut batch).map_err(|e| {
                EnhancementError::InferenceError(format!("Failed to decode: {:?}", e))
            })?;
        }

        // Convert tokens to string
        let mut result = String::new();
        for token in &output_tokens {
            if let Ok(s) = self.model.token_to_str(*token, Special::Tokenize) {
                result.push_str(&s);
            }
        }

        let decode_elapsed = decode_start.elapsed();
        let token_count = output_tokens.len();
        let tokens_per_sec = if decode_elapsed.as_secs_f64() > 0.0 {
            token_count as f64 / decode_elapsed.as_secs_f64()
        } else {
            0.0
        };

        println!(
            "[LlamaCpp] Decode ({} tokens): {:?} ({:.1} tok/s)",
            token_count, decode_elapsed, tokens_per_sec
        );

        Ok(result.trim().to_string())
    }
}
