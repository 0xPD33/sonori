use std::sync::Arc;
use wgpu;
use winit::dpi::PhysicalSize;
use winit::keyboard::{Key, NamedKey};

use super::batch_text_renderer::{BatchTextRenderer, TextItem};
use super::widgets::{Select, SelectOption, Slider, Toggle, WidgetRenderer};
use crate::backend::BackendType;
use crate::config::{AppConfig, VadSensitivity};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    Backend,
    Audio,
    Behavior,
    Display,
}

impl SettingsTab {
    pub fn label(&self) -> &'static str {
        match self {
            SettingsTab::Backend => "Backend",
            SettingsTab::Audio => "Audio",
            SettingsTab::Behavior => "Behavior",
            SettingsTab::Display => "Display",
        }
    }

    pub fn all() -> &'static [SettingsTab] {
        &[
            SettingsTab::Backend,
            SettingsTab::Audio,
            SettingsTab::Behavior,
            SettingsTab::Display,
        ]
    }
}

pub struct SettingsPanel {
    pub is_open: bool,
    pub close_requested: bool,
    active_tab: SettingsTab,
    batch_text_renderer: BatchTextRenderer,
    widget_renderer: WidgetRenderer,

    // Slide-in animation
    pub animation_progress: f32,
    pub animation_active: bool,
    animation_start: std::time::Instant,
    opening: bool,

    // Backend tab widgets
    backend_select: Select,
    model_select: Select,
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

    // Display tab widgets
    vsync_select: Select,
    target_fps_slider: Slider,
    system_tray_toggle: Toggle,

    // Apply button state
    apply_requested: bool,
    has_pending_changes: bool,

    window_width: u32,
    window_height: u32,
}

const CONTENT_Y: f32 = 42.0;
const WIDGET_X: f32 = 14.0;
const ROW_HEIGHT: f32 = 26.0;
const SPACING: f32 = 6.0;
const APPLY_BUTTON_HEIGHT: f32 = 28.0;

fn default_width(window_width: u32) -> f32 {
    window_width as f32 - 28.0
}

