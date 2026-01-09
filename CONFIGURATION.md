# Sonori Configuration Guide

This document provides comprehensive configuration options for Sonori. The application always loads `~/.config/sonori/config.toml` (or `$XDG_CONFIG_HOME/sonori/config.toml`); set `SONORI_CONFIG_PATH` to override. If not present, a default configuration is created.

**Important:** Most users don't need to change many settings! The defaults work well for everyone. You typically only need to adjust 2-3 settings based on your needs.

## Quick Start Examples

Here are the most common configurations. Just copy the relevant sections into your `config.toml`:

### 🚀 Fast & Lightweight (Good for older computers)
```toml
[general_config]
model = "base.en"                    # Fast, decent quality
language = "en"                     # Change to your language
transcription_mode = "manual"       # Push-to-talk style

[backend_config]
backend = "whisper_cpp"              # Recommended backend
gpu_enabled = false                 # CPU-only for compatibility
quantization_level = "medium"        # Good balance
```

### ⚖️ Balanced Performance (Good default for most users)
```toml
[general_config]
model = "small.en"                   # Better accuracy, still fast
language = "en"                     # Change to your language
transcription_mode = "manual"       # Push-to-talk style

[backend_config]
backend = "whisper_cpp"              # Recommended backend
gpu_enabled = true                  # Use GPU if available
quantization_level = "medium"        # Good balance
```

### 🎯 High Quality (Powerful computers with GPU)
```toml
[general_config]
model = "large-v3-turbo"             # Excellent accuracy
language = "en"                     # Change to your language
transcription_mode = "manual"       # Push-to-talk style

[backend_config]
backend = "whisper_cpp"              # Recommended backend
gpu_enabled = true                  # GPU highly recommended
quantization_level = "high"          # Maximum quality
```

### 🎤 Real-Time Transcription (Live as you speak)
```toml
[general_config]
model = "small.en"                   # Fast enough for real-time
language = "en"                     # Change to your language
transcription_mode = "realtime"     # Live transcription

[backend_config]
backend = "whisper_cpp"              # Recommended backend
gpu_enabled = true                  # GPU required for real-time
quantization_level = "medium"        # Good balance
```

### ⚡ Moonshine Real-Time (ONNX backend)
```toml
[general_config]
model = "base"                       # Moonshine model: "tiny" or "base"
language = "en"                      # Moonshine English models only
transcription_mode = "realtime"      # Live transcription

[backend_config]
backend = "moonshine"                # Moonshine ONNX backend
gpu_enabled = true                   # Optional (CPU works too)
quantization_level = "high"          # Not used by Moonshine (kept for consistency)

[moonshine_options]
enable_cache = true                  # Enable decoder cache if supported
```

### 🌍 Multilingual Support (Non-English languages)
```toml
[general_config]
model = "small"                      # No .en suffix = multilingual
language = "es"                     # Change: es, fr, de, it, pt, etc.
transcription_mode = "manual"       # Push-to-talk style

[backend_config]
backend = "whisper_cpp"              # Recommended backend
gpu_enabled = true                  # GPU helps with multiple languages
quantization_level = "medium"        # Good balance
```

## Common Questions

**Do I need to change all these settings?** No! The examples above cover 95% of use cases. Just pick one and you're good to go.

**What's the difference between manual and realtime?**
- **Manual**: Record your speech, then transcribe. Good for longer speech, less resource intensive
- **Realtime**: Transcribes as you speak. Good for short phrases, more resource intensive

**Should I use GPU acceleration?** If you have a decent GPU (NVIDIA, AMD, Intel), yes. If you're on an older laptop or having issues, set `gpu_enabled = false`.

**What model should I use?**
- `base.en` or `small.en` for most users
- `large-v3-turbo` for best accuracy (requires good GPU)
- Models without `.en` support multiple languages

**How do I change where the window appears?** Add `window_position` to the `[display_config]` section. Available positions: `BottomLeft`, `BottomCenter` (default), `BottomRight`, `TopLeft`, `TopCenter`, `TopRight`, `MiddleLeft`, `MiddleCenter`, `MiddleRight`.

