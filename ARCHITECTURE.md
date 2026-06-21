# Sonori Architecture Documentation

This document provides a comprehensive overview of the Sonori real-time speech transcription application architecture.

## System Overview

Sonori is a high-performance, real-time speech transcription application built in Rust. It provides a transparent overlay displaying live transcriptions using multiple local AI backends through the shared `speechcore` crate, with GPU-accelerated rendering and Wayland layer shell integration for seamless system integration on Linux.

The repository is split into two layers:

- **Sonori app (`flashscribe`)**: overlay UI, CLI/IPC, app configuration, portal integration, system tray, sound feedback, transcript history, and Magic Mode enhancement.
- **Shared STT runtime (`../speechcore`)**: audio capture, VAD, transcription orchestration, backend abstraction, model provisioning, transcript/status streams, and performance stats.

## Core Architecture

### High-Level Design

The application follows a **modular, multi-threaded pipeline architecture** with clear separation of concerns:

```
Audio Input → VAD Processing → Speech Transcription (Multi-Backend) → Post-Processing → GPU Rendering → System Integration
     ↓            ↓                 ↓                                    ↓                ↓              ↓
speechcore capture → speechcore VAD → speechcore backend abstraction → Text Cleanup → WGPU UI → Wayland/XDG Portal
                                      ↓                              ↓                ↓
                      CT2 / Whisper.cpp / Moonshine / Parakeet     Sound Feedback   System Tray
                       (CUDA / Vulkan / ONNX Runtime)              (CPAL)          (StatusNotifierItem)
```

### Primary Components

1. **Audio Capture Layer** - PortAudio-based real-time audio input provided by `speechcore`
2. **Voice Activity Detection** - Silero VAD with ONNX Runtime inference provided by `speechcore`
3. **Multi-Backend Transcription** - Unified backend abstraction supporting:
   - CTranslate2 (CUDA/CPU)
   - Whisper.cpp (Vulkan/OpenBLAS/CPU, default)
   - Moonshine ONNX
   - Parakeet ONNX
   - Supporting both real-time streaming and manual batch modes
4. **Text Post-Processing** - Configurable text cleanup and normalization pipeline from `speechcore`
5. **Custom GPU UI** - WGPU-based rendering with custom WGSL shaders, including mode-specific button layouts
6. **Sound Feedback System** - CPAL-based audio playback for state transitions (record start/stop, session complete, etc.)
7. **System Integration** - Wayland layer shell for transparent overlays, system tray (StatusNotifierItem), and optional XDG Desktop Portal for global shortcuts and input injection

### Transcription Modes

Sonori supports two transcription modes configurable via `transcription_mode` in config.toml:

- **RealTime Mode**: Continuous streaming transcription with low-latency VAD-triggered segments.
- **Manual Mode** (default): On-demand session-based transcription where audio is accumulated in a buffer until the user stops the session, then processed as a batch. Supports configurable max duration, auto-restart, and clearing on new sessions.

Modes can be toggled at runtime via UI button or CLI flags (`--mode manual`). `speechcore::RealTimeTranscriber` owns the mode-aware audio and transcription pipeline, while Sonori owns UI, IPC, and system integration around it.

## Module Architecture

### Core Coordination

- **`main.rs`** - Entry point with CLI/GUI mode selection, Tokio runtime setup, and mode-specific initialization
- **`config.rs`** - TOML-based Sonori configuration and conversion into `speechcore::SpeechConfig`. Sonori defaults to the Whisper.cpp backend for new configs.
- **`lib.rs`** - App library surface for Sonori modules; shared STT APIs are imported directly from `speechcore`.
- **`../speechcore/src/real_time_transcriber.rs`** - Main transcription coordinator managing audio capture, modes, backend readiness, and transcript/status streams.
- **`../speechcore/src/download.rs`** - Automatic model downloading and conversion from Hugging Face; supports CTranslate2, Whisper.cpp, Moonshine, Parakeet, and Silero VAD model assets.
- **`../speechcore/src/backend/`** - Backend abstraction, factory, and concrete backend implementations.

### Audio Processing Pipeline