fn models_for_backend(backend: BackendType) -> Vec<SelectOption> {
    let names: &[&str] = match backend {
        BackendType::WhisperCpp => &[
            "tiny", "tiny.en", "base", "base.en", "small", "small.en",
            "medium", "medium.en", "large-v1", "large-v2", "large-v3", "large-v3-turbo",
        ],
        BackendType::CTranslate2 => &[
            "tiny.en", "base.en", "small.en", "medium.en", "large-v3",
        ],
        BackendType::Moonshine => &[
            "tiny", "base",
        ],
        BackendType::Parakeet => &[],
    };
    names
        .iter()
        .map(|n| SelectOption {
            label: n.to_string(),
            value: n.to_string(),
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
        let batch_text_renderer = BatchTextRenderer::new(
            Arc::new(device.clone()),
            Arc::new(queue.clone()),
            size,
            config.format,
        );

        let widget_renderer = WidgetRenderer::new(device, config.format);

        let w = default_width(size.width);

        // Backend tab widgets
        let backend_select = Select::new(
            "Backend",
            vec![
                SelectOption { label: "CTranslate2".into(), value: "CTranslate2".into() },
                SelectOption { label: "WhisperCpp".into(), value: "WhisperCpp".into() },
                SelectOption { label: "Moonshine".into(), value: "Moonshine".into() },
                SelectOption { label: "Parakeet".into(), value: "Parakeet".into() },
            ],
            0,
            WIDGET_X, CONTENT_Y, w, ROW_HEIGHT,
        );
        let model_select = Select::new(
            "Model",
            models_for_backend(BackendType::WhisperCpp),
            0,
            WIDGET_X, CONTENT_Y + ROW_HEIGHT + SPACING, w, ROW_HEIGHT,
        );
        let gpu_toggle = Toggle::new("GPU acceleration", false, WIDGET_X, CONTENT_Y + 2.0 * (ROW_HEIGHT + SPACING), w, ROW_HEIGHT);
        let threads_slider = Slider::new("Threads", 4.0, 1.0, 8.0, 1.0, WIDGET_X, CONTENT_Y + 3.0 * (ROW_HEIGHT + SPACING), w, ROW_HEIGHT);

        // Audio tab widgets
        let vad_sensitivity_select = Select::new(
            "VAD sensitivity",
            vec![
                SelectOption { label: "Low".into(), value: "Low".into() },
                SelectOption { label: "Medium".into(), value: "Medium".into() },
                SelectOption { label: "High".into(), value: "High".into() },
            ],
            1,
            WIDGET_X, CONTENT_Y, w, ROW_HEIGHT,
        );
        let sound_toggle = Toggle::new("Sound feedback", true, WIDGET_X, CONTENT_Y + ROW_HEIGHT + SPACING, w, ROW_HEIGHT);
        let volume_slider = Slider::new("Volume", 0.5, 0.0, 1.0, 0.05, WIDGET_X, CONTENT_Y + 2.0 * (ROW_HEIGHT + SPACING), w, ROW_HEIGHT);

        // Behavior tab widgets
        let auto_paste_toggle = Toggle::new("Auto-paste", true, WIDGET_X, CONTENT_Y, w, ROW_HEIGHT);
        let clear_on_session_toggle = Toggle::new("Clear on new session", true, WIDGET_X, CONTENT_Y + ROW_HEIGHT + SPACING, w, ROW_HEIGHT);
        let post_processing_toggle = Toggle::new("Post-processing", true, WIDGET_X, CONTENT_Y + 2.0 * (ROW_HEIGHT + SPACING), w, ROW_HEIGHT);
        let typewriter_toggle = Toggle::new("Typewriter effect", false, WIDGET_X, CONTENT_Y + 3.0 * (ROW_HEIGHT + SPACING), w, ROW_HEIGHT);

        // Display tab widgets
        let vsync_select = Select::new(
            "VSync mode",
            vec![
                SelectOption { label: "Enabled".into(), value: "Enabled".into() },
                SelectOption { label: "Disabled".into(), value: "Disabled".into() },
                SelectOption { label: "Adaptive".into(), value: "Adaptive".into() },
                SelectOption { label: "Mailbox".into(), value: "Mailbox".into() },
                SelectOption { label: "Auto".into(), value: "Auto".into() },
            ],
            0,
            WIDGET_X, CONTENT_Y, w, ROW_HEIGHT,
        );
        let target_fps_slider = Slider::new("Target FPS", 60.0, 15.0, 240.0, 5.0, WIDGET_X, CONTENT_Y + ROW_HEIGHT + SPACING, w, ROW_HEIGHT);
        let system_tray_toggle = Toggle::new("System tray", true, WIDGET_X, CONTENT_Y + 2.0 * (ROW_HEIGHT + SPACING), w, ROW_HEIGHT);

        Self {
            is_open: false,
            close_requested: false,
            active_tab: SettingsTab::Backend,
            batch_text_renderer,
            widget_renderer,

            animation_progress: 0.0,
            animation_active: false,
            animation_start: std::time::Instant::now(),
            opening: false,

            backend_select,
            model_select,
            gpu_toggle,
            threads_slider,

            vad_sensitivity_select,
            sound_toggle,
            volume_slider,

            auto_paste_toggle,
            clear_on_session_toggle,
            post_processing_toggle,
            typewriter_toggle,

            vsync_select,
            target_fps_slider,
            system_tray_toggle,

            apply_requested: false,
            has_pending_changes: false,

            window_width: size.width,
            window_height: size.height,
        }
    }

    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        self.batch_text_renderer.resize(size);
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
    }

    pub fn populate_from_config(&mut self, config: &AppConfig) {
        // Backend
        self.backend_select.selected_index = match config.backend_config.backend {
            BackendType::CTranslate2 => 0,
            BackendType::WhisperCpp => 1,
            BackendType::Moonshine => 2,
            BackendType::Parakeet => 3,
        };
        self.model_select.options = models_for_backend(config.backend_config.backend);
        self.model_select.selected_index = self
            .model_select
            .options
            .iter()
            .position(|o| o.value == config.general_config.model)
            .unwrap_or(0);
        self.gpu_toggle.value = config.backend_config.gpu_enabled;
        self.threads_slider.value = config.backend_config.threads as f32;

        // Audio
        self.vad_sensitivity_select.selected_index = match config.vad_config.sensitivity {
            VadSensitivity::Low => 0,
            VadSensitivity::Medium => 1,
            VadSensitivity::High => 2,
        };
        self.sound_toggle.value = config.sound_config.enabled;
        self.volume_slider.value = config.sound_config.volume;

        // Behavior
        self.auto_paste_toggle.value = config.portal_config.enable_xdg_portal;
        self.clear_on_session_toggle.value = config.manual_mode_config.clear_on_new_session;
        self.post_processing_toggle.value = config.post_process_config.enabled;
        self.typewriter_toggle.value = config.ui_config.typewriter_effect;

        // Display
        self.vsync_select.selected_index = match config.display_config.vsync_mode.as_str() {
            "Enabled" => 0,
            "Disabled" => 1,
            "Adaptive" => 2,
            "Mailbox" => 3,
            "Auto" | _ => 4,
        };
        self.target_fps_slider.value = config.display_config.target_fps as f32;
        self.system_tray_toggle.value = config.window_behavior_config.show_in_system_tray;
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
            // Update model options for the new backend
            let new_options = models_for_backend(config.backend_config.backend);
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
        if let Some(_idx) = self.model_select.take_changed() {
            config.general_config.model = self.model_select.selected_value().to_string();
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

        (any_changed, needs_backend_reload)
    }

    pub fn take_apply_request(&mut self) -> bool {
        let v = self.apply_requested;
        self.apply_requested = false;
        v
    }

    fn tab_row_count(tab: SettingsTab) -> usize {
        match tab {
            SettingsTab::Backend => 4,
            SettingsTab::Audio => 3,
            SettingsTab::Behavior => 4,
            SettingsTab::Display => 3,
        }
    }

    pub fn recalculate_positions(&mut self, window_width: u32) {
        self.window_width = window_width;
        let x = WIDGET_X;
        let w = default_width(window_width);
        let step = ROW_HEIGHT + SPACING;

        // Backend tab
        let mut y = CONTENT_Y;
        self.backend_select.x = x; self.backend_select.y = y; self.backend_select.width = w; self.backend_select.height = ROW_HEIGHT;
        y += step;
        self.model_select.x = x; self.model_select.y = y; self.model_select.width = w; self.model_select.height = ROW_HEIGHT;
        y += step;
        self.gpu_toggle.x = x; self.gpu_toggle.y = y; self.gpu_toggle.width = w; self.gpu_toggle.height = ROW_HEIGHT;
        y += step;
        self.threads_slider.x = x; self.threads_slider.y = y; self.threads_slider.width = w; self.threads_slider.height = ROW_HEIGHT;

        // Audio tab
        y = CONTENT_Y;
        self.vad_sensitivity_select.x = x; self.vad_sensitivity_select.y = y; self.vad_sensitivity_select.width = w; self.vad_sensitivity_select.height = ROW_HEIGHT;
        y += step;
        self.sound_toggle.x = x; self.sound_toggle.y = y; self.sound_toggle.width = w; self.sound_toggle.height = ROW_HEIGHT;
        y += step;
        self.volume_slider.x = x; self.volume_slider.y = y; self.volume_slider.width = w; self.volume_slider.height = ROW_HEIGHT;

        // Behavior tab
        y = CONTENT_Y;
        self.auto_paste_toggle.x = x; self.auto_paste_toggle.y = y; self.auto_paste_toggle.width = w; self.auto_paste_toggle.height = ROW_HEIGHT;
        y += step;
        self.clear_on_session_toggle.x = x; self.clear_on_session_toggle.y = y; self.clear_on_session_toggle.width = w; self.clear_on_session_toggle.height = ROW_HEIGHT;
        y += step;
        self.post_processing_toggle.x = x; self.post_processing_toggle.y = y; self.post_processing_toggle.width = w; self.post_processing_toggle.height = ROW_HEIGHT;
        y += step;
        self.typewriter_toggle.x = x; self.typewriter_toggle.y = y; self.typewriter_toggle.width = w; self.typewriter_toggle.height = ROW_HEIGHT;

        // Display tab
        y = CONTENT_Y;
        self.vsync_select.x = x; self.vsync_select.y = y; self.vsync_select.width = w; self.vsync_select.height = ROW_HEIGHT;
        y += step;
        self.target_fps_slider.x = x; self.target_fps_slider.y = y; self.target_fps_slider.width = w; self.target_fps_slider.height = ROW_HEIGHT;
        y += step;
        self.system_tray_toggle.x = x; self.system_tray_toggle.y = y; self.system_tray_toggle.width = w; self.system_tray_toggle.height = ROW_HEIGHT;
    }

    pub fn handle_click(
        &mut self,
        x: f32,
        y: f32,
        window_width: u32,
        _window_height: u32,
    ) -> bool {
        if !self.is_open {
            return false;
        }

        let tab_bar_height = 24.0f32;
        let tab_bar_y = 8.0f32;
        let close_size = 24.0f32;
        let close_x = window_width as f32 - close_size - 4.0;

        // Check close button
        if x >= close_x && x <= close_x + close_size
            && y >= tab_bar_y && y <= tab_bar_y + close_size
        {
            self.close_requested = true;
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
                return true;
            }
        }

        // Check Apply button
        let num_rows = Self::tab_row_count(self.active_tab);
        let apply_y = CONTENT_Y + (num_rows as f32) * (ROW_HEIGHT + SPACING) + 12.0;
        let w = default_width(window_width);
        let apply_btn_width = 80.0f32;
        let apply_btn_x = WIDGET_X + (w - apply_btn_width) / 2.0;
        if x >= apply_btn_x && x <= apply_btn_x + apply_btn_width
            && y >= apply_y && y <= apply_y + APPLY_BUTTON_HEIGHT
        {
            self.apply_requested = true;
            self.has_pending_changes = false;
            return true;
        }

        // Route clicks to the active tab's widgets
        let mut widget_clicked = false;
        match self.active_tab {
            SettingsTab::Backend => {
                let prev_idx = self.backend_select.selected_index;
                if self.backend_select.handle_click(x, y) {
                    widget_clicked = true;
                    let new_idx = self.backend_select.selected_index;
                    if new_idx != prev_idx {
                        let backend = match new_idx {
                            0 => BackendType::CTranslate2,
                            1 => BackendType::WhisperCpp,
                            2 => BackendType::Moonshine,
                            3 => BackendType::Parakeet,
                            _ => BackendType::CTranslate2,
                        };
                        self.model_select.options = models_for_backend(backend);
                        self.model_select.selected_index = 0;
                    }
                }
                if !widget_clicked && self.model_select.handle_click(x, y) { widget_clicked = true; }
                if !widget_clicked && self.gpu_toggle.handle_click(x, y) { widget_clicked = true; }
                if !widget_clicked && self.threads_slider.handle_click(x, y) { widget_clicked = true; }
            }
            SettingsTab::Audio => {
                if self.vad_sensitivity_select.handle_click(x, y) { widget_clicked = true; }
                if !widget_clicked && self.sound_toggle.handle_click(x, y) { widget_clicked = true; }
                if !widget_clicked && self.volume_slider.handle_click(x, y) { widget_clicked = true; }
            }
            SettingsTab::Behavior => {
                if self.auto_paste_toggle.handle_click(x, y) { widget_clicked = true; }
                if !widget_clicked && self.clear_on_session_toggle.handle_click(x, y) { widget_clicked = true; }
                if !widget_clicked && self.post_processing_toggle.handle_click(x, y) { widget_clicked = true; }
                if !widget_clicked && self.typewriter_toggle.handle_click(x, y) { widget_clicked = true; }
            }
            SettingsTab::Display => {
                if self.vsync_select.handle_click(x, y) { widget_clicked = true; }
                if !widget_clicked && self.target_fps_slider.handle_click(x, y) { widget_clicked = true; }
                if !widget_clicked && self.system_tray_toggle.handle_click(x, y) { widget_clicked = true; }
            }
        }

        if widget_clicked {
            self.has_pending_changes = true;
        }

        true
    }

    pub fn handle_mouse_move(&mut self, x: f32, y: f32) {
        match self.active_tab {
            SettingsTab::Backend => {
                self.backend_select.handle_mouse_move(x, y);
                self.model_select.handle_mouse_move(x, y);
            }
            SettingsTab::Audio => {
                self.vad_sensitivity_select.handle_mouse_move(x, y);
            }
            SettingsTab::Behavior => {}
            SettingsTab::Display => {
                self.vsync_select.handle_mouse_move(x, y);
            }
        }

        // Route drag to active tab sliders
        match self.active_tab {
            SettingsTab::Backend => { self.threads_slider.handle_drag(x, y); }
            SettingsTab::Audio => { self.volume_slider.handle_drag(x, y); }
            SettingsTab::Display => { self.target_fps_slider.handle_drag(x, y); }
            _ => {}
        }
    }

    pub fn handle_mouse_release(&mut self) {
        self.threads_slider.handle_release();
        self.volume_slider.handle_release();
        self.target_fps_slider.handle_release();
    }

    pub fn update_animations(&mut self) {
        self.gpu_toggle.update_animation();
        self.sound_toggle.update_animation();
        self.auto_paste_toggle.update_animation();
        self.clear_on_session_toggle.update_animation();
        self.post_processing_toggle.update_animation();
        self.typewriter_toggle.update_animation();
        self.system_tray_toggle.update_animation();
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
                true
            }
            _ => false,
        }
    }

    fn draw_row_bg(&self, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, queue: &wgpu::Queue, y: f32, window_width: u32, window_height: u32) {
        let w = default_width(window_width);
        self.widget_renderer.draw_rounded_rect(
            encoder, view, queue,
            WIDGET_X, y,
            w, ROW_HEIGHT,
            6.0,
            [0.012, 0.012, 0.016, 1.0],
            window_width, window_height,
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
            encoder, view, queue,
            close_x, close_y,
            close_size, close_size,
            close_size / 2.0,
            [0.027, 0.027, 0.040, 0.7],
            window_width, window_height,
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
            encoder, view, queue,
            0.0, tab_bar_y - 4.0,
            window_width as f32, 32.0,
            6.0,
            [0.005, 0.005, 0.010, 1.0],
            window_width, window_height,
        );

        for (i, tab) in tabs.iter().enumerate() {
            let is_active = *tab == self.active_tab;
            let tab_x = (i as f32) * tab_width;

            // Active tab pill highlight
            if is_active {
                self.widget_renderer.draw_rounded_rect(
                    encoder, view, queue,
                    tab_x + 2.0, tab_bar_y - 1.0,
                    tab_width - 4.0, 22.0,
                    8.0,
                    [0.021, 0.021, 0.033, 1.0],
                    window_width, window_height,
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
                    encoder, view, queue,
                    tab_x + 4.0, tab_bar_y + 22.0,
                    tab_width - 8.0, 1.5,
                    0.75,
                    [0.010, 0.787, 0.214, 1.0],
                    window_width, window_height,
                );
            }
        }

        // Content separator line
        self.widget_renderer.draw_rounded_rect(
            encoder, view, queue,
            WIDGET_X, CONTENT_Y - 6.0 + y_offset,
            default_width(window_width), 1.0,
            0.5,
            [0.021, 0.021, 0.033, 0.4],
            window_width, window_height,
        );

        // Render active tab's widgets with y_offset applied
        let content_y_offset = y_offset;
        match self.active_tab {
            SettingsTab::Backend => {
                self.backend_select.y += content_y_offset;
                self.model_select.y += content_y_offset;
                self.gpu_toggle.y += content_y_offset;
                self.threads_slider.y += content_y_offset;
                self.draw_row_bg(encoder, view, queue, self.backend_select.y, window_width, window_height);
                self.backend_select.render(encoder, view, &self.widget_renderer, &mut text_items, queue, window_width, window_height);
                self.draw_row_bg(encoder, view, queue, self.model_select.y, window_width, window_height);
                self.model_select.render(encoder, view, &self.widget_renderer, &mut text_items, queue, window_width, window_height);
                self.draw_row_bg(encoder, view, queue, self.gpu_toggle.y, window_width, window_height);
                self.gpu_toggle.render(encoder, view, &self.widget_renderer, &mut text_items, queue, window_width, window_height);
                self.draw_row_bg(encoder, view, queue, self.threads_slider.y, window_width, window_height);
                self.threads_slider.render(encoder, view, &self.widget_renderer, &mut text_items, queue, window_width, window_height);
                self.backend_select.y -= content_y_offset;
                self.model_select.y -= content_y_offset;
                self.gpu_toggle.y -= content_y_offset;
                self.threads_slider.y -= content_y_offset;
            }
            SettingsTab::Audio => {
                self.vad_sensitivity_select.y += content_y_offset;
                self.sound_toggle.y += content_y_offset;
                self.volume_slider.y += content_y_offset;
                self.draw_row_bg(encoder, view, queue, self.vad_sensitivity_select.y, window_width, window_height);
                self.vad_sensitivity_select.render(encoder, view, &self.widget_renderer, &mut text_items, queue, window_width, window_height);
                self.draw_row_bg(encoder, view, queue, self.sound_toggle.y, window_width, window_height);
                self.sound_toggle.render(encoder, view, &self.widget_renderer, &mut text_items, queue, window_width, window_height);
                self.draw_row_bg(encoder, view, queue, self.volume_slider.y, window_width, window_height);
                self.volume_slider.render(encoder, view, &self.widget_renderer, &mut text_items, queue, window_width, window_height);
                self.vad_sensitivity_select.y -= content_y_offset;
                self.sound_toggle.y -= content_y_offset;
                self.volume_slider.y -= content_y_offset;
            }
            SettingsTab::Behavior => {
                self.auto_paste_toggle.y += content_y_offset;
                self.clear_on_session_toggle.y += content_y_offset;
                self.post_processing_toggle.y += content_y_offset;
                self.typewriter_toggle.y += content_y_offset;
                self.draw_row_bg(encoder, view, queue, self.auto_paste_toggle.y, window_width, window_height);
                self.auto_paste_toggle.render(encoder, view, &self.widget_renderer, &mut text_items, queue, window_width, window_height);
                self.draw_row_bg(encoder, view, queue, self.clear_on_session_toggle.y, window_width, window_height);
                self.clear_on_session_toggle.render(encoder, view, &self.widget_renderer, &mut text_items, queue, window_width, window_height);
                self.draw_row_bg(encoder, view, queue, self.post_processing_toggle.y, window_width, window_height);
                self.post_processing_toggle.render(encoder, view, &self.widget_renderer, &mut text_items, queue, window_width, window_height);
                self.draw_row_bg(encoder, view, queue, self.typewriter_toggle.y, window_width, window_height);
                self.typewriter_toggle.render(encoder, view, &self.widget_renderer, &mut text_items, queue, window_width, window_height);
                self.auto_paste_toggle.y -= content_y_offset;
                self.clear_on_session_toggle.y -= content_y_offset;
                self.post_processing_toggle.y -= content_y_offset;
                self.typewriter_toggle.y -= content_y_offset;
            }
            SettingsTab::Display => {
                self.vsync_select.y += content_y_offset;
                self.target_fps_slider.y += content_y_offset;
                self.system_tray_toggle.y += content_y_offset;
                self.draw_row_bg(encoder, view, queue, self.vsync_select.y, window_width, window_height);
                self.vsync_select.render(encoder, view, &self.widget_renderer, &mut text_items, queue, window_width, window_height);
                self.draw_row_bg(encoder, view, queue, self.target_fps_slider.y, window_width, window_height);
                self.target_fps_slider.render(encoder, view, &self.widget_renderer, &mut text_items, queue, window_width, window_height);
                self.draw_row_bg(encoder, view, queue, self.system_tray_toggle.y, window_width, window_height);
                self.system_tray_toggle.render(encoder, view, &self.widget_renderer, &mut text_items, queue, window_width, window_height);
                self.vsync_select.y -= content_y_offset;
                self.target_fps_slider.y -= content_y_offset;
                self.system_tray_toggle.y -= content_y_offset;
            }
        }

        // Apply button
        {
            let num_rows = Self::tab_row_count(self.active_tab);
            let apply_y = CONTENT_Y + (num_rows as f32) * (ROW_HEIGHT + SPACING) + 12.0 + content_y_offset;
            let w = default_width(window_width);
            let apply_btn_width = 80.0f32;
            let apply_btn_x = WIDGET_X + (w - apply_btn_width) / 2.0;

            let (bg_color, text_color) = if self.has_pending_changes {
                ([0.010, 0.787, 0.214, 1.0], [1.0, 1.0, 1.0, 1.0])
            } else {
                ([0.027, 0.027, 0.040, 0.5], [0.300, 0.300, 0.340, 1.0])
            };

            self.widget_renderer.draw_rounded_rect(
                encoder, view, queue,
                apply_btn_x, apply_y,
                apply_btn_width, APPLY_BUTTON_HEIGHT,
                8.0,
                bg_color,
                window_width, window_height,
            );

            let label = "Apply";
            let char_width = 6.5f32;
            let text_width = label.len() as f32 * char_width;
            let text_x = apply_btn_x + (apply_btn_width - text_width) / 2.0;
            let text_y = apply_y + (APPLY_BUTTON_HEIGHT - 14.0) / 2.0;
            text_items.push(TextItem {
                text: label.to_string(),
                x: text_x,
                y: text_y,
                scale: 1.0,
                color: text_color,
                max_width: apply_btn_width,
            });
        }

        // Flush all batched widget rects (row bgs, controls)
        self.widget_renderer.flush(encoder, view, window_width, window_height);

        // Render widget text first (below dropdowns)
        self.batch_text_renderer.render_batch(encoder, view, &text_items);

        // Render any open dropdown ON TOP of all other widgets
        let mut dropdown_text_items: Vec<TextItem> = Vec::new();
        match self.active_tab {
            SettingsTab::Backend => {
                self.backend_select.y += content_y_offset;
                self.model_select.y += content_y_offset;
                self.backend_select.render_dropdown(encoder, view, &self.widget_renderer, &mut dropdown_text_items, queue, window_width, window_height);
                self.model_select.render_dropdown(encoder, view, &self.widget_renderer, &mut dropdown_text_items, queue, window_width, window_height);
                self.backend_select.y -= content_y_offset;
                self.model_select.y -= content_y_offset;
            }
            SettingsTab::Audio => {
                self.vad_sensitivity_select.y += content_y_offset;
                self.vad_sensitivity_select.render_dropdown(encoder, view, &self.widget_renderer, &mut dropdown_text_items, queue, window_width, window_height);
                self.vad_sensitivity_select.y -= content_y_offset;
            }
            SettingsTab::Display => {
                self.vsync_select.y += content_y_offset;
                self.vsync_select.render_dropdown(encoder, view, &self.widget_renderer, &mut dropdown_text_items, queue, window_width, window_height);
                self.vsync_select.y -= content_y_offset;
            }
            _ => {}
        }

        // Flush dropdown rects
        self.widget_renderer.flush(encoder, view, window_width, window_height);

        // Render dropdown text on top of everything
        self.batch_text_renderer.render_batch(encoder, view, &dropdown_text_items);
    }
}
