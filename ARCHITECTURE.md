# Sonori Architecture

Sonori is a local speech-to-text app for Linux desktops. You press a hotkey and
speak. Sonori transcribes the speech on your machine and puts the text into the
focused app.

The code has two layers:

- **Sonori** (this repository): the UI, delivery of text to apps, hotkeys, IPC,
  configuration, the system tray, sound cues, and Magic Mode.
- **speechcore** (`../speechcore`, a git dependency): audio capture, voice
  activity detection (VAD), the transcription backends, model downloads, and
  the session logic. See `../speechcore/README.md`.

Keep reusable speech code in speechcore. Keep app and desktop code in Sonori.

The design documents in `docs/design/` describe single features in depth:
`cursor-mode.md`, `settings.md`, and `magic-mode.md`.

## Pipeline

```
microphone ─▶ speechcore ─────────────────────────────▶ Sonori
              capture → VAD or manual buffer             consume_transcripts
              → backend → post-processing                → finalize_transcripts (Magic Mode)
              → transcript broadcast                     → output_worker (Paste, Type, Copy)
                                                         → Delivery → cursor-mode bubble
```

1. speechcore captures 16 kHz mono audio through PortAudio.
2. In manual mode, speechcore keeps the audio of a session until the session
   stops. Long sessions go to the backend in parts, cut at a pause. In
   real-time mode, Silero VAD cuts the audio into speech segments.
3. The backend transcribes each segment or part. speechcore cleans the text and
   sends it on a broadcast channel.
4. `consume_transcripts` (`src/main.rs`) shows interim text and hands final
   transcripts on. It never waits on slow work, so the broadcast channel
   cannot drop transcripts.
5. `finalize_transcripts` runs Magic Mode, adds a space between parts of one
   session, writes the history file, and queues the text for output.
6. `output_worker` delivers each transcript in order, as the output mode says.
   It records the result in the shared `Delivery` slot.

## Transcription modes

- **Manual** (default): a hotkey or `sonori toggle` starts and stops a session.
  Sonori delivers the text after the stop.
- **RealTime**: Sonori records all the time and delivers each VAD segment.

`sonori switch-mode`, the tray, and Settings change the mode at run time.
speechcore owns the mode logic in `RealTimeTranscriber`.

## Backends

speechcore selects the backend from `speechcore::BackendConfig`:

| Backend | Runtime | Model format |
|---|---|---|
| Whisper.cpp (default) | whisper-rs, Vulkan or CPU | one GGML `.bin` file |
| CTranslate2 | ct2rs, CUDA or CPU | converted model directory |
| Moonshine | ONNX Runtime | ONNX directory |
| Parakeet | ONNX Runtime | split ONNX directory |
| Nemotron | ONNX Runtime | int4 ONNX export |

speechcore downloads missing models to `~/.cache/speechcore/models`.
`SPEECHCORE_MODEL_DIR` overrides that folder. A `BackendManager` runs every
backend load in one queue, so Settings and the quick menu can swap the model
while Sonori runs.

## Output modes

`output_mode` in `[portal_config]` selects how Sonori delivers text:

- **Paste**: Sonori puts the text on the clipboard and sends the paste
  shortcut. During the paste, `paste_check.rs` serves the clipboard through
  `zwlr_data_control_v1`. So Sonori sees whether an app took the text, and it
  restores the old clipboard after the paste. The shortcut goes through the
  RemoteDesktop portal when it is enabled, else through wtype or dotool.
- **Type**: `input_method.rs` inserts the text through `zwp_input_method_v2`.
  If the compositor or the app does not support that, Sonori types the text
  with key events.
- **Copy**: Sonori puts the text on the clipboard only.

If a delivery fails, the text stays on the clipboard, so you do not lose it.

## Display modes

`display_mode` in `[ui_config]` selects the UI:

- **Cursor** (default): a glass tile with a level ring at the pointer while
  you record, then a bubble that shows the result. A held hotkey opens a quick
  menu. Sonori uses cursor mode only on Wayland and in manual mode. Otherwise
  it falls back to the overlay. See `docs/design/cursor-mode.md`.
