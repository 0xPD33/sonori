use anyhow::{Context, Result};
use reqwest;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use tokio::io::AsyncWriteExt;

use crate::backend::QuantizationLevel;

/// Default Whisper model to download if none specified
const DEFAULT_WHISPER_MODEL: &str = "openai/whisper-base.en";

/// URL for Silero VAD model
const SILERO_VAD_URL: &str =
    "https://github.com/snakers4/silero-vad/raw/master/src/silero_vad/data/silero_vad.onnx";

/// Default filename for the Silero VAD model
const SILERO_MODEL_FILENAME: &str = "silero_vad.onnx";

const MOONSHINE_REPO: &str = "UsefulSensors/moonshine";
const MOONSHINE_ONNX_DIR: &str = "onnx";
const MOONSHINE_MERGED_DIR: &str = "merged";
const MOONSHINE_MERGED_VARIANT: &str = "float";
const MOONSHINE_REQUIRED_FILES: [&str; 4] = [
    "encoder_model.onnx",
    "decoder_model_merged.onnx",
    "tokenizer.json",
    "preprocessor_config.json",
];
const MOONSHINE_OPTIONAL_FILES: [&str; 0] = [];

/// Enum to represent different model types
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ModelType {
    Whisper,
    Silero,
}

/// Common file names that need to be present in converted CT2 models
const REQUIRED_FILES: [&str; 4] = [
    "model.bin",
    "config.json",
    "tokenizer.json",
    "preprocessor_config.json",
];

/// Get the models directory path
fn get_models_dir() -> Result<PathBuf> {
    let home_dir = std::env::var("HOME").context("Failed to get HOME directory")?;
    let models_dir = PathBuf::from(format!("{}/.cache/sonori/models", home_dir));

    // Create models directory if it doesn't exist
    if !models_dir.exists() {
        println!("Creating models directory: {:?}", models_dir);
        fs::create_dir_all(&models_dir).context("Failed to create models directory")?;
    }

    Ok(models_dir)
}

/// Detect if running on NixOS
fn is_nixos() -> bool {
    // Check for /etc/nixos directory which is specific to NixOS
    Path::new("/etc/nixos").exists() || 
    // Check for NIX_PATH environment variable as a fallback
    std::env::var("NIX_PATH").is_ok()
}

/// Check if we're in a nix-shell
fn in_nix_shell() -> bool {
    std::env::var("IN_NIX_SHELL").is_ok()
}

/// Checks if all required model files are present
fn is_model_complete(model_dir: &Path) -> Result<bool> {
    println!(
        "Checking if model is complete in directory: {:?}",
        model_dir
    );

    for file in REQUIRED_FILES.iter() {
        let file_path = model_dir.join(file);
        println!("  Checking for file: {:?}", file_path);
        if !file_path.exists() {
            println!("  Missing file: {:?}", file_path);
            return Ok(false);
        }
    }

    println!("All required files are present");
    Ok(true)
}

fn is_moonshine_model_complete(model_dir: &Path) -> Result<bool> {
    for file in MOONSHINE_REQUIRED_FILES.iter() {
        if !model_dir.join(file).exists() {
            return Ok(false);
        }
    }
    Ok(true)
}

fn moonshine_model_id(model_name: &str) -> String {
    let simple = model_name.split('/').last().unwrap_or(model_name);
    simple
        .strip_prefix("moonshine-")
        .unwrap_or(simple)
        .to_string()
}

fn moonshine_model_dir(models_dir: &Path, model_id: &str) -> PathBuf {
    models_dir.join(format!("moonshine-{}-onnx", model_id))
}

/// Checks if Silero model file exists and is valid
fn is_silero_model_valid(model_path: &Path) -> bool {
    if !model_path.exists() {
        return false;
    }

    // Check file size is reasonable (should be > 10KB)
    match fs::metadata(model_path) {
        Ok(metadata) => metadata.len() > 10_000, // Ensuring it's not an empty or corrupted file
        Err(_) => false,
    }
}

