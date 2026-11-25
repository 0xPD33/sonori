//! Layout, drawing, and hit testing of the Settings window. Each frame lays out
//! the open section again from the config; see `docs/design/settings.md`.

use std::collections::HashMap;
use std::f32::consts::FRAC_PI_2;
use std::ops::Range;
use std::time::{Duration, Instant};

use bytemuck::Zeroable;
use winit::event::KeyEvent;
use winit::keyboard::{Key, NamedKey};

use super::model::{
    backend_name, languages, preset_of, vocabulary, Change, Choice, Effect, Flag, Num, Section,
};
use crate::config::{AppConfig, DisplayMode, OutputMode};
use crate::ui::cursor_overlay::PRESETS;
use crate::ui::shapes::{
    glass, Shape, Spring, Ui, CHEVRON, CROSS, FLAT, INK, INK_2, OUTLINE, PANEL, RAISED, SILVER_BAR,
    SLATE, SLATE_FILL, SUNK, TICK,
};
use crate::ui::text_renderer::{Label, TextRenderer, LABEL_LINE};

// Layout in logical px.
/// Room around the glass for its shadow; the surface is this much larger on each side.
pub const MARGIN: f32 = 22.0;
pub const WIDTH: f32 = 820.0;
pub const HEIGHT: f32 = 560.0;
const CORNER: f32 = 22.0;
const SIDEBAR: f32 = 200.0;
const HEADER: f32 = 68.0;
const PAD: f32 = 22.0;
const GAP: f32 = 8.0;
const ROW_PAD: (f32, f32) = (14.0, 12.0);
const TITLE: f32 = 14.0;
const DESC: f32 = 12.5;
const MAX_INPUT: usize = 80;

const EDGE: [f32; 3] = [0.565, 0.659, 0.765];
const TOAST: Duration = Duration::from_millis(1400);
const SHEET_FADE: Duration = Duration::from_millis(160);

/// x, y, width, height in physical px.
type Rect = [f32; 4];

/// What a press at a point acts on.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Hit {
    /// Blocks the press; nothing happens.
    Inert,
    /// Window background that moves the window.
    Drag,
    /// Outside an open sheet or list; closes it.
    Dismiss,
    Nav(Section),
    Close,
    Flag(Flag),
    Choose(Choice, usize),
    OpenSelect(Choice),
    /// An option of the open list.
    Pick(usize),
    Slider(Num),
    Preset(usize),
    Display(DisplayMode),
    Advanced,
    Field(Field),
    AddWord,
    RemoveWord(usize),
    AddReplacement,
    RemoveReplacement(usize),
    OpenConfig,
    SheetCancel,
    SheetConfirm,
}

/// A text field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Word,
    Heard,
    Meant,
}

/// What the window must do after an input.
pub enum Action {
    None,
    Close,
    StartDrag,
    Save(Change),
    ToggleMagic,
    OpenConfig,
}

struct Target {
    rect: Rect,
    hit: Hit,
}

struct Dropdown {
    choice: Choice,
    /// The select the list belongs to.
    anchor: Rect,
    /// Rows scrolled out of view at the top, in logical px.
    scroll: f32,
    /// Where the list showed last frame, for the scroll wheel.
    rect: Rect,
}

/// A change that waits for the user to confirm it in the sheet.
struct Pending {
    change: Change,
    title: String,
    body: String,
    confirm: &'static str,
    since: Instant,
}

struct SliderDrag {
    num: Num,
    value: f32,
}

enum Control {
    Switch(Flag),
    Segmented(Choice),
    Select(Choice),
    Slider(Num),
    /// A read-only value; raised like a keycap, or sunk.
    Value(String, bool),
    Button(&'static str, Hit),
    Meter,
    Nothing,
}

struct Row {
    title: &'static str,
    desc: String,
    control: Control,
}

enum Item {
    Row(Row),
    Group(&'static str),
    Note(String),
    Presets,
    Disclosure(Vec<Row>),
    DisplayCards,
    Vocabulary,
    Replacements,
}

fn row(title: &'static str, desc: impl Into<String>, control: Control) -> Item {
    Item::Row(Row {
        title,
        desc: desc.into(),
        control,
    })
}

/// What a frame shows besides the config.
pub struct Context<'a> {
    pub config: &'a AppConfig,
    /// The bound hotkey, if any, and a line that explains the hotkey state.
    pub hotkey: Option<String>,
    pub hotkey_note: String,
    pub config_path: String,
    pub history_path: String,
    /// Microphone level 0..1 while Sonori records.
    pub level: Option<f32>,
    pub sf: f32,
    pub now: Instant,
}

/// One frame: shapes, then two layers that each draw shape ranges (with an
/// optional scissor rectangle) and then their labels. The second layer holds
/// the toast, an open list, and the sheet.
#[derive(Default)]
pub struct Frame {
    pub shapes: Vec<Shape>,
    pub layers: [Layer; 2],
    pub animating: bool,
}

#[derive(Default)]
pub struct Layer {
    pub batches: Vec<(Range<u32>, Option<Rect>)>,
    pub labels: Vec<Label>,
}

pub struct View {
    pub section: Section,
    /// Content scrolled out of view at the top, in logical px.
    scroll: f32,
    scroll_max: f32,
    advanced_open: bool,
    dropdown: Option<Dropdown>,
    pending: Option<Pending>,
    toast: Option<(String, Instant)>,
    focus: Option<Field>,
    word: String,
    heard: String,
    meant: String,
    slider: Option<SliderDrag>,
    /// Slider tracks of the last frame: x and width in physical px.
    tracks: Vec<(Num, [f32; 2])>,
    hover: Option<Hit>,
    knobs: HashMap<Flag, Spring>,
    /// Segmented pills, as distance of the pill's left edge from the control's
    /// right edge and pill width, so they do not trail the content when it scrolls.
    pills: HashMap<Choice, [Spring; 2]>,
    level: Spring,
    targets: Vec<Target>,
    /// Shape index, layer, and clip where each batch starts.
    marks: Vec<(u32, usize, Option<Rect>)>,
    target_clip: Option<Rect>,
    wraps: HashMap<(String, u32, u32), Vec<String>>,
    animating: bool,
    last: Instant,
    started: Instant,
}

impl View {
    pub fn new(now: Instant) -> Self {
        Self {
            section: Section::General,
            scroll: 0.0,
            scroll_max: 0.0,
            advanced_open: false,
            dropdown: None,
            pending: None,
            toast: None,
            focus: None,
            word: String::new(),
            heard: String::new(),
            meant: String::new(),
            slider: None,
            tracks: Vec::new(),
            hover: None,
            knobs: HashMap::new(),
            pills: HashMap::new(),
            level: Spring::default(),
            targets: Vec::new(),
            marks: Vec::new(),
            target_clip: None,
            wraps: HashMap::new(),
            animating: false,
            last: now,
            started: now,
        }
    }

    pub fn toast(&mut self, message: impl Into<String>) {
        self.toast = Some((message.into(), Instant::now()));
    }

    // ----- input -----

