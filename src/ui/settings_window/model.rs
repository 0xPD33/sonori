//! What each Settings control reads from and writes to `AppConfig`, and how a
//! saved change takes effect.

use std::collections::BTreeMap;

use speechcore::{BackendType, QuantizationLevel};

use crate::config::{
    AppConfig, CursorBubble, DisplayMode, OutputMode, ShortcutMode, SpectrogramSkin,
    VadSensitivity, VisualThemePreset, WindowPosition,
};
use crate::ui::cursor_overlay::PRESETS;
use crate::ui::shapes::{BARS, GEAR, MIC, PAGE, POINTER, WORDS};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    General,
    Speech,
    Microphone,
    Look,
    Words,
    Advanced,
}

impl Section {
    pub const ALL: [Section; 6] = [
        Section::General,
        Section::Speech,
        Section::Microphone,
        Section::Look,
        Section::Words,
        Section::Advanced,
    ];

    /// The section whose title is `name`, ignoring case.
    pub fn from_name(name: &str) -> Option<Section> {
        Section::ALL
            .into_iter()
            .find(|s| s.title().eq_ignore_ascii_case(name))
    }

    pub fn title(self) -> &'static str {
        match self {
            Section::General => "General",
            Section::Speech => "Speech",
            Section::Microphone => "Microphone",
            Section::Look => "Look",
            Section::Words => "Words",
            Section::Advanced => "Advanced",
        }
    }

    pub fn subtitle(self) -> &'static str {
        match self {
            Section::General => "How you talk to Sonori and where the text goes.",
            Section::Speech => "Which model turns your voice into text.",
            Section::Microphone => "What Sonori listens to.",
            Section::Look => "What you see while you dictate.",
            Section::Words => "Names and terms Sonori should get right.",
            Section::Advanced => "Clean-up rules, history, and files.",
        }
    }

    pub fn icon(self) -> f32 {
        match self {
            Section::General => PAGE,
            Section::Speech => BARS,
            Section::Microphone => MIC,
            Section::Look => POINTER,
            Section::Words => WORDS,
            Section::Advanced => GEAR,
        }
    }
}

/// When a saved change takes effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    Now,
    Restart,
    /// The backend reloads at once.
    Reload,
    /// The backend reloads with another model, which may download first. Asks first.
    Model,
}

/// A setting with a fixed list of options: a segmented control or a select.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Choice {
    ShortcutMode,
    Listening,
    Output,
    PasteShortcut,
    Language,
    Bubble,
    Theme,
    Skin,
    Position,
    Vsync,
    Vad,
    Backend,
    Model,
    Precision,
}

const BACKENDS: [(BackendType, &str); 5] = [
    (BackendType::CTranslate2, "CTranslate2"),
    (BackendType::WhisperCpp, "Whisper.cpp"),
    (BackendType::Moonshine, "Moonshine"),
    (BackendType::Parakeet, "Parakeet"),
    (BackendType::Nemotron, "Nemotron"),
];
const POSITIONS: [(WindowPosition, &str); 10] = [
    (WindowPosition::BottomLeft, "Bottom left"),
    (WindowPosition::BottomCenter, "Bottom centre"),
    (WindowPosition::BottomRight, "Bottom right"),
    (WindowPosition::TopLeft, "Top left"),
    (WindowPosition::TopCenter, "Top centre"),
    (WindowPosition::TopRight, "Top right"),
    (WindowPosition::MiddleLeft, "Middle left"),
    (WindowPosition::MiddleCenter, "Centre"),
    (WindowPosition::MiddleRight, "Middle right"),
    (WindowPosition::Custom, "Where you dragged it"),
];
const VSYNC: [(&str, &str); 5] = [
    ("Auto", "Auto"),
    ("Enabled", "On"),
    ("Adaptive", "Adaptive"),
    ("Mailbox", "Mailbox"),
    ("Disabled", "Off"),
];