/// Convert the model using ct2-transformers-converter
fn convert_model(model_name: &str, output_dir: &Path) -> Result<()> {
    println!(
        "Converting model {} to {}",
        model_name,
        output_dir.display()
    );

    // Create output directory if it doesn't exist
    if !output_dir.exists() {
        fs::create_dir_all(output_dir)?;
    }

    // Detect system type
    let on_nixos = is_nixos();
    let in_shell = in_nix_shell();

    println!(
        "System detection: NixOS={}, In nix-shell={}",
        on_nixos, in_shell
    );

    // Prepare the conversion command
    let conversion_script = format!(
        "ct2-transformers-converter --force --model {} --output_dir {} --copy_files preprocessor_config.json tokenizer.json",
        model_name,
        output_dir.to_str()
            .ok_or_else(|| anyhow::anyhow!("Model output path contains invalid UTF-8: {:?}", output_dir))?
    );

    let status = if on_nixos {
        // On NixOS but not in a shell, try to use the provided shell.nix in model-conversion directory
        println!("On NixOS: Using model-conversion/shell.nix");

        // Get the repository root directory to find model-conversion/shell.nix
        let current_dir = std::env::current_dir()?;
        let model_conversion_shell_nix = current_dir.join("model-conversion/shell.nix");

        if model_conversion_shell_nix.exists() {
            println!("Found shell.nix at {:?}", model_conversion_shell_nix);
            Command::new("nix-shell")
                .arg(model_conversion_shell_nix.to_str().ok_or_else(|| {
                    anyhow::anyhow!(
                        "shell.nix path contains invalid UTF-8: {:?}",
                        model_conversion_shell_nix
                    )
                })?)
                .arg("--command")
                .arg(&conversion_script)
                .status()
        } else {
            println!(
                "shell.nix not found at {:?}, trying default nix-shell",
                model_conversion_shell_nix
            );
            Command::new("nix-shell")
                .arg("--command")
                .arg(&conversion_script)
                .status()
        }
    } else {
        // Not on NixOS, run directly
        println!("Not on NixOS: Running conversion directly");
        Command::new("sh")
            .arg("-c")
            .arg(&conversion_script)
            .status()
    }
    .context("Failed to run conversion command")?;

    if !status.success() {
        return Err(anyhow::anyhow!(
            "Model conversion failed with status: {}",
            status
        ));
    }

    println!("Model conversion completed successfully");
    Ok(())
}

/// Download a file from a URL and save it to the specified path
pub async fn download_file(url: &str, output_path: &Path) -> Result<()> {
    println!("Downloading file from: {}", url);

    // Create parent directories if they don't exist
    if let Some(parent) = output_path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    }

    // Create a temporary file to download to
    let temp_path = output_path.with_extension("downloading");

    // Perform the download
    let response = reqwest::get(url)
        .await
        .context(format!("Failed to download file from {}", url))?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Failed to download file, status: {}",
            response.status()
        ));
    }

    let total_size = response.content_length().unwrap_or(0);
    let mut file = tokio::fs::File::create(&temp_path)
        .await
        .context(format!("Failed to create file at {:?}", temp_path))?;

    let mut stream = response.bytes_stream();
    let mut downloaded: u64 = 0;

    use futures_util::StreamExt;
    while let Some(item) = stream.next().await {
        let chunk = item.context("Error while downloading file")?;
        file.write_all(&chunk).await?;

        downloaded += chunk.len() as u64;
        if total_size > 0 {
            let progress = (downloaded as f64 / total_size as f64) * 100.0;
            print!(
                "\rDownloading... {:.1}% ({}/{} bytes)",
                progress, downloaded, total_size
            );
            io::stdout().flush()?;
        }
    }

    if total_size > 0 {
        println!(
            "\rDownload complete: {}/{} bytes (100%)    ",
            downloaded, total_size
        );
    } else {
        println!("\rDownload complete: {} bytes", downloaded);
    }

    // Close the file before renaming
    drop(file);

    // Move the downloaded file to the final location
    fs::rename(&temp_path, output_path).context(format!(
        "Failed to rename downloaded file from {:?} to {:?}",
        temp_path, output_path
    ))?;

    Ok(())
}

pub async fn download_file_optional(url: &str, output_path: &Path) -> Result<bool> {
    println!("Downloading file from: {}", url);

    if let Some(parent) = output_path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    }

    let response = reqwest::get(url)
        .await
        .context(format!("Failed to download file from {}", url))?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        println!("Optional file not found (404): {}", url);
        return Ok(false);
    }

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Failed to download file, status: {}",
            response.status()
        ));
    }

    let temp_path = output_path.with_extension("downloading");
    let total_size = response.content_length().unwrap_or(0);
    let mut file = tokio::fs::File::create(&temp_path)
        .await
        .context(format!("Failed to create file at {:?}", temp_path))?;

    let mut stream = response.bytes_stream();
    let mut downloaded: u64 = 0;

    use futures_util::StreamExt;
    while let Some(item) = stream.next().await {
        let chunk = item.context("Error while downloading file")?;
        file.write_all(&chunk).await?;

        downloaded += chunk.len() as u64;
        if total_size > 0 {
            let progress = (downloaded as f64 / total_size as f64) * 100.0;
            print!(
                "\rDownloading... {:.1}% ({}/{} bytes)",
                progress, downloaded, total_size
            );
            io::stdout().flush()?;
        }
    }

    file.flush().await?;
    tokio::fs::rename(&temp_path, output_path).await?;
    println!("\nDownloaded to {:?}", output_path);
    Ok(true)
}

