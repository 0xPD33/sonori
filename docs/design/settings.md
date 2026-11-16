# Settings window

The Settings window holds every setting that the quick menu does not show. It
opens from "More settings…" in the quick menu, from the tray, and with
`sonori settings`. Only one Settings window exists at a time.

## Layout

The window is a frosted glass panel of 820 × 560 logical px with a corner
radius of 22 px. The layer surface is 22 px larger on each side, so the shadow
has room. The input region covers only the glass, so presses on the shadow
reach the windows below.

- Sidebar (200 px): the Sonori logo with silver bars, six section buttons with
  line icons, and the config file path at the bottom.
- Header: the section title, a one-line subtitle, and a round close button.
- Content: the rows of the section. The mouse wheel scrolls the content, and a
  thin bar on the right shows the position.

Each row is a light card with a title, a one-line description in plain words,
and a control on the right. A long description wraps. Group titles are small
capitals above the rows that they group.

Colors come from the Sonori icon: ink `#202a39`, secondary ink `#5b6779`, edge
`#90a8c3`, slate `#3b4a60`, and the silver and glass gradients of the icon.
The window has one theme only.

## Controls

- Segmented control: a sunk track with a raised pill. The pill moves to the
  selected option on a spring.
- Switch: a sunk track that fills with slate, with a silver knob.
- Select: a sunk field with a chevron. A press opens a glass list under the
  field, or above it when no room is below. A list with more than 8 options
  scrolls.
- Slider: a sunk track with a slate fill and a silver knob. The value saves
  when the user releases the knob.
- Keycap: a read-only value, such as the hotkey or the microphone name.
- Buttons: raised, or dark slate for the main action of a group.
- Text field: a sunk field. A press focuses it, typing adds text, Backspace
  removes the last character, and Enter adds the entry.

## Sections

| Section | Rows | Config fields |
|---|---|---|
| General | Hotkey, Hotkey mode, Listening, Output, Paste shortcut, Language, Magic Mode, Sounds, Volume | hotkey state, `portal_config.shortcut_mode`, `general_config.transcription_mode`, `portal_config.output_mode`, `portal_config.paste_shortcut`, `general_config.language`, Magic Mode (runtime), `sound_config.enabled`, `sound_config.volume` |
| Speech | Three model cards; "Advanced: backend, model, GPU" with Backend, Model, Precision, Use the GPU, CPU threads | `backend_config.backend`, `general_config.model`, `backend_config.quantization_level`, `backend_config.gpu_enabled`, `backend_config.threads` |
| Microphone | Microphone, Input level, Start listening | `audio_processor_config.input_device` (read-only), live level, `vad_config.sensitivity` |
| Look | Cursor and Overlay cards; Text bubble (Cursor); Theme, Sound picture, Position, Text size, Recording dot, Typewriter effect, Clear on a new recording, Frame sync, Frame-rate cap (Overlay) | `ui_config.display_mode`, `ui_config.cursor_bubble`, `ui_config.*`, `display_config.*`, `manual_mode_config.clear_on_new_session` |
| Words | Vocabulary chips, Replacements ("heard → meant") | `whisper_cpp_options.initial_prompt`, `post_process_config.replacements` |
| Advanced | Clean-up rules, Magic Mode model, Keep a history, Config file, Tray icon, Commands | `post_process_config.*`, `enhancement_config.enabled`, `history_config.enabled`, `window_behavior_config.show_in_system_tray` |

The model cards are the quick menu presets (`PRESETS` in
`src/ui/cursor_overlay/menu.rs`): Parakeet v2, Parakeet v3, Nemotron 3.5,
Whisper turbo, and Moonshine base, in rows of three.
When the configured model is not a preset, a line under the cards names it.

The Select for Model lists every model of the backend. Moonshine, Parakeet v2,
and models whose name ends in `.en` understand English only. For them,
Language shows "English" as a read-only value. CTranslate2 takes the Whisper
languages, but has no auto detect. A change of backend or model fits the
language as the quick menu does (see `cursor-mode.md`).

"Start listening" maps onto the VAD sensitivity: "Easily" is `High`, "Normal"
is `Medium`, and "Only clear speech" is `Low`.

The vocabulary is the comma-separated `initial_prompt`. Only Whisper.cpp uses
it, so the row says so for other backends.

speechcore cannot list microphones or switch them while it runs. The Microphone
row therefore shows the configured device, or "System default", and tells the
user to edit the config file. In manual mode, the microphone runs only while
Sonori records, so the level meter moves only during a dictation.

## Saving

Each change saves at once; the window has no Apply button. For each change,
the window reads `config.toml`, changes only that field, and writes the file.
Other edits to the file stay. If the file has a syntax error, nothing saves,
and the status shows the error.

After the save, the app applies the new config at runtime, as for the quick
menu. A short glass toast in the lower-right corner confirms the save:

- "Saved": the change applies at once.
- "Saved · applies after a restart": speechcore or a startup component reads
  the field only at start (for example VAD, clean-up rules, replacements,
  sounds, and the tray).
- "Saved · reloading the model": GPU and thread changes reload the backend.

A change of model, backend, or precision can start a download. For these, a
sheet asks first: "Switch to …? Sonori downloads this model the first time,
then reloads it." After the user confirms, the window saves the config and
sends `BackendCommand::Reload` to speechcore.

Magic Mode and Listening change the running app. The Magic Mode switch sends
`AppCommand::ToggleMagicMode`, as the quick menu does. A change of Listening
saves the mode and asks the app to switch between manual and realtime mode.

When the quick menu or IPC changes the config while Settings is open, the app
gives Settings the new config, so the window shows it.

## Keyboard

The layer surface uses on-demand keyboard focus: the compositor gives it the
keyboard when the user presses in it. Escape closes an open list or sheet,
then ends text input, then closes the window. Enter confirms the sheet. Tab
moves between the two replacement fields.

## Moving the window

The window is a layer surface, because this winit fork delivers pointer
events only to layer surfaces. It opens without an anchor, so the compositor
centres it.

A press on the sidebar background or the header starts a drag; a press on a
control does not. At the start of the drag, the window anchors to the top-left
corner of the output, with margins at its centred position. Relative pointer
motion then moves the margins, with the same scale and catch-up as the
overlay drag. Without relative motion, the window follows the pointer position
on the surface. At least 120 px of the window stay on the output.

The app keeps the last position until Sonori quits and opens Settings there
again. The position does not go into the config file.

## Rendering

The window draws with the shape pipeline of cursor mode (`src/ui/shapes.rs`
and `shapes.wgsl`): glass, silver capsules, panel styles, and line icons. It
redraws only after input, while a spring or toast moves, and while the level
meter shows a recording.

A frame has two layers. The first holds the window and the content; scissor
rectangles cut off the sidebar fill and the scrolled content. The second holds
the toast, an open list, and the sheet. Each layer draws its shapes and then
its labels in one glyphon batch, and the window submits each layer before the
next, because glyphon keeps one vertex buffer.
