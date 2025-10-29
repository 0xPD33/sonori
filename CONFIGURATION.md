# Sonori Configuration Guide

This document provides comprehensive configuration options for Sonori. The application uses a `config.toml` file in the same directory as the executable. If not present, a default configuration is used.

## Complete Configuration Example

```toml
[general_config]
model = "large-v3-turbo"          # Whisper model size (tiny, base, small, medium, large, large-v2, large-v3, large-v3-turbo)
language = "en"                   # Language code for transcription (use "auto" for auto-detect)
transcription_mode = "manual"     # "realtime" for live transcription, "manual" for push-to-talk

[backend_config]
backend = "whisper_cpp"           # Backend: "ctranslate2", "whisper_cpp" (default)
threads = 8                       # Number of CPU threads (default: min(num_cpus, 4))
gpu_enabled = true                # Enable GPU acceleration (CUDA/Metal/Vulkan)
quantization_level = "medium"     # Precision: "high" (full), "medium" (q8_0), "low" (q5_1)

[audio_processor_config]
sample_rate = 16000               # Audio sample rate in Hz
buffer_size = 1024                # Audio buffer size (also used for visualization)

[realtime_mode_config]
max_buffer_duration_sec = 30.0    # Maximum audio buffer duration for VAD history
max_segment_count = 20            # Maximum number of speech segments to buffer

[manual_mode_config]
max_recording_duration_secs = 120 # Maximum recording time per session (2 minutes)
clear_on_new_session = true       # Clear transcript when starting new session
enable_chunk_overlap = true       # Enable overlapping chunks for long sessions
chunk_overlap_seconds = 0.5       # Overlap duration between chunks (seconds)
disable_chunking = false          # Experimental: Disable chunking for no-limit mode

[vad_config]
threshold = 0.10                  # Speech detection sensitivity (0.0-1.0, lower = more sensitive)
speech_end_threshold = 0.08       # Lower threshold for speech continuation (hysteresis)
hangbefore_frames = 5             # Frames to wait before confirming speech start (50ms)
hangover_frames = 30              # Frames to wait after speech ends before cutting (300ms)
silence_tolerance_frames = 8      # Frames of silence to tolerate during speech (80ms)
speech_prob_smoothing = 0.3       # Exponential moving average smoothing factor

[sound_config]
enabled = true                    # Enable sound feedback
volume = 0.5                      # Sound volume (0.0-1.0)

[common_transcription_options]
beam_size = 5                     # Beam search width (1 = greedy/fastest, higher = more accurate)
patience = 1.0                    # Beam search patience factor

[ctranslate2_options]
repetition_penalty = 1.25         # Penalty for repeated tokens

[whisper_cpp_options]
temperature = 0.2                 # Sampling temperature (0.0 = deterministic, higher = more creative)
suppress_blank = true             # Suppress blank outputs at beginning
no_context = true                 # Disable context to prevent double transcriptions
max_tokens = 0                    # Maximum tokens per segment (0 = auto)
entropy_thold = 2.4               # Entropy threshold for fallback sampling
logprob_thold = -1.0              # Log probability threshold for speech detection
no_speech_thold = 0.6             # No-speech probability threshold

[post_process_config]
enabled = true                    # Enable post-processing of transcriptions
remove_leading_dashes = true      # Remove leading dashes (e.g., "- text" → "text")
remove_trailing_dashes = true     # Remove trailing dashes (e.g., "text -" → "text")
normalize_whitespace = true       # Normalize whitespace

[portal_config]
enable_xdg_portal = true              # Enable XDG Desktop Portal for input injection and global shortcuts
enable_global_shortcuts = true        # Enable global shortcuts via portal
manual_toggle_accelerator = "<Super>backslash"  # Accelerator for toggling manual sessions
application_id = "dev.paddy.sonori"   # App ID for portal integration
paste_shortcut = "ctrl_shift_v"       # Paste method: "ctrl_shift_v" (terminals) or "ctrl_v" (apps)

[display_config]
vsync_mode = "Enabled"                # VSync: "Auto", "Enabled", "Adaptive", "Disabled", "Mailbox"
target_fps = 60                       # Target FPS when vsync is disabled

[window_behavior_config]
show_in_system_tray = true            # Show icon in system tray

[debug_config]
log_stats_enabled = false             # Enable detailed performance logging
```

## Configuration Sections

### Backend Selection

Sonori supports multiple transcription backends, each with different strengths:

#### Whisper.cpp (Default)
- **Models**: GGML format models (e.g., `base.en`, `small.en`)
- **Strengths**: Often faster, lighter weight, better CPU optimization, GPU acceleration support
- **Use case**: Recommended default for most users, performance-critical applications, lower resource usage
- **Model format**: Single .bin GGML files
- **GPU Support**: Optional Vulkan GPU acceleration (configure `gpu_enabled = true` in backend_config)

#### CTranslate2
- **Models**: Hugging Face Whisper models (e.g., `openai/whisper-base.en`)
- **Strengths**: Good balance of speed and accuracy, well-tested
- **Use case**: Alternative for compatibility or specific use cases
- **Model format**: CTranslate2 converted models

### Model Options

#### CTranslate2 Backend
Recommended models:
- `openai/whisper-tiny.en` - Tiny model, English only (for low-end CPUs)
- `openai/whisper-base.en` - Base model, English only (default, for low to mid-range CPUs)
- `distil-whisper/distil-small.en` - Small model, English only (for mid to high-range CPUs)
- `distil-whisper/distil-medium.en` - Medium model, English only (for high-end CPUs only)

#### Whisper.cpp Backend
Recommended models:
- `tiny.en` - Tiny model, English only (for low-end CPUs)
- `base.en` - Base model, English only (good starting point)
- `small.en` - Small model, English only (for mid-range CPUs)
- `base` - Base model, multilingual
- `small` - Small model, multilingual
- `large-v3-turbo` - Fast large model (requires GPU acceleration enabled)

For non-English languages, use the multilingual models (without `.en` suffix) and set the appropriate language code in the configuration.

### Display and Window Configuration

#### Display Configuration
- `vsync_mode`: VSync options - "Enabled" (default), "Adaptive", "Disabled", "Mailbox", "Auto"
- `target_fps`: Frame rate cap when VSync is disabled (default: 60)

#### Window Behavior
- `show_in_system_tray`: Show application icon in system tray (default: true)

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

### System Tray Integration

Sonori integrates with the system tray using StatusNotifierItem (freedesktop standard). The system tray provides quick access to:

- **Toggle Window** - Show/hide the main overlay
- **Show Window** - Force show the window
- **Hide Window** - Force hide the window
- **Toggle Recording** - Start/stop recording in real-time mode
- **Toggle Manual Session** - Start/stop manual transcription session
- **Quit** - Exit the application

The tray icon updates to reflect the current recording state and can show a preview of recent transcriptions.

## File Locations

### Model Storage
- `~/.cache/sonori/models/` - Downloaded and converted Whisper models
- `~/.cache/sonori/models/silero_vad.onnx` - Silero VAD model

### Logs and Output
- `transcription_stats.log` - Performance statistics (when `log_stats_enabled = true`)
- Created in the current working directory where Sonori is launched

### Configuration
- `config.toml` - Configuration file (searched in current directory)