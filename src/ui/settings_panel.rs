use std::sync::Arc;
use wgpu;
use winit::dpi::PhysicalSize;
use winit::keyboard::{Key, NamedKey};

use super::batch_text_renderer::{BatchTextRenderer, TextItem};
use super::widgets::{Select, SelectOption, Slider, Toggle, WidgetRenderer};
use crate::backend::BackendType;
use crate::config::{
    AppConfig, ShortcutMode, SpectrogramSkin, VadSensitivity, VisualThemePreset, WindowPosition,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    Backend,
    Audio,
    Behavior,
    Display,
    Appearance,
}

impl SettingsTab {
    pub fn label(&self) -> &'static str {
        match self {
            SettingsTab::Backend => "Backend",
            SettingsTab::Audio => "Audio",
            SettingsTab::Behavior => "Behavior",
            SettingsTab::Display => "Display",
            SettingsTab::Appearance => "Appearance",
        }
    }

    pub fn all() -> &'static [SettingsTab] {
        &[
            SettingsTab::Backend,
            SettingsTab::Audio,
            SettingsTab::Behavior,
            SettingsTab::Display,
            SettingsTab::Appearance,
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DropdownId {
    Backend,
    Model,
    Language,
    VadSensitivity,
    ShortcutMode,
    PasteShortcut,
    Vsync,
    VisualTheme,
    SpectrogramSkin,
    WindowPosition,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct SettingsTooltip {
    text: &'static str,
    row_y: f32,
}

pub struct SettingsPanel {
    pub is_open: bool,
    pub close_requested: bool,
    active_tab: SettingsTab,
    batch_text_renderer: BatchTextRenderer,
    overlay_text_renderer: Option<BatchTextRenderer>,
    widget_renderer: WidgetRenderer,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    surface_format: wgpu::TextureFormat,

    // Slide-in animation
    pub animation_progress: f32,
    pub animation_active: bool,
    animation_start: std::time::Instant,
    opening: bool,

    // Backend tab widgets
    backend_select: Select,
    english_only_toggle: Toggle,
    show_english_toggle: bool,
    model_select: Select,
    language_select: Select,
    show_language_select: bool,
    gpu_toggle: Toggle,
    threads_slider: Slider,

    // Audio tab widgets
    vad_sensitivity_select: Select,
    sound_toggle: Toggle,
    volume_slider: Slider,

    // Behavior tab widgets
    auto_paste_toggle: Toggle,
    clear_on_session_toggle: Toggle,
    post_processing_toggle: Toggle,
    typewriter_toggle: Toggle,

    // Behavior tab additions
    shortcut_mode_select: Select,
    paste_shortcut_select: Select,
    enhancement_toggle: Toggle,

    // Display tab widgets
    vsync_select: Select,
    target_fps_slider: Slider,
    system_tray_toggle: Toggle,

    // Appearance tab widgets
    visual_theme_select: Select,
    spectrogram_skin_select: Select,
    window_position_select: Select,
    font_size_slider: Slider,
    recording_indicator_toggle: Toggle,

    // Apply button state
    apply_requested: bool,
    has_pending_changes: bool,

    open_dropdown: Option<DropdownId>,
    hovered_tooltip: Option<SettingsTooltip>,
    tooltip_anchor_x: f32,
    hover_start: std::time::Instant,

    window_width: u32,
    window_height: u32,
}

const CONTENT_Y: f32 = 42.0;
const WIDGET_X: f32 = 14.0;
const ROW_HEIGHT: f32 = 26.0;
const SPACING: f32 = 6.0;
const APPLY_BUTTON_HEIGHT: f32 = 28.0;
const TOOLTIP_DELAY_MS: u128 = 150;

fn default_width(window_width: u32) -> f32 {
    window_width as f32 - 28.0
}

fn backend_has_english_toggle(backend: BackendType) -> bool {
    matches!(backend, BackendType::WhisperCpp)
}

fn backend_has_language_select(backend: BackendType, english_only: bool) -> bool {
    match backend {
        BackendType::WhisperCpp => !english_only,
        BackendType::Parakeet => true,
        _ => false,
    }
}

fn models_for_backend(backend: BackendType, english_only: bool) -> Vec<SelectOption> {
    let names: &[&str] = match backend {
        BackendType::WhisperCpp => {
            if english_only {
                &["tiny.en", "base.en", "small.en", "medium.en"]
            } else {
                &[
                    "tiny",
                    "base",
                    "small",
                    "medium",
                    "large-v1",
                    "large-v2",
                    "large-v3",
                    "large-v3-turbo",
                ]
            }
        }
        BackendType::CTranslate2 => &["tiny.en", "base.en", "small.en", "medium.en", "large-v3"],
        BackendType::Moonshine => &["tiny", "base"],
        BackendType::Parakeet => &["parakeet-tdt-0.6b-v3", "parakeet-tdt-0.6b-v2"],
    };
    names
        .iter()
        .map(|n| SelectOption {
            label: n.to_string(),
            value: n.to_string(),
        })
        .collect()
}

fn languages_for_backend(backend: BackendType) -> Vec<SelectOption> {
    let pairs: &[(&str, &str)] = match backend {
        BackendType::WhisperCpp => &[
            ("Auto detect", "auto"),
            ("English", "en"),
            ("Chinese", "zh"),
            ("German", "de"),
            ("Spanish", "es"),
            ("French", "fr"),
            ("Hindi", "hi"),
            ("Italian", "it"),
            ("Japanese", "ja"),
            ("Korean", "ko"),
            ("Dutch", "nl"),
            ("Polish", "pl"),
            ("Portuguese", "pt"),
            ("Russian", "ru"),
            ("Turkish", "tr"),
        ],
        BackendType::Parakeet => &[
            ("Auto detect", "auto"),
            ("English", "en"),
            ("German", "de"),
            ("Spanish", "es"),
            ("French", "fr"),
            ("Italian", "it"),
            ("Portuguese", "pt"),
            ("Dutch", "nl"),
            ("Polish", "pl"),
            ("Romanian", "ro"),
            ("Swedish", "sv"),
            ("Finnish", "fi"),
            ("Czech", "cs"),
            ("Ukrainian", "uk"),
            ("Hungarian", "hu"),
        ],
        _ => &[],
    };
    pairs
        .iter()
        .map(|(label, value)| SelectOption {
            label: label.to_string(),
            value: value.to_string(),
        })
        .collect()
}

impl SettingsPanel {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        config: &wgpu::SurfaceConfiguration,
        size: PhysicalSize<u32>,
    ) -> Self {
        let device = Arc::new(device.clone());
        let queue = Arc::new(queue.clone());
        let batch_text_renderer =
            BatchTextRenderer::new(device.clone(), queue.clone(), size, config.format);

        let widget_renderer = WidgetRenderer::new(&device, config.format);

        let w = default_width(size.width);

        // Backend tab widgets
        let backend_select = Select::new(
            "Backend",
            vec![
                SelectOption {
                    label: "CTranslate2".into(),
                    value: "CTranslate2".into(),
                },
                SelectOption {
                    label: "WhisperCpp".into(),
                    value: "WhisperCpp".into(),
                },
                SelectOption {
                    label: "Moonshine".into(),
                    value: "Moonshine".into(),
                },
                SelectOption {
                    label: "Parakeet".into(),
                    value: "Parakeet".into(),
                },
            ],
            0,
            WIDGET_X,
            CONTENT_Y,
            w,
            ROW_HEIGHT,
        );
        let english_only_toggle = Toggle::new(
            "English only",
            true,
            WIDGET_X,
            CONTENT_Y + ROW_HEIGHT + SPACING,
            w,
            ROW_HEIGHT,
        );
        let model_select = Select::new(
            "Model",
            models_for_backend(BackendType::WhisperCpp, true),
            0,
            WIDGET_X,
            CONTENT_Y + 2.0 * (ROW_HEIGHT + SPACING),
            w,
            ROW_HEIGHT,
        );
        let language_select = Select::new(
            "Language",
            languages_for_backend(BackendType::WhisperCpp),
            0,
            WIDGET_X,
            CONTENT_Y + 3.0 * (ROW_HEIGHT + SPACING),
            w,
            ROW_HEIGHT,
        );
        let gpu_toggle = Toggle::new(
            "GPU acceleration",
            false,
            WIDGET_X,
            CONTENT_Y + 3.0 * (ROW_HEIGHT + SPACING),
            w,
            ROW_HEIGHT,
        );
        let threads_slider = Slider::new(
            "Threads",
            4.0,
            1.0,
            8.0,
            1.0,
            WIDGET_X,
            CONTENT_Y + 4.0 * (ROW_HEIGHT + SPACING),
            w,
            ROW_HEIGHT,
        );

        // Audio tab widgets
        let vad_sensitivity_select = Select::new(
            "VAD sensitivity",
            vec![
                SelectOption {
                    label: "Low".into(),
                    value: "Low".into(),
                },
                SelectOption {
                    label: "Medium".into(),
                    value: "Medium".into(),
                },
                SelectOption {
                    label: "High".into(),
                    value: "High".into(),
                },
            ],
            1,
            WIDGET_X,
            CONTENT_Y,
            w,
            ROW_HEIGHT,
        );
        let sound_toggle = Toggle::new(
            "Sound feedback",
            true,
            WIDGET_X,
            CONTENT_Y + ROW_HEIGHT + SPACING,
            w,
            ROW_HEIGHT,
        );
        let volume_slider = Slider::new(
            "Volume",
            0.5,
            0.0,
            1.0,
            0.05,
            WIDGET_X,
            CONTENT_Y + 2.0 * (ROW_HEIGHT + SPACING),
            w,
            ROW_HEIGHT,
        );

        // Behavior tab widgets
        let auto_paste_toggle = Toggle::new("Auto-paste", true, WIDGET_X, CONTENT_Y, w, ROW_HEIGHT);
        let clear_on_session_toggle = Toggle::new(
            "Clear on new session",
            true,
            WIDGET_X,
            CONTENT_Y + ROW_HEIGHT + SPACING,
            w,
            ROW_HEIGHT,
        );
        let post_processing_toggle = Toggle::new(
            "Post-processing",
            true,
            WIDGET_X,
            CONTENT_Y + 2.0 * (ROW_HEIGHT + SPACING),
            w,
            ROW_HEIGHT,
        );
        let typewriter_toggle = Toggle::new(
            "Typewriter effect",
            false,
            WIDGET_X,
            CONTENT_Y + 3.0 * (ROW_HEIGHT + SPACING),
            w,
            ROW_HEIGHT,
        );
        let shortcut_mode_select = Select::new(
            "Shortcut mode",
            vec![
                SelectOption {
                    label: "Toggle".into(),
                    value: "Toggle".into(),
                },
                SelectOption {
                    label: "Push to Talk".into(),
                    value: "PushToTalk".into(),
                },
            ],
            0,
            WIDGET_X,
            CONTENT_Y + 4.0 * (ROW_HEIGHT + SPACING),
            w,
            ROW_HEIGHT,
        );
        let paste_shortcut_select = Select::new(
            "Paste shortcut",
            vec![
                SelectOption {
                    label: "Ctrl+Shift+V".into(),
                    value: "ctrl_shift_v".into(),
                },
                SelectOption {
                    label: "Ctrl+V".into(),
                    value: "ctrl_v".into(),
                },
            ],
            0,
            WIDGET_X,
            CONTENT_Y + 5.0 * (ROW_HEIGHT + SPACING),
            w,
            ROW_HEIGHT,
        );
        let enhancement_toggle = Toggle::new(
            "Magic mode",
            false,
            WIDGET_X,
            CONTENT_Y + 6.0 * (ROW_HEIGHT + SPACING),
            w,
            ROW_HEIGHT,
        );

        // Display tab widgets
        let vsync_select = Select::new(
            "VSync mode",
            vec![
                SelectOption {
                    label: "Enabled".into(),
                    value: "Enabled".into(),
                },
                SelectOption {
                    label: "Disabled".into(),
                    value: "Disabled".into(),
                },
                SelectOption {
                    label: "Adaptive".into(),
                    value: "Adaptive".into(),
                },
                SelectOption {
                    label: "Mailbox".into(),
                    value: "Mailbox".into(),
                },
                SelectOption {
                    label: "Auto".into(),
                    value: "Auto".into(),
                },
            ],
            0,
            WIDGET_X,
            CONTENT_Y,
            w,
            ROW_HEIGHT,
        );
        let target_fps_slider = Slider::new(
            "Target FPS",
            60.0,
            15.0,
            240.0,
            5.0,
            WIDGET_X,
            CONTENT_Y + ROW_HEIGHT + SPACING,
            w,
            ROW_HEIGHT,
        );
        let system_tray_toggle = Toggle::new(
            "System tray",
            true,
            WIDGET_X,
            CONTENT_Y + 2.0 * (ROW_HEIGHT + SPACING),
            w,
            ROW_HEIGHT,
        );

        // Appearance tab widgets
        let visual_theme_select = Select::new(
            "Visual theme",
            vec![
                SelectOption {
                    label: "Focus".into(),
                    value: "Focus".into(),
                },
                SelectOption {
                    label: "Pulse".into(),
                    value: "Pulse".into(),
                },
                SelectOption {
                    label: "Terminal".into(),
                    value: "Terminal".into(),
                },
            ],
            0,
            WIDGET_X,
            CONTENT_Y,
            w,
            ROW_HEIGHT,
        );
        let spectrogram_skin_select = Select::new(
            "Spectrogram skin",
            vec![
                SelectOption {
                    label: "Bars".into(),
                    value: "Bars".into(),
                },
                SelectOption {
                    label: "Waveform".into(),
                    value: "Waveform".into(),
                },
                SelectOption {
                    label: "Meter".into(),
                    value: "Meter".into(),
                },
            ],
            0,
            WIDGET_X,
            CONTENT_Y + ROW_HEIGHT + SPACING,
            w,
            ROW_HEIGHT,
        );
        let window_position_select = Select::new(
            "Window position",
            vec![
                SelectOption {
                    label: "Bottom Left".into(),
                    value: "BottomLeft".into(),
                },
                SelectOption {
                    label: "Bottom Center".into(),
                    value: "BottomCenter".into(),
                },
                SelectOption {
                    label: "Bottom Right".into(),
                    value: "BottomRight".into(),
                },
                SelectOption {
                    label: "Top Left".into(),
                    value: "TopLeft".into(),
                },
                SelectOption {
                    label: "Top Center".into(),
                    value: "TopCenter".into(),
                },
                SelectOption {
                    label: "Top Right".into(),
                    value: "TopRight".into(),
                },
                SelectOption {
                    label: "Middle Left".into(),
                    value: "MiddleLeft".into(),
                },
                SelectOption {
                    label: "Middle Center".into(),
                    value: "MiddleCenter".into(),
                },
                SelectOption {
                    label: "Middle Right".into(),
                    value: "MiddleRight".into(),
                },
                SelectOption {
                    label: "Custom".into(),
                    value: "Custom".into(),
                },
            ],
            1,
            WIDGET_X,
            CONTENT_Y + 2.0 * (ROW_HEIGHT + SPACING),
            w,
            ROW_HEIGHT,
        );
        let font_size_slider = Slider::new(
            "Font size",
            10.0,
            6.0,
            24.0,
            0.5,
            WIDGET_X,
            CONTENT_Y + 3.0 * (ROW_HEIGHT + SPACING),
            w,
            ROW_HEIGHT,
        );
        let recording_indicator_toggle = Toggle::new(
            "Recording indicator",
            true,
            WIDGET_X,
            CONTENT_Y + 4.0 * (ROW_HEIGHT + SPACING),
            w,
            ROW_HEIGHT,
        );

        Self {
            is_open: false,
            close_requested: false,
            active_tab: SettingsTab::Backend,
            batch_text_renderer,
            overlay_text_renderer: None,
            widget_renderer,
            device,
            queue,
            surface_format: config.format,

            animation_progress: 0.0,
            animation_active: false,
            animation_start: std::time::Instant::now(),
            opening: false,

            backend_select,
            english_only_toggle,
            show_english_toggle: true,
            model_select,
            language_select,
            show_language_select: false,
            gpu_toggle,
            threads_slider,

            vad_sensitivity_select,
            sound_toggle,
            volume_slider,

            auto_paste_toggle,
            clear_on_session_toggle,
            post_processing_toggle,
            typewriter_toggle,
            shortcut_mode_select,
            paste_shortcut_select,
            enhancement_toggle,

            vsync_select,
            target_fps_slider,
            system_tray_toggle,

            visual_theme_select,
            spectrogram_skin_select,
            window_position_select,
            font_size_slider,
            recording_indicator_toggle,

            apply_requested: false,
            has_pending_changes: false,

            open_dropdown: None,
            hovered_tooltip: None,
            tooltip_anchor_x: 0.0,
            hover_start: std::time::Instant::now(),

            window_width: size.width,
            window_height: size.height,
        }
    }

    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        self.batch_text_renderer.resize(size);
        if let Some(renderer) = &mut self.overlay_text_renderer {
            renderer.resize(size);
        }
        self.window_width = size.width;
        self.window_height = size.height;
        self.recalculate_positions(size.width);
    }

    pub fn toggle(&mut self) {
        if self.is_open {
            self.opening = false;
            self.animation_active = true;
            self.animation_start = std::time::Instant::now();
        } else {
            self.is_open = true;
            self.opening = true;
            self.animation_active = true;
            self.animation_start = std::time::Instant::now();
        }
    }

    pub fn close(&mut self) {
        if self.is_open {
            self.toggle();
        }
    }

    pub fn needs_redraw(&self) -> bool {
        self.animation_active
            || self.english_only_toggle.is_animating()
            || self.gpu_toggle.is_animating()
            || self.sound_toggle.is_animating()
            || self.auto_paste_toggle.is_animating()
            || self.clear_on_session_toggle.is_animating()
            || self.post_processing_toggle.is_animating()
            || self.typewriter_toggle.is_animating()
            || self.enhancement_toggle.is_animating()
            || self.system_tray_toggle.is_animating()
            || self.recording_indicator_toggle.is_animating()
            || self
                .hovered_tooltip
                .is_some_and(|_| self.hover_start.elapsed().as_millis() < TOOLTIP_DELAY_MS)
    }

    fn content_y_offset(&self) -> f32 {
        (1.0 - self.animation_progress.clamp(0.0, 1.0)) * 80.0
    }

    fn local_y(&self, y: f32) -> f32 {
        y - self.content_y_offset()
    }

    fn select_ref(&self, id: DropdownId) -> &Select {
        match id {
            DropdownId::Backend => &self.backend_select,
            DropdownId::Model => &self.model_select,
            DropdownId::Language => &self.language_select,
            DropdownId::VadSensitivity => &self.vad_sensitivity_select,
            DropdownId::ShortcutMode => &self.shortcut_mode_select,
            DropdownId::PasteShortcut => &self.paste_shortcut_select,
            DropdownId::Vsync => &self.vsync_select,
            DropdownId::VisualTheme => &self.visual_theme_select,
            DropdownId::SpectrogramSkin => &self.spectrogram_skin_select,
            DropdownId::WindowPosition => &self.window_position_select,
        }
    }

    fn select_mut(&mut self, id: DropdownId) -> &mut Select {
        match id {
            DropdownId::Backend => &mut self.backend_select,
            DropdownId::Model => &mut self.model_select,
            DropdownId::Language => &mut self.language_select,
            DropdownId::VadSensitivity => &mut self.vad_sensitivity_select,
            DropdownId::ShortcutMode => &mut self.shortcut_mode_select,
            DropdownId::PasteShortcut => &mut self.paste_shortcut_select,
            DropdownId::Vsync => &mut self.vsync_select,
            DropdownId::VisualTheme => &mut self.visual_theme_select,
            DropdownId::SpectrogramSkin => &mut self.spectrogram_skin_select,
            DropdownId::WindowPosition => &mut self.window_position_select,
        }
    }

    fn active_select_box_at(&self, x: f32, y: f32) -> Option<DropdownId> {
        match self.active_tab {
            SettingsTab::Backend => {
                if self.backend_select.hit_select_box(x, y) {
                    Some(DropdownId::Backend)
                } else if self.model_select.hit_select_box(x, y) {
                    Some(DropdownId::Model)
                } else if self.show_language_select && self.language_select.hit_select_box(x, y) {
                    Some(DropdownId::Language)
                } else {
                    None
                }
            }
            SettingsTab::Audio => self
                .vad_sensitivity_select
                .hit_select_box(x, y)
                .then_some(DropdownId::VadSensitivity),
            SettingsTab::Behavior => {
                if self.shortcut_mode_select.hit_select_box(x, y) {
                    Some(DropdownId::ShortcutMode)
                } else if self.paste_shortcut_select.hit_select_box(x, y) {
                    Some(DropdownId::PasteShortcut)
                } else {
                    None
                }
            }
            SettingsTab::Display => self
                .vsync_select
                .hit_select_box(x, y)
                .then_some(DropdownId::Vsync),
            SettingsTab::Appearance => {
                if self.visual_theme_select.hit_select_box(x, y) {
                    Some(DropdownId::VisualTheme)
                } else if self.spectrogram_skin_select.hit_select_box(x, y) {
                    Some(DropdownId::SpectrogramSkin)
                } else if self.window_position_select.hit_select_box(x, y) {
                    Some(DropdownId::WindowPosition)
                } else {
                    None
                }
            }
        }
    }

    fn open_only_dropdown(&mut self, id: DropdownId) {
        self.close_all_dropdowns();
        self.clear_tooltip();
        self.select_mut(id).set_expanded(true);
        self.open_dropdown = Some(id);
    }

    fn handle_select_click(&mut self, id: DropdownId, x: f32, y: f32) -> bool {
        if self.open_dropdown == Some(id) && self.select_ref(id).is_expanded() {
            if let Some(index) = self.select_ref(id).dropdown_option_at(x, y) {
                self.select_mut(id).choose_index(index);
                self.close_all_dropdowns();
                return true;
            }

            self.close_all_dropdowns();
            return true;
        }

        if self.select_ref(id).hit_select_box(x, y) {
            self.open_only_dropdown(id);
            return true;
        }

        false
    }

    fn refresh_backend_dependent_options(&mut self, window_width: u32) {
        let backend = match self.backend_select.selected_index {
            0 => BackendType::CTranslate2,
            1 => BackendType::WhisperCpp,
            2 => BackendType::Moonshine,
            3 => BackendType::Parakeet,
            _ => BackendType::CTranslate2,
        };
        let english_only = self.english_only_toggle.value;
        self.show_english_toggle = backend_has_english_toggle(backend);
        self.model_select.options = models_for_backend(backend, english_only);
        self.model_select.selected_index = 0;
        self.show_language_select = backend_has_language_select(backend, english_only);
        if self.show_language_select {
            self.language_select.options = languages_for_backend(backend);
            self.language_select.selected_index = 0;
        }
        self.recalculate_positions(window_width);
    }

    fn widgets_have_pending_changes(&self) -> bool {
        self.backend_select.has_changed()
            || self.english_only_toggle.has_changed()
            || self.model_select.has_changed()
            || self.language_select.has_changed()
            || self.gpu_toggle.has_changed()
            || self.threads_slider.has_changed()
            || self.vad_sensitivity_select.has_changed()
            || self.sound_toggle.has_changed()
            || self.volume_slider.has_changed()
            || self.auto_paste_toggle.has_changed()
            || self.clear_on_session_toggle.has_changed()
            || self.post_processing_toggle.has_changed()
            || self.typewriter_toggle.has_changed()
            || self.shortcut_mode_select.has_changed()
            || self.paste_shortcut_select.has_changed()
            || self.enhancement_toggle.has_changed()
            || self.vsync_select.has_changed()
            || self.target_fps_slider.has_changed()
            || self.system_tray_toggle.has_changed()
            || self.visual_theme_select.has_changed()
            || self.spectrogram_skin_select.has_changed()
            || self.window_position_select.has_changed()
            || self.font_size_slider.has_changed()
            || self.recording_indicator_toggle.has_changed()
    }

    fn mark_pending_if_widget_changed(&mut self) {
        if self.widgets_have_pending_changes() {
            self.has_pending_changes = true;
        }
    }

    fn clear_tooltip(&mut self) {
        self.hovered_tooltip = None;
    }

    fn update_tooltip_hover(&mut self, x: f32, y: f32) {
        let next_tooltip = if self.open_dropdown.is_some() {
            None
        } else {
            self.tooltip_for_position(y)
        };

        if next_tooltip != self.hovered_tooltip {
            self.hovered_tooltip = next_tooltip;
            self.hover_start = std::time::Instant::now();
        }
        self.tooltip_anchor_x = x;
    }

    fn tooltip_for_position(&self, y: f32) -> Option<SettingsTooltip> {
        let hit = |widget_y: f32| -> bool { y >= widget_y && y < widget_y + ROW_HEIGHT };

        macro_rules! tip {
            ($wy:expr, $text:expr) => {
                if hit($wy) {
                    return Some(SettingsTooltip {
                        text: $text,
                        row_y: $wy,
                    });
                }
            };
        }

        match self.active_tab {
            SettingsTab::Backend => {
                tip!(self.backend_select.y, "Transcription engine");
                if self.show_english_toggle {
                    tip!(self.english_only_toggle.y, "English-only models are faster");
                }
                tip!(
                    self.model_select.y,
                    "Larger models are more accurate, but slower"
                );
                if self.show_language_select {
                    tip!(self.language_select.y, "Language used for transcription");
                }
                tip!(self.gpu_toggle.y, "Use GPU when supported");
                tip!(self.threads_slider.y, "CPU threads used for transcription");
            }
            SettingsTab::Audio => {
                tip!(self.vad_sensitivity_select.y, "Voice detection sensitivity");
                tip!(self.sound_toggle.y, "Play start and stop sounds");
                tip!(self.volume_slider.y, "Sound feedback volume");
            }
            SettingsTab::Behavior => {
                tip!(
                    self.auto_paste_toggle.y,
                    "Paste transcript into the focused app"
                );
                tip!(
                    self.clear_on_session_toggle.y,
                    "Clear old transcript on new recording"
                );
                tip!(
                    self.post_processing_toggle.y,
                    "Clean transcription artifacts"
                );
                tip!(
                    self.typewriter_toggle.y,
                    "Animate text character by character"
                );
                tip!(
                    self.shortcut_mode_select.y,
                    "Toggle records until stopped; push-to-talk holds"
                );
                tip!(self.paste_shortcut_select.y, "Shortcut sent when pasting");
                tip!(
                    self.enhancement_toggle.y,
                    "Enhance transcripts with local AI"
                );
            }
            SettingsTab::Display => {
                tip!(self.vsync_select.y, "Sync frames to display refresh");
                tip!(self.target_fps_slider.y, "Frame-rate cap when VSync is off");
                tip!(self.system_tray_toggle.y, "Show a system tray icon");
            }
            SettingsTab::Appearance => {
                tip!(self.visual_theme_select.y, "Curated overlay appearance");
                tip!(self.spectrogram_skin_select.y, "Audio visualization style");
                tip!(self.window_position_select.y, "Overlay position preset");
                tip!(self.font_size_slider.y, "Transcription text size");
                tip!(
                    self.recording_indicator_toggle.y,
                    "Show recording indicator"
                );
            }
        }

        None
    }

    fn buttons_y_for_tab(&self, tab: SettingsTab) -> f32 {
        CONTENT_Y + (self.tab_row_count(tab) as f32) * (ROW_HEIGHT + SPACING) + 12.0
    }

    pub fn populate_from_config(&mut self, config: &AppConfig) {
        // Backend
        let backend = config.backend_config.backend;
        self.backend_select.selected_index = match backend {
            BackendType::CTranslate2 => 0,
            BackendType::WhisperCpp => 1,
            BackendType::Moonshine => 2,
            BackendType::Parakeet => 3,
        };
        self.show_english_toggle = backend_has_english_toggle(backend);
        let english_only = config.general_config.model.ends_with(".en");
        self.english_only_toggle.set_value(english_only);
        self.model_select.options = models_for_backend(backend, english_only);
        self.model_select.selected_index = self
            .model_select
            .options
            .iter()
            .position(|o| o.value == config.general_config.model)
            .unwrap_or(0);
        self.show_language_select = backend_has_language_select(backend, english_only);
        if self.show_language_select {
            self.language_select.options = languages_for_backend(backend);
            self.language_select.selected_index = self
                .language_select
                .options
                .iter()
                .position(|o| o.value == config.general_config.language)
                .unwrap_or(0);
        }
        self.gpu_toggle.set_value(config.backend_config.gpu_enabled);
        self.threads_slider.value = config.backend_config.threads as f32;
        self.recalculate_positions(self.window_width);

        // Audio
        self.vad_sensitivity_select.selected_index = match config.vad_config.sensitivity {
            VadSensitivity::Low => 0,
            VadSensitivity::Medium => 1,
            VadSensitivity::High => 2,
        };
        self.sound_toggle.set_value(config.sound_config.enabled);
        self.volume_slider.value = config.sound_config.volume;

        // Behavior
        self.auto_paste_toggle
            .set_value(config.portal_config.enable_xdg_portal);
        self.clear_on_session_toggle
            .set_value(config.manual_mode_config.clear_on_new_session);
        self.post_processing_toggle
            .set_value(config.post_process_config.enabled);
        self.typewriter_toggle
            .set_value(config.ui_config.typewriter_effect);
        self.shortcut_mode_select.selected_index = match config.portal_config.shortcut_mode {
            ShortcutMode::Toggle => 0,
            ShortcutMode::PushToTalk => 1,
        };
        self.paste_shortcut_select.selected_index =
            match config.portal_config.paste_shortcut.as_str() {
                "ctrl_v" => 1,
                _ => 0,
            };
        self.enhancement_toggle
            .set_value(config.enhancement_config.enabled);

        // Display
        self.vsync_select.selected_index = match config.display_config.vsync_mode.as_str() {
            "Enabled" => 0,
            "Disabled" => 1,
            "Adaptive" => 2,
            "Mailbox" => 3,
            "Auto" => 4,
            _ => 4,
        };
        self.target_fps_slider.value = config.display_config.target_fps as f32;
        self.system_tray_toggle
            .set_value(config.window_behavior_config.show_in_system_tray);

        // Appearance
        self.visual_theme_select.selected_index = match config.ui_config.visual_theme {
            VisualThemePreset::Focus => 0,
            VisualThemePreset::Pulse => 1,
            VisualThemePreset::Terminal => 2,
        };
        self.spectrogram_skin_select.selected_index = match config.ui_config.spectrogram_skin {
            SpectrogramSkin::Bars => 0,
            SpectrogramSkin::Waveform => 1,
            SpectrogramSkin::Meter => 2,
        };
        self.window_position_select.selected_index = match config.display_config.window_position {
            WindowPosition::BottomLeft => 0,
            WindowPosition::BottomCenter => 1,
            WindowPosition::BottomRight => 2,
            WindowPosition::TopLeft => 3,
            WindowPosition::TopCenter => 4,
            WindowPosition::TopRight => 5,
            WindowPosition::MiddleLeft => 6,
            WindowPosition::MiddleCenter => 7,
            WindowPosition::MiddleRight => 8,
            WindowPosition::Custom => 9,
        };
        self.font_size_slider.value = config.ui_config.font_size;
        self.recording_indicator_toggle
            .set_value(config.ui_config.show_recording_indicator);

        self.clear_pending_changes();
        self.close_all_dropdowns();
    }

    pub fn apply_pending_changes(&mut self, config: &mut AppConfig) -> (bool, bool) {
        let mut any_changed = false;
        let mut needs_backend_reload = false;

        if let Some(idx) = self.backend_select.take_changed() {
            config.backend_config.backend = match idx {
                0 => BackendType::CTranslate2,
                1 => BackendType::WhisperCpp,
                2 => BackendType::Moonshine,
                3 => BackendType::Parakeet,
                _ => config.backend_config.backend,
            };
            let english_only = self.english_only_toggle.value;
            let new_options = models_for_backend(config.backend_config.backend, english_only);
            let new_selected = new_options
                .iter()
                .position(|o| o.value == config.general_config.model)
                .unwrap_or(0);
            self.model_select.options = new_options;
            self.model_select.selected_index = new_selected;
            config.general_config.model = self.model_select.selected_value().to_string();
            needs_backend_reload = true;
            any_changed = true;
        }
        if let Some(val) = self.english_only_toggle.take_changed() {
            if val {
                config.general_config.language = "en".to_string();
            }
            any_changed = true;
        }
        if let Some(_idx) = self.model_select.take_changed() {
            config.general_config.model = self.model_select.selected_value().to_string();
            needs_backend_reload = true;
            any_changed = true;
        }
        if let Some(_idx) = self.language_select.take_changed() {
            config.general_config.language = self.language_select.selected_value().to_string();
            needs_backend_reload = true;
            any_changed = true;
        }
        if let Some(val) = self.gpu_toggle.take_changed() {
            config.backend_config.gpu_enabled = val;
            needs_backend_reload = true;
            any_changed = true;
        }
        if let Some(val) = self.threads_slider.take_changed() {
            config.backend_config.threads = val as usize;
            needs_backend_reload = true;
            any_changed = true;
        }

        if let Some(idx) = self.vad_sensitivity_select.take_changed() {
            config.vad_config.sensitivity = match idx {
                0 => VadSensitivity::Low,
                1 => VadSensitivity::Medium,
                _ => VadSensitivity::High,
            };
            any_changed = true;
        }
        if let Some(val) = self.sound_toggle.take_changed() {
            config.sound_config.enabled = val;
            any_changed = true;
        }
        if let Some(val) = self.volume_slider.take_changed() {
            config.sound_config.volume = val;
            any_changed = true;
        }

        if let Some(val) = self.auto_paste_toggle.take_changed() {
            config.portal_config.enable_xdg_portal = val;
            any_changed = true;
        }
        if let Some(val) = self.clear_on_session_toggle.take_changed() {
            config.manual_mode_config.clear_on_new_session = val;
            any_changed = true;
        }
        if let Some(val) = self.post_processing_toggle.take_changed() {
            config.post_process_config.enabled = val;
            any_changed = true;
        }
        if let Some(val) = self.typewriter_toggle.take_changed() {
            config.ui_config.typewriter_effect = val;
            any_changed = true;
        }
        if let Some(idx) = self.shortcut_mode_select.take_changed() {
            config.portal_config.shortcut_mode = match idx {
                1 => ShortcutMode::PushToTalk,
                _ => ShortcutMode::Toggle,
            };
            any_changed = true;
        }
        if let Some(idx) = self.paste_shortcut_select.take_changed() {
            config.portal_config.paste_shortcut = match idx {
                1 => "ctrl_v".to_string(),
                _ => "ctrl_shift_v".to_string(),
            };
            any_changed = true;
        }
        if let Some(val) = self.enhancement_toggle.take_changed() {
            config.enhancement_config.enabled = val;
            any_changed = true;
        }

        if let Some(idx) = self.vsync_select.take_changed() {
            config.display_config.vsync_mode = match idx {
                0 => "Enabled".to_string(),
                1 => "Disabled".to_string(),
                2 => "Adaptive".to_string(),
                3 => "Mailbox".to_string(),
                _ => "Auto".to_string(),
            };
            any_changed = true;
        }
        if let Some(val) = self.target_fps_slider.take_changed() {
            config.display_config.target_fps = val as u32;
            any_changed = true;
        }
        if let Some(val) = self.system_tray_toggle.take_changed() {
            config.window_behavior_config.show_in_system_tray = val;
            any_changed = true;
        }

        if let Some(idx) = self.visual_theme_select.take_changed() {
            config.ui_config.visual_theme = match idx {
                1 => VisualThemePreset::Pulse,
                2 => VisualThemePreset::Terminal,
                _ => VisualThemePreset::Focus,
            };
            any_changed = true;
        }
        if let Some(idx) = self.spectrogram_skin_select.take_changed() {
            config.ui_config.spectrogram_skin = match idx {
                1 => SpectrogramSkin::Waveform,
                2 => SpectrogramSkin::Meter,
                _ => SpectrogramSkin::Bars,
            };
            any_changed = true;
        }
        if let Some(idx) = self.window_position_select.take_changed() {
            config.display_config.window_position = match idx {
                0 => WindowPosition::BottomLeft,
                1 => WindowPosition::BottomCenter,
                2 => WindowPosition::BottomRight,
                3 => WindowPosition::TopLeft,
                4 => WindowPosition::TopCenter,
                5 => WindowPosition::TopRight,
                6 => WindowPosition::MiddleLeft,
                7 => WindowPosition::MiddleCenter,
                8 => WindowPosition::MiddleRight,
                _ => WindowPosition::Custom,
            };
            if config.display_config.window_position != WindowPosition::Custom {
                config.display_config.custom_window_position = None;
            }
            any_changed = true;
        }
        if let Some(val) = self.font_size_slider.take_changed() {
            config.ui_config.font_size = val;
            any_changed = true;
        }
        if let Some(val) = self.recording_indicator_toggle.take_changed() {
            config.ui_config.show_recording_indicator = val;
            any_changed = true;
        }

        (any_changed, needs_backend_reload)
    }

    pub fn take_apply_request(&mut self) -> bool {
        let v = self.apply_requested;
        self.apply_requested = false;
        v
    }

    pub fn clear_pending_changes(&mut self) {
        self.has_pending_changes = false;
        self.apply_requested = false;
        self.clear_widget_change_flags();
    }

    fn clear_widget_change_flags(&mut self) {
        self.backend_select.clear_changed();
        self.english_only_toggle.clear_changed();
        self.model_select.clear_changed();
        self.language_select.clear_changed();
        self.gpu_toggle.clear_changed();
        self.threads_slider.clear_changed();
        self.vad_sensitivity_select.clear_changed();
        self.sound_toggle.clear_changed();
        self.volume_slider.clear_changed();
        self.auto_paste_toggle.clear_changed();
        self.clear_on_session_toggle.clear_changed();
        self.post_processing_toggle.clear_changed();
        self.typewriter_toggle.clear_changed();
        self.shortcut_mode_select.clear_changed();
        self.paste_shortcut_select.clear_changed();
        self.enhancement_toggle.clear_changed();
        self.vsync_select.clear_changed();
        self.target_fps_slider.clear_changed();
        self.system_tray_toggle.clear_changed();
        self.visual_theme_select.clear_changed();
        self.spectrogram_skin_select.clear_changed();
        self.window_position_select.clear_changed();
        self.font_size_slider.clear_changed();
        self.recording_indicator_toggle.clear_changed();
    }

    fn tab_row_count(&self, tab: SettingsTab) -> usize {
        match tab {
            SettingsTab::Backend => {
                let mut rows = 4;
                if self.show_english_toggle {
                    rows += 1;
                }
                if self.show_language_select {
                    rows += 1;
                }
                rows
            }
            SettingsTab::Audio => 3,
            SettingsTab::Behavior => 7,
            SettingsTab::Display => 3,
            SettingsTab::Appearance => 5,
        }
    }

    pub fn recalculate_positions(&mut self, window_width: u32) {
        self.window_width = window_width;
        let x = WIDGET_X;
        let w = default_width(window_width);
        let step = ROW_HEIGHT + SPACING;

        // Backend tab
        let mut y = CONTENT_Y;
        self.backend_select.x = x;
        self.backend_select.y = y;
        self.backend_select.width = w;
        self.backend_select.height = ROW_HEIGHT;
        y += step;
        if self.show_english_toggle {
            self.english_only_toggle.x = x;
            self.english_only_toggle.y = y;
            self.english_only_toggle.width = w;
            self.english_only_toggle.height = ROW_HEIGHT;
            y += step;
        }
        self.model_select.x = x;
        self.model_select.y = y;
        self.model_select.width = w;
        self.model_select.height = ROW_HEIGHT;
        y += step;
        if self.show_language_select {
            self.language_select.x = x;
            self.language_select.y = y;
            self.language_select.width = w;
            self.language_select.height = ROW_HEIGHT;
            y += step;
        }
        self.gpu_toggle.x = x;
        self.gpu_toggle.y = y;
        self.gpu_toggle.width = w;
        self.gpu_toggle.height = ROW_HEIGHT;
        y += step;
        self.threads_slider.x = x;
        self.threads_slider.y = y;
        self.threads_slider.width = w;
        self.threads_slider.height = ROW_HEIGHT;

        // Audio tab
        y = CONTENT_Y;
        self.vad_sensitivity_select.x = x;
        self.vad_sensitivity_select.y = y;
        self.vad_sensitivity_select.width = w;
        self.vad_sensitivity_select.height = ROW_HEIGHT;
        y += step;
        self.sound_toggle.x = x;
        self.sound_toggle.y = y;
        self.sound_toggle.width = w;
        self.sound_toggle.height = ROW_HEIGHT;
        y += step;
        self.volume_slider.x = x;
        self.volume_slider.y = y;
        self.volume_slider.width = w;
        self.volume_slider.height = ROW_HEIGHT;

        // Behavior tab
        y = CONTENT_Y;
        self.auto_paste_toggle.x = x;
        self.auto_paste_toggle.y = y;
        self.auto_paste_toggle.width = w;
        self.auto_paste_toggle.height = ROW_HEIGHT;
        y += step;
        self.clear_on_session_toggle.x = x;
        self.clear_on_session_toggle.y = y;
        self.clear_on_session_toggle.width = w;
        self.clear_on_session_toggle.height = ROW_HEIGHT;
        y += step;
        self.post_processing_toggle.x = x;
        self.post_processing_toggle.y = y;
        self.post_processing_toggle.width = w;
        self.post_processing_toggle.height = ROW_HEIGHT;
        y += step;
        self.typewriter_toggle.x = x;
        self.typewriter_toggle.y = y;
        self.typewriter_toggle.width = w;
        self.typewriter_toggle.height = ROW_HEIGHT;
        y += step;
        self.shortcut_mode_select.x = x;
        self.shortcut_mode_select.y = y;
        self.shortcut_mode_select.width = w;
        self.shortcut_mode_select.height = ROW_HEIGHT;
        y += step;
        self.paste_shortcut_select.x = x;
        self.paste_shortcut_select.y = y;
        self.paste_shortcut_select.width = w;
        self.paste_shortcut_select.height = ROW_HEIGHT;
        y += step;
        self.enhancement_toggle.x = x;
        self.enhancement_toggle.y = y;
        self.enhancement_toggle.width = w;
        self.enhancement_toggle.height = ROW_HEIGHT;

        // Display tab
        y = CONTENT_Y;
        self.vsync_select.x = x;
        self.vsync_select.y = y;
        self.vsync_select.width = w;
        self.vsync_select.height = ROW_HEIGHT;
        y += step;
        self.target_fps_slider.x = x;
        self.target_fps_slider.y = y;
        self.target_fps_slider.width = w;
        self.target_fps_slider.height = ROW_HEIGHT;
        y += step;
        self.system_tray_toggle.x = x;
        self.system_tray_toggle.y = y;
        self.system_tray_toggle.width = w;
        self.system_tray_toggle.height = ROW_HEIGHT;

        // Appearance tab
        y = CONTENT_Y;
        self.visual_theme_select.x = x;
        self.visual_theme_select.y = y;
        self.visual_theme_select.width = w;
        self.visual_theme_select.height = ROW_HEIGHT;
        y += step;
        self.spectrogram_skin_select.x = x;
        self.spectrogram_skin_select.y = y;
        self.spectrogram_skin_select.width = w;
        self.spectrogram_skin_select.height = ROW_HEIGHT;
        y += step;
        self.window_position_select.x = x;
        self.window_position_select.y = y;
        self.window_position_select.width = w;
        self.window_position_select.height = ROW_HEIGHT;
        y += step;
        self.font_size_slider.x = x;
        self.font_size_slider.y = y;
        self.font_size_slider.width = w;
        self.font_size_slider.height = ROW_HEIGHT;
        y += step;
        self.recording_indicator_toggle.x = x;
        self.recording_indicator_toggle.y = y;
        self.recording_indicator_toggle.width = w;
        self.recording_indicator_toggle.height = ROW_HEIGHT;
    }

    pub fn handle_click(&mut self, x: f32, y: f32, window_width: u32, _window_height: u32) -> bool {
        if !self.is_open {
            return false;
        }

        let y = self.local_y(y);
        let tab_bar_height = 24.0f32;
        let tab_bar_y = 8.0f32;
        let close_size = 24.0f32;
        let close_x = window_width as f32 - close_size - 4.0;

        // Check close button
        if x >= close_x
            && x <= close_x + close_size
            && y >= tab_bar_y
            && y <= tab_bar_y + close_size
        {
            self.close_requested = true;
            self.clear_tooltip();
            return true;
        }

        let tabs = SettingsTab::all();
        let tab_count = tabs.len() as f32;
        let usable_width = close_x;
        let tab_width = usable_width / tab_count;

        // Check if click is on a tab
        if y >= tab_bar_y && y <= tab_bar_y + tab_bar_height && x < usable_width {
            let tab_index = (x / tab_width) as usize;
            if tab_index < tabs.len() {
                self.active_tab = tabs[tab_index];
                self.close_all_dropdowns();
                return true;
            }
        }

        if let Some(open_id) = self.open_dropdown {
            if self.select_ref(open_id).is_expanded() {
                if let Some(index) = self.select_ref(open_id).dropdown_option_at(x, y) {
                    let previous_index = self.select_ref(open_id).selected_index;
                    self.select_mut(open_id).choose_index(index);
                    if open_id == DropdownId::Backend
                        && self.backend_select.selected_index != previous_index
                    {
                        self.refresh_backend_dependent_options(window_width);
                    }
                    self.close_all_dropdowns();
                    self.mark_pending_if_widget_changed();
                    return true;
                }

                if self.select_ref(open_id).hit_select_box(x, y) {
                    self.close_all_dropdowns();
                    return true;
                }
            }

            if let Some(target_id) = self.active_select_box_at(x, y) {
                if target_id != open_id {
                    self.open_only_dropdown(target_id);
                    return true;
                }
            }

            self.close_all_dropdowns();
            return true;
        }

        // Check Apply and Reset buttons
        let buttons_y = self.buttons_y_for_tab(self.active_tab);
        let w = default_width(window_width);
        let btn_width = 80.0f32;
        let btn_gap = 8.0f32;
        let total_width = btn_width * 2.0 + btn_gap;
        let start_x = WIDGET_X + (w - total_width) / 2.0;
        let reset_btn_x = start_x;
        let apply_btn_x = start_x + btn_width + btn_gap;

        if y >= buttons_y && y <= buttons_y + APPLY_BUTTON_HEIGHT {
            if x >= reset_btn_x && x <= reset_btn_x + btn_width {
                self.reset_tab_to_defaults();
                self.clear_tooltip();
                return true;
            }
            if x >= apply_btn_x && x <= apply_btn_x + btn_width {
                self.apply_requested = true;
                self.clear_tooltip();
                return true;
            }
        }

        // Route clicks to the active tab's widgets
        let mut widget_clicked = false;
        match self.active_tab {
            SettingsTab::Backend => {
                let prev_idx = self.backend_select.selected_index;
                if self.handle_select_click(DropdownId::Backend, x, y) {
                    widget_clicked = true;
                    let new_idx = self.backend_select.selected_index;
                    if new_idx != prev_idx {
                        self.refresh_backend_dependent_options(window_width);
                    }
                }
                if !widget_clicked
                    && self.show_english_toggle
                    && self.english_only_toggle.handle_click(x, y)
                {
                    widget_clicked = true;
                    let english_only = self.english_only_toggle.value;
                    let backend = match self.backend_select.selected_index {
                        0 => BackendType::CTranslate2,
                        1 => BackendType::WhisperCpp,
                        2 => BackendType::Moonshine,
                        3 => BackendType::Parakeet,
                        _ => BackendType::CTranslate2,
                    };
                    let old_model = self.model_select.selected_value().to_string();
                    self.model_select.options = models_for_backend(backend, english_only);
                    let counterpart = if english_only {
                        format!("{}.en", old_model)
                    } else {
                        old_model.trim_end_matches(".en").to_string()
                    };
                    self.model_select.selected_index = self
                        .model_select
                        .options
                        .iter()
                        .position(|o| o.value == counterpart)
                        .unwrap_or(0);
                    self.model_select.mark_changed();
                    self.show_language_select = backend_has_language_select(backend, english_only);
                    if self.show_language_select {
                        self.language_select.options = languages_for_backend(backend);
                        self.language_select.selected_index = 0;
                    }
                    self.recalculate_positions(window_width);
                }
                if !widget_clicked && self.handle_select_click(DropdownId::Model, x, y) {
                    widget_clicked = true;
                }
                if !widget_clicked
                    && self.show_language_select
                    && self.handle_select_click(DropdownId::Language, x, y)
                {
                    widget_clicked = true;
                }
                if !widget_clicked && self.gpu_toggle.handle_click(x, y) {
                    widget_clicked = true;
                }
                if !widget_clicked && self.threads_slider.handle_click(x, y) {
                    widget_clicked = true;
                }
            }
            SettingsTab::Audio => {
                if self.handle_select_click(DropdownId::VadSensitivity, x, y) {
                    widget_clicked = true;
                }
                if !widget_clicked && self.sound_toggle.handle_click(x, y) {
                    widget_clicked = true;
                }
                if !widget_clicked && self.volume_slider.handle_click(x, y) {
                    widget_clicked = true;
                }
            }
            SettingsTab::Behavior => {
                if self.handle_select_click(DropdownId::ShortcutMode, x, y) {
                    widget_clicked = true;
                }
                if !widget_clicked && self.handle_select_click(DropdownId::PasteShortcut, x, y) {
                    widget_clicked = true;
                }
                if !widget_clicked && self.auto_paste_toggle.handle_click(x, y) {
                    widget_clicked = true;
                }
                if !widget_clicked && self.clear_on_session_toggle.handle_click(x, y) {
                    widget_clicked = true;
                }
                if !widget_clicked && self.post_processing_toggle.handle_click(x, y) {
                    widget_clicked = true;
                }
                if !widget_clicked && self.typewriter_toggle.handle_click(x, y) {
                    widget_clicked = true;
                }
                if !widget_clicked && self.enhancement_toggle.handle_click(x, y) {
                    widget_clicked = true;
                }
            }
            SettingsTab::Appearance => {
                if self.handle_select_click(DropdownId::VisualTheme, x, y) {
                    widget_clicked = true;
                }
                if !widget_clicked && self.handle_select_click(DropdownId::SpectrogramSkin, x, y) {
                    widget_clicked = true;
                }
                if !widget_clicked && self.handle_select_click(DropdownId::WindowPosition, x, y) {
                    widget_clicked = true;
                }
                if !widget_clicked && self.font_size_slider.handle_click(x, y) {
                    widget_clicked = true;
                }
                if !widget_clicked && self.recording_indicator_toggle.handle_click(x, y) {
                    widget_clicked = true;
                }
            }
            SettingsTab::Display => {
                if self.handle_select_click(DropdownId::Vsync, x, y) {
                    widget_clicked = true;
                }
                if !widget_clicked && self.target_fps_slider.handle_click(x, y) {
                    widget_clicked = true;
                }
                if !widget_clicked && self.system_tray_toggle.handle_click(x, y) {
                    widget_clicked = true;
                }
            }
        }

        if widget_clicked {
            self.mark_pending_if_widget_changed();
        }

        true
    }

    pub fn handle_mouse_move(&mut self, x: f32, y: f32) {
        let y = self.local_y(y);
        match self.active_tab {
            SettingsTab::Backend => {
                self.backend_select.handle_mouse_move(x, y);
                self.model_select.handle_mouse_move(x, y);
                if self.show_language_select {
                    self.language_select.handle_mouse_move(x, y);
                }
            }
            SettingsTab::Audio => {
                self.vad_sensitivity_select.handle_mouse_move(x, y);
            }
            SettingsTab::Behavior => {
                self.shortcut_mode_select.handle_mouse_move(x, y);
                self.paste_shortcut_select.handle_mouse_move(x, y);
            }
            SettingsTab::Display => {
                self.vsync_select.handle_mouse_move(x, y);
            }
            SettingsTab::Appearance => {
                self.visual_theme_select.handle_mouse_move(x, y);
                self.spectrogram_skin_select.handle_mouse_move(x, y);
                self.window_position_select.handle_mouse_move(x, y);
            }
        }

        // Route drag to active tab sliders
        match self.active_tab {
            SettingsTab::Backend => {
                self.threads_slider.handle_drag(x, y);
            }
            SettingsTab::Audio => {
                self.volume_slider.handle_drag(x, y);
            }
            SettingsTab::Display => {
                self.target_fps_slider.handle_drag(x, y);
            }
            SettingsTab::Appearance => {
                self.font_size_slider.handle_drag(x, y);
            }
            _ => {}
        }
        self.update_tooltip_hover(x, y);
        self.mark_pending_if_widget_changed();
    }

    pub fn handle_mouse_release(&mut self) {
        self.threads_slider.handle_release();
        self.volume_slider.handle_release();
        self.target_fps_slider.handle_release();
        self.font_size_slider.handle_release();
    }

    pub fn update_animations(&mut self) {
        self.english_only_toggle.update_animation();
        self.gpu_toggle.update_animation();
        self.sound_toggle.update_animation();
        self.auto_paste_toggle.update_animation();
        self.clear_on_session_toggle.update_animation();
        self.post_processing_toggle.update_animation();
        self.typewriter_toggle.update_animation();
        self.enhancement_toggle.update_animation();
        self.system_tray_toggle.update_animation();
        self.recording_indicator_toggle.update_animation();
    }

    pub fn handle_key(&mut self, key: &Key, shift: bool) -> bool {
        match key {
            Key::Named(NamedKey::Tab) => {
                let tabs = SettingsTab::all();
                let current = tabs.iter().position(|t| *t == self.active_tab).unwrap_or(0);
                if shift {
                    self.active_tab = tabs[(current + tabs.len() - 1) % tabs.len()];
                } else {
                    self.active_tab = tabs[(current + 1) % tabs.len()];
                }
                self.close_all_dropdowns();
                true
            }
            _ => false,
        }
    }

    fn close_all_dropdowns(&mut self) {
        self.backend_select.set_expanded(false);
        self.model_select.set_expanded(false);
        self.language_select.set_expanded(false);
        self.vad_sensitivity_select.set_expanded(false);
        self.shortcut_mode_select.set_expanded(false);
        self.paste_shortcut_select.set_expanded(false);
        self.vsync_select.set_expanded(false);
        self.visual_theme_select.set_expanded(false);
        self.spectrogram_skin_select.set_expanded(false);
        self.window_position_select.set_expanded(false);
        self.open_dropdown = None;
        self.clear_tooltip();
    }

    fn reset_tab_to_defaults(&mut self) {
        let defaults = AppConfig::default();
        match self.active_tab {
            SettingsTab::Backend => {
                let backend = defaults.backend_config.backend;
                self.backend_select.selected_index = match backend {
                    BackendType::CTranslate2 => 0,
                    BackendType::WhisperCpp => 1,
                    BackendType::Moonshine => 2,
                    BackendType::Parakeet => 3,
                };
                self.backend_select.mark_changed();
                self.show_english_toggle = backend_has_english_toggle(backend);
                let english_only = defaults.general_config.model.ends_with(".en");
                self.english_only_toggle.set_value(english_only);
                self.english_only_toggle.mark_changed();
                self.model_select.options = models_for_backend(backend, english_only);
                self.model_select.selected_index = self
                    .model_select
                    .options
                    .iter()
                    .position(|o| o.value == defaults.general_config.model)
                    .unwrap_or(0);
                self.model_select.mark_changed();
                self.show_language_select = backend_has_language_select(backend, english_only);
                if self.show_language_select {
                    self.language_select.options = languages_for_backend(backend);
                    self.language_select.selected_index = self
                        .language_select
                        .options
                        .iter()
                        .position(|o| o.value == defaults.general_config.language)
                        .unwrap_or(0);
                    self.language_select.mark_changed();
                }
                self.gpu_toggle
                    .set_value(defaults.backend_config.gpu_enabled);
                self.gpu_toggle.mark_changed();
                self.threads_slider.value = defaults.backend_config.threads as f32;
                self.threads_slider.mark_changed();
                self.recalculate_positions(self.window_width);
            }
            SettingsTab::Audio => {
                self.vad_sensitivity_select.selected_index = match defaults.vad_config.sensitivity {
                    VadSensitivity::Low => 0,
                    VadSensitivity::Medium => 1,
                    VadSensitivity::High => 2,
                };
                self.vad_sensitivity_select.mark_changed();
                self.sound_toggle.set_value(defaults.sound_config.enabled);
                self.sound_toggle.mark_changed();
                self.volume_slider.value = defaults.sound_config.volume;
                self.volume_slider.mark_changed();
            }
            SettingsTab::Behavior => {
                self.auto_paste_toggle
                    .set_value(defaults.portal_config.enable_xdg_portal);
                self.auto_paste_toggle.mark_changed();
                self.clear_on_session_toggle
                    .set_value(defaults.manual_mode_config.clear_on_new_session);
                self.clear_on_session_toggle.mark_changed();
                self.post_processing_toggle
                    .set_value(defaults.post_process_config.enabled);
                self.post_processing_toggle.mark_changed();
                self.typewriter_toggle
                    .set_value(defaults.ui_config.typewriter_effect);
                self.typewriter_toggle.mark_changed();
                self.shortcut_mode_select.selected_index =
                    match defaults.portal_config.shortcut_mode {
                        ShortcutMode::Toggle => 0,
                        ShortcutMode::PushToTalk => 1,
                    };
                self.shortcut_mode_select.mark_changed();
                self.paste_shortcut_select.selected_index =
                    match defaults.portal_config.paste_shortcut.as_str() {
                        "ctrl_v" => 1,
                        _ => 0,
                    };
                self.paste_shortcut_select.mark_changed();
                self.enhancement_toggle
                    .set_value(defaults.enhancement_config.enabled);
                self.enhancement_toggle.mark_changed();
            }
            SettingsTab::Display => {
                self.vsync_select.selected_index = match defaults.display_config.vsync_mode.as_str()
                {
                    "Enabled" => 0,
                    "Disabled" => 1,
                    "Adaptive" => 2,
                    "Mailbox" => 3,
                    "Auto" => 4,
                    _ => 4,
                };
                self.vsync_select.mark_changed();
                self.target_fps_slider.value = defaults.display_config.target_fps as f32;
                self.target_fps_slider.mark_changed();
                self.system_tray_toggle
                    .set_value(defaults.window_behavior_config.show_in_system_tray);
                self.system_tray_toggle.mark_changed();
            }
            SettingsTab::Appearance => {
                self.visual_theme_select.selected_index = match defaults.ui_config.visual_theme {
                    VisualThemePreset::Focus => 0,
                    VisualThemePreset::Pulse => 1,
                    VisualThemePreset::Terminal => 2,
                };
                self.visual_theme_select.mark_changed();
                self.spectrogram_skin_select.selected_index =
                    match defaults.ui_config.spectrogram_skin {
                        SpectrogramSkin::Bars => 0,
                        SpectrogramSkin::Waveform => 1,
                        SpectrogramSkin::Meter => 2,
                    };
                self.spectrogram_skin_select.mark_changed();
                self.window_position_select.selected_index =
                    match defaults.display_config.window_position {
                        WindowPosition::BottomLeft => 0,
                        WindowPosition::BottomCenter => 1,
                        WindowPosition::BottomRight => 2,
                        WindowPosition::TopLeft => 3,
                        WindowPosition::TopCenter => 4,
                        WindowPosition::TopRight => 5,
                        WindowPosition::MiddleLeft => 6,
                        WindowPosition::MiddleCenter => 7,
                        WindowPosition::MiddleRight => 8,
                        WindowPosition::Custom => 9,
                    };
                self.window_position_select.mark_changed();
                self.font_size_slider.value = defaults.ui_config.font_size;
                self.font_size_slider.mark_changed();
                self.recording_indicator_toggle
                    .set_value(defaults.ui_config.show_recording_indicator);
                self.recording_indicator_toggle.mark_changed();
            }
        }
        self.has_pending_changes = true;
    }

    fn draw_row_bg(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        queue: &wgpu::Queue,
        y: f32,
        window_width: u32,
        window_height: u32,
    ) {
        let w = default_width(window_width);
        self.widget_renderer.draw_rounded_rect(
            encoder,
            view,
            queue,
            WIDGET_X,
            y,
            w,
            ROW_HEIGHT,
            6.0,
            [0.012, 0.012, 0.016, 1.0],
            window_width,
            window_height,
        );
    }

    pub fn render(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        queue: &wgpu::Queue,
        window_width: u32,
        window_height: u32,
    ) {
        if !self.is_open {
            return;
        }

        if self.animation_active {
            let elapsed = self.animation_start.elapsed().as_secs_f32();
            let t = (elapsed / 0.2).min(1.0);
            let eased = 1.0 - (1.0 - t).powi(3);

            if self.opening {
                self.animation_progress = eased;
            } else {
                self.animation_progress = 1.0 - eased;
            }

            if t >= 1.0 {
                self.animation_active = false;
                self.animation_progress = if self.opening { 1.0 } else { 0.0 };
                if !self.opening {
                    self.is_open = false;
                    return;
                }
            }
        }

        if self.animation_progress <= 0.0 {
            return;
        }

        let y_offset = (1.0 - self.animation_progress) * 80.0;

        self.update_animations();

        // Collect all text items for batch rendering
        let mut text_items: Vec<TextItem> = Vec::new();

        // Close button
        let close_size = 24.0f32;
        let close_x = window_width as f32 - close_size - 4.0;
        let close_y = 8.0f32 + y_offset;

        // Close button circular background
        self.widget_renderer.draw_rounded_rect(
            encoder,
            view,
            queue,
            close_x,
            close_y,
            close_size,
            close_size,
            close_size / 2.0,
            [0.027, 0.027, 0.040, 0.7],
            window_width,
            window_height,
        );

        text_items.push(TextItem {
            text: "\u{2715}".to_string(),
            x: close_x + 5.0,
            y: close_y + 3.0,
            scale: 1.0,
            color: [0.604, 0.604, 0.604, 1.0],
            max_width: close_size,
        });

        // Tab bar background
        let tabs = SettingsTab::all();
        let tab_count = tabs.len() as f32;
        let usable_width = close_x;
        let tab_width = usable_width / tab_count;
        let tab_bar_y = 8.0f32 + y_offset;

        self.widget_renderer.draw_rounded_rect(
            encoder,
            view,
            queue,
            0.0,
            tab_bar_y - 4.0,
            window_width as f32,
            32.0,
            6.0,
            [0.005, 0.005, 0.010, 1.0],
            window_width,
            window_height,
        );

        for (i, tab) in tabs.iter().enumerate() {
            let is_active = *tab == self.active_tab;
            let tab_x = (i as f32) * tab_width;

            // Active tab pill highlight
            if is_active {
                self.widget_renderer.draw_rounded_rect(
                    encoder,
                    view,
                    queue,
                    tab_x + 2.0,
                    tab_bar_y - 1.0,
                    tab_width - 4.0,
                    22.0,
                    8.0,
                    [0.021, 0.021, 0.033, 1.0],
                    window_width,
                    window_height,
                );
            }

            let color = if is_active {
                [0.885, 0.885, 0.930, 1.0]
            } else {
                [0.171, 0.171, 0.214, 0.7]
            };

            let char_width = 6.5f32;
            let text_width = tab.label().len() as f32 * char_width;
            let centered_x = tab_x + (tab_width - text_width) / 2.0;

            text_items.push(TextItem {
                text: tab.label().to_string(),
                x: centered_x,
                y: tab_bar_y + 4.0,
                scale: 1.0,
                color,
                max_width: tab_width,
            });

            // Active tab accent underline
            if is_active {
                self.widget_renderer.draw_rounded_rect(
                    encoder,
                    view,
                    queue,
                    tab_x + 4.0,
                    tab_bar_y + 22.0,
                    tab_width - 8.0,
                    1.5,
                    0.75,
                    [0.010, 0.787, 0.214, 1.0],
                    window_width,
                    window_height,
                );
            }
        }

        // Content separator line
        self.widget_renderer.draw_rounded_rect(
            encoder,
            view,
            queue,
            WIDGET_X,
            CONTENT_Y - 6.0 + y_offset,
            default_width(window_width),
            1.0,
            0.5,
            [0.021, 0.021, 0.033, 0.4],
            window_width,
            window_height,
        );

        // Render active tab's widgets with an animation offset. Widget layout state
        // remains stable; only the frame-local paint position changes.
        let content_y_offset = y_offset;
        let row_y = |y: f32| y + content_y_offset;
        match self.active_tab {
            SettingsTab::Backend => {
                self.draw_row_bg(
                    encoder,
                    view,
                    queue,
                    row_y(self.backend_select.y),
                    window_width,
                    window_height,
                );
                self.backend_select.render_at(
                    row_y(self.backend_select.y),
                    encoder,
                    view,
                    &self.widget_renderer,
                    &mut text_items,
                    queue,
                    window_width,
                    window_height,
                );
                if self.show_english_toggle {
                    self.draw_row_bg(
                        encoder,
                        view,
                        queue,
                        row_y(self.english_only_toggle.y),
                        window_width,
                        window_height,
                    );
                    self.english_only_toggle.render_at(
                        row_y(self.english_only_toggle.y),
                        encoder,
                        view,
                        &self.widget_renderer,
                        &mut text_items,
                        queue,
                        window_width,
                        window_height,
                    );
                }
                self.draw_row_bg(
                    encoder,
                    view,
                    queue,
                    row_y(self.model_select.y),
                    window_width,
                    window_height,
                );
                self.model_select.render_at(
                    row_y(self.model_select.y),
                    encoder,
                    view,
                    &self.widget_renderer,
                    &mut text_items,
                    queue,
                    window_width,
                    window_height,
                );
                if self.show_language_select {
                    self.draw_row_bg(
                        encoder,
                        view,
                        queue,
                        row_y(self.language_select.y),
                        window_width,
                        window_height,
                    );
                    self.language_select.render_at(
                        row_y(self.language_select.y),
                        encoder,
                        view,
                        &self.widget_renderer,
                        &mut text_items,
                        queue,
                        window_width,
                        window_height,
                    );
                }
                self.draw_row_bg(
                    encoder,
                    view,
                    queue,
                    row_y(self.gpu_toggle.y),
                    window_width,
                    window_height,
                );
                self.gpu_toggle.render_at(
                    row_y(self.gpu_toggle.y),
                    encoder,
                    view,
                    &self.widget_renderer,
                    &mut text_items,
                    queue,
                    window_width,
                    window_height,
                );
                self.draw_row_bg(
                    encoder,
                    view,
                    queue,
                    row_y(self.threads_slider.y),
                    window_width,
                    window_height,
                );
                self.threads_slider.render_at(
                    row_y(self.threads_slider.y),
                    encoder,
                    view,
                    &self.widget_renderer,
                    &mut text_items,
                    queue,
                    window_width,
                    window_height,
                );
            }
            SettingsTab::Audio => {
                self.draw_row_bg(
                    encoder,
                    view,
                    queue,
                    row_y(self.vad_sensitivity_select.y),
                    window_width,
                    window_height,
                );
                self.vad_sensitivity_select.render_at(
                    row_y(self.vad_sensitivity_select.y),
                    encoder,
                    view,
                    &self.widget_renderer,
                    &mut text_items,
                    queue,
                    window_width,
                    window_height,
                );
                self.draw_row_bg(
                    encoder,
                    view,
                    queue,
                    row_y(self.sound_toggle.y),
                    window_width,
                    window_height,
                );
                self.sound_toggle.render_at(
                    row_y(self.sound_toggle.y),
                    encoder,
                    view,
                    &self.widget_renderer,
                    &mut text_items,
                    queue,
                    window_width,
                    window_height,
                );
                self.draw_row_bg(
                    encoder,
                    view,
                    queue,
                    row_y(self.volume_slider.y),
                    window_width,
                    window_height,
                );
                self.volume_slider.render_at(
                    row_y(self.volume_slider.y),
                    encoder,
                    view,
                    &self.widget_renderer,
                    &mut text_items,
                    queue,
                    window_width,
                    window_height,
                );
            }
            SettingsTab::Behavior => {
                self.draw_row_bg(
                    encoder,
                    view,
                    queue,
                    row_y(self.auto_paste_toggle.y),
                    window_width,
                    window_height,
                );
                self.auto_paste_toggle.render_at(
                    row_y(self.auto_paste_toggle.y),
                    encoder,
                    view,
                    &self.widget_renderer,
                    &mut text_items,
                    queue,
                    window_width,
                    window_height,
                );
                self.draw_row_bg(
                    encoder,
                    view,
                    queue,
                    row_y(self.clear_on_session_toggle.y),
                    window_width,
                    window_height,
                );
                self.clear_on_session_toggle.render_at(
                    row_y(self.clear_on_session_toggle.y),
                    encoder,
                    view,
                    &self.widget_renderer,
                    &mut text_items,
                    queue,
                    window_width,
                    window_height,
                );
                self.draw_row_bg(
                    encoder,
                    view,
                    queue,
                    row_y(self.post_processing_toggle.y),
                    window_width,
                    window_height,
                );
                self.post_processing_toggle.render_at(
                    row_y(self.post_processing_toggle.y),
                    encoder,
                    view,
                    &self.widget_renderer,
                    &mut text_items,
                    queue,
                    window_width,
                    window_height,
                );
                self.draw_row_bg(
                    encoder,
                    view,
                    queue,
                    row_y(self.typewriter_toggle.y),
                    window_width,
                    window_height,
                );
                self.typewriter_toggle.render_at(
                    row_y(self.typewriter_toggle.y),
                    encoder,
                    view,
                    &self.widget_renderer,
                    &mut text_items,
                    queue,
                    window_width,
                    window_height,
                );
                self.draw_row_bg(
                    encoder,
                    view,
                    queue,
                    row_y(self.shortcut_mode_select.y),
                    window_width,
                    window_height,
                );
                self.shortcut_mode_select.render_at(
                    row_y(self.shortcut_mode_select.y),
                    encoder,
                    view,
                    &self.widget_renderer,
                    &mut text_items,
                    queue,
                    window_width,
                    window_height,
                );
                self.draw_row_bg(
                    encoder,
                    view,
                    queue,
                    row_y(self.paste_shortcut_select.y),
                    window_width,
                    window_height,
                );
                self.paste_shortcut_select.render_at(
                    row_y(self.paste_shortcut_select.y),
                    encoder,
                    view,
                    &self.widget_renderer,
                    &mut text_items,
                    queue,
                    window_width,
                    window_height,
                );
                self.draw_row_bg(
                    encoder,
                    view,
                    queue,
                    row_y(self.enhancement_toggle.y),
                    window_width,
                    window_height,
                );
                self.enhancement_toggle.render_at(
                    row_y(self.enhancement_toggle.y),
                    encoder,
                    view,
                    &self.widget_renderer,
                    &mut text_items,
                    queue,
                    window_width,
                    window_height,
                );
            }
            SettingsTab::Display => {
                self.draw_row_bg(
                    encoder,
                    view,
                    queue,
                    row_y(self.vsync_select.y),
                    window_width,
                    window_height,
                );
                self.vsync_select.render_at(
                    row_y(self.vsync_select.y),
                    encoder,
                    view,
                    &self.widget_renderer,
                    &mut text_items,
                    queue,
                    window_width,
                    window_height,
                );
                self.draw_row_bg(
                    encoder,
                    view,
                    queue,
                    row_y(self.target_fps_slider.y),
                    window_width,
                    window_height,
                );
                self.target_fps_slider.render_at(
                    row_y(self.target_fps_slider.y),
                    encoder,
                    view,
                    &self.widget_renderer,
                    &mut text_items,
                    queue,
                    window_width,
                    window_height,
                );
                self.draw_row_bg(
                    encoder,
                    view,
                    queue,
                    row_y(self.system_tray_toggle.y),
                    window_width,
                    window_height,
                );
                self.system_tray_toggle.render_at(
                    row_y(self.system_tray_toggle.y),
                    encoder,
                    view,
                    &self.widget_renderer,
                    &mut text_items,
                    queue,
                    window_width,
                    window_height,
                );
            }
            SettingsTab::Appearance => {
                self.draw_row_bg(
                    encoder,
                    view,
                    queue,
                    row_y(self.visual_theme_select.y),
                    window_width,
                    window_height,
                );
                self.visual_theme_select.render_at(
                    row_y(self.visual_theme_select.y),
                    encoder,
                    view,
                    &self.widget_renderer,
                    &mut text_items,
                    queue,
                    window_width,
                    window_height,
                );
                self.draw_row_bg(
                    encoder,
                    view,
                    queue,
                    row_y(self.spectrogram_skin_select.y),
                    window_width,
                    window_height,
                );
                self.spectrogram_skin_select.render_at(
                    row_y(self.spectrogram_skin_select.y),
                    encoder,
                    view,
                    &self.widget_renderer,
                    &mut text_items,
                    queue,
                    window_width,
                    window_height,
                );
                self.draw_row_bg(
                    encoder,
                    view,
                    queue,
                    row_y(self.window_position_select.y),
                    window_width,
                    window_height,
                );
                self.window_position_select.render_at(
                    row_y(self.window_position_select.y),
                    encoder,
                    view,
                    &self.widget_renderer,
                    &mut text_items,
                    queue,
                    window_width,
                    window_height,
                );
                self.draw_row_bg(
                    encoder,
                    view,
                    queue,
                    row_y(self.font_size_slider.y),
                    window_width,
                    window_height,
                );
                self.font_size_slider.render_at(
                    row_y(self.font_size_slider.y),
                    encoder,
                    view,
                    &self.widget_renderer,
                    &mut text_items,
                    queue,
                    window_width,
                    window_height,
                );
                self.draw_row_bg(
                    encoder,
                    view,
                    queue,
                    row_y(self.recording_indicator_toggle.y),
                    window_width,
                    window_height,
                );
                self.recording_indicator_toggle.render_at(
                    row_y(self.recording_indicator_toggle.y),
                    encoder,
                    view,
                    &self.widget_renderer,
                    &mut text_items,
                    queue,
                    window_width,
                    window_height,
                );
            }
        }

        // Apply and Reset buttons
        {
            let buttons_y = self.buttons_y_for_tab(self.active_tab) + content_y_offset;
            let w = default_width(window_width);
            let btn_width = 80.0f32;
            let btn_gap = 8.0f32;
            let total_width = btn_width * 2.0 + btn_gap;
            let start_x = WIDGET_X + (w - total_width) / 2.0;
            let reset_btn_x = start_x;
            let apply_btn_x = start_x + btn_width + btn_gap;

            // Reset button
            self.widget_renderer.draw_rounded_rect(
                encoder,
                view,
                queue,
                reset_btn_x,
                buttons_y,
                btn_width,
                APPLY_BUTTON_HEIGHT,
                8.0,
                [0.15, 0.15, 0.2, 1.0],
                window_width,
                window_height,
            );
            {
                let label = "Reset";
                let char_width = 6.5f32;
                let text_width = label.len() as f32 * char_width;
                let text_x = reset_btn_x + (btn_width - text_width) / 2.0;
                let text_y = buttons_y + (APPLY_BUTTON_HEIGHT - 14.0) / 2.0;
                text_items.push(TextItem {
                    text: label.to_string(),
                    x: text_x,
                    y: text_y,
                    scale: 1.0,
                    color: [0.8, 0.8, 0.85, 1.0],
                    max_width: btn_width,
                });
            }

            // Apply button
            let (bg_color, text_color) = if self.has_pending_changes {
                ([0.010, 0.787, 0.214, 1.0], [1.0, 1.0, 1.0, 1.0])
            } else {
                ([0.027, 0.027, 0.040, 0.5], [0.300, 0.300, 0.340, 1.0])
            };

            self.widget_renderer.draw_rounded_rect(
                encoder,
                view,
                queue,
                apply_btn_x,
                buttons_y,
                btn_width,
                APPLY_BUTTON_HEIGHT,
                8.0,
                bg_color,
                window_width,
                window_height,
            );
            {
                let label = "Apply";
                let char_width = 6.5f32;
                let text_width = label.len() as f32 * char_width;
                let text_x = apply_btn_x + (btn_width - text_width) / 2.0;
                let text_y = buttons_y + (APPLY_BUTTON_HEIGHT - 14.0) / 2.0;
                text_items.push(TextItem {
                    text: label.to_string(),
                    x: text_x,
                    y: text_y,
                    scale: 1.0,
                    color: text_color,
                    max_width: btn_width,
                });
            }
        }

        // Flush all batched widget rects (row bgs, controls)
        self.widget_renderer
            .flush(encoder, view, window_width, window_height);
        self.batch_text_renderer
            .render_batch(encoder, view, &text_items);

        let mut overlay_text_items: Vec<TextItem> = Vec::new();

        // Render one open dropdown on top of all rows. The panel owns the open
        // menu, so sibling selects cannot accidentally stack or fight for input.
        if let Some(dropdown_id) = self.open_dropdown {
            let select = self.select_ref(dropdown_id);
            select.render_dropdown_at(
                row_y(select.y),
                encoder,
                view,
                &self.widget_renderer,
                &mut overlay_text_items,
                queue,
                window_width,
                window_height,
            );
        }

        if self.open_dropdown.is_none()
            && self.hover_start.elapsed().as_millis() >= TOOLTIP_DELAY_MS
        {
            if let Some(tooltip) = self.hovered_tooltip {
                let padding_x = 8.0f32;
                let padding_y = 5.0f32;
                let font_size = 10.5f32;
                let char_width = font_size * 0.58;
                let raw_text_width = tooltip.text.chars().count() as f32 * char_width;
                let max_tip_width = (window_width as f32 - 8.0).max(1.0);
                let min_tip_width = 80.0f32.min(max_tip_width);
                let tip_width = (raw_text_width + padding_x * 2.0)
                    .max(min_tip_width)
                    .min(max_tip_width);
                let text_width = (tip_width - padding_x * 2.0).max(1.0);
                let line_count = (raw_text_width / text_width).ceil().max(1.0);
                let tip_height = line_count * font_size * 1.2 + padding_y * 2.0;

                let right_x = self.tooltip_anchor_x + 12.0;
                let tip_x = if right_x + tip_width <= window_width as f32 - 4.0 {
                    right_x
                } else {
                    (self.tooltip_anchor_x - tip_width - 12.0).max(4.0)
                };

                let row_screen_y = row_y(tooltip.row_y);
                let below_y = row_screen_y + ROW_HEIGHT + 6.0;
                let above_y = row_screen_y - tip_height - 6.0;
                let top_limit = tab_bar_y + 28.0;
                let bottom_limit = window_height as f32 - 4.0;
                let tip_y = if below_y + tip_height <= bottom_limit {
                    below_y
                } else if above_y >= top_limit {
                    above_y
                } else {
                    (bottom_limit - tip_height).max(top_limit)
                };

                self.widget_renderer.draw_rounded_rect(
                    encoder,
                    view,
                    queue,
                    tip_x,
                    tip_y,
                    tip_width,
                    tip_height,
                    5.0,
                    [0.190, 0.190, 0.230, 0.98],
                    window_width,
                    window_height,
                );
                self.widget_renderer.draw_rounded_rect(
                    encoder,
                    view,
                    queue,
                    tip_x + 1.0,
                    tip_y + 1.0,
                    (tip_width - 2.0).max(1.0),
                    (tip_height - 2.0).max(1.0),
                    4.0,
                    [0.062, 0.062, 0.085, 0.98],
                    window_width,
                    window_height,
                );

                overlay_text_items.push(TextItem {
                    text: tooltip.text.to_string(),
                    x: tip_x + padding_x,
                    y: tip_y + padding_y - 0.5,
                    scale: font_size / 10.0,
                    color: [0.880, 0.880, 0.920, 1.0],
                    max_width: text_width,
                });
            }
        }

        // Flush overlay rects, then render only overlay text. Base row text was
        // already rendered before dropdowns so it cannot bleed through menus.
        self.widget_renderer
            .flush(encoder, view, window_width, window_height);
        if !overlay_text_items.is_empty() {
            if self.overlay_text_renderer.is_none() {
                self.overlay_text_renderer = Some(BatchTextRenderer::new(
                    self.device.clone(),
                    self.queue.clone(),
                    PhysicalSize::new(window_width, window_height),
                    self.surface_format,
                ));
            }

            if let Some(renderer) = &mut self.overlay_text_renderer {
                renderer.render_batch(encoder, view, &overlay_text_items);
            }
        }
    }
}
