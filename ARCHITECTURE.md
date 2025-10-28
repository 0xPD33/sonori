# Sonori Architecture Documentation

This document provides a comprehensive overview of the Sonori real-time speech transcription application architecture.

## System Overview

Sonori is a high-performance, real-time speech transcription application built in Rust. It provides a transparent overlay displaying live transcriptions using multiple AI backends (CTranslate2, Whisper.cpp, with GPU acceleration support), with GPU-accelerated rendering and Wayland layer shell integration for seamless system integration on Linux.

## Core Architecture

### High-Level Design

The application follows a **modular, multi-threaded pipeline architecture** with clear separation of concerns:

```
Audio Input → VAD Processing → Speech Transcription (Multi-Backend) → Post-Processing → GPU Rendering → System Integration
     ↓            ↓                 ↓                                    ↓                ↓              ↓
PortAudio → Silero VAD → Backend Abstraction (CT2/WhisperCpp) → Text Cleanup → WGPU UI → Wayland/XDG Portal
                               ↓                                ↓                ↓
                          GPU Acceleration                    Sound Feedback   System Tray
                        (CUDA/Vulkan)                      (CPAL)          (StatusNotifierItem)
```

### Primary Components

1. **Audio Capture Layer** - PortAudio-based real-time audio input with CPAL fallback
2. **Voice Activity Detection** - Silero VAD with ONNX Runtime inference
3. **Multi-Backend Transcription** - Unified backend abstraction supporting:
   - CTranslate2 (CUDA/CPU, default)
   - Whisper.cpp (Vulkan/OpenBLAS/CPU, implemented)
   - Parakeet (planned)
   - Supporting both real-time streaming and manual batch modes
4. **Text Post-Processing** - Configurable text cleanup and normalization pipeline for transcription output
5. **Custom GPU UI** - WGPU-based rendering with custom WGSL shaders, including mode-specific button layouts
6. **Sound Feedback System** - CPAL-based audio playback for state transitions (record start/stop, session complete, etc.)
7. **System Integration** - Wayland layer shell for transparent overlays, system tray (StatusNotifierItem), and optional XDG Desktop Portal for global shortcuts and input injection

### Transcription Modes

Sonori supports two transcription modes configurable via `transcription_mode` in config.toml:

- **RealTime Mode** (default): Continuous streaming transcription with low-latency VAD-triggered segments.
- **Manual Mode**: On-demand session-based transcription where audio is accumulated in a buffer until the user stops the session, then processed as a batch. Supports configurable max duration, auto-restart, and clearing on new sessions.

Modes can be toggled at runtime via UI button or CLI flags (`--mode manual`). The audio processor branches logic based on the current mode, using atomic state flags for thread-safe switching.

## Module Architecture

### Core Coordination

- **`real_time_transcriber.rs`** - Main application coordinator implementing the Facade pattern, managing transcription modes and manual session state
- **`main.rs`** - Entry point with CLI/GUI mode selection, Tokio runtime setup, and mode-specific initialization
- **`config.rs`** - TOML-based hierarchical configuration management, including mode-specific settings like [manual_mode_config], [portal_config], [backend_config], [display_config], [window_behavior_config], and [sound_config]
- **`download.rs`** - Automatic model downloading and conversion from Hugging Face; supports CTranslate2 and Whisper.cpp model formats
- **`backend/mod.rs`** - Backend abstraction layer with trait definitions, `BackendType` enum, and `QuantizationLevel` mapping
- **`backend/factory.rs`** - Factory pattern for backend instantiation based on config
- **`backend/ctranslate2.rs`** - CTranslate2 backend implementation (CUDA/CPU with INT8/FLOAT16 quantization)
- **`backend/whisper_cpp.rs`** - Whisper.cpp backend implementation (Vulkan/OpenBLAS/CPU with q8_0/q5_1 quantization)
- **`backend/traits.rs`** - Shared backend trait definitions and error handling

### Audio Processing Pipeline

- **`audio_capture.rs`** - PortAudio stream management and callback handling
- **`audio_processor.rs`** - Audio processing coordinator with circular buffer management; handles both real-time VAD-triggered processing and manual mode audio accumulation in a dedicated buffer
- **`silero_audio_processor.rs`** - VAD implementation using ONNX Runtime (used in real-time mode)
- **`transcribe.rs`** - Whisper model integration with CTranslate2 optimization
- **`transcription_processor.rs`** - Async transcription task management and queuing; supports larger batch segments for manual mode with optional chunking for long audio

