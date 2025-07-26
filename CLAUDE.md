# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**Sonori** is a real-time speech transcription application that provides a lightweight, transparent overlay displaying live transcriptions using OpenAI's Whisper AI models on Linux. The application is written in Rust and features both GUI and CLI modes with GPU-accelerated rendering.

## Development Commands

### Build and Run
- `cargo build --release` - Production build
- `cargo run` - Development execution  
- `cargo run -- --cli` - Run in CLI mode instead of GUI mode
- `nix develop` - Enter NixOS development shell with all dependencies

### Dependencies
For NixOS users, simply use `nix develop`. For other distributions, refer to the extensive dependency lists in README.md for Debian/Ubuntu, Fedora/RHEL, and Arch-based systems.

## Architecture and Code Structure

### Core Components
- **`src/main.rs`** - Entry point with CLI/GUI mode selection
- **`src/real_time_transcriber.rs`** - Main coordinator integrating all components
- **`src/audio_*.rs`** - Audio capture and processing modules using PortAudio
- **`src/ui/`** - GPU-accelerated UI components with custom WGSL shaders
- **`src/config.rs`** - Configuration management (reads `config.toml`)
- **`src/download.rs`** - Automatic model downloading from Hugging Face

### Technology Stack
- **GPU Rendering**: Custom WGPU-based UI with WGSL shaders (no traditional GUI framework)
- **Audio**: PortAudio for capture, rustfft for spectrograms
- **AI Models**: OpenAI Whisper via CTranslate2 (`ct2rs`), Silero VAD via ONNX Runtime (`ort`)  
- **Windowing**: Custom fork of `winit` with Wayland layer shell support
- **Async**: Tokio-based architecture with Arc<RwLock<T>> for thread-safe shared state

### Real-time Pipeline
Audio capture → Voice Activity Detection → Whisper transcription → GPU-rendered overlay display

## Key Design Decisions

1. **Custom UI Framework**: Uses WGPU instead of traditional GUI frameworks for learning and performance
2. **Wayland Layer Shell**: Uses layer shell protocol for transparent overlay functionality
3. **Modular Architecture**: Clean separation between audio processing, AI inference, and UI rendering
4. **Configuration-driven**: Extensive TOML-based configuration in `config.toml`

## Configuration

The application uses `config.toml` for runtime configuration:
- Whisper model selection (e.g., "openai/whisper-base.en")
- Audio parameters (buffer size, sample rate)  
- VAD thresholds and timing
- Keyboard shortcuts (customizable key bindings)
- Whisper inference parameters (beam size, repetition penalty)

Models are automatically downloaded to `~/.cache/sonori/models/` on first run.

## Development Environment

### NixOS (Recommended)
The `flake.nix` provides a complete development environment including:
- Rust toolchain (beta channel, specified in `rust-toolchain.toml`)
- All system dependencies (Vulkan, PortAudio, FFTW, Wayland/X11 libraries)
- Development tools (rust-analyzer, mold linker for faster builds)

### Platform Support
- **Primary**: Linux with Wayland (tested on KDE Plasma/KWin)
- **Fallback**: X11 support included
- **Not supported**: Windows/macOS (explicitly not planned)

## Important Implementation Notes

### Thread Safety
Heavy use of `Arc<RwLock<T>>` and `Arc<AtomicBool>` for shared state between the real-time transcription pipeline and UI rendering thread.

### GPU Requirements
Requires Vulkan support for WGPU rendering. The application will fail without proper Vulkan drivers and libraries.

### Model Conversion
Whisper models are automatically converted to CTranslate2 format for faster inference. Manual conversion can be done using the `model-conversion/shell.nix` environment.

### Audio Processing
Uses a configurable buffer size (default 1024) and sample rate (16000 Hz) for real-time processing. CPU usage is currently high and being optimized.

## Known Limitations

- No comprehensive test suite (relies on manual testing)
- High CPU usage even when idle
- Limited Wayland compositor compatibility (primarily tested with KWin)
- Occasional truncation of last words in transcription segments
- CUDA support is planned but currently broken

## Recent Development Focus

Current work centers on performance optimization, reducing memory usage, and attempting to add CUDA support for GPU acceleration of inference (see recent commits).