# Sonori

A lightweight, transparent overlay application for local AI-powered speech transcription on Linux. Choose between real-time or on-demand manual transcription modes.

> **Note:** The application is in active development. You may encounter bugs or instability as new features are added.

## Features

### Core
- **Local AI Processing** - All transcription happens on your device, no cloud services required
- **Multi-Backend Support** - Choose between CTranslate2 or Whisper.cpp backends
- **Dual Transcription Modes** - Real-time continuous transcription or manual on-demand sessions
- **Voice Activity Detection** - Uses Silero VAD for accurate speech detection
- **Automatic Model Download** - Models are downloaded automatically on first run

### Interface
- **Transparent Overlay** - Non-intrusive overlay at the bottom of your screen
- **CLI Mode** - Run without GUI using `--cli` flag for headless/terminal usage
- **Audio Visualization** - Spectrogram display shows audio input in real-time
- **System Tray Integration** - Quick access with window control and status display

### Optional Features
- **GPU Acceleration** - Vulkan-based acceleration (Whisper.cpp backend only)
- **Global Shortcuts** - System-wide hotkeys via XDG Desktop Portal (e.g., Super+\ to toggle recording)
- **Auto-Paste** - Automatic text injection via XDG Desktop Portal RemoteDesktop
- **Sound Feedback** - Audio cues for recording state changes

### Roadmap

**Planned:**
- Better error handling and UI improvements
- CUDA support for GPU acceleration
- Additional local AI backends
- Optional cloud API support (Deepgram, OpenAI)

**Not Planned:**
- GUI framework (custom wgpu/wgsl implementation by design)
- Windows/macOS support (contributions welcome)

## System Requirements

**Platform:** Linux x86_64 only

**Tested on:** NixOS with KDE Plasma/KWin (Wayland)

### Compositor (Wayland)

| Protocol | Required | Purpose |
|----------|----------|---------|
| `zwlr_layer_shell_v1` | **Yes** | Transparent overlay rendering |
| XDG Portal: GlobalShortcuts | No | System-wide hotkeys |
| XDG Portal: RemoteDesktop | No | Auto-paste via keyboard injection |

**Compositor Compatibility:**
| Compositor | Status |
|------------|--------|
| KDE Plasma (KWin) | ✅ Full support |
| Hyprland | ✅ Should work |
| Sway | ✅ Should work |
| GNOME (Mutter) | ❌ No layer shell (use CLI mode) |

### Hardware
- **GPU:** Vulkan-capable with appropriate drivers
- **Audio:** Working microphone, PipeWire or PulseAudio

## Installation

### AppImage (Recommended)

```bash
# Download from GitHub Releases
chmod +x Sonori-*-x86_64.AppImage
./Sonori-*-x86_64.AppImage
```

### Release Tarball

```bash
tar -xzf sonori-*-x86_64-linux.tar.gz
./sonori-*/sonori
```

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

