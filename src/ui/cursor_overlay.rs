//! Cursor mode: a spectrogram ring at the mouse pointer and a short-lived bubble.
//! See `docs/design/cursor-mode.md`.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use speechcore::{AudioVisualizationData, BackendStatus, BackendStatusState, ProcessingState};
use winit::cursor::CursorIcon;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::platform::wayland::{Anchor, KeyboardInteractivity, Layer, WindowAttributesWayland};
use winit::window::{Window, WindowAttributes, WindowId};

use super::shapes::{
    glass, Shape, ShapeRenderer, Spring, CROSS, FLAT, ICON, INK, INK_2, OUTLINE, PANEL, SILVER_BAR,
};
use super::text_renderer::{Label, TextRenderer};
use crate::config::{AppConfig, CursorBubble, DisplayMode, OutputMode};
use crate::hotkey::{HotkeyState, SharedHotkeyState};

mod menu;
use crate::ipc::AppCommand;
pub(crate) use menu::PRESETS;
use menu::{MenuAction, MenuState, QuickMenu};

/// How the output worker delivered a transcript.
#[derive(Debug, Clone, PartialEq)]
pub enum DeliveryOutcome {
    Pasted,
    Typed,
    Copied,
    /// The message says what failed and where the text went.
    Failed(String),
}

/// The latest delivered transcript. `seq` grows with each delivery, so the UI
/// can tell a new delivery from one it already showed. `received` counts final
/// transcripts on their way to delivery, so the UI knows a paste is coming.
/// `live` is the text of the session that records now; `live_seq` grows with it.
#[derive(Debug, Clone, Default)]
pub struct Delivery {
    pub received: u64,
    pub seq: u64,
    pub text: String,
    pub outcome: Option<DeliveryOutcome>,
    pub live_seq: u64,
    pub live: String,
    /// Words after `live` that the model may still change.
    pub live_tentative: String,
}

impl Delivery {
    pub fn set_live(&mut self, text: String, tentative: String) {
        self.live_seq += 1;
        self.live = text;
        self.live_tentative = tentative;
    }

    pub fn record(&mut self, text: String, outcome: DeliveryOutcome) {
        self.seq += 1;
        self.text = text;
        self.outcome = Some(outcome);
    }
}

pub type SharedDelivery = Arc<RwLock<Delivery>>;

/// Lets IPC open and close the cursor-mode menu and see whether it is open,
/// and ask for the Settings window.
#[derive(Debug, Default)]
pub struct MenuControl {
    request: AtomicU8,
    open: AtomicBool,
    /// Cursor mode is active, so a held hotkey opens the menu.
    available: AtomicBool,
    settings: AtomicBool,
    waker: UiWaker,
}

/// Wakes the UI event loop from another thread, so an idle loop can sleep long.
#[derive(Debug, Default, Clone)]
pub struct UiWaker(Arc<OnceLock<EventLoopProxy>>);

impl UiWaker {
    pub fn set(&self, proxy: EventLoopProxy) {
        let _ = self.0.set(proxy);
    }

    pub fn wake(&self) {
        if let Some(proxy) = self.0.get() {
            proxy.wake_up();
        }
    }
}

const MENU_OPEN: u8 = 1;
const MENU_CLOSE: u8 = 2;

impl MenuControl {
    pub fn request_open(&self) {
        self.request.store(MENU_OPEN, Ordering::Relaxed);
        self.waker.wake();
    }

    pub fn request_close(&self) {
        self.request.store(MENU_CLOSE, Ordering::Relaxed);
        self.waker.wake();
    }

    pub fn is_open(&self) -> bool {
        self.open.load(Ordering::Relaxed)
    }

    pub fn is_available(&self) -> bool {
        self.available.load(Ordering::Relaxed)
    }

    pub fn set_available(&self, available: bool) {
        self.available.store(available, Ordering::Relaxed);
    }

    pub fn request_settings(&self) {
        self.settings.store(true, Ordering::Relaxed);
    }

    pub fn take_settings_request(&self) -> bool {
        self.settings.swap(false, Ordering::Relaxed)
    }

    pub fn waker(&self) -> &UiWaker {
        &self.waker
    }
}

pub type SharedMenuControl = Arc<MenuControl>;

/// A setting changed from the menu; the app applies and saves it.
#[derive(Debug, Clone)]
pub enum MenuChange {
    Language(String),
    Output(OutputMode),
    Bubble(CursorBubble),
    Display(DisplayMode),
    /// Needs a model reload.
    Model {
        backend: speechcore::BackendType,
        model: String,
    },
}

impl MenuChange {
    pub fn apply(&self, config: &mut AppConfig) {
        match self {
            MenuChange::Language(code) => {
                config.general_config.language = code.clone();
                crate::ui::settings_window::fit_language(config);
            }
            MenuChange::Output(mode) => config.portal_config.output_mode = *mode,
            MenuChange::Bubble(bubble) => config.ui_config.cursor_bubble = *bubble,
            MenuChange::Display(mode) => config.ui_config.display_mode = *mode,
            MenuChange::Model { backend, model } => {
                config.backend_config.backend = *backend;
                config.general_config.model = model.clone();
                crate::ui::settings_window::fit_language(config);
            }
        }
    }
}

const BARS: usize = 9;
/// Audio frames kept; the outer bars echo the centre bar a few frames later.
/// One band per bar pair, mirrored from the centre: low frequencies in the middle,
/// higher ones outwards. Ranges in Hz, chosen for speech.
const BANDS: [(f32, f32); BARS / 2 + 1] = [
    (80.0, 300.0),
    (300.0, 700.0),
    (700.0, 1500.0),
    (1500.0, 3000.0),
    (3000.0, 6000.0),
];
const FFT_SIZE: usize = 512;
/// speechcore captures at 16 kHz.
const SAMPLE_RATE: f32 = 16_000.0;
/// Tile size while recording, in logical px; it shrinks to `TILE_DONE` after the stop.
const TILE_W: f32 = 100.0;
const TILE_H: f32 = 64.0;
const TILE_DONE: f32 = 0.88;
/// Corner radius, bar width, and bar spacing relative to the tile height.
const TILE_CORNER: f32 = 0.3;
const BAR_WIDTH: f32 = 0.078;
const BAR_PITCH: f32 = 0.14;
/// Tallest height of each bar relative to the tile height: a bell, a little
/// lopsided like the icon.
const BAR_PROFILE: [f32; BARS] = [0.26, 0.38, 0.52, 0.66, 0.76, 0.64, 0.50, 0.36, 0.24];
/// Share of its profile a bar shows at rest, so a quiet tile still looks like the icon.
const BAR_REST: f32 = 0.22;
/// The middle of the pointer arrow, relative to its tip.
const POINTER_OFFSET: (f32, f32) = (7.0, 11.0);
const FADE_IN: Duration = Duration::from_millis(110);
const FADE_OUT: Duration = Duration::from_millis(220);
/// The bubble flies into the pointer, then a small burst marks where the text went.
const TELEPORT: Duration = Duration::from_millis(260);
const BURST: Duration = Duration::from_millis(260);
/// Word fade-in: each word fades over `WORD_FADE`, starting up to 30 ms after the
/// last one; all words have started within `REVEAL_SPAN`.
const WORD_FADE: Duration = Duration::from_millis(180);
const WORD_STAGGER: Duration = Duration::from_millis(30);
const REVEAL_SPAN: Duration = Duration::from_millis(500);
const TEXT_COLOR: [f32; 3] = [0.17, 0.2, 0.26];
const CANCEL_FADE: Duration = Duration::from_millis(170);
const FLASH: Duration = Duration::from_millis(480);
const BUBBLE_FADE_IN: Duration = Duration::from_millis(120);
const ERROR_SHOWN: Duration = Duration::from_secs(4);
/// Without a sign of transcription this long after the stop, the session was cancelled.
const CANCEL_DETECT: Duration = Duration::from_millis(700);
const BUBBLE_MAX_WIDTH: f32 = 420.0;
const BUBBLE_FONT_SCALE: f32 = 1.4;
const TIMER_FONT_SCALE: f32 = 1.1;
const BUBBLE_MAX_CHARS: usize = 170;
/// Lines of text in the bubble while the user speaks, and after a session that
/// showed live text. Longer text shows only its end.
const LIVE_LINES: usize = 2;
const LIVE_DONE_LINES: usize = 3;
/// Opacity of live words that the model may still change.
const TENTATIVE_ALPHA: f32 = 0.45;
/// Width kept free at the right of the bubble for its close button, in logical px.
const CLOSE_SPACE: f32 = 18.0;
/// How long the startup notice stays.
const NOTICE_SHOWN: Duration = Duration::from_millis(3500);
/// The startup notice waits this long at most for the portal to bind the hotkey.
const NOTICE_WAIT: Duration = Duration::from_secs(2);
/// Transcripts the menu's Recent list keeps.
const RECENT: usize = 5;

#[derive(Debug, Clone)]
enum Phase {
    Hidden,
    Recording {
        since: Instant,
    },
    Processing {
        since: Instant,
        transcription_seen: bool,
        /// Last time the transcriber was busy; it can go idle between chunks.
        last_busy: Instant,
    },
    Done {
        since: Instant,
        until: Instant,
    },
    Error {
        since: Instant,
        until: Instant,
    },
    Cancelled {
        since: Instant,
    },
    Menu {
        since: Instant,
    },
    /// The startup notice: the tile and a bubble that says how to dictate.
    Notice {
        since: Instant,
        until: Instant,
    },
}

/// What the bubble shows, laid out once when the text changes.
struct Bubble {
    body: String,
    /// How the text was delivered; shown dimmer, after the words.
    note: String,
    error: bool,
    size: (f32, f32),
    /// When each word of `body` starts to fade in. A word that the bubble
    /// already showed keeps its old time, so only new words fade.
    word_since: Vec<Instant>,
    note_since: Instant,
    /// Words of `body` from this index on may still change; they show dimmer.
    dim_from: usize,
}

/// A word of the session text and when the bubble first showed it.
#[derive(Clone)]
struct ShownWord {
    word: String,
    since: Instant,
}