impl Choice {
    /// The option labels for `config`.
    pub fn options(self, config: &AppConfig) -> Vec<String> {
        let names: Vec<&str> = match self {
            Choice::ShortcutMode => vec!["Toggle", "Hold to talk"],
            Choice::Listening => vec!["On demand", "Live"],
            Choice::Output => vec!["Paste", "Type", "Copy"],
            Choice::PasteShortcut => vec!["By app", "Ctrl+Shift+V", "Ctrl+V"],
            Choice::Language => languages(config).iter().map(|(_, name)| *name).collect(),
            Choice::Bubble => vec!["Brief", "Long", "Off"],
            Choice::Theme => vec!["Focus", "Pulse", "Terminal"],
            Choice::Skin => vec!["Bars", "Waveform", "Meter"],
            Choice::Position => {
                let custom = config.display_config.window_position == WindowPosition::Custom;
                POSITIONS
                    .iter()
                    .filter(|(p, _)| custom || *p != WindowPosition::Custom)
                    .map(|(_, name)| *name)
                    .collect()
            }
            Choice::Vsync => VSYNC.iter().map(|(_, name)| *name).collect(),
            Choice::Vad => vec!["Very easily", "Easily", "Normal", "Only clear speech"],
            Choice::Backend => BACKENDS.iter().map(|(_, name)| *name).collect(),
            Choice::Model => models(config.backend_config.backend).to_vec(),
            Choice::Precision => vec!["High", "Medium", "Low"],
        };
        names.into_iter().map(str::to_string).collect()
    }

    pub fn selected(self, config: &AppConfig) -> Option<usize> {
        let c = config;
        match self {
            Choice::ShortcutMode => Some(match c.portal_config.shortcut_mode {
                ShortcutMode::Toggle => 0,
                ShortcutMode::PushToTalk => 1,
            }),
            Choice::Listening => Some(usize::from(
                c.general_config.transcription_mode == "realtime",
            )),
            Choice::Output => Some(match c.portal_config.output_mode {
                OutputMode::Paste => 0,
                OutputMode::Type => 1,
                OutputMode::Clipboard => 2,
            }),
            Choice::PasteShortcut => Some(match c.portal_config.paste_shortcut.as_str() {
                "ctrl_shift_v" => 1,
                "ctrl_v" => 2,
                _ => 0,
            }),
            Choice::Language => languages(c)
                .iter()
                .position(|(code, _)| *code == c.general_config.language),
            Choice::Bubble => Some(match c.ui_config.cursor_bubble {
                CursorBubble::Brief => 0,
                CursorBubble::Long => 1,
                CursorBubble::Off => 2,
            }),
            Choice::Theme => Some(match c.ui_config.visual_theme {
                VisualThemePreset::Focus => 0,
                VisualThemePreset::Pulse => 1,
                VisualThemePreset::Terminal => 2,
            }),
            Choice::Skin => Some(match c.ui_config.spectrogram_skin {
                SpectrogramSkin::Bars => 0,
                SpectrogramSkin::Waveform => 1,
                SpectrogramSkin::Meter => 2,
            }),
            Choice::Position => POSITIONS
                .iter()
                .position(|(p, _)| *p == c.display_config.window_position),
            Choice::Vsync => VSYNC
                .iter()
                .position(|(value, _)| *value == c.display_config.vsync_mode)
                .or(Some(0)),
            Choice::Vad => Some(match c.vad_config.sensitivity {
                VadSensitivity::VeryHigh => 0,
                VadSensitivity::High => 1,
                VadSensitivity::Medium => 2,
                VadSensitivity::Low => 3,
            }),
            Choice::Backend => BACKENDS
                .iter()
                .position(|(b, _)| *b == c.backend_config.backend),
            Choice::Model => models(c.backend_config.backend)
                .iter()
                .position(|m| *m == c.general_config.model),
            Choice::Precision => Some(match c.backend_config.quantization_level {
                QuantizationLevel::High => 0,
                QuantizationLevel::Medium => 1,
                QuantizationLevel::Low => 2,
            }),
        }
    }