**Prerequisites:** [Rust](https://rustup.rs/) and distribution-specific dependencies.

<details>
<summary><strong>Ubuntu/Debian 24.04+</strong></summary>

```bash
# Install system dependencies
sudo apt-get update
sudo apt-get install -y build-essential portaudio19-dev libclang-dev pkg-config \
  libxkbcommon-dev libwayland-dev libx11-dev libxcursor-dev libxi-dev libxrandr-dev \
  libasound2-dev libssl-dev libfftw3-dev curl cmake libvulkan-dev libopenblas-dev glslc

# Install ONNX Runtime (not in repos)
ONNX_VERSION=1.22.0
wget https://github.com/microsoft/onnxruntime/releases/download/v${ONNX_VERSION}/onnxruntime-linux-x64-${ONNX_VERSION}.tgz
tar -xzf onnxruntime-linux-x64-${ONNX_VERSION}.tgz
sudo cp -r onnxruntime-linux-x64-${ONNX_VERSION}/include/* /usr/local/include/
sudo cp -r onnxruntime-linux-x64-${ONNX_VERSION}/lib/* /usr/local/lib/
sudo mkdir -p /usr/local/lib64
sudo cp -r onnxruntime-linux-x64-${ONNX_VERSION}/lib/* /usr/local/lib64/
echo "/usr/local/lib" | sudo tee /etc/ld.so.conf.d/onnxruntime.conf
echo "/usr/local/lib64" | sudo tee -a /etc/ld.so.conf.d/onnxruntime.conf
sudo ldconfig
```

Set environment variables before building:
```bash
export BLAS_INCLUDE_DIRS=/usr/include/x86_64-linux-gnu
export OPENBLAS_PATH=/usr
export ORT_STRATEGY=system
export ORT_LIB_LOCATION=/usr/local/lib
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

Set environment variables before building:
```bash
export BLAS_INCLUDE_DIRS=/usr/include/openblas
export OPENBLAS_PATH=/usr
export ORT_STRATEGY=system
```
</details>

<details>
<summary><strong>Arch/Manjaro</strong></summary>

```bash
sudo pacman -S base-devel portaudio clang pkgconf \
  libxkbcommon wayland libx11 libxcursor libxi libxrandr alsa-lib openssl fftw curl cmake \
  vulkan-headers vulkan-tools openblas shaderc
# Install onnxruntime from AUR (e.g., yay -S onnxruntime)
```

Set environment variables before building:
```bash
export BLAS_INCLUDE_DIRS=/usr/include/openblas
export OPENBLAS_PATH=/usr
export ORT_STRATEGY=system
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
# Ensure environment variables are set (see distro-specific instructions above)
cargo build --release
./target/release/sonori
```

### Desktop Integration

**NixOS:** Automatic via Nix flake.

**Other distributions:**
```bash
./install-desktop.sh --user        # User installation (recommended)
sudo ./install-desktop.sh --system # System-wide installation
```

See [desktop/README.md](desktop/README.md) for details.

## Usage

### GUI Mode (Default)

```bash
sonori
```

1. A transparent overlay appears at the bottom of your screen
2. **Real-time mode:** Recording starts automatically
3. **Manual mode:** Press Record to start/stop sessions
4. Use overlay buttons to copy text, clear history, switch modes, or exit

### CLI Mode

```bash
sonori --cli
```

- Transcription appears directly in terminal
- Real-time mode: auto-starts recording
- Manual mode: use spacebar to start/stop
- `Ctrl+C` to exit

### Command Line Options

| Option | Description |
|--------|-------------|
| `--cli` | Run in CLI mode without GUI |
| `--mode <realtime\|manual>` | Set transcription mode (default: manual) |
| `--manual` | Shorthand for `--mode manual` |
| `--help` | Show help information |
| `--version` | Display version |

## Configuration

Sonori uses `config.toml` for configuration. Defaults work well for most users.

**Quick Setup:** Choose a preset from the [Configuration Guide](./CONFIGURATION.md):
- **Fast & Lightweight** - Good for older computers
- **Balanced Performance** - Recommended for most users
- **High Quality** - For powerful computers with GPU
- **Real-Time** - Live transcription as you speak
- **Multilingual** - For non-English languages

## Troubleshooting

### Wayland / Layer Shell

Sonori uses `zwlr_layer_shell_v1` for the transparent overlay.

- Verify Wayland session: `echo $XDG_SESSION_TYPE` should return `wayland`
- Check [Compositor Compatibility](#compositor-wayland) table above
- GNOME/Mutter doesn't support layer shell - use CLI mode (`--cli`)

### Vulkan / GPU

Required for UI rendering and optional GPU-accelerated transcription.

- Install Vulkan libraries: `vulkan-loader`, `vulkan-headers`
- Vendor-specific packages may be needed (e.g., `mesa-vulkan-drivers` on Ubuntu)
- Test with: `vulkaninfo` or `vkcube`
- For GPU transcription: enable `gpu_enabled = true` in `[backend_config]`

### XDG Desktop Portal Features

**Global Shortcuts** (`global_shortcuts_enabled`):
- Requires KDE Plasma 6+ or GNOME 45+
- Accept permission dialog on first run
- Check portal is running: `systemctl --user status xdg-desktop-portal`

**Auto-Paste** (`portal_input_enabled`):
- Uses RemoteDesktop portal for keyboard injection
- Some compositors require screencast permission as fallback
- Falls back to clipboard-only if declined

### Model Issues

**Automatic conversion fails:**
```bash
# NixOS
nix-shell model-conversion/shell.nix
ct2-transformers-converter --model your-model --output_dir ~/.cache/whisper/your-model --copy_files preprocessor_config.json tokenizer.json

# Other distros
pip install -U ctranslate2 huggingface_hub torch transformers
ct2-transformers-converter --model your-model --output_dir ~/.cache/whisper/your-model --copy_files preprocessor_config.json tokenizer.json
```

**30-second truncation:** Whisper's 30-second window with 448 token limit can truncate dense speech. Solutions:
1. Keep recordings under 25 seconds
2. Adjust `chunk_duration_seconds` (15-25) in `[manual_mode_config]`
3. Try CTranslate2 backend

## Known Issues

- Not all Wayland compositors supported (tested primarily on KDE Plasma/KWin)
- Transcription accuracy depends on Whisper model quality
- CPU usage can be high when idle (buffer size related)

## Contributing

Contributions welcome! Whether fixing bugs, adding features, improving docs, or testing on different distributions.

**Getting Started:**
- See [ARCHITECTURE.md](./ARCHITECTURE.md) to understand the codebase
- Check planned features and known issues above
- Test on your distribution
- Open an issue or PR

## Credits

- [Rust](https://www.rust-lang.org/)
- [CTranslate2](https://github.com/OpenNMT/CTranslate2) / [Faster Whisper](https://github.com/SYSTRAN/faster-whisper)
- [whisper.cpp](https://github.com/ggerganov/whisper.cpp) / [whisper-rs](https://codeberg.org/tazz4843/whisper-rs)
- [ONNX Runtime](https://github.com/microsoft/onnxruntime)
- [OpenAI Whisper](https://github.com/openai/whisper)
- [Silero VAD](https://github.com/snakers4/silero-vad)
- [CPAL](https://github.com/RustAudio/cpal)
- [Winit Fork](https://github.com/SergioRibera/winit)
- [WGPU](https://github.com/gfx-rs/wgpu)

## License

MIT License - see [LICENSE](LICENSE) for details.