const MAX_SHAPES: usize = 128;
/// Lowest peak the level is measured against, so room noise stays small.
const PEAK_FLOOR: f32 = 0.02;
/// Half the side of the input-region gap that follows the pointer, in logical px.
/// Inside the gap, clicks reach the apps below; leaving it reports the pointer.
const GAP: f32 = 34.0;
/// Space between the pointer and the tile edge, in logical px.
const COMPANION_GAP: f32 = 16.0;
/// Soft spring with a little overshoot: the tile trails the pointer and settles in
/// about 0.6 s.
const COMPANION_STIFFNESS: f32 = 60.0;
const COMPANION_DAMPING: f32 = 13.0;
/// The bubble trails the tile: it lags behind by the tile's velocity times
/// `BUBBLE_LAG` seconds, at most `BUBBLE_LAG_MAX` logical px, and swings back
/// on a softer spring with a small overshoot.
const BUBBLE_LAG: f32 = 0.07;
const BUBBLE_LAG_MAX: f32 = 28.0;
const BUBBLE_STIFFNESS: f32 = 110.0;
const BUBBLE_DAMPING: f32 = 12.0;
/// Without a pointer report this long into a session, the tile shows at the bottom.
const POINTER_WAIT: Duration = Duration::from_millis(400);
/// How long Sonori waits for the compositor to say which output has focus.
const FOCUS_WAIT: Duration = Duration::from_secs(1);

struct Renderer {
    shapes: ShapeRenderer,
    text: TextRenderer,
    /// Size `text` is laid out for; outputs may differ in size.
    text_size: PhysicalSize<u32>,
    format: wgpu::TextureFormat,
}

/// One full-screen layer on one output.
struct OutputSurface {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    // Dropped after `surface`, which borrows it.
    window: Arc<dyn Window>,
    /// Centre of the input-region gap, in physical px; `None` takes the whole output.
    gap: Option<(f32, f32)>,
    /// Outside recording and the menu, the only place that takes pointer input:
    /// a pinned bubble, else the bubble's close button. In physical px.
    hit: Option<[f32; 4]>,
    /// The bubble's close button, in physical px.
    close: Option<[f32; 4]>,
    /// The input region must be set again and committed with the next frame.
    region_dirty: bool,
    /// The surface showed a frame since its content last changed.
    settled: bool,
}

/// Shared state the overlay reads every frame.
pub struct CursorOverlayHandles {
    pub recording: Arc<AtomicBool>,
    pub audio_data: Arc<RwLock<AudioVisualizationData>>,
    pub backend_status: Option<Arc<RwLock<BackendStatus>>>,
    pub delivery: SharedDelivery,
    pub magic_mode_enabled: Option<Arc<AtomicBool>>,
    pub manual_session_sender: Option<tokio::sync::mpsc::Sender<speechcore::ManualSessionCommand>>,
    pub app_tx: tokio::sync::mpsc::UnboundedSender<AppCommand>,
    pub menu: SharedMenuControl,
    pub hotkey: SharedHotkeyState,
}

pub struct CursorOverlay {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    renderer: Option<Renderer>,
    surfaces: Vec<OutputSurface>,
    /// The surface on the output that has the pointer; the tile draws there.
    active: Option<WindowId>,
    /// The surface opened without an output, which the compositor puts on the
    /// focused one. Without a pointer report, Sonori shows its tile there.
    focused: Option<WindowId>,
    /// Until this time, Sonori waits to learn the focused output, then opens
    /// surfaces on the other outputs.
    others_due: Option<Instant>,
    handles: CursorOverlayHandles,
    error_color: [f32; 4],
    language: String,
    output_mode: OutputMode,
    cursor_bubble: CursorBubble,
    display_mode: DisplayMode,
    backend: speechcore::BackendType,
    model: String,
    menu: Option<QuickMenu>,
    config_changes: Vec<MenuChange>,
    settings_requested: bool,
    phase: Phase,
    /// Last pointer position on the active surface, in physical pixels.
    pointer: Option<(f32, f32)>,
    /// Tile centre, trailing the pointer; frozen when recording stops.
    anchor: Option<(f32, f32)>,
    anchor_velocity: (f32, f32),
    last_draw: Instant,
    fft: Arc<dyn rustfft::Fft<f32>>,
    fft_buffer: Vec<rustfft::num_complex::Complex<f32>>,
    /// Shape of the newest spectrum per band, 0..1 relative to the loudest band.
    spectrum: [f32; BANDS.len()],
    /// Level of the newest audio, refreshed every frame while recording.
    live: f32,
    bars: [Spring; BARS],
    tile_scale: Spring,
    tile_size: Spring,
    bubble_lift: Spring,
    /// Top left of the bubble, trailing its place beside the tile. `None` puts
    /// a new bubble there at once.
    bubble_pos: Option<(Spring, Spring)>,
    bubble_since: Instant,
    /// Loudest recent level; bars show levels relative to it, so any mic gain moves them.
    peak: f32,
    seen_seq: u64,
    /// Delivery and received counts when recording stopped.
    stop_seq: u64,
    stop_received: u64,
    session_start: Instant,
    texts: Vec<String>,
    outcome: Option<DeliveryOutcome>,
    bubble: Option<Bubble>,
    /// The session text words the bubble shows or showed, with their fade times.
    shown_words: Vec<ShownWord>,
    /// Last live text seen; a session that showed live text ends with its tail.
    live_seq: u64,
    live_used: bool,
    /// A failed paste keeps its bubble until the user closes it.
    bubble_pinned: bool,
    /// The surfaces failed to open; no retry until this recording ends.
    surface_failed: bool,
    /// Top left of a pinned bubble that the user dragged, in physical px.
    bubble_at: Option<(f32, f32)>,
    /// While the user drags the bubble: the pointer's offset from its top left.
    drag: Option<(f32, f32)>,
    /// The pointer shape last set on the surfaces.
    cursor: CursorIcon,
    close_hover: bool,
    /// When the startup notice is due; it waits briefly for the hotkey to bind.
    notice_due: Option<Instant>,
    /// Training data capture is on: the tile shows a dot while it records.
    capturing: bool,
    /// Latest transcripts, newest first, for the menu's Recent list.
    recent: std::collections::VecDeque<String>,
    /// The session whose text `recent[0]` holds; a late chunk updates it.
    recent_session: Option<Instant>,
    started: Instant,
}

/// The overlay goes away when the display mode changes, also with the menu open.
/// A stale open flag would make IPC treat every hotkey press as "close the menu".
impl Drop for CursorOverlay {
    fn drop(&mut self) {
        self.handles.menu.open.store(false, Ordering::Relaxed);
    }
}