## Complete Configuration Example

```toml
[general_config]
model = "large-v3-turbo"          # Whisper model size (tiny, base, small, medium, large, large-v2, large-v3, large-v3-turbo)
language = "en"                   # Language code for transcription (use "auto" for auto-detect)
transcription_mode = "manual"     # "realtime" for live transcription, "manual" for push-to-talk

[backend_config]
backend = "whisper_cpp"           # Backend: "ctranslate2", "whisper_cpp", "moonshine"
threads = 8                       # Number of CPU threads (default: min(num_cpus, 4))
gpu_enabled = true                # Enable GPU acceleration (CUDA/Metal/Vulkan)
quantization_level = "medium"     # Precision: "high" (full), "medium" (q8_0), "low" (q5_1)

[audio_processor_config]
buffer_size = 1024                # Audio buffer size (also used for visualization)
                                   # Note: Sample rate is hardcoded to 16000 Hz (Silero VAD requirement)

[realtime_mode_config]
max_buffer_duration_sec = 30.0    # Maximum audio buffer duration for VAD history
max_segment_count = 20            # Maximum number of speech segments to buffer

[manual_mode_config]
max_recording_duration_secs = 120 # Maximum recording time per session (2 minutes)
clear_on_new_session = true       # Clear transcript when starting new session
chunk_duration_seconds = 29.0     # Chunk size in seconds (29s recommended to avoid 30s boundary issues)
enable_chunk_overlap = true       # Enable overlapping chunks for long sessions
chunk_overlap_seconds = 2.0       # Overlap duration between chunks (seconds)
disable_chunking = false          # Experimental: Disable chunking for no-limit mode

[vad_config]
sensitivity = "Medium"            # Voice Activity Detection sensitivity preset
                                   # Low: Reduces false positives in noisy environments
                                   # Medium: Balanced for most environments (recommended)
                                   # High: Catches quiet speech, may trigger on background noise
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
                                   # Note: Internal thresholds (entropy, logprob, no_speech) are hardcoded to whisper.cpp defaults

[moonshine_options]
enable_cache = false              # Enable decoder cache if supported by the model

[post_process_config]
enabled = true                    # Enable post-processing of transcriptions
remove_leading_dashes = true      # Remove leading dashes (e.g., "- text" → "text")
remove_trailing_dashes = true     # Remove trailing dashes (e.g., "text -" → "text")
normalize_whitespace = true       # Normalize whitespace

[enhancement_config]
enabled = false                   # Enable magic mode by default
# model = ""                      # HuggingFace GGUF: "owner/repo/filename.gguf"
max_tokens = 256                  # Maximum tokens to generate
# system_prompt = ""              # Custom system prompt

[portal_config]
enable_xdg_portal = true              # Enable XDG Desktop Portal for input injection and global shortcuts
enable_global_shortcuts = true        # Enable global shortcuts via portal
manual_toggle_accelerator = "<Super>backslash"  # Accelerator for toggling manual sessions
shortcut_mode = "Toggle"              # Shortcut behavior: "Toggle" (press to start/stop) or "PushToTalk" (hold to record)
paste_shortcut = "ctrl_shift_v"       # Paste method: "ctrl_shift_v" (terminals) or "ctrl_v" (apps)
                                      # Note: Application ID for portal registration is hardcoded to "dev.sonori"

[display_config]
vsync_mode = "Enabled"                # VSync: "Auto", "Enabled", "Adaptive", "Disabled", "Mailbox"
target_fps = 60                       # Target FPS when vsync is disabled
window_position = "BottomCenter"      # Window position on screen
                                      # Available: BottomLeft, BottomCenter, BottomRight,
                                      #            TopLeft, TopCenter, TopRight,
                                      #            MiddleLeft, MiddleCenter, MiddleRight

[window_behavior_config]
show_in_system_tray = true            # Show icon in system tray

[debug_config]
log_stats_enabled = false             # Enable detailed performance logging
save_manual_audio_debug = false       # Save manual mode audio to WAV files
recording_dir = "recordings"          # Directory to save debug audio recordings
save_transcript_history = false       # Save all transcripts to persistent history file
transcript_history_path = "~/.cache/sonori/transcript_history.txt"  # History file location (optional)
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

#### Moonshine (ONNX)
- **Models**: Moonshine ONNX merged models (auto-downloaded)
- **Strengths**: Fast real-time performance; scales with audio length
- **Use case**: Real-time or low-latency transcription
- **Model format**: `encoder_model.onnx` + `decoder_model_merged.onnx` with tokenizer
- **Model names**: `tiny`, `base` (English); add language tags for supported variants (e.g., `tiny-ko`)

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

#### Moonshine Backend
Recommended models:
- `tiny` - Fastest, lowest memory
- `base` - Higher accuracy, still fast

Moonshine models are auto-downloaded on first run into `~/.cache/sonori/models/moonshine-<model>-onnx`.

### Manual Mode Configuration

Manual mode allows push-to-talk transcription with specialized chunking for longer recordings:

#### Chunk Duration (`chunk_duration_seconds`)
- **Default**: 29.0 seconds
- **Recommended range**: 25-29 seconds
- **Why not 30s?**: Whisper has a 224-token output limit per chunk. When recordings exactly match the chunk duration (30s), they can hit this limit with dense speech, causing transcription to cut off prematurely. Using 29s creates safer chunking boundaries.
- **Effect**: Recordings longer than this value are automatically split into chunks for processing

#### Chunk Overlap (`enable_chunk_overlap`, `chunk_overlap_seconds`)
- **Purpose**: Prevents words at chunk boundaries from being cut off
- **Default**: Enabled with 2.0 second overlap
- **Recommended**: Keep enabled; if you notice repetition, reduce overlap to 0.5-1.0 seconds
- **Range**: 0.5 to 2.0 seconds (reduce overlap if you see boundary repeats)

#### Other Options
- `max_recording_duration_secs`: Maximum total recording length (default: 120 seconds)
- `clear_on_new_session`: Whether to clear previous transcript when starting new session
- `disable_chunking`: Experimental mode to process entire recording without chunks (may fail on long/dense speech)

### Voice Activity Detection (VAD)

Voice Activity Detection automatically identifies when speech is present in the audio stream. Sonori uses the Silero VAD model with configurable sensitivity presets.

#### Sensitivity Presets

The `sensitivity` setting controls how aggressively the VAD detects speech. Choose based on your acoustic environment:

**Low** - Reduces false positives in noisy environments
- Best for: Noisy offices, environments with background conversations, mechanical noise
- Trade-off: May miss very quiet speech or soft consonants
- Technical: Higher detection threshold (0.15), higher speech end threshold (0.12)

**Medium** (Recommended)
- Best for: Most home/office environments with moderate background noise
- Trade-off: Balanced between catching all speech and avoiding false triggers
- Technical: Moderate detection threshold (0.10), moderate speech end threshold (0.08)

**High** - Catches quiet speech, may trigger on background noise
- Best for: Quiet environments, soft-spoken users, ASMR/whispered content
- Trade-off: May trigger on breathing, keyboard sounds, distant conversations
- Technical: Lower detection threshold (0.05), lower speech end threshold (0.03)

#### Advanced VAD Parameters

These parameters fine-tune the VAD behavior (defaults work well for most users):

- `hangbefore_frames`: Frames to wait before confirming speech start (default: 5 = 50ms)
  - Prevents false positives from sudden noises like clicks or pops

- `hangover_frames`: Frames to wait after speech ends before cutting (default: 30 = 300ms)
  - Prevents speech from cutting off during natural pauses between words

- `silence_tolerance_frames`: Frames of silence to tolerate during speech (default: 8 = 80ms)
  - Allows for natural pauses within sentences without breaking the segment

- `speech_prob_smoothing`: Exponential moving average smoothing factor (default: 0.3)
  - Smooths detection to prevent jittery start/stop behavior

**Note**: Sample rate is hardcoded to 16000 Hz as required by the Silero VAD model.

### Display and Window Configuration

#### Display Configuration
- `vsync_mode`: VSync options - "Enabled" (default), "Adaptive", "Disabled", "Mailbox", "Auto"
- `target_fps`: Frame rate cap when VSync is disabled (default: 60)
- `window_position`: Position of the overlay window on screen (default: "BottomCenter")
  - Available positions: `BottomLeft`, `BottomCenter`, `BottomRight`, `TopLeft`, `TopCenter`, `TopRight`, `MiddleLeft`, `MiddleCenter`, `MiddleRight`
  - Uses Wayland layer-shell anchors for precise positioning
  - Note: Window dragging is not supported by the Wayland layer-shell protocol

#### Window Behavior
- `show_in_system_tray`: Show application icon in system tray (default: true)

### Enhancement Configuration (Magic Mode)

The enhancement feature ("Magic Mode") post-processes transcriptions through a local LLM to clean up grammar, remove filler words (um, uh, like), and transform raw speech into clear, well-structured text.

#### Model Configuration

Uses llama.cpp with GGUF models from HuggingFace for GPU-accelerated inference.

**Model format:** `owner/repo/filename.gguf`

**Example:** `Qwen/Qwen2.5-1.5B-Instruct-GGUF/qwen2.5-1.5b-instruct-q5_k_m.gguf`

#### Configuration Options

```toml
[enhancement_config]
enabled = false           # Enable magic mode by default when starting
# model = ""              # HuggingFace GGUF: "owner/repo/filename.gguf"
max_tokens = 256          # Maximum tokens to generate
# system_prompt = ""      # Custom system prompt (uses default if empty)
```

#### Model Storage

Models are stored in `~/.cache/sonori/models/enhancement/`

#### Custom System Prompts

Override the default enhancement behavior with a custom system prompt:

```toml
[enhancement_config]
system_prompt = "Transform this speech into a formal email. Fix grammar and maintain professional tone."
```

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

### Audio Recording Debug

Save manual mode audio recordings to WAV files for debugging or review by enabling `save_manual_audio_debug = true`:

- **Format**: 16-bit mono WAV files at 16kHz sample rate
- **Location**: Saves to directory specified by `recording_dir` (default: `recordings/`)
- **Naming**: Files are timestamped: `recording_20251211_143022.wav`

### Transcript History

Enable persistent transcript history by adding to your `[debug_config]` section:

```toml
[debug_config]
save_transcript_history = true         # Enable history saving
transcript_history_path = "~/.cache/sonori/transcript_history.txt"  # Optional custom path
```

- **Format**: Plain text with timestamps, one entry per line: `[2025-12-11 14:30:22] Your transcribed text`
- **Default Location**: `~/.cache/sonori/transcript_history.txt` (respects `$XDG_CACHE_HOME`)
- **Behavior**: Appends each transcription in real-time, persists across sessions
- **Both Modes**: Works for both real-time and manual transcription modes

The history file grows unbounded. To clear it, simply delete or truncate the file.

**Note**: These settings don't appear in the default config.toml since they're optional. Add them manually to enable history.

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
- `~/.cache/sonori/models/moonshine-*-onnx` - Downloaded Moonshine ONNX models
- `~/.cache/sonori/models/silero_vad.onnx` - Silero VAD model
- `~/.cache/sonori/models/enhancement/` - Enhancement models

### Logs and Output
- `transcription_stats.log` - Performance statistics (when `log_stats_enabled = true`)
- `recordings/` - Debug audio recordings (when `save_manual_audio_debug = true`)
- `~/.cache/sonori/transcript_history.txt` - Transcript history (when `save_transcript_history = true`)

### Configuration
- `~/.config/sonori/config.toml` - User configuration file (or `$XDG_CONFIG_HOME/sonori/config.toml`)
- Set `SONORI_CONFIG_PATH` environment variable to override config location