    pub fn apply(self, index: usize, config: &mut AppConfig) {
        let c = config;
        match self {
            Choice::ShortcutMode => {
                c.portal_config.shortcut_mode =
                    [ShortcutMode::Toggle, ShortcutMode::PushToTalk][index]
            }
            Choice::Listening => {
                c.general_config.transcription_mode = ["manual", "realtime"][index].to_string()
            }
            Choice::Output => {
                c.portal_config.output_mode =
                    [OutputMode::Paste, OutputMode::Type, OutputMode::Clipboard][index]
            }
            Choice::PasteShortcut => {
                c.portal_config.paste_shortcut =
                    ["auto", "ctrl_shift_v", "ctrl_v"][index].to_string()
            }
            Choice::Language => {
                if let Some((code, _)) = languages(c).get(index) {
                    c.general_config.language = code.to_string();
                }
            }
            Choice::Bubble => {
                c.ui_config.cursor_bubble =
                    [CursorBubble::Brief, CursorBubble::Long, CursorBubble::Off][index]
            }
            Choice::Theme => {
                c.ui_config.visual_theme = [
                    VisualThemePreset::Focus,
                    VisualThemePreset::Pulse,
                    VisualThemePreset::Terminal,
                ][index]
            }
            Choice::Skin => {
                c.ui_config.spectrogram_skin = [
                    SpectrogramSkin::Bars,
                    SpectrogramSkin::Waveform,
                    SpectrogramSkin::Meter,
                ][index]
            }
            Choice::Position => {
                let position = POSITIONS[index].0;
                c.display_config.window_position = position;
                if position != WindowPosition::Custom {
                    c.display_config.custom_window_position = None;
                }
            }
            Choice::Vsync => c.display_config.vsync_mode = VSYNC[index].0.to_string(),
            Choice::Vad => {
                c.vad_config.sensitivity = [
                    VadSensitivity::VeryHigh,
                    VadSensitivity::High,
                    VadSensitivity::Medium,
                    VadSensitivity::Low,
                ][index]
            }
            Choice::Backend => {
                let backend = BACKENDS[index].0;
                c.backend_config.backend = backend;
                let list = models(backend);
                if !list.contains(&c.general_config.model.as_str()) {
                    c.general_config.model = list[0].to_string();
                }
                fit_language(c);
            }
            // The file on disk may name another backend than the list showed.
            Choice::Model => {
                if let Some(model) = models(c.backend_config.backend).get(index) {
                    c.general_config.model = model.to_string();
                    fit_language(c);
                }
            }
            Choice::Precision => {
                c.backend_config.quantization_level = [
                    QuantizationLevel::High,
                    QuantizationLevel::Medium,
                    QuantizationLevel::Low,
                ][index]
            }
        }
    }

    pub fn effect(self) -> Effect {
        match self {
            Choice::Backend | Choice::Model | Choice::Precision => Effect::Model,
            Choice::ShortcutMode | Choice::Vsync | Choice::Vad => Effect::Restart,
            _ => Effect::Now,
        }
    }
}

/// An on/off setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Flag {
    /// Magic Mode itself; the app toggles it, so it is never saved here.
    Magic,
    Sounds,
    MagicModel,
    ClearOnNew,
    Typewriter,
    LiveText,
    RecordingDot,
    CleanUp,
    Fillers,
    Repeats,
    Capitalize,
    Punctuation,
    History,
    TrainingData,
    Tray,
    Gpu,
}

impl Flag {
    pub fn get(self, c: &AppConfig) -> bool {
        match self {
            Flag::Magic => c.enhancement_config.active,
            Flag::Sounds => c.sound_config.enabled,
            Flag::MagicModel => c.enhancement_config.enabled,
            Flag::ClearOnNew => c.manual_mode_config.clear_on_new_session,
            Flag::Typewriter => c.ui_config.typewriter_effect,
            Flag::LiveText => c.ui_config.live_text,
            Flag::RecordingDot => c.ui_config.show_recording_indicator,
            Flag::CleanUp => c.post_process_config.enabled,
            Flag::Fillers => c.post_process_config.remove_fillers,
            Flag::Repeats => c.post_process_config.collapse_repeated_words,
            Flag::Capitalize => c.post_process_config.capitalize_sentences,
            Flag::Punctuation => c.post_process_config.ensure_terminal_punctuation,
            Flag::History => c.history_config.enabled,
            Flag::TrainingData => c.training_data.enabled,
            Flag::Tray => c.window_behavior_config.show_in_system_tray,
            Flag::Gpu => c.backend_config.gpu_enabled,
        }
    }