impl CursorOverlay {
    pub fn new(
        handles: CursorOverlayHandles,
        config: &AppConfig,
        gpu: super::gpu_utils::Gpu,
    ) -> Result<Self, String> {
        let super::gpu_utils::Gpu {
            instance,
            adapter,
            device,
            queue,
        } = gpu;
        let seen_seq = handles.delivery.read().seq;
        let now = Instant::now();
        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            renderer: None,
            surfaces: Vec::new(),
            active: None,
            focused: None,
            others_due: None,
            handles,
            error_color: config.ui_config.effective_recording_indicator_color(),
            language: config.general_config.language.clone(),
            output_mode: config.portal_config.output_mode,
            cursor_bubble: config.ui_config.cursor_bubble,
            display_mode: config.ui_config.display_mode,
            backend: config.backend_config.backend,
            model: config.general_config.model.clone(),
            menu: None,
            config_changes: Vec::new(),
            settings_requested: false,
            phase: Phase::Hidden,
            pointer: None,
            anchor: None,
            anchor_velocity: (0.0, 0.0),
            last_draw: now,
            fft: rustfft::FftPlanner::new().plan_fft_forward(FFT_SIZE),
            fft_buffer: vec![rustfft::num_complex::Complex::new(0.0, 0.0); FFT_SIZE],
            spectrum: [0.0; BANDS.len()],
            live: 0.0,
            bars: [Spring::default(); BARS],
            tile_scale: Spring::at(1.0),
            tile_size: Spring::at(1.0),
            bubble_lift: Spring::default(),
            bubble_pos: None,
            bubble_since: now,
            peak: PEAK_FLOOR,
            seen_seq,
            stop_seq: seen_seq,
            stop_received: 0,
            session_start: now,
            texts: Vec::new(),
            outcome: None,
            bubble: None,
            shown_words: Vec::new(),
            live_seq: 0,
            live_used: false,
            bubble_pinned: false,
            surface_failed: false,
            bubble_at: None,
            drag: None,
            cursor: CursorIcon::Default,
            close_hover: false,
            notice_due: Some(now),
            capturing: config.training_data.enabled,
            recent: std::collections::VecDeque::new(),
            recent_session: None,
            started: now,
        })
    }

    pub fn apply_config(&mut self, config: &AppConfig) {
        self.error_color = config.ui_config.effective_recording_indicator_color();
        self.language = config.general_config.language.clone();
        self.output_mode = config.portal_config.output_mode;
        self.cursor_bubble = config.ui_config.cursor_bubble;
        self.display_mode = config.ui_config.display_mode;
        self.backend = config.backend_config.backend;
        self.model = config.general_config.model.clone();
        self.capturing = config.training_data.enabled;
    }

    /// Settings the menu changed since the last call.
    pub fn take_config_changes(&mut self) -> Vec<MenuChange> {
        std::mem::take(&mut self.config_changes)
    }

    /// Whether the menu asked to open the Settings window since the last call.
    pub fn take_settings_request(&mut self) -> bool {
        std::mem::take(&mut self.settings_requested)
    }

    pub fn owns(&self, window_id: WindowId) -> bool {
        self.surfaces.iter().any(|s| s.window.id() == window_id)
    }

    /// No session, menu, or bubble is on screen.
    pub fn is_hidden(&self) -> bool {
        matches!(self.phase, Phase::Hidden)
    }

    /// Follows the session state. Called on every event-loop wake-up.
    pub fn poll(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.advance(Instant::now());
        let wants_surfaces = !matches!(self.phase, Phase::Hidden);
        if wants_surfaces && self.surfaces.is_empty() {
            if let Err(e) = self.open_surfaces(event_loop) {
                eprintln!("Cursor overlay: {e}");
                self.surfaces.clear();
                self.surface_failed = true;
                self.enter(Phase::Hidden);
            }
        } else if !wants_surfaces && !self.surfaces.is_empty() {
            self.surfaces.clear();
            self.active = None;
            self.focused = None;
            self.others_due = None;
            self.pointer = None;
            self.anchor = None;
        }
        if let Err(e) = self.open_other_surfaces(event_loop) {
            eprintln!("Cursor overlay: {e}");
        }
        let still = self.still(Instant::now());
        for output in &self.surfaces {
            if (self.active == Some(output.window.id()) && !still) || !output.settled {
                output.window.request_redraw();
            }
        }
    }

    pub fn handle_event(&mut self, window_id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::PointerEntered { position, .. }
            | WindowEvent::PointerMoved { position, .. } => {
                self.pointer_seen(window_id, (position.x as f32, position.y as f32));
            }
            WindowEvent::PointerLeft { .. } => self.set_close_hover(false),
            WindowEvent::PointerButton {
                state: winit::event::ElementState::Pressed,
                ..
            } if matches!(self.phase, Phase::Menu { .. }) => self.menu_click(window_id),
            WindowEvent::PointerButton {
                state: winit::event::ElementState::Pressed,
                ..
            } if self.close_hover => self.dismiss_bubble(Instant::now()),
            WindowEvent::PointerButton {
                state: winit::event::ElementState::Pressed,
                ..
            } => self.start_drag(window_id),
            WindowEvent::PointerButton {
                state: winit::event::ElementState::Released,
                ..
            } => {
                if self.drag.take().is_some() {
                    self.set_cursor(window_id, CursorIcon::Grab);
                }
            }
            WindowEvent::SurfaceResized(size) => self.resize(window_id, size),
            WindowEvent::RedrawRequested => {
                self.advance(Instant::now());
                self.draw(window_id);
                let still = self.still(Instant::now());
                if let Some(output) = self.surfaces.iter().find(|s| s.window.id() == window_id) {
                    if (self.active == Some(window_id) && !still) || !output.settled {
                        output.window.request_redraw();
                    }
                }
            }
            // The compositor closed the surface (e.g. its output went away).
            WindowEvent::CloseRequested => {
                self.surfaces.retain(|s| s.window.id() != window_id);
                if self.focused == Some(window_id) {
                    self.focused = None;
                }
                if self.active == Some(window_id) {
                    self.active = None;
                    self.pointer = None;
                }
            }
            _ => {}
        }
    }

    /// The pointer left the gap (or reached this output). While recording, the gap
    /// moves under it, so the pointer is over the apps again and clicks reach them.
    fn pointer_seen(&mut self, window_id: WindowId, position: (f32, f32)) {
        if let (Phase::Menu { .. }, Some(menu)) = (&self.phase, self.menu.as_mut()) {
            self.pointer = Some(position);
            menu.pointer(window_id, position);
            if menu.window() == Some(window_id) {
                self.active = Some(window_id);
            }
            return;
        }
        if let Some((ox, oy)) = self.drag {
            self.pointer = Some(position);
            self.move_bubble(window_id, (position.0 - ox, position.1 - oy));
            return;
        }
        let (close, hit) = self
            .surfaces
            .iter()
            .find(|s| s.window.id() == window_id)
            .map_or((None, None), |s| (s.close, s.hit));
        let over_close = close.is_some_and(|[x, y, w, h]| {
            position.0 >= x && position.0 < x + w && position.1 >= y && position.1 < y + h
        });
        self.set_close_hover(over_close);
        let draggable = self.bubble_pinned && !over_close && hit.is_some();
        self.set_cursor(
            window_id,
            if draggable {
                CursorIcon::Grab
            } else {
                CursorIcon::Default
            },
        );
        let switched = self.active != Some(window_id);
        self.active = Some(window_id);
        self.pointer = Some(position);
        if switched {
            self.anchor = None;
        }
        if !matches!(self.phase, Phase::Recording { .. }) {
            return;
        }
        for output in &mut self.surfaces {
            let gap = (output.window.id() == window_id).then_some(position);
            if output.gap != gap {
                output.gap = gap;
                output.region_dirty = true;
                output.settled = false;
                output.window.request_redraw();
            }
        }
    }

    /// A press on a pinned bubble starts a drag. The compositor keeps sending
    /// motion to this surface until the release, also outside its input region.
    fn start_drag(&mut self, window_id: WindowId) {
        let rect = self
            .surfaces
            .iter()
            .find(|s| s.window.id() == window_id)
            .and_then(|s| s.hit);
        if let (true, Some([x, y, ..]), Some((px, py))) = (self.bubble_pinned, rect, self.pointer) {
            self.drag = Some((px - x, py - y));
            self.set_cursor(window_id, CursorIcon::Grabbing);
        }
    }

    /// Moves the dragged bubble, kept fully on the output.
    fn move_bubble(&mut self, window_id: WindowId, (x, y): (f32, f32)) {
        let Some(output) = self.surfaces.iter().find(|s| s.window.id() == window_id) else {
            return;
        };
        let size = output.window.surface_size();
        let (w, h) = self.bubble.as_ref().map_or((0.0, 0.0), |b| b.size);
        self.bubble_at = Some((
            x.clamp(0.0, (size.width as f32 - w).max(0.0)),
            y.clamp(0.0, (size.height as f32 - h).max(0.0)),
        ));
        output.window.request_redraw();
    }

    fn set_cursor(&mut self, window_id: WindowId, cursor: CursorIcon) {
        if self.cursor == cursor {
            return;
        }
        self.cursor = cursor;
        if let Some(output) = self.surfaces.iter().find(|s| s.window.id() == window_id) {
            output.window.set_cursor(cursor.into());
        }
    }

    /// The surfaces take all pointer input while recording, so a grab shape must
    /// not outlive the bubble.
    fn reset_cursor(&mut self) {
        if self.cursor != CursorIcon::Default {
            self.cursor = CursorIcon::Default;
            for output in &self.surfaces {
                output.window.set_cursor(CursorIcon::Default.into());
            }
        }
    }

    fn enter(&mut self, phase: Phase) {
        let capture = |p: &Phase| matches!(p, Phase::Recording { .. } | Phase::Menu { .. });
        let was_capturing = capture(&self.phase);
        let capturing = capture(&phase);
        self.phase = phase;
        self.handles
            .menu
            .open
            .store(matches!(self.phase, Phase::Menu { .. }), Ordering::Relaxed);
        if !matches!(self.phase, Phase::Menu { .. }) {
            self.menu = None;
        }
        // The menu takes the whole output, so the recording gap must go too.
        if capturing != was_capturing || matches!(self.phase, Phase::Menu { .. }) {
            for output in &mut self.surfaces {
                output.gap = None;
                output.region_dirty = true;
                output.settled = false;
                output.window.request_redraw();
            }
        }
    }

    /// While recording, the surface takes pointer input everywhere except the gap;
    /// with the menu open, everywhere; otherwise nowhere. The region applies with
    /// the next commit.
    fn apply_region(output: &mut OutputSurface, capture: bool) {
        if !output.region_dirty {
            return;
        }
        output.region_dirty = false;
        if !capture {
            let rect = output.hit.and_then(|rect| {
                let wayland = output
                    .window
                    .cast_ref::<winit::platform::wayland::Window>()?;
                Some((wayland, rect))
            });
            match rect
                .and_then(|(wayland, rect)| Some((wayland, wayland.create_region().ok()?, rect)))
            {
                Some((wayland, region, [x, y, w, h])) => {
                    let sf = output.window.scale_factor() as f32;
                    region.add(
                        (x / sf).floor() as i32,
                        (y / sf).floor() as i32,
                        (w / sf).ceil() as i32,
                        (h / sf).ceil() as i32,
                    );
                    wayland.set_region(Some(&region));
                }
                None => {
                    let _ = output.window.set_cursor_hittest(false);
                }
            }
            return;
        }
        let Some(wayland) = output.window.cast_ref::<winit::platform::wayland::Window>() else {
            let _ = output.window.set_cursor_hittest(true);
            return;
        };
        let Some((x, y)) = output.gap else {
            wayland.set_region(None);
            return;
        };
        let Ok(region) = wayland.create_region() else {
            return;
        };
        let sf = output.window.scale_factor() as f32;
        let size = output.window.surface_size();
        region.add(
            0,
            0,
            (size.width as f32 / sf).ceil() as i32,
            (size.height as f32 / sf).ceil() as i32,
        );
        region.subtract(
            (x / sf - GAP).round() as i32,
            (y / sf - GAP).round() as i32,
            (2.0 * GAP) as i32,
            (2.0 * GAP) as i32,
        );
        wayland.set_region(Some(&region));
    }

    /// Moves the phase on from the shared session state.
    fn advance(&mut self, now: Instant) {
        let recording = self.handles.recording.load(Ordering::Relaxed);
        if !recording {
            self.surface_failed = false;
        }
        let processing_state = self.handles.audio_data.read().processing_state;
        match self.handles.menu.request.swap(0, Ordering::Relaxed) {
            MENU_OPEN => {
                self.enter(Phase::Menu { since: now });
                let mut menu = QuickMenu::new(now);
                // Open at once where the pointer was last seen, without waiting for motion.
                if let (Some(window_id), Some(position)) = (self.active, self.pointer) {
                    menu.pointer(window_id, position);
                }
                self.menu = Some(menu);
            }
            MENU_CLOSE if matches!(self.phase, Phase::Menu { .. }) => self.close_menu(now),
            _ => {}
        }
        self.collect_deliveries(now);
        self.collect_live_text();
        self.show_notice_when_due(now);

        match self.phase.clone() {
            Phase::Recording { .. } if !recording => {
                self.stop_seq = self.seen_seq;
                self.stop_received = self.handles.delivery.read().received;
                self.enter(Phase::Processing {
                    since: now,
                    transcription_seen: false,
                    last_busy: now,
                });
            }
            Phase::Recording { .. } => {}
            // A cancel from the overlay fades out even if the flag still lags.
            Phase::Cancelled { since } => {
                if now >= since + CANCEL_FADE {
                    self.enter(Phase::Hidden);
                }
            }
            // The session the held hotkey started may still be winding down.
            Phase::Menu { since } => {
                if recording && now >= since + Duration::from_millis(1500) {
                    self.start_session(now);
                } else if self.menu.as_ref().is_none_or(|menu| menu.closed(now)) {
                    self.enter(Phase::Hidden);
                }
            }
            _ if recording && !self.surface_failed => self.start_session(now),
            Phase::Hidden => {}
            Phase::Processing {
                since,
                mut transcription_seen,
                mut last_busy,
            } => {
                // Only `Transcribing` counts: `Completed` can linger from the last session.
                let transcribing = processing_state == ProcessingState::Transcribing;
                if transcribing {
                    transcription_seen = true;
                    last_busy = now;
                }
                let idle = transcription_seen && !transcribing;
                // A transcript arrived that is not pasted yet (Magic Mode, a slow paste).
                let received = self.handles.delivery.read().received - self.stop_received;
                let delivered = self.seen_seq - self.stop_seq;
                let pending = received > delivered;
                if processing_state == ProcessingState::Error {
                    let message = self
                        .recent_error()
                        .unwrap_or_else(|| "Transcription failed".to_string());
                    self.show_error(message, now);
                } else if idle && !pending && delivered > 0 {
                    // Transcribed and pasted.
                    self.show_result(now);
                } else if idle && now >= last_busy + self.delivery_wait(pending) {
                    // Idle this long with nothing delivered: silence. Leave quietly.
                    if self.texts.is_empty() {
                        self.enter(Phase::Cancelled { since: now });
                    } else {
                        self.show_result(now);
                    }
                } else if !transcription_seen && now >= since + CANCEL_DETECT {
                    self.enter(Phase::Cancelled { since: now });
                } else {
                    self.phase = Phase::Processing {
                        since,
                        transcription_seen,
                        last_busy,
                    };
                }
            }
            Phase::Done { since, until } => {
                if self.bubble_pinned {
                    self.phase = Phase::Done {
                        since,
                        until: until.max(now + Duration::from_millis(100)),
                    };
                } else if now >= until + TELEPORT + BURST {
                    self.enter(Phase::Hidden);
                }
            }
            Phase::Notice { until, .. } => {
                if now >= until + FADE_OUT {
                    self.enter(Phase::Hidden);
                }
            }
            Phase::Error { until, .. } => {
                if now >= until + FADE_OUT {
                    self.enter(Phase::Hidden);
                }
            }
        }
    }

    fn start_session(&mut self, now: Instant) {
        self.session_start = now;
        self.texts.clear();
        self.outcome = None;
        self.bubble = None;
        self.shown_words.clear();
        // Only live text that arrives after the start belongs to this session.
        self.live_seq = self.handles.delivery.read().live_seq;
        self.live_used = false;
        self.bubble_pinned = false;
        self.bubble_at = None;
        self.drag = None;
        self.reset_cursor();
        self.notice_due = None;
        self.spectrum = [0.0; BANDS.len()];
        self.live = 0.0;
        self.bars = [Spring::default(); BARS];
        self.tile_scale = Spring::at(0.72);
        self.tile_size = Spring::at(1.0);
        self.peak = PEAK_FLOOR;
        self.enter(Phase::Recording { since: now });
    }

    /// Long recordings deliver finished chunks while recording, so collect from the start.
    fn collect_deliveries(&mut self, now: Instant) {
        let delivery = self.handles.delivery.read();
        if delivery.seq == self.seen_seq {
            return;
        }
        self.seen_seq = delivery.seq;
        let in_session = !matches!(self.phase, Phase::Hidden | Phase::Cancelled { .. });
        if !in_session {
            return;
        }
        if !delivery.text.is_empty() {
            self.texts.push(delivery.text.clone());
        }
        self.outcome = delivery.outcome.clone();
        drop(delivery);
        // A late chunk (e.g. after Magic Mode) extends a bubble already shown.
        if matches!(self.phase, Phase::Done { .. }) {
            self.show_result(now);
        }
    }

    /// How long after transcription Sonori waits for the paste. Without a
    /// transcript on its way, the session produced no text.
    fn delivery_wait(&self, pending: bool) -> Duration {
        match (pending, self.magic_mode_active()) {
            (false, _) => Duration::from_millis(1500),
            (true, false) => Duration::from_secs(15),
            (true, true) => Duration::from_secs(60),
        }
    }

    fn recent_error(&self) -> Option<String> {
        let status = self.handles.backend_status.as_ref()?.read();
        let (message, at) = status.last_error.as_ref()?;
        (*at >= self.session_start).then(|| message.clone())
    }

    /// Shows the newest live text while the session records or transcribes.
    fn collect_live_text(&mut self) {
        if !matches!(
            self.phase,
            Phase::Recording { .. } | Phase::Processing { .. }
        ) {
            return;
        }
        let delivery = self.handles.delivery.read();
        if delivery.live_seq == self.live_seq {
            return;
        }
        self.live_seq = delivery.live_seq;
        let text = delivery.live.clone();
        let tentative = delivery.live_tentative.clone();
        drop(delivery);
        self.live_used = true;
        self.show_text(&text, &tentative, String::new(), false, LIVE_LINES);
    }

    /// Lays out the end of `text` and `tentative` in the bubble, at most
    /// `lines` lines. Words the bubble already showed keep their place and do
    /// not fade again; new and changed words fade in. `tentative` shows dimmer.
    fn show_text(&mut self, text: &str, tentative: &str, note: String, error: bool, lines: usize) {
        let now = Instant::now();
        let stable = text.split_whitespace().count();
        let words: Vec<&str> = text
            .split_whitespace()
            .chain(tentative.split_whitespace())
            .collect();
        let kept = align_words(&self.shown_words, &words);
        let from = self.tail_start(&words, lines);
        let offset = usize::from(from > 0);
        let ellipsis = if from > 0 { "\u{2026} " } else { "" };
        let body = format!("{ellipsis}{}", words[from..].join(" "));
        // The ellipsis shows at once; it only marks the cut.
        let mut known: Vec<Option<Instant>> = Vec::new();
        if from > 0 {
            known.push(Some(self.bubble_since));
        }
        known.extend_from_slice(&kept[from..]);
        self.set_bubble_with(body, note, error, &known);
        let since = match &mut self.bubble {
            Some(bubble) => {
                bubble.dim_from = offset + stable.saturating_sub(from);
                bubble.word_since.clone()
            }
            None => Vec::new(),
        };
        self.shown_words = words
            .iter()
            .enumerate()
            .map(|(i, word)| ShownWord {
                word: word.to_string(),
                since: if i < from {
                    kept[i].unwrap_or(now)
                } else {
                    since.get(i - from + offset).copied().unwrap_or(now)
                },
            })
            .collect();
    }

    /// The first word of the longest end of `words` that fits in `lines` lines.
    fn tail_start(&mut self, words: &[&str], lines: usize) -> usize {
        let limit = self.measure_bubble(&vec!["A"; lines].join("\n")).1 + 0.5;
        let mut from = words.len();
        while from > 0 {
            let ellipsis = if from > 1 { "\u{2026} " } else { "" };
            let candidate = format!("{ellipsis}{}", words[from - 1..].join(" "));
            if self.measure_bubble(&candidate).1 > limit {
                break;
            }
            from -= 1;
        }
        // At least the last word, also when it alone is wider than the bubble.
        from.min(words.len().saturating_sub(1))
    }

    fn show_result(&mut self, now: Instant) {
        let text = self.texts.join(" ");
        if text.is_empty() {
            self.enter(Phase::Cancelled { since: now });
            return;
        }
        self.remember(&text);
        let words = text.split_whitespace().count() as u64;
        let failed = matches!(self.outcome, Some(DeliveryOutcome::Failed(_)));
        self.bubble_pinned = failed;
        // A failed paste always shows: the text is only in the clipboard.
        if self.cursor_bubble == CursorBubble::Off && !failed {
            self.bubble = None;
            self.phase = Phase::Done {
                since: now,
                until: now,
            };
            return;
        }
        let (note, error) = match &self.outcome {
            Some(DeliveryOutcome::Pasted) | None => ("Pasted".to_string(), false),
            Some(DeliveryOutcome::Typed) => ("Typed".to_string(), false),
            Some(DeliveryOutcome::Copied) => ("Copied".to_string(), false),
            Some(DeliveryOutcome::Failed(message)) => (message.clone(), true),
        };
        let unit = if words == 1 { "word" } else { "words" };
        let note = format!("{note} · {words} {unit}");
        if self.live_used {
            self.show_text(&text, "", note, error, LIVE_DONE_LINES);
        } else {
            self.set_bubble(shorten(&text, BUBBLE_MAX_CHARS), note, error);
        }
        let since = match self.phase {
            Phase::Done { since, .. } => since,
            _ => now,
        };
        let life = match self.cursor_bubble {
            CursorBubble::Long => Duration::from_millis((6000 + 60 * words).min(15_000)),
            _ => Duration::from_millis((1200 + 30 * words).min(4000)),
        };
        self.phase = Phase::Done {
            since,
            until: now + life,
        };
    }

    fn show_error(&mut self, message: String, now: Instant) {
        self.set_bubble(message, String::new(), true);
        self.enter(Phase::Error {
            since: now,
            until: now + ERROR_SHOWN,
        });
    }

    /// Keeps the session text for the menu's Recent list, in memory only.
    fn remember(&mut self, text: &str) {
        if self.recent_session == Some(self.session_start) {
            self.recent.pop_front();
        }
        self.recent.retain(|t| t != text);
        self.recent.push_front(text.to_string());
        self.recent.truncate(RECENT);
        self.recent_session = Some(self.session_start);
    }

    /// Shows the startup notice once the hotkey is bound, or after a short wait.
    /// A session that starts first makes it unneeded.
    fn show_notice_when_due(&mut self, now: Instant) {
        let Some(due) = self.notice_due else {
            return;
        };
        if !matches!(self.phase, Phase::Hidden) {
            self.notice_due = None;
            return;
        }
        let hotkey = self.handles.hotkey.read().clone();
        if matches!(hotkey, HotkeyState::Pending) && now < due + NOTICE_WAIT {
            return;
        }
        self.notice_due = None;
        let hint = match hotkey {
            HotkeyState::Bound { trigger } => format!("Press {trigger} to dictate"),
            _ => "Run sonori toggle to dictate".to_string(),
        };
        println!("Cursor mode: showing the startup notice");
        self.set_bubble("Sonori is ready".to_string(), hint, false);
        self.enter(Phase::Notice {
            since: now,
            until: now + NOTICE_SHOWN,
        });
    }

    /// The close button: the bubble goes at once, also a pinned one.
    fn dismiss_bubble(&mut self, now: Instant) {
        println!("Cursor mode: bubble closed");
        self.bubble_pinned = false;
        self.drag = None;
        self.reset_cursor();
        self.set_close_hover(false);
        self.phase = match self.phase {
            Phase::Done { since, .. } => Phase::Done { since, until: now },
            Phase::Error { since, .. } => Phase::Error { since, until: now },
            Phase::Notice { since, .. } => Phase::Notice { since, until: now },
            ref other => other.clone(),
        };
    }

    fn set_close_hover(&mut self, hover: bool) {
        if self.close_hover == hover {
            return;
        }
        self.close_hover = hover;
        for output in &mut self.surfaces {
            output.settled = false;
            output.window.request_redraw();
        }
    }

    /// A pinned bubble that finished its entrance draws nothing new until the
    /// pointer moves over its close button.
    fn still(&self, now: Instant) -> bool {
        self.bubble_pinned
            && !self.close_hover
            && self.drag.is_none()
            && now >= self.bubble_since + REVEAL_SPAN + Duration::from_secs(1)
    }

    fn set_bubble(&mut self, body: String, note: String, error: bool) {
        self.set_bubble_with(body, note, error, &[]);
    }

    /// `known` gives the fade start of words the bubble already showed, by index
    /// in `body`; the other words fade in one after another.
    fn set_bubble_with(
        &mut self,
        body: String,
        note: String,
        error: bool,
        known: &[Option<Instant>],
    ) {
        let now = Instant::now();
        if self.bubble.is_none() {
            self.bubble_at = None;
            self.bubble_pos = None;
            self.bubble_since = now;
            self.bubble_lift = Spring::at(6.0);
        }
        let text = if note.is_empty() {
            body.clone()
        } else {
            format!("{body}\n{note}")
        };
        let size = self.measure_bubble(&text);
        let count = body.split_inclusive(char::is_whitespace).count();
        let fresh = (0..count)
            .filter(|&i| known.get(i).copied().flatten().is_none())
            .count();
        let stagger = WORD_STAGGER.min(REVEAL_SPAN / fresh.max(1) as u32);
        let start = now + Duration::from_millis(80);
        let mut next = start;
        let word_since = (0..count)
            .map(|i| match known.get(i).copied().flatten() {
                Some(since) => since,
                None => {
                    let since = next;
                    next += stagger;
                    since
                }
            })
            .collect();
        self.bubble = Some(Bubble {
            body,
            note,
            error,
            size,
            word_since,
            note_since: next + Duration::from_millis(100),
            dim_from: usize::MAX,
        });
    }

    fn scale(&self) -> f32 {
        self.surfaces
            .iter()
            .find(|s| Some(s.window.id()) == self.active)
            .or(self.surfaces.first())
            .map_or(1.0, |s| s.window.scale_factor() as f32)
    }

    fn measure_bubble(&mut self, text: &str) -> (f32, f32) {
        let sf = self.scale();
        let pad = (13.0 * sf, 10.0 * sf);
        // Measured again on the first frame that has a renderer.
        let Some(renderer) = self.renderer.as_mut() else {
            return (0.0, 0.0);
        };
        let scale = BUBBLE_FONT_SCALE * sf;
        let close = CLOSE_SPACE * sf;
        let widest = text
            .lines()
            .map(|line| renderer.text.measure_text(line, scale))
            .fold(0.0, f32::max);
        let inner = (widest + 2.0).min(BUBBLE_MAX_WIDTH * sf - 2.0 * pad.0 - close);
        let height = renderer
            .text
            .text_height(text, scale, [1.0; 4], text_area_width(inner));
        (inner + 2.0 * pad.0 + close, height + 2.0 * pad.1)
    }

    /// One layer per output, so the ring can follow the pointer to any monitor.
    /// The first layer names no output, so the compositor puts it on the focused
    /// one; the others follow once Sonori knows which output that is.
    fn open_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) -> Result<(), String> {
        self.open_surface(event_loop, None)?;
        self.focused = self.surfaces.last().map(|s| s.window.id());
        if event_loop.available_monitors().nth(1).is_some() {
            self.others_due = Some(Instant::now() + FOCUS_WAIT);
        }
        Ok(())
    }

    /// Opens a layer on each output but the focused one, once the compositor
    /// says which output the first layer entered.
    fn open_other_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) -> Result<(), String> {
        let Some(due) = self.others_due else {
            return Ok(());
        };
        let entered = self
            .surfaces
            .iter()
            .find(|s| Some(s.window.id()) == self.focused)
            .and_then(|s| s.window.current_monitor());
        let Some(entered) = entered else {
            if Instant::now() >= due {
                self.others_due = None;
                // A second layer on the focused output would block clicks there.
                return Err("the compositor did not say which output has focus; \
                            Sonori shows on that output only"
                    .to_string());
            }
            return Ok(());
        };
        self.others_due = None;
        println!(
            "Cursor mode: the focused output is {}",
            entered.name().as_deref().unwrap_or("unnamed")
        );
        let entered = entered.native_id();
        let others: Vec<u64> = event_loop
            .available_monitors()
            .map(|m| m.native_id())
            .filter(|&id| id != entered)
            .collect();
        for output in others {
            self.open_surface(event_loop, Some(output))?;
        }
        Ok(())
    }

    fn open_surface(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        output: Option<u64>,
    ) -> Result<(), String> {
        let mut attrs = WindowAttributes::default()
            .with_title("Sonori Cursor")
            .with_decorations(false)
            .with_transparent(true)
            // Zero means "fill the anchored edges".
            .with_surface_size(LogicalSize::new(0, 0))
            .with_cursor(CursorIcon::Default);
        let mut wayland = WindowAttributesWayland::default()
            .with_layer_shell()
            .with_layer(Layer::Overlay)
            .with_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT)
            .with_exclusive_zone(-1)
            .with_keyboard_interactivity(KeyboardInteractivity::None);
        if let Some(output) = output {
            wayland = wayland.with_output(output);
        }
        attrs = attrs.with_platform_attributes(Box::new(wayland));

        let window: Arc<dyn Window> = Arc::from(
            event_loop
                .create_window(attrs)
                .map_err(|e| format!("failed to create the layer surface: {e}"))?,
        );
        let surface = self
            .instance
            .create_surface(window.clone())
            .map_err(|e| format!("failed to create a GPU surface: {e}"))?;
        let caps = surface.get_capabilities(&self.adapter);
        // One renderer draws on every output, so all surfaces share its format.
        // Non-sRGB, so blending matches the compositor's and colours are sRGB values.
        let format = self
            .renderer
            .as_ref()
            .map(|r| r.format)
            .into_iter()
            .chain([
                wgpu::TextureFormat::Bgra8Unorm,
                wgpu::TextureFormat::Rgba8Unorm,
            ])
            .find(|f| caps.formats.contains(f))
            .ok_or_else(|| format!("no 8-bit surface format among {:?}", caps.formats))?;
        let alpha_mode = [
            wgpu::CompositeAlphaMode::PreMultiplied,
            wgpu::CompositeAlphaMode::PostMultiplied,
            wgpu::CompositeAlphaMode::Inherit,
        ]
        .into_iter()
        .find(|m| caps.alpha_modes.contains(m))
        .unwrap_or(caps.alpha_modes[0]);
        let size = window.surface_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&self.device, &config);

        if self.renderer.as_ref().is_none_or(|r| r.format != format) {
            if !self.surfaces.is_empty() {
                return Err(format!(
                    "outputs need different surface formats ({format:?})"
                ));
            }
            self.renderer = Some(Renderer::new(&self.device, &self.queue, format, size));
        }
        window.request_redraw();
        self.surfaces.push(OutputSurface {
            surface,
            config,
            window,
            gap: None,
            hit: None,
            close: None,
            region_dirty: true,
            settled: false,
        });
        Ok(())
    }

    fn resize(&mut self, window_id: WindowId, size: PhysicalSize<u32>) {
        let Some(output) = self
            .surfaces
            .iter_mut()
            .find(|s| s.window.id() == window_id)
        else {
            return;
        };
        if size.width == 0 || size.height == 0 {
            return;
        }
        output.config.width = size.width;
        output.config.height = size.height;
        output.surface.configure(&self.device, &output.config);
        output.region_dirty = true;
        output.settled = false;
    }

    /// Advances bar, ring, and bubble motion by `dt` seconds.
    fn update_motion(&mut self, now: Instant, dt: f32, recording: bool) {
        self.live = if recording { self.mic_level() } else { 0.0 };
        if recording {
            self.update_spectrum();
        }
        let t = now.duration_since(self.started).as_secs_f32();
        let loading = self.backend_loading();
        for i in 0..BARS {
            let fi = i as f32;
            let target = match self.phase {
                Phase::Recording { .. } if loading => BAR_REST * (0.7 + 0.3 * (2.5 * t).sin()),
                Phase::Recording { .. } => {
                    // Loudness sets the height, the spectrum sets the shape. A faint sway
                    // keeps the two halves of a pair from moving in lockstep.
                    let shape = self.spectrum[i.abs_diff(BARS / 2)];
                    let sway = 0.96 + 0.04 * (2.3 * t + 1.9 * fi).sin();
                    BAR_REST + (1.0 - BAR_REST) * self.live * shape * sway
                }
                // A soft wave runs across the bars while Sonori works.
                Phase::Processing { .. } => {
                    BAR_REST * 0.6 + 0.3 * (5.0 * t - 0.7 * fi).sin().max(0.0)
                }
                _ => BAR_REST,
            };
            let bar = &mut self.bars[i];
            // Quick, slightly springy rise; calm, critically damped fall.
            if target > bar.x {
                bar.step(target, 520.0, 30.0, dt);
            } else {
                bar.step(target, 150.0, 24.5, dt);
            }
            bar.x = bar.x.max(0.0);
        }
        let size = if recording { 1.0 } else { TILE_DONE };
        self.tile_size.step(size, 380.0, 28.0, dt);
        self.tile_scale.step(1.0, 380.0, 28.0, dt);
        self.bubble_lift.step(0.0, 300.0, 28.0, dt);
    }

    /// Energy per band of the newest 32 ms, relative to the loudest band. A tilt of
    /// +3 dB per octave offsets the natural fall-off of speech, so high bands move too.
    fn update_spectrum(&mut self) {
        {
            let data = self.handles.audio_data.read();
            let tail = &data.samples[data.samples.len().saturating_sub(FFT_SIZE)..];
            let n = tail.len();
            for (i, slot) in self.fft_buffer.iter_mut().enumerate() {
                let sample = if i < n { tail[i] } else { 0.0 };
                let hann =
                    0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / (FFT_SIZE - 1) as f32).cos();
                *slot = rustfft::num_complex::Complex::new(sample * hann, 0.0);
            }
        }
        self.fft.process(&mut self.fft_buffer);
        let bin_hz = SAMPLE_RATE / FFT_SIZE as f32;
        let mut energy = [0.0f32; BANDS.len()];
        for (band, (lo, hi)) in BANDS.iter().enumerate() {
            let first = (lo / bin_hz).round() as usize;
            let last = ((hi / bin_hz).round() as usize).clamp(first + 1, FFT_SIZE / 2);
            let bins = &self.fft_buffer[first..last];
            let mean = bins.iter().map(|c| c.norm()).sum::<f32>() / bins.len() as f32;
            let centre = (lo * hi).sqrt();
            energy[band] = mean * (centre / 300.0).sqrt();
        }
        let loudest = energy.iter().copied().fold(1e-9, f32::max);
        for (shape, e) in self.spectrum.iter_mut().zip(energy) {
            *shape = (e / loudest).powf(0.6);
        }
    }

    /// RMS of the newest samples relative to the recent peak, with a noise gate.
    fn mic_level(&mut self) -> f32 {
        let rms = {
            let data = self.handles.audio_data.read();
            let tail = &data.samples[data.samples.len().saturating_sub(512)..];
            if tail.is_empty() {
                return 0.0;
            }
            (tail.iter().map(|s| s * s).sum::<f32>() / tail.len() as f32).sqrt()
        };
        self.peak = (self.peak * 0.985).max(rms).max(PEAK_FLOOR);
        let level = ((rms / self.peak - 0.08) / 0.92).clamp(0.0, 1.0);
        level.powf(0.7)
    }

    fn draw(&mut self, window_id: WindowId) {
        let now = Instant::now();
        let recording = matches!(self.phase, Phase::Recording { .. });
        let Some(index) = self
            .surfaces
            .iter()
            .position(|s| s.window.id() == window_id)
        else {
            return;
        };
        let size = PhysicalSize::new(
            self.surfaces[index].config.width,
            self.surfaces[index].config.height,
        );
        let width = size.width as f32;
        let height = size.height as f32;
        let sf = self.surfaces[index].window.scale_factor() as f32;

        // Without a pointer report, the tile and menu show on the focused output.
        // Not all compositors report a still pointer.
        let focused = self.focused.is_none_or(|id| id == window_id);
        // No pointer report yet: after a short wait, open the menu in the centre.
        if let (Phase::Menu { since }, Some(menu)) = (&self.phase, self.menu.as_mut()) {
            if focused && menu.window().is_none() && now >= *since + POINTER_WAIT {
                menu.pointer(window_id, (width / 2.0, height / 2.0));
            }
        }
        // Keeps frames coming until the menu has a place.
        let menu_waiting = matches!(self.phase, Phase::Menu { .. })
            && self.menu.as_ref().is_some_and(|m| m.window().is_none());
        // The notice takes no pointer input, so it learns no pointer position.
        if focused && self.active.is_none() && matches!(self.phase, Phase::Notice { .. }) {
            self.active = Some(window_id);
            self.anchor = Some((width / 2.0, height - 140.0 * sf));
        }
        if self.bubble.as_ref().is_some_and(|b| b.size.0 == 0.0) {
            if let Some(mut bubble) = self.bubble.take() {
                bubble.size = if bubble.note.is_empty() {
                    self.measure_bubble(&bubble.body)
                } else {
                    self.measure_bubble(&format!("{}\n{}", bubble.body, bubble.note))
                };
                self.bubble = Some(bubble);
            }
        }
        // No pointer report yet: after a short wait, use the bottom of the output.
        if focused && self.active.is_none() && recording {
            if let Phase::Recording { since } = self.phase {
                if now >= since + POINTER_WAIT {
                    self.active = Some(window_id);
                    self.anchor = Some((width / 2.0, height - 140.0 * sf));
                }
            }
        }
        let is_active = self.active == Some(window_id);
        if is_active {
            let dt = now.duration_since(self.last_draw).as_secs_f32().min(0.1);
            self.last_draw = now;
            self.update_motion(now, dt, recording);
            let follow = recording || matches!(self.phase, Phase::Cancelled { .. });
            if let (true, Some((px, py))) = (follow, self.pointer) {
                let pointer = (px + POINTER_OFFSET.0 * sf, py + POINTER_OFFSET.1 * sf);
                let target = self.companion_target(now, pointer, sf, width, height);
                match self.anchor {
                    None => {
                        self.anchor = Some(target);
                        self.anchor_velocity = (0.0, 0.0);
                    }
                    Some((ax, ay)) => {
                        let mut x = Spring {
                            x: ax,
                            v: self.anchor_velocity.0,
                        };
                        let mut y = Spring {
                            x: ay,
                            v: self.anchor_velocity.1,
                        };
                        x.step(target.0, COMPANION_STIFFNESS, COMPANION_DAMPING, dt);
                        y.step(target.1, COMPANION_STIFFNESS, COMPANION_DAMPING, dt);
                        self.anchor = Some((x.x, y.x));
                        self.anchor_velocity = (x.v, y.v);
                    }
                }
            }
            if let (Some(bubble), Some((cx, cy))) = (&self.bubble, self.anchor) {
                let (ox, oy) = bubble_origin(cx, cy, bubble.size, sf, width);
                let (vx, vy) = if follow {
                    self.anchor_velocity
                } else {
                    (0.0, 0.0)
                };
                let reach = BUBBLE_LAG_MAX * sf;
                let target = (
                    ox - (vx * BUBBLE_LAG).clamp(-reach, reach),
                    oy - (vy * BUBBLE_LAG).clamp(-reach, reach),
                );
                match &mut self.bubble_pos {
                    None => self.bubble_pos = Some((Spring::at(target.0), Spring::at(target.1))),
                    Some((x, y)) => {
                        x.step(target.0, BUBBLE_STIFFNESS, BUBBLE_DAMPING, dt);
                        y.step(target.1, BUBBLE_STIFFNESS, BUBBLE_DAMPING, dt);
                    }
                }
            }
        }
        if let Some(renderer) = self.renderer.as_mut() {
            if renderer.text_size != size {
                renderer.text.resize(size);
                renderer.text_size = size;
            }
        }
        let menu_open = self.menu.as_ref().is_some_and(|m| !m.is_closing());
        let capture = recording || (matches!(self.phase, Phase::Menu { .. }) && menu_open);

        let (cx, cy) = self.anchor.unwrap_or((-1000.0, -1000.0));
        let menu = matches!(self.phase, Phase::Menu { .. });
        let visible = !menu && is_active && self.anchor.is_some();
        let timer_label = self.timer_label(now).map(|label| {
            let w = self
                .renderer
                .as_mut()
                .map_or(0.0, |r| r.text.measure_text(&label, TIMER_FONT_SCALE * sf));
            (label, w)
        });
        let menu_here = menu && self.menu.as_ref().and_then(|m| m.window()) == Some(window_id);
        let mut scene = if visible {
            self.build_scene(now, cx, cy, sf, width, timer_label)
        } else {
            Scene::default()
        };
        if menu_here {
            let state = self.menu_state();
            if let (Some(menu), Some(renderer)) = (self.menu.as_mut(), self.renderer.as_mut()) {
                menu.build(
                    &state,
                    now,
                    sf,
                    (width, height),
                    &mut renderer.text,
                    &mut scene.shapes,
                    &mut scene.labels,
                );
            }
        }
        let is_active = is_active || menu_here;
        let still = self.still(now);
        let shapes = scene.shapes;

        let output = &mut self.surfaces[index];
        // Whole pixels, so the bubble's settling lift does not reset the region each frame.
        let hit = scene.grab.or(scene.close).map(|rect| rect.map(f32::round));
        output.close = scene.close;
        if output.hit != hit {
            output.hit = hit;
            output.region_dirty = true;
        }
        Self::apply_region(output, capture);
        let frame = match output.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(wgpu::SurfaceError::Outdated | wgpu::SurfaceError::Lost) => {
                output.surface.configure(&self.device, &output.config);
                return;
            }
            Err(_) => return,
        };
        output.settled = (!is_active || still) && !menu_waiting;
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };

        let count = shapes.len().min(MAX_SHAPES);
        renderer.shapes.upload(
            &self.device,
            &self.queue,
            [width, height],
            now.duration_since(self.started).as_secs_f32(),
            &shapes[..count],
        );

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Cursor Overlay Encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Cursor Overlay Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            renderer.shapes.draw(&mut pass, 0..count as u32);
        }

        let timer_drawn = scene.timer.is_some();
        if let Some((text, x, y, scale, alpha)) = scene.timer {
            renderer.text.render_text(
                &view,
                &mut encoder,
                &text,
                x,
                y,
                scale,
                [0.17, 0.2, 0.26, alpha],
                text_area_width(200.0 * sf),
                (40.0 * sf) as u32,
                None,
            );
        }
        if let (Some((x, y, alpha)), Some(bubble)) = (scene.bubble_text, &self.bubble) {
            if timer_drawn {
                // glyphon keeps one vertex buffer, so the timer must draw before
                // the bubble text is prepared.
                self.queue.submit(Some(encoder.finish()));
                encoder = self
                    .device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("Cursor Overlay Bubble Text"),
                    });
            }
            let pad = (13.0 * sf, 10.0 * sf);
            let spans = reveal_spans(bubble, now, alpha);
            renderer.text.render_spans(
                &view,
                &mut encoder,
                &spans,
                x + pad.0,
                y + pad.1,
                BUBBLE_FONT_SCALE * sf,
                1.1,
                text_area_width(bubble.size.0 - 2.0 * pad.0 - CLOSE_SPACE * sf),
            );
        }

        renderer
            .text
            .render_labels(&view, &mut encoder, &scene.labels);

        self.queue.submit(Some(encoder.finish()));
        frame.present();
        renderer.text.trim_atlas();
    }

    /// Where the ring rests: beside the pointer, below and to the right unless an
    /// edge is near, drifting slowly around that spot.
    fn companion_target(
        &self,
        now: Instant,
        pointer: (f32, f32),
        sf: f32,
        width: f32,
        height: f32,
    ) -> (f32, f32) {
        use std::f32::consts::FRAC_PI_4;
        let t = now.duration_since(self.started).as_secs_f32();
        let dist = (0.6 * TILE_W + COMPANION_GAP) * sf * (1.0 + 0.05 * (0.9 * t).sin());
        let reach = dist + (0.6 * TILE_W + 12.0) * sf;
        let right = pointer.0 + reach < width;
        let below = pointer.1 + reach < height;
        let base = match (right, below) {
            (true, true) => FRAC_PI_4,
            (false, true) => 3.0 * FRAC_PI_4,
            (true, false) => -FRAC_PI_4,
            (false, false) => -3.0 * FRAC_PI_4,
        };
        let drift = 0.45 * (0.5 * t).sin() + 0.2 * (0.23 * t + 1.0).sin();
        let angle = base + drift;
        (
            pointer.0 + dist * angle.cos(),
            pointer.1 + dist * angle.sin(),
        )
    }

    /// Starts the close animation; clicks pass through to the apps at once.
    fn close_menu(&mut self, now: Instant) {
        let Some(menu) = self.menu.as_mut() else {
            return;
        };
        menu.close(now);
        self.handles.menu.open.store(false, Ordering::Relaxed);
        for output in &mut self.surfaces {
            output.region_dirty = true;
            output.settled = false;
            output.window.request_redraw();
        }
    }

    fn menu_state(&self) -> MenuState {
        // Moonshine accepts "base" and "moonshine-base" for the same model.
        let model = self.model.strip_prefix("moonshine-").unwrap_or(&self.model);
        MenuState {
            language: self.language.clone(),
            magic: self.magic_mode_active(),
            output: self.output_mode,
            display: self.display_mode,
            preset: PRESETS
                .iter()
                .position(|p| p.backend == self.backend && p.model == model),
            model: model.to_string(),
            recent: Some(self.recent.iter().cloned().collect()),
        }
    }

    /// A click acts on the menu; a click outside closes it.
    fn menu_click(&mut self, window_id: WindowId) {
        let Some(position) = self.pointer else {
            return;
        };
        let state = self.menu_state();
        let Some(menu) = self.menu.as_mut() else {
            return;
        };
        let now = Instant::now();
        match menu.click(window_id, position, &state) {
            None => {}
            Some(MenuAction::Close) => self.close_menu(now),
            Some(MenuAction::ToggleMagic) => {
                let _ = self.handles.app_tx.send(AppCommand::ToggleMagicMode);
            }
            Some(MenuAction::OpenSettings) => {
                self.settings_requested = true;
                self.close_menu(now);
            }
            Some(MenuAction::Paste(text)) => {
                // The menu takes no keyboard focus, so the text lands in the focused app.
                let _ = self.handles.app_tx.send(AppCommand::Deliver(text));
                self.close_menu(now);
            }
            Some(MenuAction::Change(change)) => {
                // Show the change at once; the app saves it and applies it everywhere.
                match &change {
                    MenuChange::Language(code) => self.language = code.clone(),
                    MenuChange::Output(mode) => self.output_mode = *mode,
                    MenuChange::Bubble(bubble) => self.cursor_bubble = *bubble,
                    MenuChange::Display(mode) => self.display_mode = *mode,
                    MenuChange::Model { backend, model } => {
                        self.backend = *backend;
                        self.model = model.clone();
                    }
                }
                self.config_changes.push(change);
            }
        }
    }

    /// Shapes and text placement for one frame of the active output.
    fn build_scene(
        &self,
        now: Instant,
        cx: f32,
        cy: f32,
        sf: f32,
        width: f32,
        timer_label: Option<(String, f32)>,
    ) -> Scene {
        let mut scene = Scene::default();
        let since_phase = |since: Instant| now.duration_since(since).as_secs_f32();

        // Tile opacity, extra exit scale, edge glow, and sideways shake, per phase.
        let (alpha, exit_scale, glow, shake) = match &self.phase {
            Phase::Hidden => return scene,
            Phase::Recording { since } => {
                let t = (since_phase(*since) / FADE_IN.as_secs_f32()).min(1.0);
                (1.0 - (1.0 - t) * (1.0 - t), 1.0, 0.0, 0.0)
            }
            Phase::Processing { .. } => (1.0, 1.0, 0.0, 0.0),
            Phase::Done { since, .. } => {
                let t = (since_phase(*since) / FLASH.as_secs_f32()).min(1.0);
                (1.0 - t, 1.0 + 0.06 * t, 1.0 - t, 0.0)
            }
            Phase::Error { since, until } => {
                let el = since_phase(*since);
                let q = (el / FLASH.as_secs_f32()).min(1.0);
                let shake = (1.0 - q) * 5.0 * sf * (el * 62.0).sin();
                (self.fade_out(now, *until), 1.0, 0.0, shake)
            }
            Phase::Cancelled { since } => {
                let t = (since_phase(*since) / CANCEL_FADE.as_secs_f32()).min(1.0);
                (1.0 - t * t, 1.0 - 0.12 * t * t, 0.0, 0.0)
            }
            Phase::Notice { since, until } => {
                let t = (since_phase(*since) / FADE_IN.as_secs_f32()).min(1.0);
                (t.min(self.fade_out(now, *until)), 1.0, 0.0, 0.0)
            }
            Phase::Menu { .. } => return scene,
        };
        let k = self.tile_size.x * sf * self.tile_scale.x * exit_scale;
        let (tw, th) = (TILE_W * k, TILE_H * k);
        let tx = cx + shake;
        if alpha > 0.0 {
            scene.shapes.push(glass(
                tx,
                cy,
                tw / 2.0,
                th / 2.0,
                TILE_CORNER * th,
                14.0 * sf,
                0.62,
                glow,
                alpha,
            ));
            if matches!(self.phase, Phase::Error { .. }) {
                let mut red = self.error_color;
                red[3] = 0.7 * alpha;
                scene.shapes.push(Shape {
                    rect: [tx - tw / 2.0, cy - th / 2.0, tw, th],
                    color: [0.0; 4],
                    params: [OUTLINE, TILE_CORNER * th, 1.5 * sf, 0.0],
                    extra: red,
                });
            }
            let bar_w = BAR_WIDTH * th;
            let margin = 2.0 * sf;
            for (i, bar) in self.bars.iter().enumerate() {
                let x = tx + (i as f32 - (BARS / 2) as f32) * BAR_PITCH * th;
                let h = (BAR_PROFILE[i] * th * bar.x.min(1.2)).max(bar_w);
                scene.shapes.push(Shape {
                    rect: [
                        x - bar_w / 2.0 - margin,
                        cy - h / 2.0 - margin,
                        bar_w + 2.0 * margin,
                        h + 2.0 * margin,
                    ],
                    color: [1.0, 1.0, 1.0, alpha],
                    params: [SILVER_BAR, 0.0, margin, 0.0],
                    extra: [0.0; 4],
                });
            }
            if self.capturing && matches!(self.phase, Phase::Recording { .. }) {
                // Inside the top right corner, clear of the bars.
                let r = 3.5 * k;
                let inset = 0.55 * TILE_CORNER * th;
                let (dx, dy) = (tx + tw / 2.0 - inset, cy - th / 2.0 + inset);
                scene.shapes.push(Shape {
                    rect: [dx - r, dy - r, 2.0 * r, 2.0 * r],
                    color: [0.86, 0.27, 0.24, 0.9 * alpha],
                    params: [PANEL, r, FLAT, 0.0],
                    extra: [0.0; 4],
                });
            }
        }
        if let Some((label, label_w)) = timer_label {
            self.add_timer(&mut scene, label, label_w, cx, cy + th / 2.0, sf, alpha);
        }
        if matches!(
            self.phase,
            Phase::Recording { .. } | Phase::Processing { .. } | Phase::Cancelled { .. }
        ) {
            self.add_live_bubble(&mut scene, now, cx, cy, sf, width, alpha);
        }
        if matches!(
            self.phase,
            Phase::Done { .. } | Phase::Error { .. } | Phase::Notice { .. }
        ) {
            self.add_bubble(&mut scene, now, cx, cy, sf, width);
        }
        scene
    }

    /// Opacity that falls from 1 to 0 over `FADE_OUT` after `until`.
    fn fade_out(&self, now: Instant, until: Instant) -> f32 {
        if now <= until {
            1.0
        } else {
            (1.0 - (now - until).as_secs_f32() / FADE_OUT.as_secs_f32()).max(0.0)
        }
    }

    /// Top left of the bubble: its trailing spring, else its place beside the tile.
    fn bubble_place(&self, bubble: &Bubble, cx: f32, cy: f32, sf: f32, width: f32) -> (f32, f32) {
        self.bubble_pos.map_or_else(
            || bubble_origin(cx, cy, bubble.size, sf, width),
            |(x, y)| (x.x, y.x),
        )
    }

    /// The live text beside the tile. It takes no pointer input and has no close button.
    #[allow(clippy::too_many_arguments)]
    fn add_live_bubble(
        &self,
        scene: &mut Scene,
        now: Instant,
        cx: f32,
        cy: f32,
        sf: f32,
        width: f32,
        tile_alpha: f32,
    ) {
        let Some(bubble) = &self.bubble else {
            return;
        };
        let fade_in = (now.duration_since(self.bubble_since).as_secs_f32()
            / BUBBLE_FADE_IN.as_secs_f32())
        .min(1.0);
        let alpha = fade_in * tile_alpha;
        let (bx, by) = self.bubble_place(bubble, cx, cy, sf, width);
        let by = by + self.bubble_lift.x * sf;
        let (w, h) = bubble.size;
        scene.shapes.push(glass(
            bx + w / 2.0,
            by + h / 2.0,
            w / 2.0,
            h / 2.0,
            14.0 * sf,
            18.0 * sf,
            0.82,
            0.0,
            alpha,
        ));
        scene.bubble_text = Some((bx, by, alpha));
    }

    fn add_bubble(&self, scene: &mut Scene, now: Instant, cx: f32, cy: f32, sf: f32, width: f32) {
        let Some(bubble) = &self.bubble else {
            // Bubble off: only the burst at the pointer marks the delivery.
            if let Phase::Done { until, .. } = self.phase {
                let after = now.saturating_duration_since(until).as_secs_f32();
                let target = self.pointer.unwrap_or((cx, cy));
                add_burst(scene, target, after / BURST.as_secs_f32(), sf);
            }
            return;
        };
        let until = match self.phase {
            Phase::Done { until, .. }
            | Phase::Error { until, .. }
            | Phase::Notice { until, .. } => until,
            _ => return,
        };
        let fade_in = (now.duration_since(self.bubble_since).as_secs_f32()
            / BUBBLE_FADE_IN.as_secs_f32())
        .min(1.0);
        let (bx, by) = self.bubble_place(bubble, cx, cy, sf, width);
        let (bx, by) = self.bubble_at.unwrap_or((bx, by + self.bubble_lift.x * sf));
        let (w, h) = bubble.size;

        if matches!(self.phase, Phase::Done { .. }) {
            // After its time, the bubble flies into the pointer and vanishes there.
            let target = self.pointer.unwrap_or((cx, cy));
            let after = now.saturating_duration_since(until).as_secs_f32();
            let p = (after / TELEPORT.as_secs_f32()).min(1.0);
            if p < 1.0 {
                let e = p * p;
                let (ox, oy) = (bx + w / 2.0, by + h / 2.0);
                let (gx, gy) = (ox + (target.0 - ox) * e, oy + (target.1 - oy) * e);
                let shrink = 1.0 - 0.96 * e;
                let (hw, hh) = (w / 2.0 * shrink, h / 2.0 * shrink);
                let glass_alpha = fade_in * (1.0 - 0.5 * e);
                scene.shapes.push(glass(
                    gx,
                    gy,
                    hw,
                    hh,
                    14.0 * sf * shrink,
                    18.0 * sf,
                    0.82,
                    0.0,
                    glass_alpha,
                ));
                let text_alpha = fade_in * (1.0 - smoothstep(0.0, 0.3, p));
                if text_alpha > 0.0 {
                    scene.bubble_text = Some((bx, by, text_alpha));
                }
                if p == 0.0 {
                    self.add_close(scene, bx + w, by, sf, fade_in);
                    if self.bubble_pinned {
                        scene.grab = Some([bx, by, w, h]);
                    }
                }
            } else {
                let q = (after - TELEPORT.as_secs_f32()) / BURST.as_secs_f32();
                add_burst(scene, target, q, sf);
            }
            return;
        }

        let alpha = fade_in * self.fade_out(now, until);
        scene.shapes.push(glass(
            bx + w / 2.0,
            by + h / 2.0,
            w / 2.0,
            h / 2.0,
            14.0 * sf,
            18.0 * sf,
            0.82,
            0.0,
            alpha,
        ));
        if bubble.error {
            let mut red = self.error_color;
            red[3] = 0.6 * alpha;
            scene.shapes.push(Shape {
                rect: [bx, by, w, h],
                color: [0.0; 4],
                params: [OUTLINE, 14.0 * sf, 1.0 * sf, 0.0],
                extra: red,
            });
        }
        scene.bubble_text = Some((bx, by, alpha));
        if now <= until {
            self.add_close(scene, bx + w, by, sf, alpha);
        }
    }

    /// The close button in the bubble's top right corner (`right`, `top`).
    fn add_close(&self, scene: &mut Scene, right: f32, top: f32, sf: f32, alpha: f32) {
        let (cx, cy) = (right - 15.0 * sf, top + 15.0 * sf);
        let half = 11.0 * sf;
        if self.close_hover {
            scene.shapes.push(Shape {
                rect: [cx - half, cy - half, 2.0 * half, 2.0 * half],
                color: [0.565, 0.659, 0.765, 0.28 * alpha],
                params: [PANEL, half, FLAT, 0.0],
                extra: [0.0; 4],
            });
        }
        let color = if self.close_hover { INK } else { INK_2 };
        let icon = 5.0 * sf;
        scene.shapes.push(Shape {
            rect: [cx - icon, cy - icon, 2.0 * icon, 2.0 * icon],
            color: [color[0], color[1], color[2], 0.9 * alpha],
            params: [ICON, CROSS, 0.0, 0.0],
            extra: [0.0; 4],
        });
        if alpha > 0.5 {
            scene.close = Some([cx - half, cy - half, 2.0 * half, 2.0 * half]);
        }
    }

    /// The pill text: elapsed time while recording, else what Sonori waits for.
    fn timer_label(&self, now: Instant) -> Option<String> {
        match self.phase {
            Phase::Recording { .. } | Phase::Processing { .. } if self.backend_loading() => {
                Some("Loading model…".to_string())
            }
            Phase::Recording { since } => {
                let secs = now.duration_since(since).as_secs();
                Some(format!("{}:{:02}", secs / 60, secs % 60))
            }
            Phase::Processing {
                transcription_seen: true,
                ..
            } if self.magic_mode_active()
                && self.handles.audio_data.read().processing_state
                    != ProcessingState::Transcribing =>
            {
                Some("Polishing…".to_string())
            }
            Phase::Processing { .. } => Some("Transcribing…".to_string()),
            _ => None,
        }
    }

    fn magic_mode_active(&self) -> bool {
        self.handles
            .magic_mode_enabled
            .as_ref()
            .is_some_and(|m| m.load(Ordering::Relaxed))
    }

    fn backend_loading(&self) -> bool {
        self.handles
            .backend_status
            .as_ref()
            .is_some_and(|s| matches!(s.read().state, BackendStatusState::Loading(_)))
    }

    /// A glass pill under the tile, `top` being the tile's lowest point.
    #[allow(clippy::too_many_arguments)]
    fn add_timer(
        &self,
        scene: &mut Scene,
        label: String,
        text_w: f32,
        cx: f32,
        top: f32,
        sf: f32,
        alpha: f32,
    ) {
        let scale = TIMER_FONT_SCALE * sf;
        let font_px = 10.0 * scale;
        let w = text_w + 16.0 * sf;
        let h = 20.0 * sf;
        let y = top + 8.0 * sf;
        scene.shapes.push(glass(
            cx,
            y + h / 2.0,
            w / 2.0,
            h / 2.0,
            h / 2.0,
            10.0 * sf,
            0.75,
            0.0,
            alpha,
        ));
        let text_y = y + (h - font_px * 1.1) / 2.0;
        scene.timer = Some((label, cx - text_w / 2.0, text_y, scale, 0.94 * alpha));
    }
}

