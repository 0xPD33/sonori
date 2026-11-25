# Sonori Configuration Guide

This document provides comprehensive configuration options for Sonori. The application always loads `~/.config/sonori/config.toml` (or `$XDG_CONFIG_HOME/sonori/config.toml`); set `SONORI_CONFIG_PATH` to override. If not present, a default configuration is created.

**Important:** Most users don't need to change many settings! The defaults work well for everyone. You typically only need to adjust 2-3 settings based on your needs.

Existing user configs are merged with new defaults on startup. Values already present in your local config are preserved; missing keys are added from `AppConfig::default()`.

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

### 🦜 Parakeet TDT (NVIDIA NeMo via sherpa-onnx)
```toml
[general_config]
model = "parakeet-tdt-0.6b-v3"      # "parakeet-tdt-0.6b-v3" (multilingual) or "parakeet-tdt-0.6b-v2" (English-only)
language = "en"                      # Language code; v3 supports 25 European languages
transcription_mode = "manual"        # Push-to-talk style

[backend_config]
backend = "parakeet"                 # Parakeet TDT backend
gpu_enabled = true                   # Optional (CPU works too)
quantization_level = "high"          # Not used by Parakeet (kept for consistency)

[parakeet_options]
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

**How do I change where the window appears?** Add `window_position` to the `[display_config]` section. Available positions: `BottomLeft`, `BottomCenter` (default), `BottomRight`, `TopLeft`, `TopCenter`, `TopRight`, `MiddleLeft`, `MiddleCenter`, `MiddleRight`, `Custom`. Holding Alt or Super while dragging the overlay stores a `Custom` position automatically.

## Complete Configuration Example

```toml
[general_config]
model = "large-v3-turbo"          # Whisper model size (tiny, base, small, medium, large, large-v2, large-v3, large-v3-turbo)
language = "en"                   # Language code for transcription (use "auto" for auto-detect)
transcription_mode = "manual"     # "realtime" for live transcription, "manual" for push-to-talk

[backend_config]
backend = "whisper_cpp"           # Backend: "ctranslate2", "whisper_cpp", "moonshine", "parakeet"
threads = 8                       # Number of CPU threads (default: min(num_cpus, 4))
gpu_enabled = true                # Enable GPU acceleration (CUDA/Metal/Vulkan)
quantization_level = "medium"     # Precision: "high" (full), "medium" (q8_0), "low" (q5_1)

[audio_processor_config]
buffer_size = 1024                # Audio buffer size (also used for visualization)
# input_device = "USB"             # Part of the microphone name; unset = system default
                                   # Note: Sample rate is hardcoded to 16000 Hz (Silero VAD requirement)

[realtime_mode_config]
max_buffer_duration_sec = 30.0    # Maximum audio buffer duration for VAD history
max_segment_count = 20            # Maximum number of speech segments to buffer

[manual_mode_config]
max_recording_duration_secs = 600 # Limit for disable_chunking only (10 minutes)
silence_stop_secs = 300           # Stop after 5 minutes without speech; 0 = never
clear_on_new_session = true       # Clear transcript when starting new session
# chunk_duration_seconds = 60.0   # Optional. Unset: the loaded model decides
disable_chunking = false          # Experimental: Disable chunking for no-limit mode

[vad_config]
sensitivity = "Medium"            # Low, Medium, High, or VeryHigh
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
# initial_prompt = "Sonori, NixOS, Wayland"  # Custom vocabulary: names and jargon to recognize
                                   # Note: Internal thresholds (entropy, logprob, no_speech) are hardcoded to whisper.cpp defaults

[moonshine_options]
enable_cache = false              # Enable decoder cache if supported by the model

[parakeet_options]