- **`../speechcore/src/audio_capture.rs`** - PortAudio stream management and callback handling
- **`../speechcore/src/audio_processor.rs`** - Audio processing coordinator with circular buffer management; handles both real-time VAD-triggered processing and manual mode audio accumulation.
- **`../speechcore/src/silero_audio_processor.rs`** - VAD implementation using ONNX Runtime
- **`../speechcore/src/transcription_processor.rs`** - Async transcription task management and queuing; supports manual-mode chunking with configurable overlap.

### Text Post-Processing Pipeline

- **`../speechcore/src/post_processor.rs`** - Text cleanup and normalization for transcription output
  - Removes leading/trailing dashes and artifacts
  - Normalizes whitespace and character encoding
  - Configurable cleaning rules via `[post_process_config]`
  - Applied to all transcription output before display

### GPU-Accelerated UI Framework

**Core Rendering:**
- **`ui/app.rs`** - Winit application event handler and window management
- **`ui/window.rs`** - Main rendering orchestration and state management, including mode detection for layout updates. Manages hover animation state, button panel fade, loading animation, and coordinates all 5 concurrent animation systems (hover, scroll, button, panel, loading).
- **`ui/render_pipeline.rs`** - WGPU render pipeline setup and shader compilation
- **`ui/text_renderer.rs`** - Text rendering via Glyphon with font management
- **`ui/text_processor.rs`** - Text layout and processing for transcript display
- **`ui/text_window.rs`** - Text window rendering and scrolling management
- **`ui/spectogram.rs`** - Real-time FFT-based audio visualization

**UI Components:**
- **`ui/buttons.rs`** - Interactive buttons with mode-specific layouts (e.g., RecordToggle, Accept for manual mode)
- **`ui/button_texture.rs`** - Button texture management and rendering
- **`ui/button_panel.rs`** - Animated button panel background with fade-in/out effects during hover transitions
- **`ui/event_handler.rs`** - Mouse and keyboard input event processing
- **`ui/layout_manager.rs`** - Dynamic UI layout management and positioning
- **`ui/scrollbar.rs`** - Custom scrollbar implementation for transcript scrolling
- **`ui/loading_animation.rs`** - State-based animation system with dots, spinner, success, and error visual indicators; uses `ProcessingState` enum for state management
- **`ui/common.rs`** - Shared UI utilities and constants, including `ProcessingState` enum (Idle, Loading, Transcribing, Completed, Error)

**Scroll State Management:**
- **`ui/scroll_state.rs`** - Centralized scroll state management with smooth LERP-based interpolation (20% per frame), auto-scroll tracking, and transcript change detection

**GPU Rendering Utilities:**
- **`ui/gpu_utils.rs`** - Reusable `GpuQuadRenderer` utility for circle/quad rendering, reduces boilerplate for animated elements
- **`ui/viewport.rs`** - Viewport calculation utilities and composable viewport transformations (factory methods for text area, spectrogram, scrollbar positioning)
- **`ui/render_context.rs`** - WGPU rendering context wrapper (currently unused; architectural pattern for future refactoring)

**Shaders:**
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
- **`transcript_writer.rs`** - Optional transcript history persistence
- **`../speechcore/src/stats_reporter.rs`** - Performance monitoring and telemetry collection
- **`../speechcore/src/transcription_stats.rs`** - Transcription quality metrics and analysis

## Backend System Architecture

### Multi-Backend Abstraction

`speechcore` uses an enum-based dispatch pattern for zero-cost abstraction across multiple transcription backends. Sonori configures this through `speechcore::BackendConfig`.

#### Backend Types
- **CTranslate2** - Fast CPU/GPU inference using CTranslate2 optimization of Whisper models
  - GPU: CUDA support
  - Quantization: INT8 (default), FLOAT16, FLOAT32
  - Model format: Directory with model.bin, config.json, tokenizer.json
  - Max segment: 60 seconds

- **Whisper.cpp** (default) - Lightweight, portable inference using whisper.cpp bindings (implemented)
  - GPU: Vulkan support, CPU optimization with OpenBLAS
  - Quantization: q8_0 (default), q5_1, f32
  - Model format: Single .bin GGML file
  - Max segment: No hard limit - adaptive segmentation based on audio length

- **Moonshine** - ONNX encoder/decoder backend optimized for low latency
  - GPU: Optional ONNX Runtime execution providers
  - Model format: ONNX model directory with tokenizer assets