/// A small light ring and dot at `target`, `q` running from 0 to 1.
fn add_burst(scene: &mut Scene, target: (f32, f32), q: f32, sf: f32) {
    let q = q.clamp(0.0, 1.0);
    let fade = (1.0 - q).powf(1.5);
    let ring = (3.0 + 20.0 * q) * sf;
    let dot = 4.0 * (1.0 - q) * sf;
    for (radius, width, alpha) in [
        (ring, (2.0 * (1.0 - q) + 0.5) * sf, fade),
        (dot, dot, 1.0 - q),
    ] {
        if radius <= 0.0 {
            continue;
        }
        scene.shapes.push(Shape {
            rect: [
                target.0 - radius,
                target.1 - radius,
                2.0 * radius,
                2.0 * radius,
            ],
            color: [0.0; 4],
            params: [OUTLINE, radius, width, 0.0],
            extra: [0.95, 0.97, 1.0, alpha],
        });
    }
}

/// What one frame draws on the active output.
#[derive(Default)]
struct Scene {
    shapes: Vec<Shape>,
    /// Label, x, y, font scale, and opacity.
    timer: Option<(String, f32, f32, f32, f32)>,
    /// Bubble origin and text opacity.
    bubble_text: Option<(f32, f32, f32)>,
    /// The close button's hit area, in physical px.
    close: Option<[f32; 4]>,
    /// A pinned bubble, which the user can drag.
    grab: Option<[f32; 4]>,
    /// Menu text, drawn in one batch.
    labels: Vec<Label>,
}