[post_process_config]
enabled = true                    # Enable post-processing of transcriptions
remove_leading_dashes = true      # Remove leading dashes (e.g., "- text" → "text")
remove_trailing_dashes = true     # Remove trailing dashes (e.g., "text -" → "text")
normalize_whitespace = true       # Normalize whitespace
remove_fillers = true             # Drop standalone "um", "uh", "erm", "hm"
collapse_repeated_words = false   # Collapse stutters (e.g., "the the" → "the")
capitalize_sentences = false      # Capitalize the first letter of each sentence
ensure_terminal_punctuation = false  # Append a full stop if the text ends on a word

[enhancement_config]
enabled = false                   # Enable magic mode by default
# model = ""                      # HuggingFace GGUF: "owner/repo/filename.gguf"
max_tokens = 1024                 # Maximum tokens to generate
active = false                    # Magic Mode on or off; the overlay button and `sonori magic` change it
system_prompt = "Rewrite the transcript into clean, natural text while preserving the speaker's meaning. Fix obvious transcription artifacts, punctuation, and casing. Do not add facts, explanations, or commentary."

[portal_config]
enable_xdg_portal = true              # Enable XDG Desktop Portal for input injection and global shortcuts
enable_global_shortcuts = true        # Enable global shortcuts via portal
manual_toggle_accelerator = "<Super>backslash"  # Accelerator for toggling manual sessions
shortcut_mode = "Toggle"              # Shortcut behavior: "Toggle" (press to start/stop) or "PushToTalk" (hold to record)
paste_shortcut = "auto"               # "auto": Ctrl+Shift+V in terminals, Ctrl+V elsewhere (niri, Sway, Hyprland);
                                      # "ctrl_shift_v" or "ctrl_v" forces one shortcut
output_mode = "Paste"                 # "Paste" (clipboard + paste shortcut), "Type" (input method on niri and Sway,
                                      # else wtype/dotool; clipboard untouched) or "Clipboard" (copy only)
                                      # Note: Application ID for portal registration is hardcoded to "dev.sonori"

[display_config]
vsync_mode = "Enabled"                # VSync: "Auto", "Enabled", "Adaptive", "Disabled", "Mailbox"
target_fps = 60                       # Target FPS when vsync is disabled
window_position = "BottomCenter"      # Window position on screen
                                      # Available: BottomLeft, BottomCenter, BottomRight,
                                      #            TopLeft, TopCenter, TopRight,
                                      #            MiddleLeft, MiddleCenter, MiddleRight, Custom

[window_behavior_config]
show_in_system_tray = true            # Show icon in system tray

[ui_config]
display_mode = "Cursor"              # "Cursor": a tile at the pointer and a bubble; "Overlay": the fixed panel
cursor_bubble = "Brief"              # How long cursor mode shows the text: "Brief", "Long", or "Off"
live_text = false                    # Show the words while you dictate; needs a GPU and a fast model
visual_theme = "Focus"               # Focus, Pulse, Terminal (the fixed overlay)
spectrogram_skin = "Bars"            # Bars, Waveform, Meter
font_size = 10.0                     # Transcript font size
speaking_color = [0.1, 0.9, 0.5, 1.0]
idle_color = [1.0, 0.85, 0.15, 1.0]
recording_indicator_color = [0.9, 0.2, 0.2, 1.0]
show_recording_indicator = true
typewriter_effect = false            # Animate text reveal in manual mode

[debug_config]
log_stats_enabled = false             # Enable detailed performance logging

[history_config]
enabled = false                       # Append every transcript to the history file
# path = "/home/you/notes/dictation.txt"  # Optional; default ~/.cache/sonori/transcript_history.txt

[training_data]
enabled = false                       # Save the audio and text of every dictation
dir = "~/.local/share/sonori/training-data"
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
- `openai/whisper-base.en` - Base model, English only, for low to mid-range CPUs
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

Moonshine models are auto-downloaded on first run into `~/.cache/speechcore/models/moonshine-<model>-onnx`.

#### Parakeet TDT Backend
- **Models**: NVIDIA NeMo Parakeet TDT INT8 ONNX models via sherpa-onnx (auto-downloaded, ~640-660MB)
- **Strengths**: High accuracy, fast inference; v3 supports 25 European languages
- **Use case**: High-quality transcription with multilingual support
- **Model format**: Split INT8 ONNX models
- **GPU Support**: Optional via ONNX Runtime

