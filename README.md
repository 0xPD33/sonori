# Sonori

A lightweight, transparent overlay application that displays real-time transcriptions of your speech using multiple AI backends on Linux.

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

- **Multi-Backend Support**: Choose between CTranslate2, Whisper.cpp, and other transcription backends
- **GPU Acceleration**: Accelerate transcription using Vulkan (no CUDA yet and only works using the `whisper_cpp` backend)
- **Real-Time Transcription**: Transcribes your speech in real-time using configurable AI models
- **Manual Transcription Mode**: Accumulate audio in sessions for on-demand batch transcription (toggle with --manual or UI button)
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
- **Cloud API Support**: Integration with cloud providers (Deepgram, OpenAI) for higher accuracy and speed
- **Additional Backends**: Support for other specialized transcription models
- **CUDA Support**: Enhanced GPU acceleration across all backends

### NOT Planned

- **Using a GUI framework**: I want to learn more about wgpu and wgsl and think a GUI written from scratch is perfectly fine for this application
- **Support for Windows/macOS**: Not planned by me personally but if anyone wants to give it a shot feel free

## Requirements

**Platform:** Linux only (x86_64, aarch64)

### Dependencies

**Note:** Primarily tested on NixOS, but should work on other Linux distributions with proper dependencies installed. Feedback on other distros is welcome!

For Debian/Ubuntu-based distributions:

**Ubuntu 24.04+ (Noble and later):**
```bash
sudo apt install build-essential portaudio19-dev libclang-dev pkg-config wl-copy \
  libxkbcommon-dev libwayland-dev libx11-dev libxcursor-dev libxi-dev libxrandr-dev \
  libasound2-dev libssl-dev libfftw3-dev curl cmake libvulkan-dev \
  libopenblas-dev glslc
```

**Ubuntu 22.04 and earlier:**
Note: `glslc` is not available in standard repositories. You'll need to either:
- Upgrade to Ubuntu 24.04, or
- Download glslc from [LunarG Vulkan SDK](https://vulkan.lunarg.com/), or
- Build shaderc from [source](https://github.com/google/shaderc)

For Fedora/RHEL-based distributions:

```bash
sudo dnf install gcc gcc-c++ portaudio-devel clang-devel pkg-config wl-copy \
  libxkbcommon-devel wayland-devel libX11-devel libXcursor-devel libXi-devel libXrandr-devel \
  alsa-lib-devel openssl-devel fftw-devel curl cmake vulkan-loader-devel vulkan-headers \
  openblas-devel shaderc
```

For Arch-based distributions:

```bash
sudo pacman -S base-devel portaudio clang pkgconf wl-copy \
  libxkbcommon wayland libx11 libxcursor libxi libxrandr alsa-lib openssl fftw curl cmake \
  vulkan-headers vulkan-tools blas shaderc
```

For NixOS:

Simply use the provided flake.nix by running

```bash
nix develop
```

while in the root directory of the repository. The flake includes all necessary dependencies including vulkan-loader.

### Required Models

Sonori needs models to function properly, depending on the selected backend:

1. **Transcription Model** - Downloaded automatically based on backend selection:
   - **CTranslate2**: Hugging Face models converted to CTranslate2 format
   - **Whisper.cpp**: GGML format models from whisper.cpp repository
2. **Silero VAD Model** - Downloaded automatically on first run (shared across all backends)

   Note: If you need to download the Silero model manually for any reason, you can get it from:
   https://github.com/snakers4/silero-vad/
   And place it in `~/.cache/sonori/models/`

### Additional Requirements

- **ONNX Runtime**: Required for the Silero VAD model
- **CTranslate2**: Used for CTranslate2 backend inference
- **whisper-rs**: Used for Whisper.cpp backend inference
- **OpenBLAS**: Required for Whisper.cpp CPU optimization. For better performance on modern CPUs, ensure this is installed
- **CPAL**: Required for sound feedback system
- **Vulkan**: Required for WGPU rendering and optional GPU acceleration in Whisper.cpp. Your system must have:
  - Vulkan loader and headers
  - Shader compiler (shaderc) for Vulkan GPU compilation

## Installation

### NixOS (Recommended)

**Try without installing:**
```bash
nix run github:0xPD33/sonori
```

**Install to profile:**
```bash
nix profile install github:0xPD33/sonori
```

**Add to configuration.nix:**
```nix
{
  inputs.sonori.url = "github:0xPD33/sonori";

  # In your system configuration:
  environment.systemPackages = [ inputs.sonori.packages.${system}.default ];
}
```

### From Releases

1. Download the latest tarball from [GitHub Releases](https://github.com/0xPD33/sonori/releases)
2. Extract: `tar -xzf sonori-*.tar.gz`
3. Run: `./sonori-*/sonori`

### Building from Source

**Requirements:** Install Rust and Cargo from https://rustup.rs/

#### Arch/Manjaro
```bash
sudo pacman -S base-devel portaudio clang pkgconf wl-copy \
  libxkbcommon wayland libx11 libxcursor libxi libxrandr alsa-lib openssl fftw curl cmake \
  vulkan-headers vulkan-tools blas shaderc
```

#### Fedora/RHEL
```bash
sudo dnf install gcc gcc-c++ portaudio-devel clang-devel pkg-config wl-copy \
  libxkbcommon-devel wayland-devel libX11-devel libXcursor-devel libXi-devel libXrandr-devel \
  alsa-lib-devel openssl-devel fftw-devel curl cmake vulkan-loader-devel vulkan-headers \
  openblas-devel shaderc
```

#### Debian/Ubuntu

**Ubuntu 24.04+:**
```bash
sudo apt install build-essential portaudio19-dev libclang-dev pkg-config wl-copy \
  libxkbcommon-dev libwayland-dev libx11-dev libxcursor-dev libxi-dev libxrandr-dev \
  libasound2-dev libssl-dev libfftw3-dev curl cmake libvulkan-dev \
  libopenblas-dev glslc
```

**Ubuntu 22.04:** See note above about glslc availability

#### NixOS
```bash
nix develop
```

**Build:**
```bash
git clone https://github.com/0xPD33/sonori
cd sonori
cargo build --release
./target/release/sonori
```

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

- Ensure you have the Vulkan libraries installed for your distribution (see Dependencies section)
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

**Supported:**
- Linux x86_64 (64-bit Intel/AMD)
- Linux aarch64 (64-bit ARM)

**Tested on:**
- NixOS with KDE Plasma/KWin (Wayland)
- Other major Linux distributions should work with proper dependencies

**Not supported:**
- Windows
- macOS
- 32-bit architectures

**Note:** While primarily developed and tested on NixOS, Sonori should work on other Linux distributions with the proper dependencies installed. Feedback and testing on other distros is welcome!

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

### Contributing

Contributions are welcome and encouraged! Whether you're fixing bugs, adding features, improving documentation, or testing on different distributions, your help is appreciated.

**Getting Started:**
- Check out [ARCHITECTURE.md](./ARCHITECTURE.md) to understand the codebase structure
- Look at the planned features and known issues below for ideas
- Test your changes on your distribution (we aim to support NixOS and other major distros)
- Open an issue or PR - no formal guidelines yet, just make sure it works!

**Note:** The application is in active development. You may encounter bugs or instability as new features are added.