### Text Post-Processing Pipeline

- **`post_processor.rs`** - Text cleanup and normalization for transcription output
  - Removes leading/trailing dashes and artifacts
  - Normalizes whitespace and character encoding
  - Configurable cleaning rules via `[post_process_config]`
  - Applied to all transcription output before display

### GPU-Accelerated UI Framework

- **`ui/app.rs`** - Winit application event handler and window management
- **`ui/window.rs`** - Main rendering orchestration and state management, including mode detection for layout updates
- **`ui/render_pipeline.rs`** - WGPU render pipeline setup and shader compilation
- **`ui/text_renderer.rs`** - Text rendering via Glyphon with font management
- **`ui/text_processor.rs`** - Text layout and processing for transcript display
- **`ui/text_window.rs`** - Text window rendering and scrolling management
- **`ui/spectogram.rs`** - Real-time FFT-based audio visualization
- **`ui/buttons.rs`** - Interactive buttons with mode-specific layouts (e.g., RecordToggle, Accept for manual mode)
- **`ui/button_texture.rs`** - Button texture management and rendering
- **`ui/event_handler.rs`** - Mouse and keyboard input event processing
- **`ui/layout_manager.rs`** - Dynamic UI layout management and positioning
- **`ui/scrollbar.rs`** - Custom scrollbar implementation for transcript scrolling
- **`ui/common.rs`** - Shared UI utilities and constants
- **`ui/*.wgsl`** - Custom GPU shaders for UI components

### Sound Feedback System

- **`sound_player.rs`** - CPAL-based audio playback with threading and volume control
- **`sound_generator.rs`** - Sine tone and sweep generation for audio feedback (5 sound types)

### System Integration

- **`portal_input.rs`** - XDG Desktop Portal integration for remote desktop and keyboard input injection (e.g., automatic Ctrl+V pasting); handles session lifecycle and token persistence
- **`portal_tokens.rs`** - Portal session token persistence and restoration across runs
- **`global_shortcuts.rs`** - Global shortcut registration via XDG Desktop Portal (e.g., Super+backslash to toggle manual sessions); handles accelerator normalization and signal management
- **`system_tray.rs`** - Full StatusNotifierItem D-Bus integration with context menu, status indicators, and command support (window control, recording toggle, session management, mode switching, quit)
- **`copy.rs`** - Wayland clipboard operations using wl-copy
- **`stats_reporter.rs`** - Performance monitoring and telemetry collection
- **`transcription_stats.rs`** - Transcription quality metrics and analysis

## Backend System Architecture

### Multi-Backend Abstraction

Sonori uses an enum-based dispatch pattern for zero-cost abstraction across multiple transcription backends:

#### Backend Types
- **CTranslate2** (default) - Fast CPU/GPU inference using CTranslate2 optimization of Whisper models
  - GPU: CUDA support
  - Quantization: INT8 (default), FLOAT16, FLOAT32
  - Model format: Directory with model.bin, config.json, tokenizer.json
  - Max segment: 60 seconds

- **Whisper.cpp** - Lightweight, portable inference using whisper.cpp bindings (implemented)
  - GPU: Vulkan support, CPU optimization with OpenBLAS
  - Quantization: q8_0 (default), q5_1, f32
  - Model format: Single .bin GGML file
  - Max segment: No hard limit - adaptive segmentation based on audio length

- **Parakeet** (planned) - NVIDIA Parakeet RNNT models for improved accuracy
  - GPU: CUDA/GPU support via ONNX Runtime
  - Quantization: INT8, full precision
  - Model format: ONNX model files

#### Quantization Level Mapping
The unified `QuantizationLevel` enum maps to backend-specific implementations:
- **High** - Full precision (CT2: FLOAT32/FLOAT16, WhisperCpp: f32)
- **Medium** (default) - Balanced (CT2: INT8, WhisperCpp: q8_0)
- **Low** - Compact (CT2: INT8, WhisperCpp: q5_1)

#### Backend Factory Pattern
The factory (`backend/factory.rs`) instantiates the correct backend based on config:
1. Read `backend_config.backend` from config.toml
2. Load appropriate model file(s)
3. Initialize with thread count, GPU settings, and quantization
4. Return trait object for unified interface