- **Overlay**: a fixed layer-shell bar with a spectrogram, the transcript, and
  buttons.

The Settings window works in both modes. See `docs/design/settings.md`.

## Control

You control a running Sonori in four ways:

- **Hotkey**: `global_shortcuts.rs` binds the hotkey through the
  GlobalShortcuts portal (KDE, GNOME, Hyprland). A hold opens the quick menu.
  `hotkey.rs` keeps the result of the bind for the startup notice and Settings.
- **IPC**: `ipc.rs` serves a Unix socket at `$XDG_RUNTIME_DIR/sonori/control.sock`.
  The `sonori` subcommands (`toggle`, `start`, `stop`, `cancel`, `status`,
  `menu`, `settings`, `language`, and others) use it. Compositor keybindings
  (niri, Sway) call these subcommands. `ipc.rs` also detects a held key from
  repeated `toggle` calls.
- **System tray**: `system_tray.rs` implements StatusNotifierItem over D-Bus.
- **UI**: the overlay buttons, the quick menu, and the Settings window.

Each path sends a `ManualSessionCommand` to speechcore.

## Configuration

`src/config/` reads `~/.config/sonori/config.toml`, or the file in
`SONORI_CONFIG_PATH`. Missing keys get their default values. Sonori then writes
the file back with all keys, so the file shows every setting.
`migrate_legacy_keys` updates keys from older releases.

Settings and the quick menu change the config while Sonori runs. They write
only the changed field to disk and send the new config on a
`tokio::sync::watch` channel. The pipeline reads that channel, so most changes
apply without a restart. Sonori does not watch the file for edits.
`CONFIGURATION.md` lists all keys.

## Threads and tasks

- **Main thread**: the winit event loop. It draws all windows with one shared
  wgpu device (`ui/gpu_utils.rs::Gpu`).
- **Tokio tasks**: the transcript pipeline, the output worker, IPC, the
  portals, the tray, and the speechcore session loop.
- **PortAudio callback thread**: audio capture, in speechcore.
- **Blocking threads**: model inference, in speechcore.
- **Sound thread**: `sound_player.rs` plays the cues through CPAL.

Shared state follows these rules:

- `Arc<AtomicBool>` for flags: `running`, `recording`, Magic Mode.
- `Arc<RwLock<T>>` for data that the UI reads: `AudioVisualizationData`,
  `BackendStatus`, `Delivery`, the transcript history.
- Channels for work: transcripts, output jobs, app commands, config changes.

## Event loop and redraws

The UI does not draw frames while nothing changes. `about_to_wait` in
`ui/app.rs` checks the shared state on each wake-up:

- The overlay redraws while `WindowState` animates, or when the state it shows
  changes.
- The cursor overlay polls every 30 ms while it shows something.
- While nothing shows, the loop wakes every 100 ms. A session start and a
  quick-menu request wake it at once through an `EventLoopProxy`
  (`UiWaker` in `ui/cursor_overlay.rs`). So the tile appears without delay
  after the hotkey.

## Modules

### App

| File | Purpose |
|---|---|
| `main.rs` | Startup, CLI parsing, the transcript pipeline, and output delivery |
| `lib.rs` | The library surface of the app modules |
| `config/mod.rs` | `AppConfig`, file read and write, migration, conversion to `SpeechConfig` |
| `config/backend.rs` | Backend and VAD options |
| `config/theme.rs` | Window position and theme options |
| `copy.rs` | Clipboard (wl-copy), paste shortcuts, and key events (wtype, dotool) |
| `paste_check.rs` | Serves the clipboard during a paste, sees if an app took it, restores the old content |
| `input_method.rs` | Type mode through `zwp_input_method_v2` |
| `portal_input.rs` | Paste keys through the RemoteDesktop portal |
| `portal_tokens.rs` | Restore tokens for the portal sessions |
| `global_shortcuts.rs` | The hotkey through the GlobalShortcuts portal, with hold detection |
| `hotkey.rs` | Shared state of the hotkey bind |
| `ipc.rs` | The control socket and the `sonori` subcommands |
| `system_tray.rs` | StatusNotifierItem tray icon and menu |
| `doctor.rs` | `sonori doctor`: checks protocols, portals, helpers, the GPU, and microphones |
| `sound_player.rs`, `sound_generator.rs` | Sound cues for recording and session events |
| `transcript_writer.rs` | Appends transcripts to the history file |
| `enhancement/` | Magic Mode: a local llama.cpp model rewrites the transcript |