    fn hit(&self, (x, y): (f32, f32)) -> Option<Hit> {
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

    /// Updates hover; returns whether the window must redraw.
    pub fn pointer_moved(&mut self, position: (f32, f32)) -> bool {
        if let Some(slider) = self.slider.as_mut() {
            if let Some((_, [x, w])) = self.tracks.iter().find(|(n, _)| *n == slider.num) {
                let (min, max, _) = slider.num.range();
                slider.value = slider.num.snap(min + (position.0 - x) / w * (max - min));
            }
            return true;
        }
        let hover = self.hit(position);
        let changed = hover != self.hover;
        self.hover = hover;
        changed
    }

    pub fn pointer_left(&mut self) {
        self.hover = None;
    }

    pub fn press(&mut self, position: (f32, f32), config: &AppConfig) -> Action {
        let hit = self.hit(position);
        if !matches!(hit, Some(Hit::Field(_))) {
            self.focus = None;
        }
        let Some(hit) = hit else {
            return Action::None;
        };
        match hit {
            Hit::Inert => {}
            Hit::Drag => return Action::StartDrag,
            Hit::Dismiss => {
                self.dropdown = None;
                self.pending = None;
            }
            Hit::Nav(section) => {
                if self.section != section {
                    self.section = section;
                    self.scroll = 0.0;
                }
            }
            Hit::Close => return Action::Close,
            Hit::Flag(Flag::Magic) => return Action::ToggleMagic,
            Hit::Flag(flag) if !flag.get(config) && flag.asks_first() => {
                self.ask_to_turn_on(flag, config)
            }
            Hit::Flag(flag) => return Action::Save(Change::Flag(flag, !flag.get(config))),
            Hit::Choose(choice, index) => return self.choose(choice, index, config),
            Hit::OpenSelect(choice) => {
                if self.dropdown.as_ref().is_some_and(|d| d.choice == choice) {
                    self.dropdown = None;
                } else if let Some(target) = self.targets.iter().find(|t| t.hit == hit) {
                    let selected = choice.selected(config).unwrap_or(0) as f32;
                    self.dropdown = Some(Dropdown {
                        choice,
                        anchor: target.rect,
                        scroll: (selected - 3.0).max(0.0) * 30.0,
                        rect: [0.0; 4],
                    });
                }
            }
            Hit::Pick(index) => {
                if let Some(dropdown) = self.dropdown.take() {
                    return self.choose(dropdown.choice, index, config);
                }
            }
            Hit::Slider(num) => {
                self.slider = Some(SliderDrag {
                    num,
                    value: num.get(config),
                });
                self.pointer_moved(position);
            }
            Hit::Preset(index) => {
                if preset_of(config) != Some(index) {
                    self.ask_switch(Change::Preset(index), PRESETS[index].name.to_string());
                }
            }
            Hit::Display(mode) => {
                if config.ui_config.display_mode != mode {
                    return Action::Save(Change::Display(mode));
                }
            }
            Hit::Advanced => self.advanced_open = !self.advanced_open,
            Hit::Field(field) => self.focus = Some(field),
            Hit::AddWord => return self.add_word(config),
            Hit::RemoveWord(index) => {
                let mut words = vocabulary(config);
                if index < words.len() {
                    words.remove(index);
                    return Action::Save(Change::Vocabulary(words));
                }
            }
            Hit::AddReplacement => return self.add_replacement(config),
            Hit::RemoveReplacement(index) => {
                let mut map = config.post_process_config.replacements.clone();
                if let Some(key) = map.keys().nth(index).cloned() {
                    map.remove(&key);
                    return Action::Save(Change::Replacements(map));
                }
            }
            Hit::OpenConfig => return Action::OpenConfig,
            Hit::SheetCancel => self.pending = None,
            Hit::SheetConfirm => {
                if let Some(pending) = self.pending.take() {
                    return Action::Save(pending.change);
                }
            }
        }
        Action::None
    }

    /// Ends a slider drag; returns its value as a change, if it moved.
    pub fn release(&mut self, config: &AppConfig) -> Option<Change> {
        let slider = self.slider.take()?;
        ((slider.value - slider.num.get(config)).abs() > 1e-4)
            .then_some(Change::Num(slider.num, slider.value))
    }

    /// Scrolls the list under the pointer, else the content. `dy` is in logical px.
    pub fn scroll(&mut self, dy: f32, position: (f32, f32)) {
        if let Some(dropdown) = self.dropdown.as_mut() {
            let [x, y, w, h] = dropdown.rect;
            if position.0 >= x && position.0 < x + w && position.1 >= y && position.1 < y + h {
                dropdown.scroll += dy;
                return;
            }
            self.dropdown = None;
        }
        self.scroll += dy;
    }

    pub fn key(&mut self, event: &KeyEvent, config: &AppConfig) -> Action {
        match &event.logical_key {
            Key::Named(NamedKey::Escape) => {
                if self.pending.is_some() {
                    self.pending = None;
                } else if self.dropdown.is_some() {
                    self.dropdown = None;
                } else if self.focus.is_some() {
                    self.focus = None;
                } else {
                    return Action::Close;
                }
                return Action::None;
            }
            Key::Named(NamedKey::Enter) if self.pending.is_some() => {
                let pending = self.pending.take().expect("checked above");
                return Action::Save(pending.change);
            }
            _ => {}
        }
        let Some(field) = self.focus else {
            return Action::None;
        };
        match &event.logical_key {
            Key::Named(NamedKey::Enter) => match field {
                Field::Word => return self.add_word(config),
                Field::Heard | Field::Meant => return self.add_replacement(config),
            },
            Key::Named(NamedKey::Tab) => {
                self.focus = Some(match field {
                    Field::Word => Field::Word,
                    Field::Heard => Field::Meant,
                    Field::Meant => Field::Heard,
                });
            }
            Key::Named(NamedKey::Backspace) => {
                self.buffer(field).pop();
            }
            _ => {
                if let Some(text) = &event.text {
                    let buffer = self.buffer(field);
                    for c in text.chars().filter(|c| !c.is_control()) {
                        if buffer.chars().count() < MAX_INPUT {
                            buffer.push(c);
                        }
                    }
                }
            }
        }
        Action::None
    }

    fn buffer(&mut self, field: Field) -> &mut String {
        match field {
            Field::Word => &mut self.word,
            Field::Heard => &mut self.heard,
            Field::Meant => &mut self.meant,
        }
    }

    fn choose(&mut self, choice: Choice, index: usize, config: &AppConfig) -> Action {
        if choice.selected(config) == Some(index) {
            return Action::None;
        }
        let change = Change::Choose(choice, index);
        if choice.effect() != Effect::Model {
            return Action::Save(change);
        }
        let label = choice
            .options(config)
            .get(index)
            .cloned()
            .unwrap_or_default();
        let name = match choice {
            Choice::Precision => format!("{} precision", label.to_lowercase()),
            _ => label,
        };
        self.ask_switch(change, name);
        Action::None
    }

    fn ask_switch(&mut self, change: Change, name: String) {
        self.ask(
            change,
            format!("Switch to {name}?"),
            "Sonori downloads this model the first time, then reloads it. Dictation pauses until it is ready.".to_string(),
            "Switch",
        );
    }

    /// The user reads the side effects before the switch turns on.
    fn ask_to_turn_on(&mut self, flag: Flag, config: &AppConfig) {
        let (title, body, confirm) = match flag {
            Flag::LiveText => (
                "Show words while you speak?",
                format!(
                    "Sonori transcribes your speech again about twice a second and shows the words beside the pointer. Dim words can still change. The pasted text comes from the full transcription, so it can differ a little. This keeps the model busy while you speak: it needs a GPU and a fast model, such as Parakeet. Long dictations are then cut every 20 s instead of at the model's length. Applies after a restart.\n\n{}",
                    live_text_fit(config)
                ),
                "Turn on",
            ),
            Flag::TrainingData => (
                "Save dictations for training?",
                format!(
                    "Sonori will save the audio and the text of every dictation in {}. Anyone who can open this folder can hear and read them, also passwords and private things that you dictate. One hour of dictation uses about 115 MB. Applies after a restart.",
                    config.training_data.dir
                ),
                "Save my dictations",
            ),
            _ => (
                "Keep a history?",
                format!(
                    "Sonori will save the text of every dictation in {}. Anyone who can open this file can read them, also passwords and private things that you dictate.",
                    super::home_relative(&config.history_config.path)
                ),
                "Keep a history",
            ),
        };
        self.ask(Change::Flag(flag, true), title.to_string(), body, confirm);
    }

    fn ask(&mut self, change: Change, title: String, body: String, confirm: &'static str) {
        self.dropdown = None;
        self.pending = Some(Pending {
            change,
            title,
            body,
            confirm,
            since: Instant::now(),
        });
    }

    fn add_word(&mut self, config: &AppConfig) -> Action {
        // Commas separate the words in the config.
        let word = self.word.replace(',', " ").trim().to_string();
        if word.is_empty() {
            return Action::None;
        }
        self.word.clear();
        let mut words = vocabulary(config);
        if words.iter().any(|w| w.eq_ignore_ascii_case(&word)) {
            return Action::None;
        }
        words.push(word);
        Action::Save(Change::Vocabulary(words))
    }

    fn add_replacement(&mut self, config: &AppConfig) -> Action {
        let (heard, meant) = (self.heard.trim().to_string(), self.meant.trim().to_string());
        if heard.is_empty() || meant.is_empty() {
            self.focus = Some(if heard.is_empty() {
                Field::Heard
            } else {
                Field::Meant
            });
            return Action::None;
        }
        self.heard.clear();
        self.meant.clear();
        self.focus = Some(Field::Heard);
        let mut map = config.post_process_config.replacements.clone();
        map.insert(heard, meant);
        Action::Save(Change::Replacements(map))
    }

    // ----- frame -----

    /// Lays out and draws one frame.
    pub fn build(&mut self, cx: &Context, text: &mut TextRenderer) -> Frame {
        let now = cx.now;
        let dt = now.duration_since(self.last).as_secs_f32().min(0.1);
        self.last = now;
        self.animating = false;
        self.targets.clear();
        self.tracks.clear();
        self.marks.clear();

        let sf = cx.sf;
        let px = |v: f32| v * sf;
        let glass_rect = [px(MARGIN), px(MARGIN), px(WIDTH), px(HEIGHT)];
        let mut frame = Frame::default();
        let mut base = Vec::new();
        let mut overlay = Vec::new();
        {
            let mut ui = Ui {
                sf,
                alpha: 1.0,
                shapes: &mut frame.shapes,
                labels: &mut base,
                text,
                clip: None,
            };
            self.clip(&mut ui, 0, None);
            self.chrome(&mut ui, cx, glass_rect);
            self.content(&mut ui, cx, glass_rect, dt);
        }
        // Lists and sheets take every press while they show.
        let modal = self.dropdown.is_some() || self.pending.is_some();
        if modal {
            self.targets.clear();
            self.target_clip = None;
            self.targets.push(Target {
                rect: [0.0, 0.0, 1e6, 1e6],
                hit: Hit::Dismiss,
            });
        }
        {
            let mut ui = Ui {
                sf,
                alpha: 1.0,
                shapes: &mut frame.shapes,
                labels: &mut overlay,
                text,
                clip: None,
            };
            self.clip(&mut ui, 1, None);
            self.draw_toast(&mut ui, glass_rect, now);
            self.draw_dropdown(&mut ui, cx, glass_rect);
            self.draw_sheet(&mut ui, glass_rect, now);
        }

        let total = frame.shapes.len() as u32;
        for (i, &(start, layer, clip)) in self.marks.iter().enumerate() {
            let end = self.marks.get(i + 1).map_or(total, |m| m.0);
            if end > start {
                frame.layers[layer].batches.push((start..end, clip));
            }
        }
        frame.layers[0].labels = base;
        frame.layers[1].labels = overlay;
        frame.animating = self.animating || self.slider.is_some();
        frame
    }

    /// Starts a batch of shapes and labels cut off outside `clip`.
    fn clip(&mut self, ui: &mut Ui, layer: usize, clip: Option<Rect>) {
        self.marks.push((ui.shapes.len() as u32, layer, clip));
        ui.clip = clip;
        self.target_clip = clip;
    }

    fn target(&mut self, rect: Rect, hit: Hit) {
        let rect = match self.target_clip {
            Some(clip) => intersect(rect, clip),
            None => rect,
        };
        if rect[2] > 0.0 && rect[3] > 0.0 {
            self.targets.push(Target { rect, hit });
        }
    }

    /// `text` broken into lines no wider than `width` physical px.
    fn wrap(&mut self, ui: &mut Ui, text: &str, size: f32, bold: bool, width: f32) -> Vec<String> {
        let key = (
            text.to_string(),
            (size * ui.sf * 100.0) as u32 + u32::from(bold),
            width.max(0.0) as u32,
        );
        if let Some(lines) = self.wraps.get(&key) {
            return lines.clone();
        }
        let mut lines = Vec::new();
        let mut line = String::new();
        for word in text.split_whitespace() {
            let candidate = if line.is_empty() {
                word.to_string()
            } else {
                format!("{line} {word}")
            };
            if !line.is_empty() && ui.measure(&candidate, size, bold) > width {
                lines.push(std::mem::replace(&mut line, word.to_string()));
            } else {
                line = candidate;
            }
        }
        if !line.is_empty() {
            lines.push(line);
        }
        if self.wraps.len() > 512 {
            self.wraps.clear();
        }
        self.wraps.insert(key, lines.clone());
        lines
    }

    fn step_spring(
        &mut self,
        spring: &mut Spring,
        goal: f32,
        stiffness: f32,
        damping: f32,
        dt: f32,
    ) {
        spring.step(goal, stiffness, damping, dt);
        if !spring.settled(goal, 0.002) {
            self.animating = true;
        }
    }

    // ----- chrome: glass, sidebar, header -----

    fn chrome(&mut self, ui: &mut Ui, cx: &Context, g: Rect) {
        let sf = ui.sf;
        let px = |v: f32| v * sf;
        ui.shapes.push(glass(
            g[0] + g[2] / 2.0,
            g[1] + g[3] / 2.0,
            g[2] / 2.0,
            g[3] / 2.0,
            px(CORNER),
            px(MARGIN),
            0.94,
            0.0,
            1.0,
        ));
        // The sidebar and the header move the window.
        self.target(g, Hit::Inert);
        self.target([g[0], g[1], px(SIDEBAR), g[3]], Hit::Drag);
        self.target(
            [g[0] + px(SIDEBAR), g[1], g[2] - px(SIDEBAR), px(HEADER)],
            Hit::Drag,
        );

        let side = [g[0], g[1], px(SIDEBAR), g[3]];
        self.clip(ui, 0, Some(side));
        fill(
            ui,
            [g[0], g[1], px(SIDEBAR + CORNER * 2.0), g[3]],
            CORNER,
            [1.0, 1.0, 1.0, 0.18],
        );
        self.clip(ui, 0, None);
        fill(
            ui,
            [g[0] + px(SIDEBAR) - px(0.5), g[1], px(1.0), g[3]],
            0.0,
            [EDGE[0], EDGE[1], EDGE[2], 0.35],
        );

        // Brand: the icon's bars on a raised tile.
        let logo = [g[0] + px(18.0), g[1] + px(18.0), px(26.0), px(26.0)];
        ui.panel(logo, 7.0, RAISED, 1.0);
        let (lx, ly) = (logo[0] + logo[2] / 2.0, logo[1] + logo[3] / 2.0);
        for (i, h) in [4.0, 9.0, 14.0, 8.0, 3.6].into_iter().enumerate() {
            silver_bar(
                ui,
                lx - px(8.0) + px(1.3 + 3.3 * i as f32),
                ly,
                px(2.6),
                px(h),
                px(0.5),
                1.0,
            );
        }
        ui.label(
            "Sonori",
            logo[0] + logo[2] + px(10.0),
            ly,
            15.0,
            true,
            INK,
            1.0,
        );

        let mut y = g[1] + px(58.0);
        for section in Section::ALL {
            let rect = [g[0] + px(10.0), y, px(SIDEBAR - 20.0), px(36.0)];
            let current = self.section == section;
            let hover = self.hover == Some(Hit::Nav(section));
            if current {
                ui.panel(rect, 10.0, RAISED, 1.0);
            } else if hover {
                fill(ui, rect, 10.0, [EDGE[0], EDGE[1], EDGE[2], 0.15]);
            }
            let color = if current || hover { INK } else { INK_2 };
            let cy = y + rect[3] / 2.0;
            ui.icon(
                section.icon(),
                rect[0] + px(18.5),
                cy,
                17.0,
                0.0,
                color,
                1.0,
            );
            ui.label(
                section.title(),
                rect[0] + px(37.0),
                cy,
                13.5,
                true,
                color,
                1.0,
            );
            self.target(rect, Hit::Nav(section));
            y += px(38.0);
        }
        // The config path breaks after a slash, as it has no spaces.
        let mut lines: Vec<String> = Vec::new();
        let mut current = String::new();
        for part in cx.config_path.split_inclusive('/') {
            let candidate = format!("{current}{part}");
            if !current.is_empty() && ui.measure(&candidate, 11.0, false) > px(SIDEBAR - 36.0) {
                lines.push(std::mem::replace(&mut current, part.to_string()));
            } else {
                current = candidate;
            }
        }
        lines.push(current);
        let line = px(11.0 * LABEL_LINE);
        let mut ly = g[1] + g[3] - px(18.0) - line * lines.len() as f32;
        ui.clip = Some(side);
        for l in &lines {
            ui.label_top(l, g[0] + px(20.0), ly, 11.0, false, INK_2, 1.0);
            ly += line;
        }
        ui.clip = None;

        let hx = g[0] + px(SIDEBAR + PAD);
        ui.label_top(
            self.section.title(),
            hx,
            g[1] + px(18.0),
            19.0,
            true,
            INK,
            1.0,
        );
        ui.label_top(
            self.section.subtitle(),
            hx,
            g[1] + px(18.0 + 19.0 * LABEL_LINE + 1.0),
            12.5,
            false,
            INK_2,
            1.0,
        );
        let close = [
            g[0] + g[2] - px(PAD + 30.0),
            g[1] + px(22.0),
            px(30.0),
            px(30.0),
        ];
        ui.panel(close, 15.0, RAISED, 1.0);
        let hover = if self.hover == Some(Hit::Close) {
            INK
        } else {
            SLATE
        };
        ui.icon(
            CROSS,
            close[0] + close[2] / 2.0,
            close[1] + close[3] / 2.0,
            11.0,
            0.0,
            hover,
            1.0,
        );
        self.target(close, Hit::Close);
    }

    // ----- content -----

    fn content(&mut self, ui: &mut Ui, cx: &Context, g: Rect, dt: f32) {
        let sf = ui.sf;
        let px = |v: f32| v * sf;
        let clip = [
            g[0] + px(SIDEBAR + 1.0),
            g[1] + px(HEADER),
            g[2] - px(SIDEBAR + 1.0),
            g[3] - px(HEADER + 4.0),
        ];
        let x = g[0] + px(SIDEBAR + PAD);
        let w = g[2] - px(SIDEBAR + 2.0 * PAD);
        let top = clip[1] + px(6.0);

        self.clip(ui, 0, Some(clip));
        self.scroll = self.scroll.clamp(0.0, self.scroll_max);
        let items = self.items(cx);
        let mut y = top - px(self.scroll);
        for item in &items {
            y += self.item(ui, cx, item, x, y, w, dt) + px(GAP);
        }
        let height = (y - px(GAP) + px(PAD)) - (top - px(self.scroll));
        let visible = clip[3] - px(6.0);
        let max = ((height - visible) / sf).max(0.0);
        self.scroll_max = max;
        if self.scroll > max {
            // The content got shorter; the next frame shows it at a valid offset.
            self.animating = true;
        }
        self.clip(ui, 0, None);
        if max > 0.0 {
            let track = clip[3] - px(12.0);
            let thumb = (track * visible / height).max(px(28.0));
            let ty = clip[1] + px(6.0) + (track - thumb) * (self.scroll / max);
            fill(
                ui,
                [g[0] + g[2] - px(8.0), ty, px(4.0), thumb],
                2.0,
                [SLATE[0], SLATE[1], SLATE[2], 0.22],
            );
        }
    }

    fn items(&self, cx: &Context) -> Vec<Item> {
        let c = cx.config;
        let mut v = Vec::new();
        match self.section {
            Section::General => {
                let (key, desc) = match &cx.hotkey {
                    Some(trigger) => (
                        trigger.clone(),
                        "Press it to dictate, press it again to paste. Hold it for the quick menu."
                            .to_string(),
                    ),
                    None => ("Not set".to_string(), cx.hotkey_note.clone()),
                };
                v.push(row("Hotkey", desc, Control::Value(key, true)));
                v.push(row(
                    "Hotkey mode",
                    "Toggle starts and stops with a press. Hold to talk records while you hold the key. Applies after a restart.",
                    Control::Segmented(Choice::ShortcutMode),
                ));
                v.push(row(
                    "Listening",
                    "On demand records while you hold or toggle the hotkey. Live writes as you speak.",
                    Control::Segmented(Choice::Listening),
                ));
                v.push(row(
                    "Output",
                    "Paste uses the clipboard; Type inserts the text directly; Copy only copies it.",
                    Control::Segmented(Choice::Output),
                ));
                if c.portal_config.output_mode == OutputMode::Paste {
                    v.push(row(
                        "Paste shortcut",
                        "Terminals need Ctrl+Shift+V; most other apps use Ctrl+V. By app picks one for the focused app on niri, Sway and Hyprland.",
                        Control::Select(Choice::PasteShortcut),
                    ));
                }
                if languages(c).is_empty() {
                    v.push(row(
                        "Language",
                        "This model understands English only. Pick another model in Speech for more languages.",
                        Control::Value("English".to_string(), false),
                    ));
                } else {
                    v.push(row(
                        "Language",
                        "The language you speak. Auto detect works with multilingual models.",
                        Control::Select(Choice::Language),
                    ));
                }
                v.push(row(
                    "Magic Mode",
                    "A local language model cleans up grammar and filler words before pasting.",
                    Control::Switch(Flag::Magic),
                ));
                v.push(row(
                    "Sounds",
                    "A short tone when recording starts and stops. Applies after a restart.",
                    Control::Switch(Flag::Sounds),
                ));
                if c.sound_config.enabled {
                    v.push(row(
                        "Volume",
                        "How loud the tones are. Applies after a restart.",
                        Control::Slider(Num::Volume),
                    ));
                }
            }
            Section::Speech => {
                v.push(Item::Presets);
                if preset_of(c).is_none() {
                    v.push(Item::Note(format!(
                        "Now using {} · {}. Pick a card to switch.",
                        backend_name(c.backend_config.backend),
                        c.general_config.model
                    )));
                }
                v.push(Item::Disclosure(vec![
                    Row {
                        title: "Backend",
                        desc: "The speech engine.".to_string(),
                        control: Control::Select(Choice::Backend),
                    },
                    Row {
                        title: "Model",
                        desc: "Larger models are slower and more accurate. Names that end in .en know English only.".to_string(),
                        control: Control::Select(Choice::Model),
                    },
                    Row {
                        title: "Precision",
                        desc: "Lower precision is faster and smaller; higher precision is more exact.".to_string(),
                        control: Control::Segmented(Choice::Precision),
                    },
                    Row {
                        title: "Use the GPU",
                        desc: "Faster transcription with Vulkan, CUDA, or an ONNX GPU provider.".to_string(),
                        control: Control::Switch(Flag::Gpu),
                    },
                    Row {
                        title: "CPU threads",
                        desc: "Threads for transcription on the CPU.".to_string(),
                        control: Control::Slider(Num::Threads),
                    },
                ]));
            }
            Section::Microphone => {
                let device = c
                    .audio_processor_config
                    .input_device
                    .clone()
                    .unwrap_or_else(|| "System default".to_string());
                v.push(row(
                    "Microphone",
                    "To use another microphone, set audio_processor_config.input_device in the config file. Applies after a restart.",
                    Control::Value(device, false),
                ));
                let level = if cx.level.is_some() {
                    "Aim for the middle while you talk normally."
                } else {
                    "Moves while Sonori records. Start a dictation to test the microphone."
                };
                v.push(row("Input level", level, Control::Meter));
                v.push(row(
                    "Start listening",
                    "How easily Sonori treats sound as speech. Use \u{201c}Only clear speech\u{201d} in noisy rooms. Applies after a restart.",
                    Control::Segmented(Choice::Vad),
                ));
            }
            Section::Look => {
                v.push(Item::DisplayCards);
                if c.ui_config.display_mode == DisplayMode::Cursor {
                    if c.general_config.transcription_mode == "realtime" {
                        v.push(Item::Note(
                            "Cursor mode works with On demand listening; Live listening shows the overlay.".to_string(),
                        ));
                    }
                    v.push(row(
                        "Text bubble",
                        "How long the pasted text stays next to the pointer.",
                        Control::Segmented(Choice::Bubble),
                    ));
                    v.push(row(
                        "Words while you speak",
                        "Shows the text beside the pointer while you dictate. Needs a GPU and a fast model, such as Parakeet. Applies after a restart.",
                        Control::Switch(Flag::LiveText),
                    ));
                } else {
                    v.push(Item::Group("Overlay"));
                    v.push(row(
                        "Theme",
                        "Colours of the overlay.",
                        Control::Select(Choice::Theme),
                    ));
                    v.push(row(
                        "Sound picture",
                        "How the overlay draws your voice.",
                        Control::Select(Choice::Skin),
                    ));
                    v.push(row(
                        "Position",
                        "Where the overlay sits. Alt-drag the overlay to place it anywhere.",
                        Control::Select(Choice::Position),
                    ));
                    v.push(row(
                        "Text size",
                        "Size of the transcript in the overlay.",
                        Control::Slider(Num::FontSize),
                    ));
                    v.push(row(
                        "Recording dot",
                        "A pulsing dot while Sonori records.",
                        Control::Switch(Flag::RecordingDot),
                    ));
                    v.push(row(
                        "Typewriter effect",
                        "Types the finished text out letter by letter.",
                        Control::Switch(Flag::Typewriter),
                    ));
                    v.push(row(
                        "Clear on a new recording",
                        "Starts each recording with an empty transcript. Applies after a restart.",
                        Control::Switch(Flag::ClearOnNew),
                    ));
                    v.push(row(
                        "Frame sync",
                        "Syncs frames to the display. Applies after a restart.",
                        Control::Select(Choice::Vsync),
                    ));
                    if c.display_config.vsync_mode == "Disabled" {
                        v.push(row(
                            "Frame-rate cap",
                            "Highest frame rate while frame sync is off.",
                            Control::Slider(Num::Fps),
                        ));
                    }
                }
            }
            Section::Words => {
                v.push(Item::Vocabulary);
                v.push(Item::Replacements);
            }
            Section::Advanced => {
                v.push(Item::Group("Clean-up"));
                v.push(row(
                    "Clean up text",
                    "Applies the rules below and removes stray dashes and spaces. Applies after a restart.",
                    Control::Switch(Flag::CleanUp),
                ));
                v.push(row("Remove filler words", "Drops \u{201c}um\u{201d}, \u{201c}uh\u{201d}, \u{201c}erm\u{201d} and similar.", Control::Switch(Flag::Fillers)));
                v.push(row(
                    "Collapse repeated words",
                    "Turns \u{201c}the the\u{201d} into \u{201c}the\u{201d}. Off keeps \u{201c}had had\u{201d}.",
                    Control::Switch(Flag::Repeats),
                ));
                v.push(row(
                    "Capitalize sentences",
                    "Most models already do this.",
                    Control::Switch(Flag::Capitalize),
                ));
                v.push(row(
                    "End with punctuation",
                    "Adds a period when a dictation ends without one.",
                    Control::Switch(Flag::Punctuation),
                ));
                v.push(Item::Group("Magic Mode"));
                v.push(row(
                    "Magic Mode model",
                    "Loads a local language model at start, so Magic Mode can run. Applies after a restart.",
                    Control::Switch(Flag::MagicModel),
                ));
                v.push(Item::Group("Files"));
                v.push(row(
                    "Keep a history",
                    format!("Saves the text of every dictation to {}.", cx.history_path),
                    Control::Switch(Flag::History),
                ));
                v.push(row(
                    "Save dictations for training",
                    format!(
                        "Saves the audio and the text of every dictation to {}, to train or test speech models.",
                        cx.config.training_data.dir
                    ),
                    Control::Switch(Flag::TrainingData),
                ));
                v.push(row(
                    "Config file",
                    cx.config_path.clone(),
                    Control::Button("Open", Hit::OpenConfig),
                ));
                v.push(Item::Group("App"));
                v.push(row(
                    "Tray icon",
                    "Shows Sonori in the system tray. Applies after a restart.",
                    Control::Switch(Flag::Tray),
                ));
                v.push(row(
                    "Commands",
                    "Bind these in your compositor: sonori toggle, start, stop, cancel, status, copy-last, paste-last, magic, language <code>, menu, settings.",
                    Control::Nothing,
                ));
            }
        }
        v
    }

    /// Draws `item` at `y`; returns its height.
    #[allow(clippy::too_many_arguments)]
    fn item(
        &mut self,
        ui: &mut Ui,
        cx: &Context,
        item: &Item,
        x: f32,
        y: f32,
        w: f32,
        dt: f32,
    ) -> f32 {
        let sf = ui.sf;
        let px = |v: f32| v * sf;
        match item {
            Item::Row(r) => self.row(ui, cx, r, x, y, w, dt),
            Item::Group(title) => {
                ui.label_top(
                    &title.to_uppercase(),
                    x + px(2.0),
                    y + px(10.0),
                    11.5,
                    true,
                    INK_2,
                    1.0,
                );
                px(10.0 + 11.5 * LABEL_LINE)
            }
            Item::Note(note) => {
                let lines = self.wrap(ui, note, DESC, false, w - px(4.0));
                for (i, l) in lines.iter().enumerate() {
                    ui.label_top(
                        l,
                        x + px(2.0),
                        y + px(DESC * LABEL_LINE) * i as f32,
                        DESC,
                        false,
                        INK_2,
                        1.0,
                    );
                }
                px(DESC * LABEL_LINE) * lines.len() as f32
            }
            Item::Presets => self.presets(ui, cx, x, y, w),
            Item::Disclosure(rows) => self.disclosure(ui, cx, rows, x, y, w, dt),
            Item::DisplayCards => self.display_cards(ui, cx, x, y, w),
            Item::Vocabulary => self.vocabulary(ui, cx, x, y, w),
            Item::Replacements => self.replacements(ui, cx, x, y, w),
        }
    }

    fn row_layout(
        &mut self,
        ui: &mut Ui,
        cx: &Context,
        r: &Row,
        w: f32,
    ) -> (f32, f32, Vec<String>) {
        let sf = ui.sf;
        let px = |v: f32| v * sf;
        let (control_w, control_h) = self.control_size(ui, cx, &r.control);
        let gap = if control_w > 0.0 { px(14.0) } else { 0.0 };
        let text_w = w - px(2.0 * ROW_PAD.0) - control_w - gap;
        let lines = self.wrap(ui, &r.desc, DESC, false, text_w);
        let mut text_h = px(TITLE * LABEL_LINE);
        if !lines.is_empty() {
            text_h += px(2.0) + px(DESC * LABEL_LINE) * lines.len() as f32;
        }
        let h = text_h.max(control_h) + px(2.0 * ROW_PAD.1);
        (h, text_h, lines)
    }

    #[allow(clippy::too_many_arguments)]
    fn row(&mut self, ui: &mut Ui, cx: &Context, r: &Row, x: f32, y: f32, w: f32, dt: f32) -> f32 {
        let sf = ui.sf;
        let px = |v: f32| v * sf;
        let (h, text_h, lines) = self.row_layout(ui, cx, r, w);
        card(ui, [x, y, w, h], 13.0, 0.52, 0.28);
        let ty = y + (h - text_h) / 2.0;
        ui.label_top(r.title, x + px(ROW_PAD.0), ty, TITLE, true, INK, 1.0);
        let mut ly = ty + px(TITLE * LABEL_LINE + 2.0);
        for l in &lines {
            ui.label_top(l, x + px(ROW_PAD.0), ly, DESC, false, INK_2, 1.0);
            ly += px(DESC * LABEL_LINE);
        }
        self.control(ui, cx, &r.control, x + w - px(ROW_PAD.0), y + h / 2.0, dt);
        h
    }

    fn control_size(&mut self, ui: &mut Ui, cx: &Context, control: &Control) -> (f32, f32) {
        let sf = ui.sf;
        let px = |v: f32| v * sf;
        match control {
            Control::Switch(_) => (px(42.0), px(24.0)),
            Control::Segmented(choice) => {
                let options = choice.options(cx.config);
                let w: f32 = options
                    .iter()
                    .map(|o| ui.measure(o, 12.5, true) + px(24.0))
                    .sum();
                (
                    w + px(2.0) * (options.len().max(1) - 1) as f32 + px(6.0),
                    px(33.0),
                )
            }
            Control::Select(choice) => {
                let label = select_label(*choice, cx.config);
                (ui.measure(&label, 12.5, true) + px(42.0), px(31.0))
            }
            Control::Slider(num) => (px(160.0) + ui.measure(num.widest(), 12.5, true), px(24.0)),
            Control::Value(text, _) => (ui.measure(text, 12.0, false) + px(16.0), px(24.0)),
            Control::Button(label, _) => (ui.measure(label, 12.5, true) + px(24.0), px(29.0)),
            Control::Meter => (px(16.0 * 4.0 + 15.0 * 3.0), px(22.0)),
            Control::Nothing => (0.0, 0.0),
        }
    }

    /// Draws `control` with its right edge at `right`, centred on `cy`.
    fn control(
        &mut self,
        ui: &mut Ui,
        cx: &Context,
        control: &Control,
        right: f32,
        cy: f32,
        dt: f32,
    ) {
        let sf = ui.sf;
        let px = |v: f32| v * sf;
        let config = cx.config;
        match control {
            Control::Switch(flag) => {
                let goal = if flag.get(config) { 1.0 } else { 0.0 };
                let mut knob = *self.knobs.entry(*flag).or_insert(Spring::at(goal));
                self.step_spring(&mut knob, goal, 500.0, 34.0, dt);
                self.knobs.insert(*flag, knob);
                let k = knob.x.clamp(0.0, 1.0);
                let track = [right - px(42.0), cy - px(12.0), px(42.0), px(24.0)];
                ui.panel(track, 12.0, SUNK, 1.0);
                ui.panel(track, 12.0, SLATE_FILL, k);
                silver_knob(ui, track[0] + px(12.0 + 18.0 * knob.x), cy, px(9.0));
                self.target(grow(track, px(4.0)), Hit::Flag(*flag));
            }
            Control::Segmented(choice) => {
                let options = choice.options(config);
                let names: Vec<&str> = options.iter().map(String::as_str).collect();
                let selected = choice.selected(config);
                // The pill springs towards the selected button, which sits where
                // `Ui::segmented` lays it out.
                let mut springs = [Spring::default(); 2];
                if let Some(i) = selected {
                    let widths: Vec<f32> = names
                        .iter()
                        .map(|n| ui.measure(n, 12.5, true) + px(24.0))
                        .collect();
                    let after: f32 = widths[i + 1..].iter().map(|w| w + px(2.0)).sum();
                    let goal = [after + px(3.0) + widths[i], widths[i]];
                    springs = *self
                        .pills
                        .entry(*choice)
                        .or_insert([Spring::at(goal[0]), Spring::at(goal[1])]);
                    for (spring, goal) in springs.iter_mut().zip(goal) {
                        self.step_spring(spring, goal, 600.0, 44.0, dt);
                    }
                    self.pills.insert(*choice, springs);
                }
                let pill = [Spring::at(right - springs[0].x), springs[1]];
                let buttons = ui.segmented(&names, selected, pill, right, cy);
                for (i, rect) in buttons.iter().enumerate() {
                    self.target(*rect, Hit::Choose(*choice, i));
                }
            }
            Control::Select(choice) => {
                let label = select_label(*choice, config);
                let w = ui.measure(&label, 12.5, true) + px(42.0);
                let rect = [right - w, cy - px(15.5), w, px(31.0)];
                ui.panel(rect, 9.0, SUNK, 1.0);
                if self.hover == Some(Hit::OpenSelect(*choice)) {
                    fill(ui, rect, 9.0, [EDGE[0], EDGE[1], EDGE[2], 0.12]);
                }
                ui.label(&label, rect[0] + px(12.0), cy, 12.5, true, INK, 1.0);
                ui.icon(CHEVRON, right - px(15.0), cy, 10.0, FRAC_PI_2, SLATE, 1.0);
                self.target(rect, Hit::OpenSelect(*choice));
                if let Some(dropdown) = self.dropdown.as_mut().filter(|d| d.choice == *choice) {
                    dropdown.anchor = rect;
                }
            }
            Control::Slider(num) => {
                let value = match &self.slider {
                    Some(drag) if drag.num == *num => drag.value,
                    _ => num.get(config),
                };
                let (min, max, _) = num.range();
                let f = ((value - min) / (max - min)).clamp(0.0, 1.0);
                let value_w = ui.measure(num.widest(), 12.5, true);
                let track_w = px(150.0);
                let tx = right - value_w - px(10.0) - track_w;
                let track = [tx, cy - px(2.0), track_w, px(4.0)];
                ui.panel(track, 2.0, SUNK, 1.0);
                if f > 0.0 {
                    ui.panel(
                        [tx, cy - px(2.0), track_w * f, px(4.0)],
                        2.0,
                        SLATE_FILL,
                        1.0,
                    );
                }
                silver_knob(ui, tx + track_w * f, cy, px(9.0));
                let text = num.format(value);
                let text_w = ui.measure(&text, 12.5, true);
                ui.label(&text, right - text_w, cy, 12.5, true, INK, 1.0);
                self.tracks.push((*num, [tx, track_w]));
                self.target(
                    [tx - px(10.0), cy - px(12.0), track_w + px(20.0), px(24.0)],
                    Hit::Slider(*num),
                );
            }
            Control::Value(text, raised) => {
                let w = ui.measure(text, 12.0, false) + px(16.0);
                let rect = [right - w, cy - px(12.0), w, px(24.0)];
                ui.panel(rect, 7.0, if *raised { RAISED } else { SUNK }, 1.0);
                ui.label(text, rect[0] + px(8.0), cy, 12.0, false, INK, 1.0);
            }
            Control::Button(label, hit) => {
                let w = ui.measure(label, 12.5, true) + px(24.0);
                let rect = [right - w, cy - px(14.5), w, px(29.0)];
                button(ui, rect, label, false, self.hover == Some(*hit));
                self.target(rect, *hit);
            }
            Control::Meter => {
                let goal = cx.level.unwrap_or(0.0);
                let mut level = self.level;
                self.step_spring(&mut level, goal, 400.0, 30.0, dt);
                self.level = level;
                if cx.level.is_some() {
                    self.animating = true;
                }
                let t = cx.now.duration_since(self.started).as_secs_f32();
                let left = right - px(16.0 * 4.0 + 15.0 * 3.0);
                for i in 0..16 {
                    let shape = (1.0 - (i as f32 - 7.5).abs() / 10.0).max(0.0);
                    let wobble = 0.8 + 0.2 * (9.0 * t + 1.7 * i as f32).sin();
                    let h = px(4.0) + px(16.0) * level.x.clamp(0.0, 1.2) * shape * wobble;
                    silver_bar(
                        ui,
                        left + px(2.0 + 7.0 * i as f32),
                        cy,
                        px(4.0),
                        h,
                        px(0.8),
                        1.0,
                    );
                }
            }
            Control::Nothing => {}
        }
    }

    fn presets(&mut self, ui: &mut Ui, cx: &Context, x: f32, y: f32, w: f32) -> f32 {
        let sf = ui.sf;
        let px = |v: f32| v * sf;
        let card_w = (w - px(2.0 * GAP)) / 3.0;
        let pad = px(12.0);
        let details: Vec<Vec<String>> = PRESETS
            .iter()
            .map(|p| self.wrap(ui, p.detail, 12.0, false, card_w - 2.0 * pad))
            .collect();
        let lines = details.iter().map(Vec::len).max().unwrap_or(1) as f32;
        let h =
            2.0 * pad + px(18.0 + 8.0 + TITLE * LABEL_LINE + 3.0) + lines * px(12.0 * LABEL_LINE);
        let current = preset_of(cx.config);
        for (i, preset) in PRESETS.iter().enumerate() {
            let (col, line) = ((i % 3) as f32, (i / 3) as f32);
            let rect = [
                x + col * (card_w + px(GAP)),
                y + line * (h + px(GAP)),
                card_w,
                h,
            ];
            if current == Some(i) {
                ui.panel(rect, 13.0, RAISED, 1.0);
                ring(ui, rect, 13.0, 2.0, [SLATE[0], SLATE[1], SLATE[2], 1.0]);
            } else {
                card(ui, rect, 13.0, 0.5, 0.3);
                if self.hover == Some(Hit::Preset(i)) {
                    fill(ui, rect, 13.0, [1.0, 1.0, 1.0, 0.35]);
                }
            }
            let base = rect[1] + pad + px(18.0);
            for j in 0..5 {
                let bh = px(6.0 + 3.0 * j as f32);
                let bx = rect[0] + pad + px(2.0 + 7.0 * j as f32);
                if j < preset.bars {
                    silver_bar(ui, bx, base - bh / 2.0, px(4.0), bh, px(0.6), 1.0);
                } else {
                    fill(
                        ui,
                        [bx - px(2.0), base - bh, px(4.0), bh],
                        2.0,
                        [EDGE[0], EDGE[1], EDGE[2], 0.35],
                    );
                }
            }
            let name_y = base + px(8.0);
            ui.label_top(preset.name, rect[0] + pad, name_y, TITLE, true, INK, 1.0);
            let mut ly = name_y + px(TITLE * LABEL_LINE + 3.0);
            for l in &details[i] {
                ui.label_top(l, rect[0] + pad, ly, 12.0, false, INK_2, 1.0);
                ly += px(12.0 * LABEL_LINE);
            }
            self.target(rect, Hit::Preset(i));
        }
        let rows = PRESETS.len().div_ceil(3) as f32;
        rows * h + (rows - 1.0) * px(GAP)
    }

    #[allow(clippy::too_many_arguments)]
    fn disclosure(
        &mut self,
        ui: &mut Ui,
        cx: &Context,
        rows: &[Row],
        x: f32,
        y: f32,
        w: f32,
        dt: f32,
    ) -> f32 {
        let sf = ui.sf;
        let px = |v: f32| v * sf;
        let header = px(40.0);
        let bg = ui.shapes.len();
        ui.shapes.push(Shape::zeroed());
        ui.shapes.push(Shape::zeroed());
        let angle = if self.advanced_open { FRAC_PI_2 } else { 0.0 };
        ui.icon(
            CHEVRON,
            x + px(ROW_PAD.0 + 4.0),
            y + header / 2.0,
            10.0,
            angle,
            INK_2,
            1.0,
        );
        ui.label(
            "Advanced: backend, model, GPU",
            x + px(ROW_PAD.0 + 16.0),
            y + header / 2.0,
            13.0,
            true,
            INK_2,
            1.0,
        );
        self.target([x, y, w, header], Hit::Advanced);
        let mut h = header;
        if self.advanced_open {
            let inset = px(10.0);
            for r in rows {
                h += self.row(ui, cx, r, x + inset, y + h, w - 2.0 * inset, dt) + px(GAP);
            }
            h += inset - px(GAP);
        }
        ui.shapes[bg] = fill_shape(sf, [x, y, w, h], 13.0, [1.0, 1.0, 1.0, 0.35]);
        ui.shapes[bg + 1] = ring_shape(
            sf,
            [x, y, w, h],
            13.0,
            1.0,
            [EDGE[0], EDGE[1], EDGE[2], 0.25],
        );
        h
    }

    fn display_cards(&mut self, ui: &mut Ui, cx: &Context, x: f32, y: f32, w: f32) -> f32 {
        let sf = ui.sf;
        let px = |v: f32| v * sf;
        let card_w = (w - px(GAP)) / 2.0;
        let pad = px(10.0);
        let cards = [
            (
                DisplayMode::Cursor,
                "Cursor",
                "A small glass tile follows your pointer while you talk.",
            ),
            (
                DisplayMode::Overlay,
                "Overlay",
                "A fixed panel at the screen edge with text and buttons.",
            ),
        ];
        let descs: Vec<Vec<String>> = cards
            .iter()
            .map(|(_, _, d)| self.wrap(ui, d, 12.0, false, card_w - 2.0 * pad))
            .collect();
        let lines = descs.iter().map(Vec::len).max().unwrap_or(1) as f32;
        let preview_h = px(76.0);
        let h = 2.0 * pad
            + preview_h
            + px(8.0 + 13.5 * LABEL_LINE + 4.0)
            + lines * px(12.0 * LABEL_LINE);
        for (i, (mode, name, _)) in cards.iter().enumerate() {
            let rect = [x + i as f32 * (card_w + px(GAP)), y, card_w, h];
            if cx.config.ui_config.display_mode == *mode {
                fill(ui, rect, 13.0, [1.0, 1.0, 1.0, 1.0]);
                ring(ui, rect, 13.0, 2.0, [SLATE[0], SLATE[1], SLATE[2], 1.0]);
            } else {
                card(ui, rect, 13.0, 0.5, 0.3);
                if self.hover == Some(Hit::Display(*mode)) {
                    fill(ui, rect, 13.0, [1.0, 1.0, 1.0, 0.35]);
                }
            }
            let p = [rect[0] + pad, rect[1] + pad, card_w - 2.0 * pad, preview_h];
            fill(ui, p, 9.0, [0.13, 0.16, 0.22, 1.0]);
            match mode {
                DisplayMode::Cursor => {
                    let (tx, ty) = (p[0] + p[2] * 0.44 + px(26.0), p[1] + p[3] * 0.34 + px(16.0));
                    ui.shapes.push(glass(
                        tx,
                        ty,
                        px(26.0),
                        px(16.0),
                        px(10.0),
                        px(6.0),
                        0.62,
                        0.0,
                        1.0,
                    ));
                    for (j, bh) in [5.0, 9.0, 13.0, 8.0, 4.0].into_iter().enumerate() {
                        silver_bar(
                            ui,
                            tx + px(5.0 * (j as f32 - 2.0)),
                            ty,
                            px(2.6),
                            px(bh),
                            px(0.5),
                            1.0,
                        );
                    }
                }
                DisplayMode::Overlay => {
                    let bar = [
                        p[0] + p[2] * 0.3,
                        p[1] + p[3] - px(42.0),
                        p[2] * 0.4,
                        px(34.0),
                    ];
                    fill(ui, bar, 7.0, [0.0, 0.0, 0.0, 0.6]);
                    ring(ui, bar, 7.0, 1.0, [1.0, 1.0, 1.0, 0.15]);
                }
            }
            let name_y = p[1] + preview_h + px(8.0);
            ui.label_top(name, rect[0] + pad, name_y, 13.5, true, INK, 1.0);
            let mut ly = name_y + px(13.5 * LABEL_LINE + 4.0);
            for l in &descs[i] {
                ui.label_top(l, rect[0] + pad, ly, 12.0, false, INK_2, 1.0);
                ly += px(12.0 * LABEL_LINE);
            }
            self.target(rect, Hit::Display(*mode));
        }
        h
    }

    /// Title and description of a full-width card; returns the y below them.
    fn card_heading(
        &mut self,
        ui: &mut Ui,
        title: &str,
        desc: &str,
        x: f32,
        y: f32,
        w: f32,
    ) -> f32 {
        let sf = ui.sf;
        let px = |v: f32| v * sf;
        let tx = x + px(ROW_PAD.0);
        ui.label_top(title, tx, y + px(ROW_PAD.1), TITLE, true, INK, 1.0);
        let mut ly = y + px(ROW_PAD.1 + TITLE * LABEL_LINE + 2.0);
        for l in self.wrap(ui, desc, DESC, false, w - px(2.0 * ROW_PAD.0)) {
            ui.label_top(&l, tx, ly, DESC, false, INK_2, 1.0);
            ly += px(DESC * LABEL_LINE);
        }
        ly
    }

    fn vocabulary(&mut self, ui: &mut Ui, cx: &Context, x: f32, y: f32, w: f32) -> f32 {
        let sf = ui.sf;
        let px = |v: f32| v * sf;
        let bg = ui.shapes.len();
        ui.shapes.push(Shape::zeroed());
        ui.shapes.push(Shape::zeroed());
        let desc = if cx.config.backend_config.backend == speechcore::BackendType::WhisperCpp {
            "Names and terms the model should expect. Applies after a restart."
        } else {
            "Names and terms the model should expect. Only Whisper.cpp models use this list; Replacements work with every model."
        };
        let mut cy = self.card_heading(ui, "Vocabulary", desc, x, y, w) + px(10.0);
        let left = x + px(ROW_PAD.0);
        let right = x + w - px(ROW_PAD.0);
        let words = vocabulary(cx.config);
        let chip_h = px(26.0);
        if words.is_empty() {
            ui.label_top("No words yet.", left, cy, DESC, false, INK_2, 1.0);
            cy += px(DESC * LABEL_LINE);
        } else {
            let mut cx_ = left;
            for (i, word) in words.iter().enumerate() {
                let tw = ui.measure(word, 12.5, true);
                let chip_w = px(10.0) + tw + px(24.0);
                if cx_ > left && cx_ + chip_w > right {
                    cx_ = left;
                    cy += chip_h + px(6.0);
                }
                let rect = [cx_, cy, chip_w, chip_h];
                ui.panel(rect, 13.0, RAISED, 1.0);
                ui.label(
                    word,
                    cx_ + px(10.0),
                    cy + chip_h / 2.0,
                    12.5,
                    true,
                    INK,
                    1.0,
                );
                let remove = [
                    cx_ + chip_w - px(22.0),
                    cy + px(3.0),
                    px(20.0),
                    chip_h - px(6.0),
                ];
                let color = if self.hover == Some(Hit::RemoveWord(i)) {
                    INK
                } else {
                    INK_2
                };
                ui.icon(
                    CROSS,
                    remove[0] + remove[2] / 2.0 - px(1.0),
                    cy + chip_h / 2.0,
                    8.0,
                    0.0,
                    color,
                    1.0,
                );
                self.target(remove, Hit::RemoveWord(i));
                cx_ += chip_w + px(6.0);
            }
            cy += chip_h;
        }
        cy += px(10.0);
        let add_w = ui.measure("Add", 12.5, true) + px(24.0);
        let field = [left, cy, right - left - add_w - px(6.0), px(31.0)];
        self.field(ui, Field::Word, field, "Add a word");
        let add = [right - add_w, cy, add_w, px(31.0)];
        button(ui, add, "Add", true, self.hover == Some(Hit::AddWord));
        self.target(add, Hit::AddWord);
        let h = cy + px(31.0) + px(ROW_PAD.1) - y;
        ui.shapes[bg] = fill_shape(sf, [x, y, w, h], 13.0, [1.0, 1.0, 1.0, 0.52]);
        ui.shapes[bg + 1] = ring_shape(
            sf,
            [x, y, w, h],
            13.0,
            1.0,
            [EDGE[0], EDGE[1], EDGE[2], 0.28],
        );
        h
    }

    fn replacements(&mut self, ui: &mut Ui, cx: &Context, x: f32, y: f32, w: f32) -> f32 {
        let sf = ui.sf;
        let px = |v: f32| v * sf;
        let bg = ui.shapes.len();
        ui.shapes.push(Shape::zeroed());
        ui.shapes.push(Shape::zeroed());
        let mut cy = self.card_heading(
            ui,
            "Replacements",
            "Fix words Sonori keeps hearing wrong. Whole words, any case. Applies after a restart.",
            x,
            y,
            w,
        ) + px(10.0);
        let left = x + px(ROW_PAD.0);
        let inner = w - px(2.0 * ROW_PAD.0);
        let (arrow_w, remove_w, gap) = (px(20.0), px(28.0), px(6.0));
        let col = (inner - arrow_w - remove_w - 3.0 * gap) / 2.0;
        let cols = [
            left,
            left + col + gap,
            left + col + arrow_w + 2.0 * gap,
            left + inner - remove_w,
        ];
        let row_h = px(30.0);
        for (i, (heard, meant)) in cx
            .config
            .post_process_config
            .replacements
            .iter()
            .enumerate()
        {
            let mid = cy + row_h / 2.0;
            for (text, x0, raised) in [(heard, cols[0], false), (meant, cols[2], true)] {
                let kw = (ui.measure(text, 12.5, false) + px(16.0)).min(col);
                let rect = [x0, mid - px(12.0), kw, px(24.0)];
                ui.panel(rect, 7.0, if raised { RAISED } else { SUNK }, 1.0);
                let clip = ui.clip;
                ui.clip = Some(clip.map_or(rect, |c| intersect(c, rect)));
                ui.label(text, x0 + px(8.0), mid, 12.5, false, INK, 1.0);
                ui.clip = clip;
            }
            ui.label(
                "\u{2192}",
                cols[1] + arrow_w / 2.0 - px(5.0),
                mid,
                13.0,
                false,
                INK_2,
                1.0,
            );
            let remove = [cols[3], mid - px(13.0), remove_w, px(26.0)];
            let color = if self.hover == Some(Hit::RemoveReplacement(i)) {
                INK
            } else {
                INK_2
            };
            ui.icon(
                CROSS,
                remove[0] + remove[2] / 2.0,
                mid,
                9.0,
                0.0,
                color,
                1.0,
            );
            self.target(remove, Hit::RemoveReplacement(i));
            cy += row_h + px(6.0);
        }
        let mid = cy + px(31.0) / 2.0;
        self.field(ui, Field::Heard, [cols[0], cy, col, px(31.0)], "heard");
        ui.label(
            "\u{2192}",
            cols[1] + arrow_w / 2.0 - px(5.0),
            mid,
            13.0,
            false,
            INK_2,
            1.0,
        );
        self.field(ui, Field::Meant, [cols[2], cy, col, px(31.0)], "meant");
        let add = [cols[3], cy, remove_w, px(31.0)];
        button(ui, add, "+", true, self.hover == Some(Hit::AddReplacement));
        self.target(add, Hit::AddReplacement);
        let h = cy + px(31.0) + px(ROW_PAD.1) - y;
        ui.shapes[bg] = fill_shape(sf, [x, y, w, h], 13.0, [1.0, 1.0, 1.0, 0.52]);
        ui.shapes[bg + 1] = ring_shape(
            sf,
            [x, y, w, h],
            13.0,
            1.0,
            [EDGE[0], EDGE[1], EDGE[2], 0.28],
        );
        h
    }

    fn field(&mut self, ui: &mut Ui, field: Field, rect: Rect, placeholder: &str) {
        let sf = ui.sf;
        let px = |v: f32| v * sf;
        ui.panel(rect, 9.0, SUNK, 1.0);
        let focused = self.focus == Some(field);
        if focused {
            ring(ui, rect, 9.0, 1.5, [SLATE[0], SLATE[1], SLATE[2], 0.9]);
        }
        let value = match field {
            Field::Word => self.word.clone(),
            Field::Heard => self.heard.clone(),
            Field::Meant => self.meant.clone(),
        };
        let cy = rect[1] + rect[3] / 2.0;
        let pad = px(10.0);
        let inner = rect[2] - 2.0 * pad;
        let tw = if value.is_empty() {
            0.0
        } else {
            ui.measure(&value, 13.0, false)
        };
        // Keep the end of long text, where the caret is, in view.
        let shift = if focused {
            (tw - inner + px(2.0)).max(0.0)
        } else {
            0.0
        };
        let clip = ui.clip;
        let inside = [rect[0] + px(4.0), rect[1], rect[2] - px(8.0), rect[3]];
        ui.clip = Some(clip.map_or(inside, |c| intersect(c, inside)));
        if value.is_empty() {
            ui.label(placeholder, rect[0] + pad, cy, 13.0, false, INK_2, 0.75);
        } else {
            ui.label(&value, rect[0] + pad - shift, cy, 13.0, false, INK, 1.0);
        }
        ui.clip = clip;
        if focused {
            let caret_x = rect[0] + pad - shift + tw + px(1.0);
            fill(
                ui,
                [caret_x, cy - px(8.0), px(1.5), px(16.0)],
                0.5,
                [SLATE[0], SLATE[1], SLATE[2], 1.0],
            );
        }
        self.target(rect, Hit::Field(field));
    }

    // ----- overlay layer -----

    fn draw_toast(&mut self, ui: &mut Ui, g: Rect, now: Instant) {
        let Some((message, since)) = &self.toast else {
            return;
        };
        let t = now.duration_since(*since);
        if t >= TOAST {
            self.toast = None;
            return;
        }
        self.animating = true;
        let sf = ui.sf;
        let px = |v: f32| v * sf;
        let t = t.as_secs_f32();
        let fade = 0.18;
        let alpha = (t / fade)
            .min(1.0)
            .min((TOAST.as_secs_f32() - t) / fade)
            .clamp(0.0, 1.0);
        let rise = px(6.0) * (1.0 - (t / fade).min(1.0));
        let message = message.clone();
        let tw = ui.measure(&message, 12.0, true);
        let (w, h) = (tw + px(24.0), px(28.0));
        let (right, bottom) = (g[0] + g[2] - px(18.0), g[1] + g[3] - px(16.0) + rise);
        ui.shapes.push(glass(
            right - w / 2.0,
            bottom - h / 2.0,
            w / 2.0,
            h / 2.0,
            h / 2.0,
            px(10.0),
            0.92,
            0.0,
            alpha,
        ));
        ui.label(
            &message,
            right - w + px(12.0),
            bottom - h / 2.0,
            12.0,
            true,
            INK,
            alpha,
        );
    }

    fn draw_dropdown(&mut self, ui: &mut Ui, cx: &Context, g: Rect) {
        let Some(dropdown) = &self.dropdown else {
            return;
        };
        let sf = ui.sf;
        let px = |v: f32| v * sf;
        let choice = dropdown.choice;
        let anchor = dropdown.anchor;
        let options = choice.options(cx.config);
        let selected = choice.selected(cx.config);
        let item = 30.0;
        let pad = px(6.0);
        let visible = options.len().min(8) as f32;
        let list_h = px(visible * item) + 2.0 * pad;
        let widest = options
            .iter()
            .map(|o| ui.measure(o, 13.0, false))
            .fold(0.0, f32::max);
        let w = (widest + px(52.0)).max(anchor[2]).min(px(300.0));
        let x = (anchor[0] + anchor[2] - w).max(g[0] + px(8.0));
        let below = anchor[1] + anchor[3] + px(4.0);
        let y = if below + list_h <= g[1] + g[3] - px(8.0) {
            below
        } else {
            (anchor[1] - px(4.0) - list_h).max(g[1] + px(8.0))
        };
        let max_scroll = ((options.len() as f32 - visible) * item).max(0.0);
        let scroll = dropdown.scroll.clamp(0.0, max_scroll);
        if let Some(d) = self.dropdown.as_mut() {
            d.scroll = scroll;
            d.rect = [x, y, w, list_h];
        }
        ui.shapes.push(glass(
            x + w / 2.0,
            y + list_h / 2.0,
            w / 2.0,
            list_h / 2.0,
            px(12.0),
            px(14.0),
            0.97,
            0.0,
            1.0,
        ));
        self.target([x, y, w, list_h], Hit::Inert);
        let clip = [x, y + pad, w, list_h - 2.0 * pad];
        self.clip(ui, 1, Some(clip));
        for (i, option) in options.iter().enumerate() {
            let oy = y + pad + px(i as f32 * item - scroll);
            let rect = [x + pad, oy, w - 2.0 * pad, px(item - 2.0)];
            let cy = oy + rect[3] / 2.0;
            if selected == Some(i) {
                ui.panel(rect, 8.0, RAISED, 1.0);
                ui.icon(
                    TICK,
                    rect[0] + rect[2] - px(14.0),
                    cy,
                    12.0,
                    0.0,
                    SLATE,
                    1.0,
                );
            } else if self.hover == Some(Hit::Pick(i)) {
                fill(ui, rect, 8.0, [EDGE[0], EDGE[1], EDGE[2], 0.18]);
            }
            ui.label(option, rect[0] + px(10.0), cy, 13.0, false, INK, 1.0);
            self.target(rect, Hit::Pick(i));
        }
        self.clip(ui, 1, None);
        if max_scroll > 0.0 {
            let track = list_h - 2.0 * pad;
            let thumb = (track * visible / options.len() as f32).max(px(20.0));
            let ty = y + pad + (track - thumb) * scroll / max_scroll;
            fill(
                ui,
                [x + w - px(5.0), ty, px(3.0), thumb],
                1.5,
                [SLATE[0], SLATE[1], SLATE[2], 0.25],
            );
        }
    }

    fn draw_sheet(&mut self, ui: &mut Ui, g: Rect, now: Instant) {
        let Some((title, body, confirm, since)) = self
            .pending
            .as_ref()
            .map(|p| (p.title.clone(), p.body.clone(), p.confirm, p.since))
        else {
            return;
        };
        let sf = ui.sf;
        let px = |v: f32| v * sf;
        let t = now.duration_since(since).as_secs_f32() / SHEET_FADE.as_secs_f32();
        if t < 1.0 {
            self.animating = true;
        }
        let fade = 1.0 - (1.0 - t.min(1.0)).powi(3);
        fill(ui, g, CORNER, [0.055, 0.07, 0.094, 0.25 * fade]);
        let w = px(340.0);
        let pad = px(18.0);
        // Paragraphs are split by a blank line, which stays as an empty line.
        let mut lines = Vec::new();
        for (i, paragraph) in body.split("\n\n").enumerate() {
            if i > 0 {
                lines.push(String::new());
            }
            lines.extend(self.wrap(ui, paragraph, 13.0, false, w - 2.0 * pad));
        }
        let line = px(13.0 * 1.5);
        let h =
            2.0 * pad + px(16.0 * LABEL_LINE + 10.0) + line * lines.len() as f32 + px(14.0 + 29.0);
        let (x, y) = (
            g[0] + (g[2] - w) / 2.0,
            g[1] + (g[3] - h) / 2.0 + px(6.0) * (1.0 - fade),
        );
        ui.alpha = fade;
        ui.shapes.push(glass(
            x + w / 2.0,
            y + h / 2.0,
            w / 2.0,
            h / 2.0,
            px(18.0),
            px(18.0),
            0.97,
            0.0,
            fade,
        ));
        self.target([x, y, w, h], Hit::Inert);
        ui.label_top(&title, x + pad, y + pad, 16.0, true, INK, 1.0);
        let mut ly = y + pad + px(16.0 * LABEL_LINE + 10.0);
        for l in &lines {
            ui.label_top(
                l,
                x + pad,
                ly + (line - px(13.0 * LABEL_LINE)) / 2.0,
                13.0,
                false,
                INK_2,
                1.0,
            );
            ly += line;
        }
        let by = y + h - pad - px(29.0);
        let switch_w = ui.measure(confirm, 12.5, true) + px(24.0);
        let cancel_w = ui.measure("Cancel", 12.5, true) + px(24.0);
        let switch = [x + w - pad - switch_w, by, switch_w, px(29.0)];
        let cancel = [switch[0] - px(8.0) - cancel_w, by, cancel_w, px(29.0)];
        button(
            ui,
            cancel,
            "Cancel",
            false,
            self.hover == Some(Hit::SheetCancel),
        );
        button(
            ui,
            switch,
            confirm,
            true,
            self.hover == Some(Hit::SheetConfirm),
        );
        self.target(cancel, Hit::SheetCancel);
        self.target(switch, Hit::SheetConfirm);
        ui.alpha = 1.0;
    }
}

/// How well the current model and GPU setting suit live words.
fn live_text_fit(config: &AppConfig) -> String {
    use speechcore::BackendType;
    let model = &config.general_config.model;
    if !config.backend_config.gpu_enabled {
        return format!("Your setup: {model} without the GPU. The words will come late, and Stop can take longer.");
    }
    match config.backend_config.backend {
        BackendType::Parakeet => format!("Your setup: {model} on the GPU. This suits live words."),
        BackendType::Nemotron => format!("Your setup: {model} streams its own live words."),
        BackendType::Moonshine => {
            format!("Your setup: {model} on the GPU. Small and fast, but less accurate.")
        }
        _ => format!(
            "Your setup: {model}, a Whisper model. Its passes are slow; Parakeet works better."
        ),
    }
}

/// The text a select shows: the chosen option, else the raw config value.
fn select_label(choice: Choice, config: &AppConfig) -> String {
    let options = choice.options(config);
    if let Some(label) = choice.selected(config).and_then(|i| options.get(i)) {
        return label.clone();
    }
    match choice {
        Choice::Model => config.general_config.model.clone(),
        Choice::Language => config.general_config.language.clone(),
        _ => "\u{2014}".to_string(),
    }
}

fn intersect(a: Rect, b: Rect) -> Rect {
    let x0 = a[0].max(b[0]);
    let y0 = a[1].max(b[1]);
    let x1 = (a[0] + a[2]).min(b[0] + b[2]);
    let y1 = (a[1] + a[3]).min(b[1] + b[3]);
    [x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0)]
}

fn grow(r: Rect, by: f32) -> Rect {
    [r[0] - by, r[1] - by, r[2] + 2.0 * by, r[3] + 2.0 * by]
}

/// A rounded rectangle in `color` (straight alpha). `corner` is in logical px.
fn fill_shape(sf: f32, rect: Rect, corner: f32, color: [f32; 4]) -> Shape {
    Shape {
        rect,
        color,
        params: [PANEL, corner * sf, FLAT, 0.0],
        extra: [0.0; 4],
    }
}

/// A line of `width` logical px along the inside of a rounded rectangle.
fn ring_shape(sf: f32, rect: Rect, corner: f32, width: f32, color: [f32; 4]) -> Shape {
    Shape {
        rect,
        color: [0.0; 4],
        params: [OUTLINE, corner * sf, width * sf, 0.0],
        extra: color,
    }
}

fn fill(ui: &mut Ui, rect: Rect, corner: f32, mut color: [f32; 4]) {
    color[3] *= ui.alpha;
    ui.shapes.push(fill_shape(ui.sf, rect, corner, color));
}

fn ring(ui: &mut Ui, rect: Rect, corner: f32, width: f32, mut color: [f32; 4]) {
    color[3] *= ui.alpha;
    ui.shapes
        .push(ring_shape(ui.sf, rect, corner, width, color));
}

/// A light card with a cool hairline, like each settings row.
fn card(ui: &mut Ui, rect: Rect, corner: f32, fill_alpha: f32, ring_alpha: f32) {
    fill(ui, rect, corner, [1.0, 1.0, 1.0, fill_alpha]);
    ring(
        ui,
        rect,
        corner,
        1.0,
        [EDGE[0], EDGE[1], EDGE[2], ring_alpha],
    );
}

/// A raised button, or a dark slate one.
fn button(ui: &mut Ui, rect: Rect, label: &str, dark: bool, hover: bool) {
    if dark {
        ui.panel(rect, 9.0, SLATE_FILL, 1.0);
        if hover {
            fill(ui, rect, 9.0, [1.0, 1.0, 1.0, 0.12]);
        }
    } else {
        ui.panel(rect, 9.0, RAISED, 1.0);
        if hover {
            fill(ui, rect, 9.0, [EDGE[0], EDGE[1], EDGE[2], 0.12]);
        }
    }
    let tw = ui.measure(label, 12.5, true);
    let color = if dark { [1.0, 1.0, 1.0] } else { INK };
    ui.label(
        label,
        rect[0] + (rect[2] - tw) / 2.0,
        rect[1] + rect[3] / 2.0,
        12.5,
        true,
        color,
        1.0,
    );
}

/// A vertical silver capsule centred on (`cx`, `cy`), `w` by `h` physical px.
fn silver_bar(ui: &mut Ui, cx: f32, cy: f32, w: f32, h: f32, margin: f32, opacity: f32) {
    let h = h.max(w);
    ui.shapes.push(Shape {
        rect: [
            cx - w / 2.0 - margin,
            cy - h / 2.0 - margin,
            w + 2.0 * margin,
            h + 2.0 * margin,
        ],
        color: [1.0, 1.0, 1.0, opacity * ui.alpha],
        params: [SILVER_BAR, 0.0, margin, 0.0],
        extra: [0.0; 4],
    });
}

/// A silver circle of `radius` physical px: the knob of switches and sliders.
fn silver_knob(ui: &mut Ui, cx: f32, cy: f32, radius: f32) {
    silver_bar(ui, cx, cy, 2.0 * radius, 2.0 * radius, 2.0 * ui.sf, 1.0);
}
