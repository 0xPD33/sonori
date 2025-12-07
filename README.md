# Sonori

A lightweight, transparent overlay application for local AI-powered speech transcription on Linux. Choose between real-time or on-demand manual transcription modes.

## Contributing

Contributions are welcome and encouraged! Whether you're fixing bugs, adding features, improving documentation, or testing on different distributions, your help is appreciated.

**Getting Started:**
- Check out [ARCHITECTURE.md](./ARCHITECTURE.md) to understand the codebase structure
- Look at the planned features and known issues below for ideas
- Test your changes on your distribution (we aim to support NixOS and other major distros)
- Open an issue or PR - no formal guidelines yet, just make sure it works!

**Note:** The application is in active development. You may encounter bugs or instability as new features are added.

## Features

### Current

- **Local AI Processing**: All transcription happens on your device - no cloud services required
- **Multi-Backend Support**: Choose between CTranslate2, Whisper.cpp, and other local AI backends
- **Dual Transcription Modes**: Real-time continuous transcription or manual on-demand sessions
- **GPU Acceleration**: Accelerate transcription using Vulkan (no CUDA yet and only works using the `whisper_cpp` backend)
- **Voice Activity Detection**: Uses Silero VAD for accurate speech detection
- **Transparent Overlay**: Non-intrusive overlay that sits at the bottom of your screen
- **Audio Visualization**: Visual feedback when speaking with a spectrogram display
- **Copy/Paste Functionality**: Easily copy transcribed text to clipboard
- **Pause/Resume Recording**: Pause/Resume recording (real-time mode) or Start/Stop sessions (manual mode)
- **Auto-Start Recording**: Begins recording automatically in real-time mode (manual mode requires manual start)
- **Scroll Controls**: Navigate through longer transcripts
- **CLI Mode**: Run without GUI in terminal mode using `--cli` flag for headless usage
- **Sound Feedback**: Optional audio cues for recording state changes
- **Configurable**: Configure the backend, model, language, transcription mode, and other settings in the config file (config.toml)
- **Automatic Model Download**: Models are downloaded automatically based on selected backend
- **Performance Monitoring**: Optional statistics logging for transcription performance analysis
- **Global Shortcuts**: Optional XDG Desktop Portal integration for system-wide hotkeys (e.g., Super+backslash to toggle manual sessions)
- **Portal Input**: Optional automatic pasting via XDG Desktop Portal for seamless text injection
- **System Tray Integration**: Quick access via system tray with window control and status display
- **Display Configuration**: VSync and frame rate control for optimized rendering
- **Window Behavior Control**: Auto-hide, window positioning, and system tray integration options

### Planned

- **Better error handling**: Handle errors gracefully and provide useful error messages
- **Better UI**: A better UI with a focus on more usability
- **Additional Local AI Backends**: Support for other specialized local transcription models
- **CUDA Support**: Enhanced GPU acceleration across all backends
- **Cloud API Support**: Optional integration with cloud providers (Deepgram, OpenAI) for users who prefer cloud processing

### NOT Planned

- **Using a GUI framework**: I want to learn more about wgpu and wgsl and think a GUI written from scratch is perfectly fine for this application
- **Support for Windows/macOS**: Not planned by me personally but if anyone wants to give it a shot feel free

## Installation

**Platform:** Linux only (x86_64)

### AppImage (Recommended for most users)

The easiest way to run Sonori - no installation required:

1. Download `Sonori-*-x86_64.AppImage` from [GitHub Releases](https://github.com/0xPD33/sonori/releases)
2. Make executable: `chmod +x Sonori-*-x86_64.AppImage`
3. Run: `./Sonori-*-x86_64.AppImage`

### Release Tarball

Pre-built binary with bundled libraries:

1. Download `sonori-*-x86_64-linux.tar.gz` from [GitHub Releases](https://github.com/0xPD33/sonori/releases)
2. Extract: `tar -xzf sonori-*-x86_64-linux.tar.gz`
3. Run: `./sonori-*/sonori`

### NixOS

```bash
# Try without installing
nix run github:0xPD33/sonori

# Install to profile
nix profile install github:0xPD33/sonori
```

Or add to your flake:
```nix
{
  inputs.sonori.url = "github:0xPD33/sonori";
  # Then add: inputs.sonori.packages.${system}.default
}
```

### Building from Source

For developers or if you need to customize the build.

**Prerequisites:** [Rust](https://rustup.rs/) and distribution-specific dependencies.

<details>
<summary><strong>Ubuntu/Debian 24.04+</strong></summary>

```bash
sudo apt install build-essential portaudio19-dev libclang-dev pkg-config \
  libxkbcommon-dev libwayland-dev libx11-dev libxcursor-dev libxi-dev libxrandr-dev \
  libasound2-dev libssl-dev libfftw3-dev curl cmake libvulkan-dev libopenblas-dev glslc

# ONNX Runtime (not in repos)
ONNX_VERSION=1.22.0
wget https://github.com/microsoft/onnxruntime/releases/download/v${ONNX_VERSION}/onnxruntime-linux-x64-${ONNX_VERSION}.tgz
tar -xzf onnxruntime-linux-x64-${ONNX_VERSION}.tgz
sudo cp -r onnxruntime-linux-x64-${ONNX_VERSION}/include/* /usr/local/include/
sudo cp -r onnxruntime-linux-x64-${ONNX_VERSION}/lib/* /usr/local/lib/
sudo ldconfig
```
</details>

<details>
<summary><strong>Fedora/RHEL</strong></summary>

```bash
sudo dnf install gcc gcc-c++ portaudio-devel clang-devel pkg-config \
  libxkbcommon-devel wayland-devel libX11-devel libXcursor-devel libXi-devel libXrandr-devel \
  alsa-lib-devel openssl-devel fftw-devel curl cmake vulkan-loader-devel vulkan-headers \
  openblas-devel shaderc onnxruntime-devel
```
</details>

<details>
<summary><strong>Arch/Manjaro</strong></summary>

```bash
sudo pacman -S base-devel portaudio clang pkgconf \
  libxkbcommon wayland libx11 libxcursor libxi libxrandr alsa-lib openssl fftw curl cmake \
  vulkan-headers vulkan-tools openblas shaderc
# Install onnxruntime from AUR
```
</details>

<details>
<summary><strong>NixOS</strong></summary>

```bash
nix develop  # All dependencies included
```
</details>

**Build:**
```bash
git clone https://github.com/0xPD33/sonori
cd sonori
cargo build --release
./target/release/sonori
```

### Models

Sonori downloads required models automatically on first run:
- **Transcription model** - Based on your backend choice (CTranslate2 or Whisper.cpp)
- **Silero VAD model** - For voice activity detection

### Desktop Integration

To integrate Sonori with your application menu and system:

**For NixOS:** Desktop integration is automatic via the Nix flake.

**For other distributions:**
```bash
# User installation (recommended)
./install-desktop.sh --user

# System-wide installation (requires root)
sudo ./install-desktop.sh --system
```

This installs:
- Application menu entry (.desktop file)
- AppStream metadata for software centers
- Application icon

See [desktop/README.md](desktop/README.md) for detailed instructions and manual installation steps.

## Usage

### GUI Mode (Default)

1. Launch the application:
   ```bash
   ./target/release/sonori
   ```
2. A transparent overlay will appear at the bottom of your screen
3. In real-time mode, recording starts automatically; in manual mode, press Record to start sessions
4. Speak naturally - your speech will be transcribed in real-time or near real-time (based on the model and hardware)
5. Use the buttons on the overlay to:
   - Pause/Resume recording (real-time mode)
   - Start/Stop manual sessions and Accept transcript (manual mode)
   - Copy text to clipboard
   - Clear transcript history
   - Toggle between real-time and manual modes
   - Exit the application

For manual mode, start a session with the Record button, speak, then stop and accept to transcribe the accumulated audio.

### CLI Mode

For headless usage or terminal-based transcription:

1. Launch in CLI mode:
   ```bash
   ./target/release/sonori --cli
   ```
2. Transcription will appear directly in your terminal
3. In real-time mode, recording starts automatically; in manual mode, use spacebar to start/stop sessions
4. Speak naturally - transcriptions will update in real-time on the same line (real-time mode) or after session acceptance (manual mode)
5. Press `Ctrl+C` to exit gracefully

### Command Line Options

- `--cli`: Run in CLI mode without GUI
- `--mode <realtime|manual>`: Set transcription mode (default: manual)
- `--manual`: Shorthand for `--mode manual` to start in manual transcription mode
- `--help`: Show help information
- `--version`: Display version information

## Configuration

Sonori uses a `config.toml` file for configuration. The defaults work well for most users - you typically only need to change 2-3 settings.

**Quick Setup**: Most users just need to choose a configuration from the [Configuration Guide](./CONFIGURATION.md) and copy it to `config.toml`.

### Common Choices:
- **Fast & Lightweight**: Good for older computers
- **Balanced Performance**: Recommended for most users
- **High Quality**: For powerful computers with GPU
- **Real-Time**: Live transcription as you speak
- **Multilingual**: For non-English languages

See the [complete configuration guide](./CONFIGURATION.md) for all examples and advanced settings.

## Known Issues

- The application might not work with all Wayland compositors (I only tested it with KDE Plasma and KWin).
- The transcriptions are not 100% accurate and might contain errors. This is closely related to the whisper model that is used.
- **30-second transcription truncation**: Recordings exactly 30 seconds long may get truncated. This is a known architectural limitation of Whisper models, not a bug. Whisper uses 30-second processing windows with a 448 token limit - dense speech can exhaust this limit before the full 30 seconds are transcribed. See Troubleshooting section for solutions.
- The CPU usage is too high, even when idle. This might be related to bad code on my side or some overhead of the models. I already identified that changing the buffer size will help (or make it worse).

## Troubleshooting

### Wayland Support

Sonori uses layer shell protocol for Wayland compositors. If you experience issues:

- Make sure you are in a wayland session and your compositor supports the layer shell protocol

### Vulkan Support

Sonori uses WGPU for rendering and has the ability to accelerate transcription using the GPU, which requires Vulkan support. If you encounter errors related to adapter detection or Vulkan:

- Ensure you have Vulkan libraries installed (`vulkan-loader`, `vulkan-headers`)
- Verify that your GPU supports Vulkan and that drivers are properly installed
- On some systems, you may need to install additional vendor-specific Vulkan packages (e.g., `mesa-vulkan-drivers` on Ubuntu/Debian)
- You can test Vulkan support by running `vulkaninfo` or `vkcube` if available on your system

### GPU Acceleration (Whisper.cpp Backend)

If GPU acceleration is enabled but not working:

- Ensure `gpu_enabled = true` in `[backend_config]` section
- Verify that your system has Vulkan support (see Vulkan Support section above)
- Check that shaderc is properly installed (required for shader compilation)
- For NVIDIA GPUs: ensure CUDA drivers are installed and up-to-date
- For AMD/Intel: ensure appropriate Vulkan drivers are installed
- If compilation fails with shader errors, try disabling GPU acceleration and using CPU mode instead
- Monitor GPU usage with `nvidia-smi` (NVIDIA) or `rocm-smi` (AMD) while transcribing

### Model Conversion Issues

If you encounter issues with automatic model conversion:

For NixOS:

```bash
nix-shell model-conversion/shell.nix
ct2-transformers-converter --model your-model --output_dir ~/.cache/whisper/your-model --copy_files preprocessor_config.json tokenizer.json
```

For other distributions:

```bash
pip install -U ctranslate2 huggingface_hub torch transformers
ct2-transformers-converter --model your-model --output_dir ~/.cache/whisper/your-model --copy_files preprocessor_config.json tokenizer.json
```

### 30-Second Transcription Truncation

If you experience transcription cutoffs with recordings exactly 30 seconds long, this is due to Whisper's architectural limitations:

**Root Cause**: Whisper models process audio in 30-second windows with a 448 token limit. Dense speech can exhaust this limit before the full 30 seconds are transcribed.

**Solutions**:

1. **Keep recordings under 30 seconds** (simplest): For manual mode, try to keep your recordings around 25 seconds or less to avoid this boundary entirely.

2. **Adjust chunk settings** (recommended):
```toml
[manual_mode_config]
chunk_duration_seconds = 20.0    # Experiment with values between 15-25
chunk_overlap_seconds = 2.0      # Overlap helps prevent word cutoff
```

3. **Switch to CTranslate2 backend**:
```toml
[backend_config]
backend = "ctranslate2"
```

Try different `chunk_duration_seconds` values to find what works best for your speech patterns and content density.

## Platform Support

**Supported:** Linux x86_64 (64-bit Intel/AMD)

**Tested on:** NixOS with KDE Plasma/KWin (Wayland). Other distributions should work - feedback welcome!

**Not supported:** Windows, macOS, 32-bit architectures, ARM (aarch64 support planned)

## Credits

- [Rust](https://www.rust-lang.org/)
- [CTranslate2](https://github.com/OpenNMT/CTranslate2) and [Faster Whisper](https://github.com/SYSTRAN/faster-whisper)
- [whisper.cpp](https://github.com/ggerganov/whisper.cpp) and [whisper-rs](https://codeberg.org/tazz4843/whisper-rs)
- [Onnx Runtime](https://github.com/microsoft/onnxruntime)
- [OpenAI Whisper](https://github.com/openai/whisper)
- [Silero VAD](https://github.com/snakers4/silero-vad)
- [CPAL](https://github.com/RustAudio/cpal)
- [Winit Fork](https://github.com/SergioRibera/winit)
- [WGPU](https://github.com/gfx-rs/wgpu)

## License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.
