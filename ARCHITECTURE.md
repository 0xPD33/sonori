# Sonori Architecture Documentation

This document provides a comprehensive overview of the Sonori real-time speech transcription application architecture.

## System Overview

Sonori is a high-performance, real-time speech transcription application built in Rust. It provides a transparent overlay displaying live transcriptions using OpenAI's Whisper models, with GPU-accelerated rendering and Wayland layer shell integration for seamless system integration on Linux.

## Core Architecture

### High-Level Design

The application follows a **modular, multi-threaded pipeline architecture** with clear separation of concerns:

```
Audio Input → VAD Processing → Speech Transcription → GPU Rendering → System Integration
     ↓            ↓                 ↓                  ↓              ↓
PortAudio → Silero VAD → CTranslate2 Whisper → WGPU UI → Wayland Paste
```

### Primary Components

1. **Audio Capture Layer** - PortAudio-based real-time audio input
2. **Voice Activity Detection** - Silero VAD with ONNX Runtime inference
3. **Speech Transcription** - Whisper models via CTranslate2 optimization
4. **Custom GPU UI** - WGPU-based rendering with custom WGSL shaders
5. **System Integration** - Wayland layer shell for transparent overlays

## Module Architecture

### Core Coordination
- **`real_time_transcriber.rs`** - Main application coordinator implementing the Facade pattern
- **`main.rs`** - Entry point with CLI/GUI mode selection and Tokio runtime setup
- **`config.rs`** - TOML-based hierarchical configuration management
- **`download.rs`** - Automatic model downloading and conversion from Hugging Face

### Audio Processing Pipeline
- **`audio_capture.rs`** - PortAudio stream management and callback handling
- **`audio_processor.rs`** - Audio processing coordinator with circular buffer management
- **`silero_audio_processor.rs`** - VAD implementation using ONNX Runtime
- **`transcribe.rs`** - Whisper model integration with CTranslate2 optimization
- **`transcription_processor.rs`** - Async transcription task management and queuing

### GPU-Accelerated UI Framework
- **`ui/app.rs`** - Winit application event handler and window management
- **`ui/window.rs`** - Main rendering orchestration and state management
- **`ui/render_pipeline.rs`** - WGPU render pipeline setup and shader compilation
- **`ui/text_renderer.rs`** - Text rendering via Glyphon with font management
- **`ui/spectogram.rs`** - Real-time FFT-based audio visualization
- **`ui/*.wgsl`** - Custom GPU shaders for UI components

### System Integration
- **`wayland_connection.rs`** - Wayland protocol integration for text input automation
- **`stats_reporter.rs`** - Performance monitoring and telemetry collection
- **`transcription_stats.rs`** - Transcription quality metrics and analysis

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

### Real-time Processing Pipeline

1. **Audio Capture** (`AudioCapture`)
   - 16kHz mono audio sampling via PortAudio
   - Circular buffer management with configurable size
   - Real-time callback processing with minimal latency

2. **Voice Activity Detection** (`SileroAudioProcessor`)
   - ONNX Runtime inference with Silero VAD model
   - State machine-based speech segmentation
   - Adaptive thresholding and hangover frame handling

3. **Speech Transcription** (`TranscriptionProcessor`)
   - Whisper model inference via CTranslate2
   - Asynchronous processing with configurable beam search
   - Segment-based processing with context preservation

4. **GPU Rendering** (`UI` modules)
   - Real-time spectrogram visualization
   - Scrollable transcript display with syntax highlighting
   - Interactive button system with hover states

5. **System Integration** (`WaylandConnection`)
   - Automatic text pasting to focused applications
   - Wayland layer shell positioning and transparency

### State Management

#### Global Application State
- **`AppConfig`** - Centralized configuration with runtime updates
- **`AudioVisualizationData`** - Shared state for UI components
- **Component State** - Encapsulated within respective modules

#### Memory Management
- Pre-allocated circular buffers for audio data
- Object pooling for frequently allocated structures
- Automatic buffer trimming to prevent memory growth
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
- Try-lock patterns to avoid blocking critical paths
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
- **Keyboard Shortcuts** - Global hotkey registration

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

## Conclusion

Sonori's architecture demonstrates sophisticated systems programming techniques optimized for real-time audio processing. The custom GPU-accelerated UI framework, combined with careful threading design and platform integration, creates a responsive and efficient transcription experience.