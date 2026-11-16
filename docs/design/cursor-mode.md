# Cursor mode

Cursor mode is the default display mode. It replaces the fixed overlay with
an animated copy of the Sonori icon beside the mouse pointer and a short-lived
speech bubble. `[ui_config] display_mode = "Overlay"` selects the fixed overlay.
Realtime mode and sessions outside Wayland always use the fixed overlay.

## Behavior

- Start: the tile and a bubble show at the bottom center of the focused output
  for 3.5 s: "Sonori is ready", and below it the bound hotkey ("Press
  LOGO+backslash to dictate") or "Run sonori toggle to dictate". The notice
  waits up to 2 s for the portal to bind the hotkey. A session that starts
  first replaces it.
- Idle: Sonori shows nothing.
- Recording: a translucent frosted glass tile (100 × 64 logical px) with nine
  polished silver capsule bars, in the style of the Sonori icon, floats
  beside the pointer: below
  and to the right unless an edge is near. It trails the pointer on a soft
  spring, also onto other outputs, and drifts slowly around its resting spot.
  At rest the bars show a small bell silhouette. While the user speaks, the
  newest 32 ms of audio is split into five bands (80-300 Hz, 300-700 Hz,
  0.7-1.5 kHz, 1.5-3 kHz, 3-6 kHz): the lowest band drives the centre bar, and
  higher bands drive mirrored pairs outwards. Loudness sets the height, the
  spectrum the shape, so vowels fill the middle and sibilants lift the edges. Levels are
  relative to the recent peak, so any microphone gain moves the bars. A glass
  pill below the tile shows the timer.
- While recording, the user keeps using the computer: clicks, scrolling, and
  hover reach the apps under the pointer. A click on a text field moves the
  focus there, and the transcript is pasted there. The hotkey, tray, and IPC
  commands start, stop, and cancel sessions.
- Processing: a soft wave runs across the bars, and the pill says
  "Transcribing…" ("Polishing…" while Magic Mode rewrites the text).
- Done: the tile edge glows briefly and the tile fades. A glass bubble shows
  the delivered text: its words fade in one after another, then a dimmer note
  says how the text was delivered (pasted, typed, copied). Then the bubble
  shrinks and flies into the pointer position of the stop, and a small light
  burst marks the spot. `cursor_bubble` sets how long the bubble stays:
  `Brief` (1.2 s plus 30 ms per word, at most 4 s), `Long` (6 s plus 60 ms per
  word, at most 15 s), or `Off` (only the burst).
- Every bubble has a close button in its top right corner. A click removes
  the bubble at once.
- A failed paste keeps its bubble until the close button or the next session
  removes it, because the text is then only in the clipboard. This applies
  also with `cursor_bubble = "Off"`. A paste that no app takes counts as
  failed: the bubble says "Not pasted — it is in the clipboard". This happens
  when no text field has focus, or when the app ignores the paste shortcut.
  The user can drag such a bubble anywhere on its output; it stays fully on
  screen. The whole bubble then takes pointer input, and the pointer shows a
  grab hand over it.
- Error: the tile shakes, gets a red outline, and the bubble shows the message
  for 4 s.
- Model loading: the bars pulse at rest height, and the pill says
  "Loading model…".

The look follows the icon: glass from #ffffff through #edf3f9 to #d1dce8 with a
curved upper reflection, bright edges, and a cool-grey shadow and hairline that
hold the edge on white pages; bars with the icon's silver gradient and vertical
reflection. Teal, cyan, gold, halos, and wings are out of this direction.

## Menu

Holding the hotkey opens a menu at the pointer instead of a dictation; so does
`sonori menu`. The menu is the same glass, drawn with the overlay's shader
(panels, switches, segmented controls, and line icons as signed distance
functions) and one batch of glyphon labels. Its rows:

- Language: opens a list in place (Auto detect, English, Deutsch, Français,
  Español, and a link to Settings for more).
- Magic Mode: a switch with a silver knob.
- Output: Paste, Type, or Copy.
- Model: opens a list of five models. A model switch needs a second click,
  because it may download the model; the app then reloads the backend.
- Display: Cursor or Overlay.
- Recent: opens a list of the last five transcripts, newest first, one line
  each. A click delivers the transcript again with the current output mode.
  The list lives in memory only. With the history off
  (`history_config.enabled = false`), Sonori keeps no list and hides the row.
- More settings: opens the Settings window.

The list gives one good model for each need:

- Parakeet TDT v2: mean English WER 6.05 on the Open ASR leaderboard (paper
  data of 2026-03-27). English only.
- Parakeet TDT v3: WER 6.32, 25 European languages.
- Nemotron 3.5: 7.91 on FLEURS English. NVIDIA names 15 languages as ready
  for transcription, Japanese, Korean, Hindi and Arabic included.
- Whisper large-v3 turbo: WER 7.83, 99 languages. It runs about 17 times
  slower than Parakeet.
- Moonshine base: the smallest and fastest. English only.

A model switch keeps the language when the new model knows it. Otherwise
Sonori picks the same language in the form that the model uses ("de" or
"de-DE"), else auto detect.

A change saves to `config.toml` through the app, so the Settings window sees
it. A highlight glides to the hovered row, and lists, the knob, and the
segment pills move on springs. A click outside the menu, or the hotkey, closes
it with a short fade. While the menu is open, the surface takes all pointer
input; during the fade, it takes none.

Compositor key bindings (such as niri's `spawn "sonori" "toggle"`) repeat
while held: one toggle, then after the key-repeat delay (about 600 ms) one
toggle about every 40 ms. Sonori treats toggles less than 200 ms apart as
repeats. A press starts dictation at once. A stop within 1.2 s of the start
waits 200 ms; if a repeat arrives, the key is held, so Sonori cancels the
session it just started and opens the menu. Hold detection applies only while
cursor mode is active.

Cursor mode applies to manual sessions only. In realtime mode, Sonori shows the
fixed overlay, because a session never ends there.

## Surfaces

On Wayland, a client learns the pointer position only while the pointer is over
one of its own surfaces. Cursor mode therefore uses one transparent full-screen
layer-shell surface on the `Overlay` layer, with all four edges anchored and an
exclusive zone of -1. Sonori creates it when a session starts and destroys it
after the bubble fades, so no full-screen surface stays mapped while idle. A
permanent full-screen layer can block direct scanout for full-screen games and
video.

Sonori opens one such surface per output, so the tile can follow the pointer to
any monitor. The first surface names no output, so the compositor puts it on
the focused output (niri, Sway, Hyprland, and KWin do so). When the compositor
reports which output the surface entered, Sonori opens the surfaces on the
other outputs. If no report comes within 1 s, Sonori stays on that one output:
a second surface on the same output would block clicks there. All surfaces share one 8-bit non-sRGB format: one renderer draws on
all of them, and blending then matches the compositor's.

While recording, each surface takes pointer input everywhere except a square gap
(68 logical px) around the last known pointer position. Inside the gap, the
pointer is over the apps below, so they get clicks, scrolling, and hover. When
the pointer leaves the gap, it enters Sonori's region; the compositor reports
the exact position, and Sonori moves the gap under the pointer with the next
commit. niri sends `wl_pointer.enter` with the exact position when a surface
maps under a still pointer (tested on niri 2026-11-12). A fast move can briefly
toggle hover in the app below. After the session stops, the input region shrinks to the bubble's close
button (the whole bubble when a failed paste pins it), or to nothing without a
bubble, so the rest of the screen stays free.

If no output reports the pointer within 400 ms, the tile shows at the bottom
center of the focused output, and the menu opens in its center. Sway, for
example, sends no `wl_pointer.enter` to a surface that maps under a still
pointer.

## Data flow

The UI polls shared state, as the fixed overlay does:

- `recording` (atomic flag) starts and ends the recording phase.
- `AudioVisualizationData::samples` gives the level for the bars: RMS of the
  latest samples, mapped from -50 dBFS to -10 dBFS onto 0..1.
- `AudioVisualizationData::processing_state` reports transcription and errors.
- `Delivery` (shared slot with a sequence number) reports each delivered
  transcript and how the output worker delivered it. The bubble shows the
  deliveries of the current session. Its `received` count grows when a final
  transcript arrives, before Magic Mode and the paste.
- `BackendStatus::last_error` supplies the error text.

## Paste check and clipboard restore

An app reads the clipboard only when it takes a paste. In Paste mode, Sonori
therefore serves the clipboard itself through `zwlr_data_control_v1` while it
sends the paste keys, and counts the reads. If no app reads the text within
1 s after the keys, the paste counts as failed, and the transcript stays in
the clipboard while Sonori runs.

Before the paste, Sonori saves the clipboard content in every type it offers:
text, rich text, images, files. After an app reads the transcript, Sonori
waits 200 ms for reads in other types, then offers the saved content again.
The user's clipboard is then as before the dictation. Sonori serves the
restored content until another client takes the selection, so it is lost when
Sonori exits first. Sonori does not restore when the content is larger than
20 MB or takes longer than 500 ms to read; the transcript then stays in the
clipboard.

The transcript offer includes `x-kde-passwordManagerHint` ("secret"). Klipper
and cliphist (through `wl-paste --watch`) then keep no history entry for it.
Reads of the hint do not count as pastes.

Sonori cannot tell in two cases, and then reports a paste:

- The compositor has no data-control protocol (GNOME). Sonori copies with
  wl-copy, as before, and does not restore.
- Another client takes the selection before the paste, for example a
  clipboard manager that keeps the clipboard alive. Sonori then does not
  restore either.

A clipboard manager that reads every new selection late, after the paste
keys, can hide a failed paste. A good paste looks failed only when the app
takes more than 1 s to read the clipboard.

## Paste shortcut

A terminal sends Ctrl+V to the program and pastes on Ctrl+Shift+V. In other
apps, Ctrl+Shift+V often does something else or nothing. With
`paste_shortcut = "auto"`, the default, Sonori asks the compositor for the
app ID of the focused window just before the paste keys. It uses
`niri msg`, `swaymsg` or `hyprctl`, with a 500 ms limit.

- A known terminal, or an app ID that contains "term", gets Ctrl+Shift+V.
- Any other app gets Ctrl+V.
- Without an answer (GNOME, KDE, no focused window), Sonori uses
  Ctrl+Shift+V.

The setting values `ctrl_shift_v` and `ctrl_v` force one shortcut for all
apps.

## Rendering

One wgpu pipeline draws instanced shapes with signed distance functions:
glass tiles, silver capsule bars, and outlines. The pipeline is in
`src/ui/shapes.rs`, and the Settings window draws with it too. Bars, tile, and bubble move on
damped springs that step with the frame time. Each shape covers only its own
quad, so a frame shades a few hundred pixels around the pointer, not the whole
output. Glyphon renders the timer and bubble text. All cursor-mode text uses the
bundled Inter (Medium and SemiBold), or the system's Inter if installed. So the
bubble has the same size on every system, and its close button stays where
the bubble text leaves room for it. The surface redraws every
frame while it exists and uses premultiplied alpha.

The GPU device is shared with the Settings window, so Settings opens in cursor
mode, where the fixed overlay does not exist.

## Type through the input method

In Type mode, Sonori inserts the text through `zwp_input_method_v2`, as an
input method does. It sends no keys and does not touch the clipboard, so
terminals need no paste shortcut, and every character arrives as written.

niri and Sway let a text field enable text input only while an input method
is bound. So Sonori binds one for each insertion and destroys it after:

1. Sonori binds the input method. The compositor tells the focused text field
   that text input is available.
2. The field enables text input, and the compositor activates Sonori's input
   method.
3. Sonori sends the text with `commit_string` and `commit`. The commit carries
   the number of `done` events received; with another number, the compositor
   discards the text. Text over 3000 bytes goes in parts, because Wayland caps
   a message at 4 KiB.

Sonori types the text with wtype or dotool, as before, in these cases:

- The compositor has no `zwp_input_method_manager_v2` (GNOME, KDE).
- Another input method runs, such as fcitx5. The compositor allows one per
  seat and answers `unavailable`.
- The focused app does not activate the input method within 300 ms. Chromium
  and Electron apps do this without `--enable-wayland-ime`.

The input method commits the text as one string. A line break in the text
reaches a terminal as Enter, as with typed keys.

## Later steps

- Blur behind the bubble through `ext-background-effect-v1`.
- Pick the microphone in the menu. speechcore opens the input device once at
  start and cannot list devices yet.
