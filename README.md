# Sonori

A lightweight, transparent overlay application that displays real-time transcriptions of your speech using Whisper AI models on Linux.

The application is currently in very early development and might be unstable, buggy and/or crash.

Contributions are welcome. There are no guidelines yet. Just check the planned features, known issues and make sure your changes work on NixOS and other distros!

## Features

### Current

- **Real-Time Transcription**: Transcribes your speech in real-time using OpenAI's Whisper models
- **Voice Activity Detection**: Uses Silero VAD for accurate speech detection
- **Transparent Overlay**: Non-intrusive overlay that sits at the bottom of your screen
- **Audio Visualization**: Visual feedback when speaking with a spectrogram display
- **Copy/Paste Functionality**: Easily copy transcribed text to clipboard
- **Pause/Resume Recording**: Pause/Resume recording
- **Auto-Start Recording**: Begins recording as soon as the application launches
- **Scroll Controls**: Navigate through longer transcripts
- **CLI Mode**: Run without GUI in terminal mode using `--cli` flag for headless usage
- **Configurable**: Configure the model, language, and other settings like keyboard shortcuts in the config file (config.toml)
- **Automatic Model Download**: Both Whisper and Silero VAD models are downloaded automatically with automatic CTranslate2 conversion
- **Performance Monitoring**: Optional statistics logging for transcription performance analysis

### Planned

- **Better error handling**: Handle errors gracefully and provide useful error messages
- **Improve performance**: Lower CPU usage, lower latency, better multi-threaded code
- **Better UI**: A better UI with a focus on more usability
- **VSYNC**: Add VSYNC support for optionally reducing rendered frames
- **Input field detection**: Automatically detect input fields and transcribe text into them (might be a bit tricky to implement)
- **CUDA support**: Add support for CUDA to speed up inference on supported GPUs
- **Other backends**: I want to add other optional backends like Whisper.cpp or even an API (which would greatly increase speed/accuracy at the cost of some latency and maybe your privacy)

### NOT Planned

- **Using a GUI framework**: I want to learn more about wgpu and wgsl and think a GUI written from scratch is perfectly fine for this application
- **Support for Windows/macOS**: Not planned by me personally but if anyone wants to give it a shot feel free

## Requirements

### Dependencies

DISCLAIMER: Building from source, installing dependencies and running the application has only been tested on NixOS and I'm unsure if it will work on other distributions.

For Debian/Ubuntu-based distributions:

```bash
sudo apt install build-essential portaudio19-dev libclang-dev pkg-config wl-copy \
  libxkbcommon-dev libwayland-dev libx11-dev libxcursor-dev libxi-dev libxrandr-dev \
  libasound2-dev libssl-dev libfftw3-dev curl cmake libvulkan-dev
```

For Fedora/RHEL-based distributions:

```bash
sudo dnf install gcc gcc-c++ portaudio-devel clang-devel pkg-config wl-copy \
  libxkbcommon-devel wayland-devel libX11-devel libXcursor-devel libXi-devel libXrandr-devel \
  alsa-lib-devel openssl-devel fftw-devel curl cmake vulkan-loader-devel
```

For Arch-based distributions:

```bash
sudo pacman -S base-devel portaudio clang pkgconf wl-copy \
  libxkbcommon wayland libx11 libxcursor libxi libxrandr alsa-lib openssl fftw curl cmake \
  vulkan-headers vulkan-tools
```

For NixOS:

Simply use the provided flake.nix by running

```bash
nix develop
```

while in the root directory of the repository. The flake includes all necessary dependencies including vulkan-loader.

### Required Models

Sonori needs two types of models to function properly:

1. **Whisper Model** - Configured in the `config.toml` file and downloaded automatically on first run
2. **Silero VAD Model** - Also downloaded automatically on first run

   Note: If you need to download the Silero model manually for any reason, you should head to the repo and download the model yourself:

   https://github.com/snakers4/silero-vad/

   And then place it in `~/.cache/sonori/models/`

### Additional Requirements

- **ONNX Runtime**: Required for the Silero VAD model.
- **CTranslate2**: Used for Whisper model inference.
- **Vulkan**: Required for WGPU rendering. Your system must have a working Vulkan installation.

## Installation

### Building from Source