/// Download and initialize the Silero VAD model
pub async fn init_silero_model() -> Result<PathBuf> {
    println!("Initializing Silero VAD model...");

    // Get models directory
    let models_dir = get_models_dir()?;
    let silero_model_path = models_dir.join(SILERO_MODEL_FILENAME);

    if is_silero_model_valid(&silero_model_path) {
        println!("Silero VAD model already exists at {:?}", silero_model_path);
        return Ok(silero_model_path);
    }

    println!("Downloading Silero VAD model from GitHub...");
    download_file(SILERO_VAD_URL, &silero_model_path).await?;

    // Verify the downloaded model
    if !is_silero_model_valid(&silero_model_path) {
        return Err(anyhow::anyhow!(
            "Downloaded Silero model is invalid or corrupted"
        ));
    }

    println!("Silero VAD model initialized at: {:?}", silero_model_path);
    Ok(silero_model_path)
}

/// Initialize a model, downloading and converting it if necessary
pub async fn init_model(model_name: Option<&str>) -> Result<PathBuf> {
    let model = model_name.unwrap_or(DEFAULT_WHISPER_MODEL);
    println!("Initializing Whisper model: {}", model);

    // Define paths
    let models_dir = get_models_dir()?;
    let model_name_simple = model.split('/').last().unwrap_or(model);
    let ct2_model_dir = models_dir.join(format!("{}-ct2", model_name_simple));

    // Check if converted model already exists
    if ct2_model_dir.exists() && is_model_complete(&ct2_model_dir)? {
        println!("Converted model already exists at {:?}", ct2_model_dir);
        return Ok(ct2_model_dir);
    }

    // Detect system type
    let on_nixos = is_nixos();
    println!("System detection: Running on NixOS = {}", on_nixos);

    // Try automatic conversion
    println!("Converting model {} to CTranslate2 format...", model);
    if let Err(e) = convert_model(model, &ct2_model_dir) {
        println!("Automatic conversion failed: {}", e);

        if on_nixos {
            println!("\nManual conversion instructions for NixOS:");
            println!("1. Enter the nix-shell with: nix-shell model-conversion/shell.nix");
            println!("2. Run the following command:");
        } else {
            println!("\nManual conversion instructions:");
            println!(
                "1. Install required packages: pip install -U ctranslate2 huggingface_hub torch transformers"
            );
            println!("2. Run the following command:");
        }

        println!(
            "   ct2-transformers-converter --model {} --output_dir {} --copy_files preprocessor_config.json tokenizer.json",
            model,
            ct2_model_dir.display()
        );
        println!("3. Then run this application again\n");

        return Err(anyhow::anyhow!(
            "Model conversion failed. Please follow the manual instructions."
        ));
    }

    // Verify the converted model
    if !is_model_complete(&ct2_model_dir)? {
        return Err(anyhow::anyhow!("Model conversion failed or is incomplete"));
    }

    println!("Model initialized at: {:?}", ct2_model_dir);
    Ok(ct2_model_dir)
}

/// Initialize a model of the specified type
pub async fn init_model_by_type(
    model_type: ModelType,
    model_name: Option<&str>,
) -> Result<PathBuf> {
    match model_type {
        ModelType::Whisper => init_model(model_name).await,
        ModelType::Silero => init_silero_model().await,
    }
}

/// Normalize model name based on backend type
///
/// This allows users to specify simple model names (e.g., "base.en", "small")
/// that work across different backends:
/// - CT2: Maps to distil-whisper models for better performance
/// - WhisperCpp: Uses standard OpenAI Whisper models
///
/// # Arguments
/// * `model_name` - User-specified model name
/// * `backend_type` - Which backend is being used
///
/// # Returns
/// Normalized model name appropriate for the backend
fn normalize_model_name(model_name: &str, backend_type: crate::backend::BackendType) -> String {
    // If model already has an organization prefix, use as-is
    if model_name.contains('/') {
        return model_name.to_string();
    }

    match backend_type {
        crate::backend::BackendType::CTranslate2 => {
            // Map simple names intelligently:
            // - Small models (tiny, base): Use standard OpenAI (already fast enough)
            // - Larger models (small+): Use distil-whisper (need speed optimization)
            let ct2_mapping = match model_name {
                "tiny" | "tiny.en" => "openai/whisper-tiny.en",
                "base" | "base.en" => "openai/whisper-base.en",
                "small" | "small.en" => "distil-whisper/distil-small.en",
                "medium" | "medium.en" => "distil-whisper/distil-medium.en",
                "large" | "large-v1" | "large-v2" | "large-v3" => "distil-whisper/distil-large-v3",
                // If it's already a full model name, use as-is
                other => other,
            };

            ct2_mapping.to_string()
        }
        crate::backend::BackendType::WhisperCpp => {
            // WhisperCpp uses standard OpenAI model names as-is
            // Just strip any "distil-" prefix if present
            model_name
                .strip_prefix("distil-")
                .unwrap_or(model_name)
                .to_string()
        }
        crate::backend::BackendType::Parakeet => {
            // Parakeet will use standard model names
            model_name.to_string()
        }
        crate::backend::BackendType::Moonshine => model_name.to_string(),
    }
}