/// Pairs each word of `new` with the same word in `old`, in order, as the
/// longest common run of words. Returns the old fade start of each paired word.
fn align_words(old: &[ShownWord], new: &[&str]) -> Vec<Option<Instant>> {
    // Words before the window keep their place; the bubble shows only the end.
    const WINDOW: usize = 160;
    let mut kept = vec![None; new.len()];
    let o0 = old.len().saturating_sub(WINDOW);
    let n0 = new.len().saturating_sub(WINDOW);
    for (i, word) in new.iter().enumerate().take(n0) {
        kept[i] = old
            .get(i)
            .filter(|w| same_word(&w.word, word))
            .map(|w| w.since);
    }
    let (a, b) = (&old[o0..], &new[n0..]);
    let (n, m) = (a.len(), b.len());
    let at = |i: usize, j: usize| i * (m + 1) + j;
    let mut lcs = vec![0u16; (n + 1) * (m + 1)];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[at(i, j)] = if same_word(&a[i].word, b[j]) {
                lcs[at(i + 1, j + 1)] + 1
            } else {
                lcs[at(i + 1, j)].max(lcs[at(i, j + 1)])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if same_word(&a[i].word, b[j]) {
            kept[n0 + j] = Some(a[i].since);
            i += 1;
            j += 1;
        } else if lcs[at(i + 1, j)] >= lcs[at(i, j + 1)] {
            i += 1;
        } else {
            j += 1;
        }
    }
    kept
}

