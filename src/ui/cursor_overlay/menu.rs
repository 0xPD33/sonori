//! The quick menu that a held hotkey opens at the pointer. It draws with the
//! overlay's shapes and one batch of labels; see `docs/design/cursor-mode.md`.

use std::f32::consts::FRAC_PI_2;
use std::time::{Duration, Instant};

use bytemuck::Zeroable;
use speechcore::BackendType;
use winit::window::WindowId;

use super::MenuChange;
use crate::config::{DisplayMode, OutputMode};
use crate::ui::shapes::{
    glass, Shape, Spring, Ui, BARS, CHEVRON, FLAT, GEAR, GLOBE, HAIRLINE, INK, INK_2, PAGE, PANEL,
    POINTER, RAISED, SILVER_BAR, SLATE, SLATE_FILL, SPARKLES, SUNK, TICK,
};
use crate::ui::text_renderer::{Label, TextRenderer, LABEL_LINE};

/// A model the menu offers.
pub(crate) struct Preset {
    pub name: &'static str,
    pub detail: &'static str,
    pub backend: BackendType,
    pub model: &'static str,
}

/// Parakeet leads the Open ASR leaderboard among the models speechcore runs:
/// mean English WER 6.05 (v2) and 6.32 (v3), against 7.83 for Whisper large-v3 turbo.
pub(crate) const PRESETS: [Preset; 3] = [
    Preset {
        name: "Parakeet v2",
        detail: "Most accurate · English only",
        backend: BackendType::Parakeet,
        model: "parakeet-tdt-0.6b-v2",
    },
    Preset {
        name: "Parakeet v3",
        detail: "25 languages, German included",
        backend: BackendType::Parakeet,
        model: "parakeet-tdt-0.6b-v3",
    },
    Preset {
        name: "Moonshine base",
        detail: "Smallest and fastest · English only",
        backend: BackendType::Moonshine,
        model: "base",
    },
];

const LANGUAGES: [(&str, &str); 5] = [
    ("auto", "Auto detect"),
    ("en", "English"),
    ("de", "Deutsch"),
    ("fr", "Français"),
    ("es", "Español"),
];
const OUTPUTS: [(OutputMode, &str); 3] = [
    (OutputMode::Paste, "Paste"),
    (OutputMode::Type, "Type"),
    (OutputMode::Clipboard, "Copy"),
];
const DISPLAYS: [(DisplayMode, &str); 2] = [
    (DisplayMode::Cursor, "Cursor"),
    (DisplayMode::Overlay, "Overlay"),
];

// Layout in logical px.
const WIDTH: f32 = 320.0;
const PAD: f32 = 8.0;
const ROW: f32 = 42.0;
const OPTION: f32 = 34.0;
const OPTION_TALL: f32 = 50.0;
const OPTION_GAP: f32 = 4.0;
/// Options start under the row label, as in the mockup.
const OPTION_INDENT: f32 = 42.0;
const SEPARATOR: f32 = 13.0;
const HINT: f32 = 26.0;
/// Distance between the pointer and the menu corner.
const OFFSET: f32 = 14.0;

const POP: Duration = Duration::from_millis(180);
const CLOSE: Duration = Duration::from_millis(120);

/// The settings the menu shows.
pub(super) struct MenuState {
    pub language: String,
    pub magic: bool,
    pub output: OutputMode,
    pub display: DisplayMode,
    /// Index into `PRESETS`, if the configured model is one of them.
    pub preset: Option<usize>,
}