### UI (`src/ui/`)

| File | Purpose |
|---|---|
| `app.rs` | The winit application: windows, display mode, event loop, tray commands |
| `gpu_utils.rs` | The shared `Gpu` and a quad renderer |
| `cursor_overlay.rs` | Cursor mode: layer surfaces per output, tile, ring, bubble, input regions |
| `cursor_overlay/menu.rs` | The quick menu |
| `settings_window.rs` | The Settings window: state, input, and config changes |
| `settings_window/model.rs` | What each Settings control reads and writes |
| `settings_window/view.rs` | Layout, drawing, and hit tests of Settings |
| `shapes.rs`, `shapes.wgsl` | Signed-distance shapes for cursor mode and Settings |
| `text_renderer.rs` | Text through Glyphon |
| `window.rs` | The overlay window: state, frame order, redraw policy |
| `spectogram.rs` | The overlay spectrogram |
| `text_window.rs`, `text_processor.rs`, `typewriter.rs` | Overlay transcript text |
| `scroll_state.rs`, `scrollbar.rs` | Overlay scrolling |
| `buttons/`, `button_texture.rs`, `button_panel.rs` | Overlay buttons |
| `status_bar.rs`, `timer_badge.rs`, `tooltip.rs` | Overlay status, timer, and tooltips |
| `loading_animation.rs` | Overlay loading and state indicator |
| `event_handler.rs`, `layout_manager.rs`, `viewport.rs` | Overlay input and layout |
| `render_pipeline.rs` | Pipeline setup for the overlay shaders |
| `render_context.rs` | Not in use |

The overlay shaders are `button.wgsl`, `button_panel.wgsl`, `quad.wgsl`,
`rounded_rect.wgsl`, `scrollbar.wgsl`, `spectogram.wgsl`, `status_bar.wgsl`,
`text_window.wgsl`, and `tooltip.wgsl`.

### speechcore (`../speechcore/src/`)

| File | Purpose |
|---|---|
| `real_time_transcriber.rs` | The coordinator: modes, sessions, status, transcript stream |
| `audio_capture.rs` | PortAudio input and device recovery |
| `audio_processor.rs` | Real-time VAD flow and the manual session buffer |
| `silero_audio_processor.rs` | Silero VAD through ONNX Runtime |
| `transcription_processor.rs` | Transcription tasks, manual parts, cancelled sessions |
| `backend_manager.rs` | One queue for backend loads and reloads |
| `backend/` | The backends and their factory |
| `post_processor.rs` | Text cleanup |
| `download.rs` | Model downloads |
| `state.rs` | `ProcessingState` and `BackendStatus` |
| `feedback.rs` | `FeedbackSink`: session events for the app |

## Platform support

Sonori needs Wayland with `wlr-layer-shell` for its windows. Without layer
shell, Sonori stops with a clear message, and `sonori doctor` reports the
problem. KDE, Sway, Hyprland, and niri support layer shell. GNOME does not.
On X11, winit opens a normal window for the overlay. Cursor mode needs Wayland.

The README lists which compositor supports which protocol.

## Tests

- **CI** runs `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  and `cargo test`. A second workflow builds Sonori on several distributions.
- **VM tests** in `nix/vm-tests.nix` start Sonori in NixOS VMs with Sway,
  Hyprland, KDE Plasma, and GNOME. Each run saves screenshots and logs. The
  Sway run also dictates a WAV file through a virtual microphone and clicks
  with a virtual mouse. Run one with `nix build .#checks.x86_64-linux.vm-sway -L`.

## Desktop integration reference

- App ID for the XDG desktop portals: `dev.paddy.sonori`
- Desktop file: `/home/paddy/.local/share/applications/dev.paddy.sonori.desktop`
- Release binary (Exec in the desktop file): `/home/paddy/dev/rust/flashscribe/target/release/sonori`