    fn set(self, c: &mut AppConfig, value: bool) {
        let field = match self {
            Flag::Magic => &mut c.enhancement_config.active,
            Flag::Sounds => &mut c.sound_config.enabled,
            Flag::MagicModel => &mut c.enhancement_config.enabled,
            Flag::ClearOnNew => &mut c.manual_mode_config.clear_on_new_session,
            Flag::Typewriter => &mut c.ui_config.typewriter_effect,
            Flag::LiveText => &mut c.ui_config.live_text,
            Flag::RecordingDot => &mut c.ui_config.show_recording_indicator,
            Flag::CleanUp => &mut c.post_process_config.enabled,
            Flag::Fillers => &mut c.post_process_config.remove_fillers,
            Flag::Repeats => &mut c.post_process_config.collapse_repeated_words,
            Flag::Capitalize => &mut c.post_process_config.capitalize_sentences,
            Flag::Punctuation => &mut c.post_process_config.ensure_terminal_punctuation,
            Flag::History => &mut c.history_config.enabled,
            Flag::TrainingData => &mut c.training_data.enabled,
            Flag::Tray => &mut c.window_behavior_config.show_in_system_tray,
            Flag::Gpu => &mut c.backend_config.gpu_enabled,
        };
        *field = value;
    }

    /// Turning it on has side effects the user must know first: dictations on
    /// disk, or model work while the user speaks.
    pub fn asks_first(self) -> bool {
        matches!(self, Flag::History | Flag::TrainingData | Flag::LiveText)
    }

    fn effect(self) -> Effect {
        match self {
            Flag::Magic | Flag::Typewriter | Flag::RecordingDot | Flag::History => Effect::Now,
            Flag::Gpu => Effect::Reload,
            _ => Effect::Restart,
        }
    }
}

/// A number set with a slider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Num {
    Volume,
    Threads,
    FontSize,
    Fps,
}

impl Num {
    /// Lowest value, highest value, and step.
    pub fn range(self) -> (f32, f32, f32) {
        match self {
            Num::Volume => (0.0, 1.0, 0.05),
            Num::Threads => (1.0, 16.0, 1.0),
            Num::FontSize => (6.0, 24.0, 0.5),
            Num::Fps => (15.0, 240.0, 5.0),
        }
    }

    pub fn get(self, c: &AppConfig) -> f32 {
        match self {
            Num::Volume => c.sound_config.volume,
            Num::Threads => c.backend_config.threads as f32,
            Num::FontSize => c.ui_config.font_size,
            Num::Fps => c.display_config.target_fps as f32,
        }
    }

    fn set(self, c: &mut AppConfig, value: f32) {
        match self {
            Num::Volume => c.sound_config.volume = value,
            Num::Threads => c.backend_config.threads = value.round() as usize,
            Num::FontSize => c.ui_config.font_size = value,
            Num::Fps => c.display_config.target_fps = value.round() as u32,
        }
    }

    pub fn format(self, value: f32) -> String {
        match self {
            Num::Volume => format!("{:.0}%", value * 100.0),
            Num::FontSize => format!("{value:.1}"),
            Num::Threads | Num::Fps => format!("{value:.0}"),
        }
    }