Recommended models:
- `parakeet-tdt-0.6b-v3` - Multilingual (25 European languages)
- `parakeet-tdt-0.6b-v2` - English-only

Parakeet models are auto-downloaded on first run from HuggingFace.

### Manual Mode Configuration

Manual mode allows push-to-talk transcription with specialized chunking for longer recordings:

#### Chunk Duration (`chunk_duration_seconds`)
- **Default**: unset. The loaded model sets the length.
- **Whisper** (whisper.cpp, CTranslate2), **Moonshine** and **Nemotron**: 29 seconds. Whisper reads audio in 30-second windows and writes at most 224 tokens for each window. A chunk of exactly 30 seconds can reach this limit with dense speech, and the end of the text is lost.
- **Parakeet**: 60 seconds. A longer chunk gives the model more context and fewer cut points. Parakeet takes at most 90 seconds in one call.
- **Effect**: Sonori cuts a chunk at the first pause after this length. If there is no pause, Sonori cuts at the model limit.
- **Override**: set a value to use it for every model. Sonori reduces it to the limit of the loaded model. Old configs that hold the earlier default 29.0 lose that line, so the model picks the length.

#### Live Words (`ui_config.live_text`)

Off by default. When on, the cursor-mode bubble shows your words while you dictate. The overlay shows them too. Turn it on in Settings → Look → "Words while you speak"; a sheet first explains the points below and rates your current model and GPU setting. It applies after a restart.

How it works: about twice a second, Sonori transcribes the audio of the current chunk again, and shows the result. A word turns solid once two passes in a row agree on it, and it stays until the chunk is transcribed. The newest words show dim, because they can still change. The pasted text always comes from the full transcription of each chunk, as without live words.

Use it with a GPU and a fast model. Parakeet suits it best; Nemotron streams its own live words. Whisper models and any model without the GPU are slow for it.

Side effects:
- The model works while you speak. This uses GPU time and power, and it competes with Magic Mode for the GPU.
- Chunks aim for 20 s instead of the model's length (60 s for Parakeet), so each pass stays short.
- Stop waits for a pass that is still running. With Parakeet on a GPU this is about 0.1 to 0.3 s.
- Anyone who sees your screen, for example during screen sharing, can read the words.

Known limits:
- The final text can differ a little from the live words, mostly in the last half second before Stop.
- If you speak for more than 25 s without a pause, the bubble shows only the last 25 s. The pasted text is complete.
- If one pass takes more than 2 s, Sonori stops the live words for the rest of that session.
- With `disable_chunking`, Sonori shows no live words.

#### Other Options
- A session has no length limit. Sonori transcribes it in chunks while you speak, so Stop only waits for the last chunk. Chunks with no speech are not transcribed.
- `silence_stop_secs`: Sonori stops a session after this many seconds without speech (default: 300). A click or a cough does not count as speech. Set 0 to turn it off.
- After Stop, the microphone stays open until your last word ends, at most 0.5 seconds. If you are already quiet, it closes at once.
- `max_recording_duration_secs`: Longest audio that waits for Stop (default: 600 seconds). Only `disable_chunking` keeps all audio until Stop, so only then this limits a session
- `clear_on_new_session`: Whether to clear previous transcript when starting new session
- `disable_chunking`: Experimental mode to process entire recording without chunks (may fail on long/dense speech)

### Transcript Cleanup (`post_process_config`)

Deterministic text cleanup applied to every transcript, whichever backend produced it. It runs before Magic Mode, so enabling these often removes the need for an LLM pass at all.

