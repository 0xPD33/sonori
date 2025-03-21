use anyhow::{Context, Result};
use reqwest;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use tokio::io::AsyncWriteExt;

/// Default Whisper model to download if none specified
const DEFAULT_WHISPER_MODEL: &str = "openai/whisper-base.en";

/// URL for Silero VAD model
const SILERO_VAD_URL: &str =
    "https://github.com/snakers4/silero-vad/raw/master/src/silero_vad/data/silero_vad.onnx";

/// Default filename for the Silero VAD model
const SILERO_MODEL_FILENAME: &str = "silero_vad.onnx";

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
        output_dir.to_str().unwrap()
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
                .arg(model_conversion_shell_nix.to_str().unwrap())
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

/// Initialize all required models (Whisper and Silero)
pub async fn init_all_models(whisper_model_name: Option<&str>) -> Result<(PathBuf, PathBuf)> {
    // Initialize Silero VAD model
    let silero_model_path = init_silero_model().await?;

    // Initialize Whisper model
    let whisper_model_path = init_model(whisper_model_name).await?;

    Ok((whisper_model_path, silero_model_path))
}