    /// The widest text `format` gives, to reserve room for it.
    pub fn widest(self) -> &'static str {
        match self {
            Num::Volume => "100%",
            Num::Threads => "16",
            Num::FontSize => "24.0",
            Num::Fps => "240",
        }
    }

    fn effect(self) -> Effect {
        match self {
            Num::Threads => Effect::Reload,
            Num::Volume => Effect::Restart,
            Num::FontSize | Num::Fps => Effect::Now,
        }
    }

    /// `value` rounded to the step and held in range.
    pub fn snap(self, value: f32) -> f32 {
        let (min, max, step) = self.range();
        (min + ((value - min) / step).round() * step).clamp(min, max)
    }
}

/// One edit from the Settings window, applied to a fresh copy of the config file.
#[derive(Debug, Clone)]
pub enum Change {
    Flag(Flag, bool),
    Choose(Choice, usize),
    Num(Num, f32),
    Preset(usize),
    Display(DisplayMode),
    Vocabulary(Vec<String>),
    Replacements(BTreeMap<String, String>),
}

impl Change {
    pub fn apply(&self, c: &mut AppConfig) {
        match self {
            Change::Flag(flag, value) => flag.set(c, *value),
            Change::Choose(choice, index) => choice.apply(*index, c),
            Change::Num(num, value) => num.set(c, *value),
            Change::Preset(i) => {
                c.backend_config.backend = PRESETS[*i].backend;
                c.general_config.model = PRESETS[*i].model.to_string();
                fit_language(c);
            }
            Change::Display(mode) => c.ui_config.display_mode = *mode,
            Change::Vocabulary(words) => {
                c.whisper_cpp_options.initial_prompt =
                    (!words.is_empty()).then(|| words.join(", "));
            }
            Change::Replacements(map) => c.post_process_config.replacements = map.clone(),
        }
    }

    pub fn effect(&self) -> Effect {
        match self {
            Change::Flag(flag, _) => flag.effect(),
            Change::Choose(choice, _) => choice.effect(),
            Change::Num(num, _) => num.effect(),
            Change::Preset(_) => Effect::Model,
            Change::Display(_) => Effect::Now,
            Change::Vocabulary(_) | Change::Replacements(_) => Effect::Restart,
        }
    }
}

/// Changes a language that the configured model does not list into the same
/// language in its form ("de" and "de-DE"), else into auto detect.
pub fn fit_language(c: &mut AppConfig) {
    let current = c.general_config.language.clone();
    let codes: Vec<&str> = languages(c).iter().map(|(code, _)| *code).collect();
    if codes.is_empty() {
        // An English-only model.
        c.general_config.language = "en".to_string();
        return;
    }
    if codes.contains(&current.as_str()) {
        return;
    }
    let base = |code: &str| code.split('-').next().unwrap_or_default().to_string();
    let fallback = if codes.contains(&"auto") {
        "auto"
    } else {
        codes[0]
    };
    c.general_config.language = codes
        .iter()
        .find(|code| base(code) == base(&current))
        .unwrap_or(&fallback)
        .to_string();
}

/// Index into `PRESETS` of the configured model, if it is one of them.
pub fn preset_of(c: &AppConfig) -> Option<usize> {
    // Moonshine accepts "base" and "moonshine-base" for the same model.
    let model = c.general_config.model.as_str();
    let model = model.strip_prefix("moonshine-").unwrap_or(model);
    PRESETS
        .iter()
        .position(|p| p.backend == c.backend_config.backend && p.model == model)
}

/// The vocabulary words in `whisper_cpp_options.initial_prompt`, split at commas.
pub fn vocabulary(c: &AppConfig) -> Vec<String> {
    c.whisper_cpp_options
        .initial_prompt
        .as_deref()
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

pub fn backend_name(backend: BackendType) -> &'static str {
    BACKENDS
        .iter()
        .find(|(b, _)| *b == backend)
        .map_or("Unknown", |(_, name)| name)
}

fn models(backend: BackendType) -> &'static [&'static str] {
    match backend {
        BackendType::WhisperCpp => &[
            "tiny.en",
            "base.en",
            "small.en",
            "medium.en",
            "tiny",
            "base",
            "small",
            "medium",
            "large-v1",
            "large-v2",
            "large-v3",
            "large-v3-turbo",
        ],
        BackendType::CTranslate2 => &["tiny.en", "base.en", "small.en", "medium.en", "large-v3"],
        BackendType::Moonshine => &["tiny", "base"],
        BackendType::Parakeet => &["parakeet-tdt-0.6b-v3", "parakeet-tdt-0.6b-v2"],
        BackendType::Nemotron => &["nemotron-3.5-asr-streaming-0.6b"],
    }
}