- **Parakeet** - NVIDIA Parakeet TDT ONNX models
  - GPU: Optional ONNX Runtime execution providers
  - Quantization: INT8 model assets
  - Model format: Split ONNX model directory

#### Quantization Level Mapping
The unified `QuantizationLevel` enum maps to backend-specific implementations:
- **High** - Full precision (CT2: FLOAT32/FLOAT16, WhisperCpp: f32)
- **Medium** (default) - Balanced (CT2: INT8, WhisperCpp: q8_0)
- **Low** - Compact (CT2: INT8, WhisperCpp: q5_1)

#### Backend Factory Pattern
The factory (`../speechcore/src/backend/factory.rs`) instantiates the correct backend based on config:
1. Read `backend_config.backend` from config.toml
2. Load appropriate model file(s)
3. Initialize with thread count, GPU settings, and quantization
4. Return trait object for unified interface

#### Backend Configuration
Unified `BackendConfig` structure provides:
- `backend`: Backend selection (enum)
- `threads`: CPU thread count (default: min(4, num_cpus))
- `gpu_enabled`: GPU acceleration toggle (default: true)
- `quantization_level`: Model precision trade-off

Backend-specific options are maintained in separate config structs:
- `common_transcription_options`: beam_size and patience shared by compatible backends
- `ctranslate2_options`: repetition penalty
- `whisper_cpp_options`: temperature, context handling, max token limit, and prompt handling
- `moonshine_options`: decoder cache toggle
- `parakeet_options`: reserved for Parakeet-specific settings

### Model Management

#### Automatic Download & Conversion
`../speechcore/src/download.rs` handles backend-specific model acquisition. The default STT cache is `~/.cache/speechcore/models` and can be overridden with `SPEECHCORE_MODEL_DIR`.

**CTranslate2**:
- Downloads HuggingFace Whisper models
- Converts using `ct2-transformers-converter` (requires Python/PyTorch)
- Stores in `~/.cache/speechcore/models/{model}-ct2/`
- Supports model aliases for distilled variants

**Whisper.cpp**:
- Downloads pre-quantized GGML models from Hugging Face
- Automatic quantization level selection
- Stores in `~/.cache/speechcore/models/ggml-{model}{quantization}.bin`
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

### Backend Readiness Synchronization

The application implements a startup synchronization system to prevent transcription processing before the backend is fully initialized:

#### Initialization Sequence
1. **Main Thread** - Creates `RealTimeTranscriber` with `backend_ready: Arc<AtomicBool>` set to `false`
2. **Async Backend Loader** - Spawned task loads model (1-5 seconds typical), sets flag to `true` on success
3. **Transcription Processor** - Polls `backend_ready` every 100ms with 10-second timeout
4. **Processing Begins** - Once ready, queued audio segments are transcribed

#### Error Handling
- **Timeout**: If backend fails to initialize within 10 seconds, transcription processor exits with error
- **No Retry**: Failed initialization requires app restart (no automatic recovery mechanism)
- **Graceful Degradation**: UI shows error state; audio capture continues but segments aren't processed

#### Timeline
```
T+0.0s: App starts, backend_ready = false
T+0.1s: Transcription processor starts waiting
T+3.0s: Backend initialization completes, backend_ready = true
T+3.1s: Transcription processor unblocks, processing begins
```

This pattern ensures no race conditions between model loading and transcription requests, maintaining system stability during startup.

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
- **`rounded_rect.wgsl`** - Rounded rectangle primitives with anti-aliasing, shadow rendering, and hover animation support
- **`spectogram.wgsl`** - Instanced bar rendering for audio visualization with per-instance positioning and coloring
- **`text_window.wgsl`** - Background rendering for text regions with animated opacity on hover
- **`button.wgsl`** - Multi-variant button rendering (textured symbols and procedural drawing for mode toggle)
- **`quad.wgsl`** - Circle/quad rendering via distance field with anti-aliasing (used by loading animation and UI elements)
- **`button_panel.wgsl`** - Full-screen button panel background with fade animation and hover support for opacity_multiplier uniform
- **`scrollbar.wgsl`** - Simplified scrollbar rendering (track and thumb) with color uniform, eliminates unnecessary hover computation

#### GPU Resource Management

**Uniform Buffers:**
- **Hover Uniforms** - 16 bytes (f32 opacity_multiplier + padding)
  - Shared across rounded_rect, button_panel, and text_window shaders
  - Updated per-frame with current hover animation progress
  - Bind group reused across multiple components

