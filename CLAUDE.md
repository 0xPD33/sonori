# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**Sonori** is a real-time speech transcription application built in Rust with GPU-accelerated rendering and Wayland layer shell integration on Linux.

For detailed architectural information, component relationships, and system design patterns, see [ARCHITECTURE.md](./ARCHITECTURE.md).

## Development Commands

### Build and Run
- `cargo build --release` - Production build
- `cargo run` - Development execution  
- `cargo run -- --cli` - Run in CLI mode instead of GUI mode
- `nix develop` - Enter NixOS development shell with all dependencies

### Dependencies
For NixOS users, use `nix develop`. For other distributions, refer to dependency lists in README.md.

## Implementation Guidelines

### Threading and Concurrency Patterns
- Use `Arc<AtomicBool>` for lock-free state flags (running, recording, processing)
- Use `Arc<RwLock<T>>` for shared mutable state (transcript history, visualization data)  
- Use `Arc<Mutex<T>>` for thread-safe resource access (models, statistics)
- Prefer bounded/unbounded channels for data pipeline communication
- Use try-lock patterns to avoid blocking critical real-time paths
- Implement producer-consumer patterns for audio processing pipeline

### Performance Best Practices
- Maintain real-time constraints: < 50ms audio latency, < 500ms transcription delay
- Use minimal allocations with object pooling and pre-allocation patterns
- Implement circular buffer management with automatic trimming
- Use lock-free data structures with atomic operations where possible
- Pre-compute transformation matrices and batch rendering operations
- Target 60 FPS UI rendering with < 16ms frame time

### Memory Management
- Implement RAII-based resource cleanup with Drop implementations
- Use pre-allocated circular buffers for audio data
- Implement automatic buffer trimming to prevent memory growth
- Use object pooling for frequently allocated structures
- Ensure bounded memory usage with automatic cleanup

### Audio Processing Guidelines
- Use configurable buffer size (default 1024) and sample rate (16000 Hz)
- Implement state machine patterns for Voice Activity Detection
- Use adaptive thresholding and hangover frame handling
- Preserve speech context with padding around segments
- Handle noise robustly in various acoustic environments

### GPU Rendering Practices
- Require Vulkan support for WGPU rendering (will fail without proper drivers)
- Use instanced rendering for repeated UI elements
- Implement vertex buffer management and reuse
- Use efficient shader variants for different rendering modes
- Implement multi-pass rendering with layered alpha blending

### Configuration Management
- Use TOML-based hierarchical configuration with runtime updates
- Implement hot-reloading of configuration files
- Provide validation and error handling with graceful fallbacks
- Support component-specific update handling
- Maintain default values as hardcoded fallbacks

### Error Handling and Robustness
- Implement graceful degradation for missing dependencies
- Handle Wayland/X11 fallback scenarios appropriately
- Provide clear error messages for missing Vulkan drivers
- Handle model download and conversion failures gracefully
- Implement proper cleanup on shutdown signals

### Platform Integration
- Use Wayland layer shell protocol for transparent overlays when available
- Implement X11 fallback with override-redirect and composite extension
- Handle multiple monitor configurations properly
- Implement proper focus management and stacking order
- Use system clipboard integration (wl-copy/wtype) for text pasting

### Code Organization Principles
- Maintain clean separation between audio processing, AI inference, and UI rendering
- Use Facade pattern for main coordination (RealTimeTranscriber)
- Encapsulate component state within respective modules
- Implement modular, self-contained UI components
- Use async/await patterns for I/O operations and model management

### Testing and Validation
- Currently relies on manual testing (no comprehensive test suite)
- Validate real-time performance constraints during development  
- Test across different Wayland compositors when possible
- Verify memory usage patterns and cleanup behavior
- Test graceful shutdown and error recovery paths