pub(super) enum MenuAction {
    Close,
    Change(MenuChange),
    ToggleMagic,
    OpenSettings,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum List {
    Language,
    Model,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Hit {
    /// Inside the menu, on nothing that acts.
    Panel,
    /// A row whose controls take the clicks.
    Row,
    Expand(List),
    Choose(List, usize),
    Magic,
    Output(usize),
    Display(usize),
    Settings,
}

/// A clickable area and the rectangle the hover highlight moves to.
struct Target {
    rect: [f32; 4],
    hit: Hit,
    highlight: Option<[f32; 4]>,
}

pub(super) struct QuickMenu {
    since: Instant,
    closing: Option<Instant>,
    /// Output and pointer position (physical px) where the menu opened.
    at: Option<(WindowId, (f32, f32))>,
    /// Whether the menu opens to the left of and above the pointer; fixed on open.
    flip: Option<(bool, bool)>,
    open: Option<List>,
    expand: [Spring; 2],
    hover: Option<Hit>,
    highlight: [Spring; 4],
    highlight_alpha: Spring,
    knob: Spring,
    output_pill: [Spring; 2],
    display_pill: [Spring; 2],
    /// A model choice that waits for a second click, as it may download.
    pending: Option<usize>,
    targets: Vec<Target>,
    last: Instant,
    /// Springs jump to their targets on the first frame.
    fresh: bool,
}

impl QuickMenu {
    pub fn new(now: Instant) -> Self {
        Self {
            since: now,
            closing: None,
            at: None,
            flip: None,
            open: None,
            expand: [Spring::default(); 2],
            hover: None,
            highlight: [Spring::default(); 4],
            highlight_alpha: Spring::default(),
            knob: Spring::default(),
            output_pill: [Spring::default(); 2],
            display_pill: [Spring::default(); 2],
            pending: None,
            targets: Vec::new(),
            last: now,
            fresh: true,
        }
    }

    /// The output the menu shows on, once the pointer was seen.
    pub fn window(&self) -> Option<WindowId> {
        self.at.map(|(id, _)| id)
    }

    pub fn is_closing(&self) -> bool {
        self.closing.is_some()
    }

    pub fn close(&mut self, now: Instant) {
        self.closing.get_or_insert(now);
    }

    /// The close animation finished.
    pub fn closed(&self, now: Instant) -> bool {
        self.closing.is_some_and(|since| now >= since + CLOSE)
    }

    /// Opens the menu at the first pointer report, then tracks hover.
    pub fn pointer(&mut self, window_id: WindowId, position: (f32, f32)) {
        if self.at.is_none() {
            self.at = Some((window_id, position));
        }
        self.hover = self.hit(window_id, position);
    }

    pub fn click(
        &mut self,
        window_id: WindowId,
        position: (f32, f32),
        state: &MenuState,
    ) -> Option<MenuAction> {
        if self.closing.is_some() {
            return None;
        }
        let Some(hit) = self.hit(window_id, position) else {
            return Some(MenuAction::Close);
        };
        if !matches!(hit, Hit::Choose(List::Model, _)) {
            self.pending = None;
        }
        match hit {
            Hit::Panel | Hit::Row => None,
            Hit::Expand(list) => {
                self.open = if self.open == Some(list) {
                    None
                } else {
                    Some(list)
                };
                None
            }
            Hit::Choose(List::Language, i) => Some(MenuAction::Change(MenuChange::Language(
                LANGUAGES[i].0.to_string(),
            ))),
            Hit::Choose(List::Model, i) if state.preset == Some(i) => {
                self.pending = None;
                None
            }
            Hit::Choose(List::Model, i) if self.pending == Some(i) => {
                self.pending = None;
                Some(MenuAction::Change(MenuChange::Model {
                    backend: PRESETS[i].backend,
                    model: PRESETS[i].model.to_string(),
                }))
            }
            Hit::Choose(List::Model, i) => {
                self.pending = Some(i);
                None
            }
            Hit::Magic => Some(MenuAction::ToggleMagic),
            Hit::Output(i) => Some(MenuAction::Change(MenuChange::Output(OUTPUTS[i].0))),
            Hit::Display(i) => Some(MenuAction::Change(MenuChange::Display(DISPLAYS[i].0))),
            Hit::Settings => Some(MenuAction::OpenSettings),
        }
    }

    fn hit(&self, window_id: WindowId, (x, y): (f32, f32)) -> Option<Hit> {
        if self.at.is_some_and(|(id, _)| id != window_id) {
            return None;
        }
        self.targets
            .iter()
            .rev()
            .find(|t| {
                x >= t.rect[0]
                    && x < t.rect[0] + t.rect[2]
                    && y >= t.rect[1]
                    && y < t.rect[1] + t.rect[3]
            })
            .map(|t| t.hit)
    }

    /// Lays out and animates one frame. `size` is the output size in physical px.
    pub fn build(
        &mut self,
        state: &MenuState,
        now: Instant,
        sf: f32,
        size: (f32, f32),
        text: &mut TextRenderer,
        shapes: &mut Vec<Shape>,
        labels: &mut Vec<Label>,
    ) {
        let Some((_, pointer)) = self.at else {
            return;
        };
        let dt = now.duration_since(self.last).as_secs_f32().min(0.1);
        self.last = now;
        self.animate(state, dt);

        // Opacity and drop: a short pop on open, a quicker fade on close.
        let (alpha, drop) = match self.closing {
            Some(since) => {
                let t = (now.duration_since(since).as_secs_f32() / CLOSE.as_secs_f32()).min(1.0);
                (1.0 - t, 4.0 * t)
            }
            None => {
                let t = (now.duration_since(self.since).as_secs_f32() / POP.as_secs_f32()).min(1.0);
                let ease = 1.0 - (1.0 - t).powi(3);
                (ease, 6.0 * (1.0 - ease))
            }
        };
        let px = |v: f32| v * sf;
        let lists = [
            Self::list_height(List::Language),
            Self::list_height(List::Model),
        ];
        let base = 2.0 * PAD + 6.0 * ROW + SEPARATOR + HINT;
        let height = base + lists[0] * self.expand[0].x + lists[1] * self.expand[1].x;
        let (left, above) = *self.flip.get_or_insert_with(|| {
            let tallest = base + lists[0].max(lists[1]);
            (
                pointer.0 + px(OFFSET + WIDTH) > size.0 - px(8.0),
                pointer.1 + px(OFFSET + tallest) > size.1 - px(8.0),
            )
        });
        let x0 = if left {
            pointer.0 - px(OFFSET + WIDTH)
        } else {
            pointer.0 + px(OFFSET)
        }
        .max(px(8.0));
        let y0 = if above {
            pointer.1 - px(OFFSET + height)
        } else {
            pointer.1 + px(OFFSET)
        }
        .max(px(8.0))
            + px(drop);

        shapes.push(glass(
            x0 + px(WIDTH) / 2.0,
            y0 + px(height) / 2.0,
            px(WIDTH) / 2.0,
            px(height) / 2.0,
            px(20.0),
            px(18.0),
            0.86,
            0.0,
            alpha,
        ));
        // Filled in once the hovered target is known.
        let highlight_index = shapes.len();
        shapes.push(Shape::zeroed());

        let mut targets = vec![Target {
            rect: [x0, y0, px(WIDTH), px(height)],
            hit: Hit::Panel,
            highlight: None,
        }];
        let mut ui = Ui {
            sf,
            alpha,
            shapes,
            labels,
            text,
            clip: None,
        };
        let row_x = x0 + px(PAD);
        let row_w = px(WIDTH - 2.0 * PAD);
        let right = x0 + px(WIDTH - PAD - 10.0);
        let label_x = x0 + px(PAD + 46.0);
        let mut y = y0 + px(PAD);

        for row in 0..6 {
            let rect = [row_x, y, row_w, px(ROW)];
            let cy = y + px(ROW) / 2.0;
            let icon = [GLOBE, SPARKLES, PAGE, BARS, POINTER, GEAR][row];
            ui.icon(icon, x0 + px(PAD + 23.0), cy, 18.0, 0.0, SLATE, 1.0);
            let title = [
                "Language",
                "Magic Mode",
                "Output",
                "Model",
                "Display",
                "More settings…",
            ][row];
            ui.label(title, label_x, cy, 14.0, true, INK, 1.0);
            match row {
                0 | 3 => {
                    let (list, index) = if row == 0 {
                        (List::Language, 0)
                    } else {
                        (List::Model, 1)
                    };
                    let value = match list {
                        List::Language => LANGUAGES
                            .iter()
                            .find(|(code, _)| *code == state.language)
                            .map_or(state.language.to_uppercase(), |(_, name)| name.to_string()),
                        List::Model => state
                            .preset
                            .map_or("Custom".to_string(), |i| PRESETS[i].name.to_string()),
                    };
                    let e = self.expand[index].x;
                    ui.icon(
                        CHEVRON,
                        right - px(5.0),
                        cy,
                        10.0,
                        e * FRAC_PI_2,
                        INK_2,
                        1.0,
                    );
                    let w = ui.measure(&value, 13.0, false);
                    ui.label(&value, right - px(16.0) - w, cy, 13.0, false, INK_2, 1.0);
                    targets.push(Target {
                        rect,
                        hit: Hit::Expand(list),
                        highlight: Some(rect),
                    });
                    y += px(ROW);
                    if e > 0.01 {
                        y = self.options(&mut ui, &mut targets, list, state, x0, y, e);
                    }
                    continue;
                }
                1 => {
                    let k = self.knob.x;
                    let (tx, ty) = (right - px(42.0), cy - px(12.0));
                    ui.panel([tx, ty, px(42.0), px(24.0)], 12.0, SUNK, 1.0);
                    ui.panel([tx, ty, px(42.0), px(24.0)], 12.0, SLATE_FILL, k);
                    let kx = tx + px(12.0 + 18.0 * k);
                    let m = px(2.0);
                    ui.shapes.push(Shape {
                        rect: [
                            kx - px(9.0) - m,
                            cy - px(9.0) - m,
                            px(18.0) + 2.0 * m,
                            px(18.0) + 2.0 * m,
                        ],
                        color: [1.0, 1.0, 1.0, alpha],
                        params: [SILVER_BAR, 0.0, m, 0.0],
                        extra: [0.0; 4],
                    });
                    targets.push(Target {
                        rect,
                        hit: Hit::Magic,
                        highlight: Some(rect),
                    });
                }
                2 | 4 => {
                    targets.push(Target {
                        rect,
                        hit: Hit::Row,
                        highlight: Some(rect),
                    });
                    let names: Vec<&str> = if row == 2 {
                        OUTPUTS.iter().map(|(_, n)| *n).collect()
                    } else {
                        DISPLAYS.iter().map(|(_, n)| *n).collect()
                    };
                    let selected = if row == 2 {
                        OUTPUTS.iter().position(|(m, _)| *m == state.output)
                    } else {
                        DISPLAYS.iter().position(|(m, _)| *m == state.display)
                    };
                    let pill = if row == 2 {
                        self.output_pill
                    } else {
                        self.display_pill
                    };
                    let buttons = ui.segmented(&names, selected, pill, right, cy);
                    for (i, rect_i) in buttons.iter().enumerate() {
                        targets.push(Target {
                            rect: *rect_i,
                            hit: if row == 2 {
                                Hit::Output(i)
                            } else {
                                Hit::Display(i)
                            },
                            highlight: Some(rect),
                        });
                    }
                    // The pill springs towards the selected button.
                    if let Some(i) = selected {
                        let goal = [buttons[i][0], buttons[i][2]];
                        let pill = if row == 2 {
                            &mut self.output_pill
                        } else {
                            &mut self.display_pill
                        };
                        for (spring, goal) in pill.iter_mut().zip(goal) {
                            if self.fresh {
                                *spring = Spring::at(goal);
                            }
                            spring.step(goal, 600.0, 44.0, dt);
                        }
                    }
                }
                _ => {
                    ui.icon(CHEVRON, right - px(5.0), cy, 10.0, 0.0, INK_2, 1.0);
                    targets.push(Target {
                        rect,
                        hit: Hit::Settings,
                        highlight: Some(rect),
                    });
                }
            }
            y += px(ROW);
            if row == 4 {
                ui.panel(
                    [
                        x0 + px(PAD + 10.0),
                        y + px(6.0),
                        px(WIDTH - 2.0 * PAD - 20.0),
                        px(1.0),
                    ],
                    0.0,
                    HAIRLINE,
                    1.0,
                );
                y += px(SEPARATOR);
            }
        }
        ui.label_top(
            "Click outside to close",
            x0 + px(PAD + 12.0),
            y + px(6.0),
            11.5,
            false,
            INK_2,
            1.0,
        );

        // The highlight glides to the hovered row or option.
        let goal = self
            .hover
            .and_then(|hover| targets.iter().rev().find(|t| t.hit == hover))
            .and_then(|t| t.highlight);
        match goal {
            Some(goal) => {
                for (spring, goal) in self.highlight.iter_mut().zip(goal) {
                    if self.highlight_alpha.x < 0.05 {
                        *spring = Spring::at(goal);
                    }
                    spring.step(goal, 700.0, 50.0, dt);
                }
                self.highlight_alpha.step(1.0, 400.0, 40.0, dt);
            }
            None => self.highlight_alpha.step(0.0, 400.0, 40.0, dt),
        }
        let h = self.highlight.map(|s| s.x);
        ui.shapes[highlight_index] = Shape {
            rect: h,
            color: [
                0.565,
                0.659,
                0.765,
                0.2 * alpha * self.highlight_alpha.x.clamp(0.0, 1.0),
            ],
            params: [PANEL, px(12.0), FLAT, 0.0],
            extra: [0.0; 4],
        };
        self.targets = targets;
        self.fresh = false;
    }

    fn animate(&mut self, state: &MenuState, dt: f32) {
        for (i, list) in [List::Language, List::Model].into_iter().enumerate() {
            let goal = if self.open == Some(list) { 1.0 } else { 0.0 };
            self.expand[i].step(goal, 380.0, 40.0, dt);
            self.expand[i].x = self.expand[i].x.clamp(0.0, 1.0);
        }
        let goal = if state.magic { 1.0 } else { 0.0 };
        if self.fresh {
            self.knob = Spring::at(goal);
        }
        self.knob.step(goal, 500.0, 34.0, dt);
    }

    fn list_height(list: List) -> f32 {
        let (count, height) = match list {
            List::Language => (LANGUAGES.len() + 1, OPTION),
            List::Model => (PRESETS.len(), OPTION_TALL),
        };
        2.0 + count as f32 * (height + OPTION_GAP) - OPTION_GAP + 8.0
    }

    /// Draws the options of an open list, squeezed by `e` while it opens or
    /// closes. Returns the y below them.
    #[allow(clippy::too_many_arguments)]
    fn options(
        &self,
        ui: &mut Ui,
        targets: &mut Vec<Target>,
        list: List,
        state: &MenuState,
        x0: f32,
        top: f32,
        e: f32,
    ) -> f32 {
        let sf = ui.sf;
        let px = |v: f32| v * sf;
        let fade = ((e - 0.3) / 0.7).clamp(0.0, 1.0);
        let x = x0 + px(PAD + OPTION_INDENT);
        let w = px(WIDTH - 2.0 * PAD - OPTION_INDENT - 6.0);
        let height = match list {
            List::Language => OPTION,
            List::Model => OPTION_TALL,
        };
        let count = match list {
            List::Language => LANGUAGES.len() + 1,
            List::Model => PRESETS.len(),
        };
        let mut y = top + px(2.0) * e;
        for i in 0..count {
            let rect = [x, y, w, px(height)];
            let cy = y + px(height) / 2.0;
            let selected = match list {
                List::Language => LANGUAGES
                    .get(i)
                    .is_some_and(|(code, _)| *code == state.language),
                List::Model => state.preset == Some(i),
            };
            if selected {
                ui.panel(rect, 10.0, RAISED, fade);
                ui.icon(TICK, x + w - px(17.0), cy, 14.0, 0.0, SLATE, fade);
            }
            match (list, LANGUAGES.get(i), PRESETS.get(i)) {
                (List::Language, Some((_, name)), _) => {
                    ui.label(name, x + px(10.0), cy, 13.0, false, INK, fade)
                }
                (List::Language, None, _) => ui.label(
                    "More languages in Settings",
                    x + px(10.0),
                    cy,
                    13.0,
                    false,
                    INK_2,
                    fade,
                ),
                (List::Model, _, None) => {}
                (List::Model, _, Some(preset)) => {
                    let detail = if self.pending == Some(i) {
                        "Click again to switch; may download"
                    } else {
                        preset.detail
                    };
                    let line = 13.0 * LABEL_LINE;
                    let first = cy - px(line + 12.0 * LABEL_LINE) / 2.0;
                    ui.label_top(preset.name, x + px(10.0), first, 13.0, true, INK, fade);
                    ui.label_top(
                        detail,
                        x + px(10.0),
                        first + px(line),
                        12.0,
                        false,
                        INK_2,
                        fade,
                    );
                }
            }
            if e > 0.9 {
                let hit = match (list, i < LANGUAGES.len()) {
                    (List::Language, false) => Hit::Settings,
                    _ => Hit::Choose(list, i),
                };
                targets.push(Target {
                    rect,
                    hit,
                    highlight: Some(rect),
                });
            }
            y += px(height + OPTION_GAP) * e;
        }
        top + px(Self::list_height(list)) * e
    }
}