/// Whether two words are the same apart from case and punctuation.
fn same_word(a: &str, b: &str) -> bool {
    let letters = |w: &str| {
        w.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    letters(a) == letters(b)
}

/// The bubble text as spans whose words fade in at their own times, then the note.
fn reveal_spans(bubble: &Bubble, now: Instant, alpha: f32) -> Vec<(&str, [f32; 4])> {
    let fade = WORD_FADE.as_secs_f32();
    let shown = |since: Instant| {
        let t = now.saturating_duration_since(since).as_secs_f32();
        if now < since {
            0.0
        } else {
            smoothstep(0.0, fade, t)
        }
    };
    let color = |a: f32| [TEXT_COLOR[0], TEXT_COLOR[1], TEXT_COLOR[2], a * alpha];
    let mut spans: Vec<(&str, [f32; 4])> = bubble
        .body
        .split_inclusive(char::is_whitespace)
        .enumerate()
        .map(|(i, word)| {
            let since = bubble.word_since.get(i).copied().unwrap_or(now);
            let dim = if i >= bubble.dim_from {
                TENTATIVE_ALPHA
            } else {
                1.0
            };
            (word, color(dim * shown(since)))
        })
        .collect();
    if !bubble.note.is_empty() {
        let a = 0.55 * shown(bubble.note_since);
        spans.push(("\n", color(a)));
        spans.push((bubble.note.as_str(), color(a)));
    }
    spans
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// `TextRenderer` wraps at the area width minus its fixed side margins.
fn text_area_width(inner: f32) -> u32 {
    (inner + super::window::LEFT_MARGIN + super::window::RIGHT_MARGIN).ceil() as u32
}

/// Up and to the right of the pointer; flipped at the right and top edges.
fn bubble_origin(cx: f32, cy: f32, size: (f32, f32), sf: f32, width: f32) -> (f32, f32) {
    let gap = 44.0 * sf;
    let mut x = cx + gap;
    let mut y = cy - gap - size.1;
    if x + size.0 > width - 10.0 * sf {
        x = cx - gap - size.0;
    }
    if y < 10.0 * sf {
        y = cy + 34.0 * sf;
    }
    (x.max(10.0 * sf), y)
}

/// Cuts `text` at a word boundary so the bubble stays near three lines.
fn shorten(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let cut: String = text.chars().take(max_chars).collect();
    let cut = cut.rsplit_once(' ').map_or(cut.as_str(), |(head, _)| head);
    format!("{cut}…")
}

impl Renderer {
    fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        size: PhysicalSize<u32>,
    ) -> Self {
        let mut text = TextRenderer::with_color_mode(
            Arc::new(device.clone()),
            Arc::new(queue.clone()),
            size,
            format,
            glyphon::ColorMode::Web,
        );
        // The close button sits at the bubble's edge, so the bubble's size must not
        // depend on the system font.
        text.use_bundled_font();
        Self {
            shapes: ShapeRenderer::new(device, format),
            text,
            text_size: size,
            format,
        }
    }
}