- `remove_fillers` (default: on) — drops standalone `um`, `uh`, `uhm`, `erm`, `hm`, `hmm`, `mhm` along with their trailing punctuation. Only whole words match, so `Umbrella` and `humming` are untouched. The list is deliberately short: `ah`, `oh`, `so` and `like` carry meaning often enough that removing them would change what you said.
- `collapse_repeated_words` (default: **off**) — collapses `the the` to `the`. Off because `had had`, `that that` and `very very` are legitimate English. Repetition across punctuation (`No, no`) is always preserved, since that reads as deliberate.
- `capitalize_sentences` (default: **off**) — off because every backend Sonori ships already capitalizes, and it is wrong when dictating shell commands. Useful for a backend that emits lowercase.
- `ensure_terminal_punctuation` (default: **off**) — appends a full stop when the text ends on a word. Off for the same reason.

The two defaults that are on (`remove_fillers`, `normalize_whitespace`) are safe for prose and code alike. Turn the others on if you dictate mostly prose.

To fix names and jargon for every backend, add replacements. Each key matches whole words without case; the value is written exactly:

```toml
[post_process_config.replacements]
"nix os" = "NixOS"
"claude code" = "Claude Code"
```

For whisper.cpp you can also bias recognition itself with `whisper_cpp_options.initial_prompt`, a list of names and terms.

### Voice Activity Detection (VAD)

Voice Activity Detection automatically identifies when speech is present in the audio stream. Sonori uses the Silero VAD model with configurable sensitivity presets. In real-time mode the VAD cuts the audio into segments. In manual mode it finds pauses to cut chunks at, skips chunks without speech, decides when the microphone closes after Stop, and drives the silence stop.

#### Sensitivity Presets

The `sensitivity` setting controls how aggressively the VAD detects speech. Choose based on your acoustic environment:

**Low** - Reduces false positives in noisy environments
- Best for: Noisy offices, environments with background conversations, mechanical noise
- Trade-off: May miss very quiet speech or soft consonants
- Technical: Higher detection threshold (0.5), higher speech end threshold (0.35)

**Medium** (Recommended)
- Best for: Most home/office environments with moderate background noise
- Trade-off: Balanced between catching all speech and avoiding false triggers
- Technical: Moderate detection threshold (0.4), moderate speech end threshold (0.25)

**High** - Catches quiet speech, may trigger on background noise
- Best for: Quiet environments, soft-spoken users, ASMR/whispered content
- Trade-off: May trigger on breathing, keyboard sounds, distant conversations
- Technical: Lower detection threshold (0.3), lower speech end threshold (0.15)

**VeryHigh** - Catches very quiet or distant speech
- Best for: Quiet rooms, whispering, a microphone far from your mouth
- Trade-off: Breathing and small noises often count as speech, so chunks with only noise are transcribed and the silence stop comes later
- Technical: Lowest detection threshold (0.2), lowest speech end threshold (0.1)

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
  - Available positions: `BottomLeft`, `BottomCenter`, `BottomRight`, `TopLeft`, `TopCenter`, `TopRight`, `MiddleLeft`, `MiddleCenter`, `MiddleRight`, `Custom`
  - Hold Alt or Super while dragging the overlay to persist a custom position
  - Uses Wayland layer-shell anchors for precise positioning

#### Window Behavior
- `show_in_system_tray`: Show application icon in system tray (default: true)

#### UI Configuration
- `visual_theme`: Built-in visual theme (`Focus`, `Pulse`, `Terminal`)
- `spectrogram_skin`: Audio visualization style (`Bars`, `Waveform`, `Meter`)
- `font_size`: Transcript font size
- `speaking_color`, `idle_color`, `recording_indicator_color`: RGBA colors used by the Focus theme
- `show_recording_indicator`: Show the pulsing recording indicator
- `typewriter_effect`: Animate text reveal character-by-character when transcription completes in manual mode (default: false)

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
max_tokens = 1024         # Maximum tokens to generate
active = false            # Whether Magic Mode is on; kept across restarts
system_prompt = "Rewrite the transcript into clean, natural text while preserving the speaker's meaning. Fix obvious transcription artifacts, punctuation, and casing. Do not add facts, explanations, or commentary."
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

### Transcript History