#### Backend Configuration
Unified `BackendConfig` structure provides:
- `backend`: Backend selection (enum)
- `threads`: CPU thread count (default: min(4, num_cpus))
- `gpu_enabled`: GPU acceleration toggle (default: false for compatibility)
- `quantization_level`: Model precision trade-off

Backend-specific options are maintained in separate config structs:
- `ct2_options`: beam_size, patience, repetition_penalty
- `whisper_cpp_options`: beam_size, patience, temperature, thresholds, etc.

### Model Management

#### Automatic Download & Conversion
`download.rs` handles backend-specific model acquisition:

**CTranslate2**:
- Downloads HuggingFace Whisper models
- Converts using `ct2-transformers-converter` (requires Python/PyTorch)
- Stores in `~/.cache/sonori/models/{model}-ct2/`
- Supports model aliases for distilled variants

**Whisper.cpp**:
- Downloads pre-quantized GGML models from Hugging Face
- Automatic quantization level selection
- Stores in `~/.cache/sonori/models/ggml-{model}{quantization}.bin`
- Validates file integrity before use

#### Model Name Resolution
Intelligent mapping of simple model names to backend-appropriate formats:
- `"small"` → CTranslate2: `"distil-whisper/distil-small.en"` (faster)
- `"small"` → Whisper.cpp: `"small"` or `"small-q8_0"` (quantized)

### GPU Acceleration

#### CTranslate2 GPU Path
- CUDA device selection via `gpu_enabled` flag
- Automatic device detection if available
- Falls back to CPU if GPU unavailable
- Thread pool uses GPU for matrix operations

#### Whisper.cpp GPU Path
- Vulkan support via whisper-rs bindings
- OpenBLAS CPU acceleration as fallback
- GPU context initialization and session setup
- Automatic fallback on shader compilation errors

#### Performance Considerations
- GPU warm-up on first transcription (~1-2s)
- CPU more efficient for <100ms audio
- GPU better for >500ms segments
- Quantization trades accuracy for speed/memory

## Threading Model

### Multi-threaded Async Architecture

The application employs a sophisticated threading strategy optimized for real-time performance:

#### Thread Allocation
- **Main Thread** - UI rendering and Winit event loop (60 FPS target)
- **Audio Thread** - PortAudio callback processing (real-time priority)
- **Processing Thread** - VAD and audio analysis (Tokio async task)
- **Transcription Thread** - Whisper inference (spawn_blocking thread pool)
- **Wayland Thread** - System integration and clipboard operations

#### Synchronization Strategy
- **`Arc<AtomicBool>`** - Lock-free state flags (running, recording, processing)
- **`Arc<RwLock<T>>`** - Shared mutable state (transcript history, visualization data)
- **`Arc<Mutex<T>>`** - Thread-safe resource access (models, statistics)
- **Channel Communication** - Bounded/unbounded channels for data pipeline

### Concurrency Patterns

#### Producer-Consumer Pipeline
```rust
Audio Capture → [channel] → Audio Processing → [channel] → Transcription → [broadcast] → UI
```

#### State Machine Implementation
Voice Activity Detection implements a finite state machine:
```
Silence → PossibleSpeech → Speech → PossibleSilence → Silence
```

## Data Flow Architecture

### Real-time Processing Pipeline (RealTime Mode)

1. **Audio Capture** (`AudioCapture`)
   - 16kHz mono audio sampling via PortAudio
   - Circular buffer management with configurable size
   - Real-time callback processing with minimal latency

2. **Voice Activity Detection** (`SileroAudioProcessor`)
   - ONNX Runtime inference with Silero VAD model
   - State machine-based speech segmentation
   - Adaptive thresholding and hangover frame handling

3. **Speech Transcription** (`TranscriptionProcessor`)
   - Whisper model inference via CTranslate2 or whisper.cpp backends
   - Asynchronous processing with configurable beam search
   - Segment-based processing with context preservation

4. **Text Post-Processing** (`post_processor`)
   - Configurable text cleanup and normalization
   - Removal of artifacts (leading/trailing dashes)
   - Whitespace normalization and character encoding cleanup