- **Scrollbar Color Uniforms** - 16 bytes (vec4 color)
  - Separate bind groups for track (dark) vs thumb (light gray)
  - Static data, not updated after initialization

- **Button Rotation Uniforms** - Variable size
  - Per-button rotation angle and mode state
  - Updated on state transitions

**Instance Buffers:**
- **Spectrogram Bars** - 32 bytes per instance (position, size, color)
  - Dynamic allocation based on frequency bin count (default: 240)
  - Rewritten every frame (~4.8KB per frame for 240 bars)
  - Instance step mode for GPU-side per-bar transformation

**Vertex Buffers:**
- **Quad Vertices** - 32 bytes (4 vertices × 8 bytes)
  - Reused for all quad-based primitives (buttons, panels, loading animation)
  - Static allocation, no per-frame updates

**Bind Group Strategy:**
- Shared hover bind group minimizes GPU state changes
- Component-specific bind groups for unique uniform requirements
- No descriptor set caching (eager allocation on startup)

**Blending Configuration:**
- All UI passes use alpha blending: `SrcAlpha + OneMinusSrcAlpha`
- Layered rendering with proper depth ordering (back to front)
- Transparent clear enables proper compositing with desktop

#### UI Component Architecture
- **Modular Components** - Self-contained rendering and event handling
- **Layout Management** - Flexible positioning and sizing system
- **Event Handling** - Mouse and keyboard input processing
- **Animation System** - Smooth transitions and visual feedback

### Animation and Processing State System

The UI implements a sophisticated multi-animation system coordinated through `window.rs` with 5 concurrent animation types:

#### Processing State Machine
The `ProcessingState` enum (in `ui/common.rs`) drives UI rendering decisions:
- **Idle** - Ready for user input
- **Loading** - Model initialization in progress
- **Transcribing** - Active speech processing
- **Completed** - Success indicator
- **Error** - Processing failure

State is stored in `Arc<RwLock<AudioVisualizationData>>` for thread-safe sharing between audio and UI threads.

#### Animation Systems

**1. Hover Animation** (~300ms duration)
- Linear interpolation of `hover_animation_progress` (0.0 → 1.0)
- Animation speed: 3.5 units/second
- Updates every frame via `delta_time` calculation
- Drives text window opacity and button panel fade
- GPU uniform buffer updated per frame

**2. Scroll Animation** (continuous)
- LERP-based smooth scrolling: `offset += (target - offset) * 0.2`
- Snap threshold: 0.5 pixels to prevent infinite approach
- Speed: ~150-200ms to reach target
- Auto-scroll tracks transcript changes
- Managed in dedicated `ScrollState` module

**3. Button Hover Effects** (150ms duration)
- Scale transition: 1.0 → 1.15 on hover
- Rotation: 0° → 15° for interactive feedback
- Eased interpolation with 0.2 smoothing factor
- Per-button animation state tracking

**4. Button Panel Fade** (200ms duration)
- Quadratic easing: `progress² * 0.01`
- Fade in/out based on hover state
- Alpha blending in shader
- Target progress: 0.0 (hidden) or 1.0 (visible)

**5. Loading Animation** (800ms cycle)
- State-driven rendering: dots, spinner, success/error indicators
- Modulo-based continuous loop
- Uses `GpuQuadRenderer` for circle primitives
- Color coding by processing state:
  - Loading: Gray [0.7, 0.7, 0.7, 0.6]
  - Transcribing: Teal [0.1, 0.9, 0.5, 0.6]
  - Completed: Green [0.2, 0.8, 0.2, 0.7]
  - Error: Red [0.9, 0.2, 0.2, 0.7]

#### Frame Timing
All animations use frame-independent timing via `Instant::now().elapsed()`, ensuring consistent behavior regardless of frame rate. The system targets 60 FPS with < 16ms frame time.

### Frame Rendering Pipeline

The `window.rs` module orchestrates a multi-pass GPU rendering sequence coordinating all UI components:

#### Rendering Pass Sequence
1. **Background Clear** - Clear to transparent black (0,0,0,0)
2. **Spectrogram Background** - Rounded rectangle container below text area
3. **Spectrogram Bars** - Instanced FFT visualization with per-bar coloring
4. **Text Window** - Transcript background with animated hover opacity
5. **Loading Animation** (conditional) - Shown when processing and transcript empty
6. **Transcript Text** (conditional) - Rendered text with scroll offset
7. **Scrollbar** (conditional) - Track and thumb when text overflows
8. **Button Panel** (conditional) - Fade-animated backgrounds when hovering
9. **Button Icons** (conditional) - Interactive buttons when hovering transcript area

#### Window State Structure
The `WindowState` struct manages 70+ fields across multiple categories:
- **GPU Resources** - Surface, device, queue, configuration
- **UI Components** - Text window, spectrogram, buttons, scrollbar, loading animation
- **Animation State** - Hover progress, button animations, panel fade, scroll interpolation
- **External References** - Running flag, recording flag, transcription mode
- **Frame Timing** - Target duration, present mode, last frame time

#### Frame Loop Flow
1. **Frame Rate Check** - Skip if elapsed < target_duration (Immediate present mode)
2. **State Updates** - Sync transcription mode, update hover animation progress
3. **Audio Data Read** - Acquire audio samples and transcript via RwLock
4. **Scroll Calculation** - Update smooth scroll animation, sync scrollbar
5. **Processing State Check** - Determine animation vs text rendering
6. **GPU Submission** - Record all render passes into command encoder
7. **Present** - Submit commands and present frame to surface
8. **Request Redraw** - Always request next frame for continuous animation

The system maintains bounded lock durations (< 2ms per RwLock) and updates all animations every frame regardless of user interaction, ensuring smooth transitions.

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
- **Manual Mode Settings** - Chunking behavior, session duration, overlap configuration

### Manual Mode Configuration

Manual mode includes sophisticated audio chunking for long recordings:

#### Chunking System
- **`chunk_duration_seconds`** (default: 29.0) - Segment size for transcription
  - Why 29.0? Whisper trained on 30s chunks; 29s provides 1-second safety buffer to avoid edge cases
- **`enable_chunk_overlap`** (default: true) - Enables overlap between chunks
- **`chunk_overlap_seconds`** (default: 0.5-2.0) - Overlap duration for context preservation
- **`max_recording_duration_secs`** (default: 120) - Maximum session length before auto-stop
- **`clear_on_new_session`** (default: true) - Clear previous transcript on new session
- **`disable_chunking`** (default: false) - Experimental flag to disable automatic chunking

#### Chunking Algorithm
1. Trigger when segment duration ≥ `chunk_duration_seconds`
2. Split into fixed-size chunks with configurable overlap
3. Merge trailing remainder if < 5 seconds
4. Transcribe each chunk independently
5. Join results with spaces

#### Segment Flagging
Audio segments include `is_manual` boolean flag set by `audio_processor.rs` to explicitly identify mode, replacing duration-based heuristics. This enables specialized handling in the transcription pipeline.

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

- **CPU Usage** - Continuous rendering at target FPS (60 FPS by default) for animation maintenance
- **Compositor Support** - Primary support for KDE Plasma/KWin; limited compatibility with GNOME/Mutter and wlroots-based compositors
- **GPU Requirements** - Requires Vulkan drivers for WGPU rendering; no graceful degradation if GPU unavailable
- **Error Recovery** - No automatic backend initialization retry on failure; app restart required
- **Manual Mode** - Configured chunk duration of 29 seconds for long audio; no built-in speaker diarization
- **Test Coverage** - Relies on manual testing; no comprehensive automated test suite

## Conclusion

Sonori's architecture demonstrates sophisticated systems programming techniques optimized for real-time and on-demand audio processing. The custom GPU-accelerated UI framework, combined with careful threading design, mode-aware pipelines, and platform integration (including XDG Portal), creates a responsive and flexible transcription experience supporting both continuous and session-based workflows.

The architecture supports future extensibility through its modular design, backend abstraction layer, and component-based UI framework, allowing for incremental enhancements while maintaining backward compatibility and performance characteristics.

## Desktop Integration Reference

- App ID used for XDG Desktop Portals: `dev.paddy.sonori`
- Desktop file path (absolute): `/home/paddy/.local/share/applications/dev.paddy.sonori.desktop`
- Release binary path (Exec in desktop file): `/home/paddy/dev/rust/flashscribe/target/release/sonori`