/// Initialize all required models (Whisper and Silero)
///
/// # Arguments
/// * `whisper_model_name` - Name of the whisper model to use
/// * `backend_type` - Which backend to use (determines model format)
/// * `quantization` - Quantization level (for whisper.cpp models)
///
/// # Returns
/// Tuple of (whisper_model_path, silero_model_path)
pub async fn init_all_models(
    whisper_model_name: Option<&str>,
    backend_type: crate::backend::BackendType,
    quantization: &QuantizationLevel,
) -> Result<(PathBuf, PathBuf)> {
    // Initialize Silero VAD model
    let silero_model_path = init_silero_model().await?;

    // Normalize model name based on backend
    let original_model = whisper_model_name.unwrap_or("base.en");
    let normalized_model = normalize_model_name(original_model, backend_type);

    // Show model mapping if it changed
    if original_model != normalized_model {
        println!(
            "Model mapping for {} backend: '{}' → '{}'",
            backend_type, original_model, normalized_model
        );
    }

    // Initialize Whisper model based on backend type
    let whisper_model_path = match backend_type {
        crate::backend::BackendType::CTranslate2 => {
            // CT2: Download and convert model to CT2 format
            init_model(Some(&normalized_model)).await?
        }
        crate::backend::BackendType::WhisperCpp => {
            // WhisperCpp: Get expected GGML model path
            // The factory will handle auto-download if the file doesn't exist
            get_whisper_cpp_model_path(&normalized_model, quantization)?
        }
        crate::backend::BackendType::Parakeet => {
            return Err(anyhow::anyhow!("Parakeet backend not yet implemented"));
        }
        crate::backend::BackendType::Moonshine => {
            let model_id = moonshine_model_id(&normalized_model);
            init_moonshine_model(&model_id).await?
        }
    };

    Ok((whisper_model_path, silero_model_path))
}

async fn init_moonshine_model(model_id: &str) -> Result<PathBuf> {
    let models_dir = get_models_dir()?;
    let model_dir = moonshine_model_dir(&models_dir, model_id);

    if model_dir.exists() && is_moonshine_model_complete(&model_dir)? {
        println!("Moonshine model already exists at: {:?}", model_dir);
        return Ok(model_dir);
    }

    if !model_dir.exists() {
        fs::create_dir_all(&model_dir)?;
    }

    for file in MOONSHINE_REQUIRED_FILES.iter() {
        let output_path = model_dir.join(file);
        if output_path.exists() {
            continue;
        }

        let url = format!(
            "https://huggingface.co/{}/resolve/main/{}/{}/{}/{}/{}",
            MOONSHINE_REPO,
            MOONSHINE_ONNX_DIR,
            MOONSHINE_MERGED_DIR,
            model_id,
            MOONSHINE_MERGED_VARIANT,
            file
        );
        download_file(&url, &output_path).await?;
    }

    for file in MOONSHINE_OPTIONAL_FILES.iter() {
        let url = format!(
            "https://huggingface.co/{}/resolve/main/{}/{}/{}/{}/{}",
            MOONSHINE_REPO,
            MOONSHINE_ONNX_DIR,
            MOONSHINE_MERGED_DIR,
            model_id,
            MOONSHINE_MERGED_VARIANT,
            file
        );
        let output_path = model_dir.join(file);
        if output_path.exists() {
            continue;
        }
        let _ = download_file_optional(&url, &output_path).await?;
    }

    Ok(model_dir)
}