/// Whisper's languages; "auto" first, because CTranslate2 cannot detect.
const WHISPER_LANGUAGES: [(&str, &str); 15] = [
    ("auto", "Auto detect"),
    ("en", "English"),
    ("zh", "Chinese"),
    ("de", "German"),
    ("es", "Spanish"),
    ("fr", "French"),
    ("hi", "Hindi"),
    ("it", "Italian"),
    ("ja", "Japanese"),
    ("ko", "Korean"),
    ("nl", "Dutch"),
    ("pl", "Polish"),
    ("pt", "Portuguese"),
    ("ru", "Russian"),
    ("tr", "Turkish"),
];

/// Languages the configured model can take, as (code, name). Empty when the
/// model understands English only.
pub fn languages(c: &AppConfig) -> &'static [(&'static str, &'static str)] {
    match c.backend_config.backend {
        BackendType::WhisperCpp if !c.general_config.model.ends_with(".en") => &WHISPER_LANGUAGES,
        BackendType::CTranslate2 if !c.general_config.model.ends_with(".en") => {
            &WHISPER_LANGUAGES[1..]
        }
        // Parakeet v2 is English only.
        BackendType::Parakeet if !c.general_config.model.ends_with("-v2") => &[
            ("auto", "Auto detect"),
            ("en", "English"),
            ("de", "German"),
            ("es", "Spanish"),
            ("fr", "French"),
            ("it", "Italian"),
            ("pt", "Portuguese"),
            ("nl", "Dutch"),
            ("pl", "Polish"),
            ("ro", "Romanian"),
            ("sv", "Swedish"),
            ("fi", "Finnish"),
            ("cs", "Czech"),
            ("uk", "Ukrainian"),
            ("hu", "Hungarian"),
        ],
        BackendType::Nemotron => &[
            ("auto", "Auto detect"),
            ("en-US", "English (US)"),
            ("en-GB", "English (UK)"),
            ("es-ES", "Spanish (ES)"),
            ("es-US", "Spanish (US)"),
            ("fr-FR", "French (FR)"),
            ("fr-CA", "French (CA)"),
            ("de-DE", "German"),
            ("it-IT", "Italian"),
            ("pt-BR", "Portuguese (BR)"),
            ("pt-PT", "Portuguese (PT)"),
            ("nl-NL", "Dutch"),
            ("ru-RU", "Russian"),
            ("uk-UA", "Ukrainian"),
            ("pl-PL", "Polish"),
            ("cs-CZ", "Czech"),
            ("sk-SK", "Slovak"),
            ("hr-HR", "Croatian"),
            ("bg-BG", "Bulgarian"),
            ("sl-SI", "Slovenian"),
            ("ro-RO", "Romanian"),
            ("el-GR", "Greek"),
            ("hu-HU", "Hungarian"),
            ("sv-SE", "Swedish"),
            ("da-DK", "Danish"),
            ("fi-FI", "Finnish"),
            ("nb-NO", "Norwegian (Bokmål)"),
            ("nn-NO", "Norwegian (Nynorsk)"),
            ("lt-LT", "Lithuanian"),
            ("lv-LV", "Latvian"),
            ("et-EE", "Estonian"),
            ("tr-TR", "Turkish"),
            ("ar-AR", "Arabic"),
            ("he-IL", "Hebrew"),
            ("hi-IN", "Hindi"),
            ("ja-JP", "Japanese"),
            ("ko-KR", "Korean"),
            ("zh-CN", "Chinese"),
            ("th-TH", "Thai"),
            ("vi-VN", "Vietnamese"),
            ("mt-MT", "Maltese"),
        ],
        _ => &[],
    }
}