Transcript history is off by default. When you turn it on in Settings, Sonori first shows what it will save and where. Configure it in `[history_config]`:

```toml
[history_config]
enabled = true                          # Default false
path = "~/notes/dictation.txt"          # Optional custom path; a leading ~/ means your home
```

Older releases turned the history on without a warning. On the first start of this release, Sonori turns it off in configs from those releases. Turn it on again in Settings if you want it. Older configs that set `debug_config.transcript_history_path` keep that path; Sonori moves it into `history_config` on the next start.

- **Format**: Plain text with timestamps, one entry per line: `[2025-12-11 14:30:22] Your transcribed text`
- **Default Location**: `~/.cache/sonori/transcript_history.txt` (respects `$XDG_CACHE_HOME`)
- **Behavior**: Appends each transcription in real-time, persists across sessions
- **Both Modes**: Works for both real-time and manual transcription modes

The history file grows unbounded. To clear it, simply delete or truncate the file.

The Recent list in the quick menu lives in memory only. It works also with the history off.

### Training Data (`training_data`)

Sonori can save the audio and the text of every manual dictation, so you can train or test speech models on your own voice. It is off by default. When you turn it on in Settings, Sonori first shows what it will save and where. While it is on, the cursor-mode tile shows a small red dot during recording, and the log shows a warning at each start.

```toml
[training_data]
enabled = true                                  # Default false. Applies after a restart
dir = "~/.local/share/sonori/training-data"     # A leading ~/ means your home
```

Warning: the folder holds everything you dictate, also passwords and private text. Anyone who can open the folder can hear and read it.

- **Audio**: `audio/<date>-<time>-<session>.wav`, the whole session as 16 kHz mono 16-bit PCM. One hour of dictation uses about 115 MB.
- **Manifest**: one JSON line for each session in `manifest.jsonl`, in the NeMo manifest format (`audio_filepath`, `duration`, `text`), so NeMo can fine-tune Parakeet on it directly.
- **`text`**: the model's own words, before the clean-up rules. These are not checked labels. Correct them before you train on them.
- **More fields**: `created`, `session_id`, `sonori_version`, `speechcore_version`, `backend`, `model`, `language`, `gpu`, `vad_sensitivity`, `speech` (where the VAD heard speech, in seconds), `chunks` (start, end, raw and cleaned text, and inference time of each model call), `processed_text`, `magic_mode_text`, `app_id` (the focused app, on niri, Sway, and Hyprland), `output_mode`, `delivery`, and `delivery_error`.

This replaces `debug_config.save_manual_audio_debug`. That option saved only the audio after the last part of a long session. Sonori removes the old keys from the config.

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
- `~/.cache/speechcore/models/` - Shared STT model cache used by `speechcore`
- `~/.cache/speechcore/models/ggml-*.bin` - Whisper.cpp GGML models
- `~/.cache/speechcore/models/*-ct2/` - Downloaded and converted CTranslate2 models
- `~/.cache/speechcore/models/moonshine-*-onnx` - Downloaded Moonshine ONNX models
- `~/.cache/speechcore/models/parakeet-tdt-v3-int8/` - Parakeet TDT v3 model (multilingual)
- `~/.cache/speechcore/models/parakeet-tdt-v2-int8/` - Parakeet TDT v2 model (English-only)
- `~/.cache/speechcore/models/silero_vad.onnx` - Silero VAD model
- `~/.cache/sonori/models/enhancement/` - Enhancement models

### Logs and Output
- `transcription_stats.log` - Performance statistics (when `log_stats_enabled = true`)
- `~/.cache/sonori/transcript_history.txt` - Transcript history (when `history_config.enabled = true`)
- `~/.local/share/sonori/training-data/` - Audio and text of every dictation (when `training_data.enabled = true`)

### Configuration
- `~/.config/sonori/config.toml` - User configuration file (or `$XDG_CONFIG_HOME/sonori/config.toml`)
- Set `SONORI_CONFIG_PATH` environment variable to override config location