1. Install Rust and Cargo (https://rustup.rs/) and make sure the dependencies are installed
2. Clone this repository
3. Build the application:
   ```bash
   cargo build --release
   ```
4. The executable will be in `target/release/sonori`

## Usage

### GUI Mode (Default)

1. Launch the application:
   ```bash
   ./target/release/sonori
   ```
2. A transparent overlay will appear at the bottom of your screen
3. Recording starts automatically
4. Speak naturally - your speech will be transcribed in real-time or near real-time (based on the model and hardware)
5. Use the buttons on the overlay to:
   - Pause/Resume recording
   - Copy text to clipboard
   - Clear transcript history
   - Exit the application

### CLI Mode

For headless usage or terminal-based transcription:

1. Launch in CLI mode:
   ```bash
   ./target/release/sonori --cli
   ```
2. Transcription will appear directly in your terminal
3. Recording starts automatically
4. Speak naturally - transcriptions will update in real-time on the same line
5. Press `Ctrl+C` to exit gracefully

### Command Line Options

- `--cli`: Run in CLI mode without GUI
- `--help`: Show help information
- `--version`: Display version information

## Configuration

Sonori uses a `config.toml` file in the same directory as the executable. If not present, a default configuration is used.

Example configuration:

```toml
model = "openai/whisper-base.en"  # Whisper model from Hugging Face
language = "en"                   # Language code for transcription
compute_type = "INT8"             # Compute precision: INT8, FLOAT16
device = "CPU"                    # Device type: CPU, CUDA (if available)
log_stats_enabled = false         # Enable performance statistics logging
buffer_size = 1024                # Audio buffer size (affects latency/performance)
sample_rate = 16000               # Audio sample rate in Hz

[whisper_options]
beam_size = 5                     # Beam search width (higher = more accurate, slower)
patience = 1.0                    # Search patience factor
repetition_penalty = 1.25         # Penalty for repetitive transcriptions

[vad_config]
threshold = 0.2                   # Voice activity detection sensitivity (0.0-1.0)
hangbefore_frames = 1             # Frames to wait before confirming speech start
hangover_frames = 15              # Frames to wait after speech before ending segment
max_buffer_duration_sec = 30.0    # Maximum audio buffer duration
max_segment_count = 20            # Maximum segments to keep in memory

[audio_processor_config]
max_vis_samples = 1024            # Maximum samples for audio visualization

[keyboard_shortcuts]
copy_transcript = "KeyC"          # Copy transcription to clipboard (Ctrl+C)
reset_transcript = "KeyR"         # Clear current transcript (Ctrl+R)
quit_application = "KeyQ"         # Alternative quit shortcut
toggle_recording = "Space"        # Pause/resume recording
exit_application = "Escape"       # Exit the application
```

### Keyboard Shortcuts

You can customize the keyboard shortcuts used in the application by editing the `keyboard_shortcuts` section in the config.toml file. The default shortcuts are:

- `copy_transcript`: KeyC (Ctrl+C) - Copy the transcription to clipboard
- `reset_transcript`: KeyR (Ctrl+R) - Clear the current transcript
- `toggle_recording`: Space - Toggle recording on/off
- `exit_application`: Escape - Exit the application

When specifying keys, use the key names from the [KeyCode enum in winit](https://docs.rs/winit/latest/winit/keyboard/enum.KeyCode.html), such as:

- Letter keys: KeyA, KeyB, KeyC, etc.
- Number keys: Digit0, Digit1, etc.
- Function keys: F1, F2, etc.
- Special keys: Space, Escape, Enter, Tab, etc.

Note: The Ctrl modifier is automatically applied to copy_transcript, reset_transcript shortcuts.

### Model Options

Recommended Local Whisper models:

- `openai/whisper-tiny.en` - Tiny model, English only (for low-end CPUs)
- `openai/whisper-base.en` - Base model, English only (default, for low to mid-range CPUs)
- `distil-whisper/distil-small.en` - Small model, English only (for mid to high-range CPUs)
- `distil-whisper/distil-medium.en` - Medium model, English only (for high-end CPUs only)
- any other bigger whisper model - probably too slow to run on CPU only in real-time

For non-English languages, use the multilingual models (without `.en` suffix) and set the appropriate language code in the configuration.

### Performance Monitoring

Sonori includes optional performance monitoring that can be enabled by setting `log_stats_enabled = true` in your configuration:

- **Statistics Logging**: Detailed performance metrics are logged to `transcription_stats.log` in the current directory
- **Real-time Factor (RTF)**: Tracks minimum, maximum, and average processing speed relative to real-time
- **Processing Metrics**: Monitors transcription processing time and segments processed
- **Automatic Reporting**: Statistics are automatically reported every 10 seconds during operation

This feature is useful for:
- Optimizing model and configuration choices for your hardware
- Monitoring performance degradation over time
- Debugging transcription issues
- Benchmarking different model configurations

### File Locations

Sonori creates and uses several files and directories:

**Model Storage:**
- `~/.cache/sonori/models/` - Downloaded and converted Whisper models
- `~/.cache/sonori/models/silero_vad.onnx` - Silero VAD model

**Logs and Output:**
- `transcription_stats.log` - Performance statistics (when `log_stats_enabled = true`)
- Created in the current working directory where Sonori is launched

**Configuration:**
- `config.toml` - Configuration file (searched in current directory)

## Known Issues

- The application might not work with all Wayland compositors (I only tested it with KDE Plasma and KWin).
- The transcriptions are not 100% accurate and might contain errors. This is closely related to the whisper model that is used.
- Sometimes the last word of a "segment" is cut off. This is probably an issue with processing the audio data.
- The CPU usage is too high, even when idle. This might be related to bad code on my side or some overhead of the models. I already identified that changing the buffer size will help (or make it worse).

## Troubleshooting

### Wayland Support

Sonori uses layer shell protocol for Wayland compositors. If you experience issues:

- Make sure you are in a wayland session and your compositor supports the layer shell protocol

### Vulkan Support

Sonori uses WGPU for rendering, which requires Vulkan support. If you encounter errors related to adapter detection or Vulkan:

- Ensure you have the Vulkan libraries installed for your distribution (see Dependencies section)
- Verify that your GPU supports Vulkan and that drivers are properly installed
- On some systems, you may need to install additional vendor-specific Vulkan packages (e.g., `mesa-vulkan-drivers` on Ubuntu/Debian)
- You can test Vulkan support by running `vulkaninfo` or `vkcube` if available on your system

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

## Platform Support

- **Linux**: Supported (tested on Wayland using KDE Plasma and KWin)
- **Windows/macOS**: Not officially supported or tested

## Credits

- [Rust](https://www.rust-lang.org/)
- [CTranslate2](https://github.com/OpenNMT/CTranslate2) and [Faster Whisper](https://github.com/SYSTRAN/faster-whisper)
- [Onnx Runtime](https://github.com/microsoft/onnxruntime)
- [OpenAI Whisper](https://github.com/openai/whisper)
- [Silero VAD](https://github.com/snakers4/silero-vad)
- [Winit Fork](https://github.com/SergioRibera/winit)
- [WGPU](https://github.com/gfx-rs/wgpu)

## License

[MIT](LICENSE)