5. **GPU Rendering** (`UI` modules)
   - Real-time spectrogram visualization
   - Scrollable transcript display with syntax highlighting
   - Interactive button system with hover states

6. **System Integration** (`WaylandConnection` / `PortalInput`)
   - Automatic text pasting to focused applications via wl-copy or XDG Portal
   - Wayland layer shell positioning and transparency

### Manual Mode Pipeline

In Manual Mode, the flow branches after audio capture:

1. **Audio Accumulation** (`AudioProcessor`): Samples are buffered in a manual_audio_buffer instead of immediate VAD processing.
2. **Session Management** (`RealTimeTranscriber`): Tracks session state (recording/processing) with configurable max duration and auto-restart.
3. **Batch Transcription** (`TranscriptionProcessor`): On session stop, the full buffer is sent as a single/large segment; long segments (>30s) are automatically chunked with overlap to avoid memory issues.
4. **UI Feedback**: Mode-specific buttons (RecordToggle, Accept) and status indicators; global shortcuts can trigger session toggle.

Mode switching is atomic and thread-safe, with cleanup (e.g., processing pending manual audio when switching to RealTime).

### State Management

#### Global Application State

- **`AppConfig`** - Centralized configuration with runtime updates, including mode-specific sections
- **`AudioVisualizationData`** - Shared state for UI components, with mode-aware visualization (e.g., continuous in RealTime, session-based in Manual)
- **Component State** - Encapsulated within respective modules, with atomic flags for mode transitions

#### Memory Management

- Pre-allocated circular buffers for audio data (real-time) and session buffers (manual)
- Object pooling for frequently allocated structures
- Automatic buffer trimming to prevent memory growth, with special handling for manual mode accumulation
- RAII-based resource cleanup with Drop implementations

## GPU Rendering Architecture

### Custom WGPU Framework

The application implements a custom GPU-accelerated UI framework built on WGPU:

#### Rendering Pipeline
1. **Surface Initialization** - Wayland layer shell or X11 window setup
2. **Device Configuration** - GPU device selection and feature detection
3. **Shader Compilation** - WGSL shader loading and pipeline creation
4. **Multi-pass Rendering** - Layered rendering with alpha blending

#### Custom Shaders (WGSL)
- **`rounded_rect.wgsl`** - Rounded rectangle primitives with anti-aliasing
- **`spectogram.wgsl`** - Instanced bar rendering for audio visualization
- **`text_window.wgsl`** - Background rendering for text regions
- **`button.wgsl`** - Multi-variant button rendering (textured and procedural)

#### UI Component Architecture
- **Modular Components** - Self-contained rendering and event handling
- **Layout Management** - Flexible positioning and sizing system
- **Event Handling** - Mouse and keyboard input processing
- **Animation System** - Smooth transitions and visual feedback

### Performance Optimizations

#### GPU Utilization
- Instanced rendering for repeated elements
- Vertex buffer management and reuse
- Texture atlas optimization for UI elements
- Efficient shader variants for different rendering modes

#### CPU Efficiency
- Dedicated async tasks with bounded blocking locks for deterministic throughput
- Minimal allocations in hot code paths
- Pre-computed transformation matrices
- Batch rendering operations

## Audio Processing Implementation

### Voice Activity Detection Pipeline

#### Silero VAD Integration
The application uses the Silero VAD model for robust voice activity detection:

```rust
Audio Samples → FFT Analysis → VAD Model → Probability → State Machine → Speech Segments
```

#### Processing Features
- **Adaptive Thresholding** - Dynamic adjustment based on audio characteristics
- **Context Preservation** - Padding around speech segments for complete words
- **Noise Handling** - Robust detection in various acoustic environments
- **Real-time Processing** - Low-latency inference with ONNX Runtime optimization
- **Sliding Windows** - 512-sample frames with 160-sample hops for overlapping detection and sub-20 ms updates

### Spectrogram Visualization

#### FFT-based Analysis
Real-time frequency domain analysis for visual feedback:
- **Window Function** - Hamming window for spectral analysis
- **Frequency Bins** - Configurable resolution for display
- **Magnitude Scaling** - Logarithmic scaling for perceptual accuracy
- **GPU Rendering** - Instanced bar rendering for smooth animation

## Configuration System

### Hierarchical Configuration

The application uses a sophisticated TOML-based configuration system:

#### Configuration Layers
1. **Default Values** - Hardcoded fallbacks in source code
2. **System Config** - Global configuration file
3. **User Config** - User-specific overrides
4. **Runtime Config** - Dynamic updates during execution

#### Key Configuration Areas
- **Audio Parameters** - Sample rate, buffer size, device selection
- **Model Selection** - Whisper model variants and parameters
- **UI Preferences** - Colors, fonts, positioning, keyboard shortcuts
- **VAD Settings** - Thresholds, timing parameters, sensitivity
- **System Integration** - Wayland/X11 preferences, paste behavior

### Runtime Reconfiguration

The application supports dynamic configuration updates without restart:
- Hot-reloading of configuration files
- Validation and error handling with graceful fallbacks
- Component-specific update handling
- User interface for common settings

## Platform Integration

### Wayland Layer Shell

The application leverages Wayland's layer shell protocol for system-level integration:

#### Layer Shell Benefits

- **True Transparency** - Compositor-level alpha blending
- **System Integration** - Proper stacking order and focus management
- **Multi-monitor Support** - Per-output positioning and scaling
- **Keyboard Shortcuts** - Global hotkey registration via layer shell or XDG Portal

#### Compositor Compatibility

Primary testing and support for:

- **KDE Plasma/KWin** - Full feature support
- **GNOME/Mutter** - Basic functionality
- **wlroots-based** - Partial support depending on compositor

### X11 Fallback Support

X11 support is maintained for broader compatibility:

- Traditional window management with override-redirect
- Composite extension for transparency effects
- Input event handling and global shortcuts
- Multi-display configuration support

### XDG Desktop Portal Integration

Optional integration via ashpd for enhanced UX:

- **Global Shortcuts** (`global_shortcuts.rs`): Register system-wide accelerators (e.g., Super+Tab for manual session toggle) without conflicting with other apps.
- **Input Injection** (`portal_input.rs`): Use RemoteDesktop portal to simulate keystrokes (e.g., Ctrl+V for pasting transcripts directly into focused fields).
- **Configuration**: Enabled via `[portal_config]` with app ID `dev.paddy.sonori` for stable identity.

## Performance Characteristics

### Real-time Constraints

The application is designed for real-time audio processing with strict latency requirements:

#### Target Metrics
- **Audio Latency** - < 50ms from input to VAD processing
- **Transcription Delay** - < 500ms for typical speech segments
- **UI Responsiveness** - 60 FPS rendering with < 16ms frame time
- **Memory Usage** - Bounded growth with automatic cleanup

#### Optimization Strategies
- **Lock-free Data Structures** - Atomic operations for shared state
- **Minimal Allocations** - Object pooling and pre-allocation
- **GPU Acceleration** - Offload rendering and computation to GPU
- **Thread Affinity** - CPU pinning for critical audio processing

### Scalability Considerations

The architecture supports various performance scaling approaches:
- **Model Selection** - Trade-off between accuracy and speed
- **Buffer Sizing** - Configurable latency vs. stability
- **GPU Utilization** - Dynamic quality adjustment based on performance
- **Processing Queues** - Backpressure handling for sustained loads

## Current Implementation Status

### Known Limitations

- **CPU Usage** - High idle CPU consumption
- **Compositor Support** - Limited Wayland compositor compatibility  
- **Model Performance** - CUDA support currently broken
- **Transcription Quality** - Occasional word truncation at segment boundaries
- **Manual Mode** - Long sessions (>120s) may require chunking; no built-in speaker diarization

## Conclusion

Sonori's architecture demonstrates sophisticated systems programming techniques optimized for real-time and on-demand audio processing. The custom GPU-accelerated UI framework, combined with careful threading design, mode-aware pipelines, and platform integration (including XDG Portal), creates a responsive and flexible transcription experience supporting both continuous and session-based workflows.

The architecture supports future extensibility through its modular design, backend abstraction layer, and component-based UI framework, allowing for incremental enhancements while maintaining backward compatibility and performance characteristics.

## Desktop Integration Reference

- App ID used for XDG Desktop Portals: `dev.paddy.sonori`
- Desktop file path (absolute): `/home/paddy/.local/share/applications/dev.paddy.sonori.desktop`
- Release binary path (Exec in desktop file): `/home/paddy/dev/rust/flashscribe/target/release/sonori`