/// Download a whisper.cpp GGML model file
///
/// # Arguments
/// * `model_name` - Base model name (e.g., "base.en", "small", "medium")
/// * `quantization` - Quantization level to determine file variant
///
/// # Returns
/// Path to the downloaded model file
pub async fn download_whisper_cpp_model(
    model_name: &str,
    quantization: &QuantizationLevel,
) -> Result<PathBuf> {
    let models_dir = get_models_dir()?;

    // Determine quantization suffix for filename
    // Available for whisper.cpp: full precision (no suffix), q8_0, q5_1
    let quant_suffix = match quantization {
        QuantizationLevel::High => "", // Full precision (148MB for base.en)
        QuantizationLevel::Medium => "-q8_0", // Q8_0 quantization (82MB for base.en)
        QuantizationLevel::Low => "-q5_1", // Q5_1 quantization (60MB for base.en)
    };

    // Build filename: ggml-{model}{quant}.bin
    let filename = format!("ggml-{}{}.bin", model_name, quant_suffix);
    let output_path = models_dir.join(&filename);

    // Check if already exists
    if output_path.exists() {
        println!("whisper.cpp model already exists at: {:?}", output_path);
        return Ok(output_path);
    }

    // Download from HuggingFace ggerganov/whisper.cpp repository
    let url = format!(
        "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{}",
        filename
    );

    println!("Downloading whisper.cpp model: {}", filename);
    println!("  From: {}", url);
    println!("  To: {:?}", output_path);

    download_file(&url, &output_path).await?;

    println!("whisper.cpp model downloaded successfully!");
    Ok(output_path)
}

/// Get the expected path for a whisper.cpp model
///
/// # Arguments
/// * `model_name` - Base model name (e.g., "base.en", "small")
/// * `quantization` - Quantization level
///
/// # Returns
/// Expected path to the GGML model file
pub fn get_whisper_cpp_model_path(
    model_name: &str,
    quantization: &QuantizationLevel,
) -> Result<PathBuf> {
    let models_dir = get_models_dir()?;

    // Available for whisper.cpp: full precision (no suffix), q8_0, q5_1
    let quant_suffix = match quantization {
        QuantizationLevel::High => "",        // Full precision
        QuantizationLevel::Medium => "-q8_0", // Q8_0 quantization
        QuantizationLevel::Low => "-q5_1",    // Q5_1 quantization
    };

    let filename = format!("ggml-{}{}.bin", model_name, quant_suffix);
    Ok(models_dir.join(filename))
}

// =============================================================================
// Enhancement Model Download Functions
// =============================================================================
// GGUF models are downloaded from HuggingFace.
// Format: "owner/repo/filename.gguf"

/// Get the enhancement model directory
pub fn get_enhancement_model_dir() -> Result<PathBuf> {
    let models_dir = get_models_dir()?;
    Ok(models_dir.join("enhancement"))
}

/// Download a GGUF model from HuggingFace for enhancement
///
/// # Arguments
/// * `model` - Format: "owner/repo/filename.gguf"
///
/// # Returns
/// Path to the downloaded GGUF file
pub async fn download_enhancement_gguf(model: &str) -> Result<PathBuf> {
    let model_dir = get_enhancement_model_dir()?;

    if !model_dir.exists() {
        fs::create_dir_all(&model_dir)?;
    }

    // Parse model string: "owner/repo/filename.gguf"
    let parts: Vec<&str> = model.splitn(3, '/').collect();
    if parts.len() < 3 {
        return Err(anyhow::anyhow!(
            "Invalid model format. Expected: owner/repo/filename.gguf"
        ));
    }

    let repo = format!("{}/{}", parts[0], parts[1]);
    let filename = parts[2];
    let output_path = model_dir.join(filename);

    if output_path.exists() {
        println!(
            "Enhancement GGUF model already exists at: {:?}",
            output_path
        );
        return Ok(output_path);
    }

    // Build HuggingFace URL
    let url = format!("https://huggingface.co/{}/resolve/main/{}", repo, filename);

    println!("Downloading enhancement model: {}", filename);
    println!("  From: {}", url);
    println!("  To: {:?}", output_path);

    download_file(&url, &output_path).await?;

    println!("Enhancement model downloaded successfully!");
    Ok(output_path)
}

/// Get the expected path for an enhancement GGUF model (without downloading)
pub fn get_enhancement_gguf_path(model: &str) -> Result<PathBuf> {
    let model_dir = get_enhancement_model_dir()?;

    // Parse model string to get filename
    let filename = model.split('/').last().unwrap_or(model);
    Ok(model_dir.join(filename))
}

/// Check if an enhancement GGUF model exists
pub fn is_enhancement_gguf_available(model: &str) -> bool {
    match get_enhancement_gguf_path(model) {
        Ok(path) => path.exists(),
        Err(_) => false,
    }
}
