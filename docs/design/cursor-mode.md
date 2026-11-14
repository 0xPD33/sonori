# Cursor mode

Cursor mode is an experimental display mode. It replaces the fixed overlay with
an animated copy of the Sonori icon beside the mouse pointer and a short-lived speech bubble. It is
selected with `[ui_config] display_mode = "Cursor"`; the default stays
`"Overlay"`.

## Behavior

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
- Model: opens a list of three models. A model switch needs a second click,
  because it may download the model; the app then reloads the backend.
- Display: Cursor or Overlay.
- More settings: opens the Settings window.

The models are the ones speechcore runs that lead the Open ASR leaderboard
(paper data of 2026-03-27): Parakeet TDT v2 (mean English WER 6.05, English
only), Parakeet TDT v3 (6.32, 25 languages), and Moonshine base (smallest).
Whisper large-v3 turbo scores 7.83 and runs about 17 times slower than
Parakeet, so the menu does not offer it.

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
any monitor. All surfaces share one 8-bit non-sRGB format: one renderer draws on
all of them, and blending then matches the compositor's.

While recording, each surface takes pointer input everywhere except a square gap
(68 logical px) around the last known pointer position. Inside the gap, the
pointer is over the apps below, so they get clicks, scrolling, and hover. When
the pointer leaves the gap, it enters Sonori's region; the compositor reports
the exact position, and Sonori moves the gap under the pointer with the next
commit. niri sends `wl_pointer.enter` with the exact position when a surface
maps under a still pointer (tested on niri 2026-11-12). A fast move can briefly
toggle hover in the app below. After the session stops, the input region becomes
empty.

If no output reports the pointer within 400 ms, the tile shows at the bottom
center of the first output.

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

speechcore reports `Completed` only after the transcript of the stopped session
is out. If transcription never starts within 700 ms after recording stops, the
session counts as cancelled and the tile fades without a bubble. After
transcription, "Transcribing…" stays until every received transcript is
delivered: up to 15 s, or 60 s with Magic Mode. If no transcript arrives
within 1.5 s, the session produced no text, and the tile fades.

## Rendering

One wgpu pipeline draws instanced shapes with signed distance functions:
glass tiles, silver capsule bars, and outlines. The pipeline is in
`src/ui/shapes.rs`, and the Settings window draws with it too. Bars, tile, and bubble move on
damped springs that step with the frame time. Each shape covers only its own
quad, so a frame shades a few hundred pixels around the pointer, not the whole
output. Glyphon renders the timer and bubble text. The surface redraws every
frame while it exists and uses premultiplied alpha.

The GPU device is shared with the Settings window, so Settings opens in cursor
mode, where the fixed overlay does not exist.

## Later steps

- Insert text through `zwp_input_method_v2` (`commit_string`), so the clipboard
  stays untouched and terminals need no paste shortcut.
- Blur behind the bubble through `ext-background-effect-v1`.
- Pick the microphone in the menu. speechcore opens the input device once at
  start and cannot list devices yet.
