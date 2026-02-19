use super::widget_renderer::WidgetRenderer;
use crate::ui::batch_text_renderer::TextItem;

const TOGGLE_WIDTH: f32 = 36.0;
const TOGGLE_HEIGHT: f32 = 18.0;
const KNOB_RADIUS: f32 = 7.0;
const KNOB_PADDING: f32 = 2.0;
const ANIMATION_DURATION_SECS: f32 = 0.15;

pub struct Toggle {
    pub label: String,
    pub value: bool,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    changed: bool,
    animation_progress: f32, // 0.0 = off, 1.0 = on
    animation_active: bool,
    animation_start: std::time::Instant,
    animation_from: f32,
}

impl Toggle {
    pub fn new(label: &str, value: bool, x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            label: label.to_string(),
            value,
            x,
            y,
            width,
            height,
            changed: false,
            animation_progress: if value { 1.0 } else { 0.0 },
            animation_active: false,
            animation_start: std::time::Instant::now(),
            animation_from: if value { 1.0 } else { 0.0 },
        }
    }

    pub fn handle_click(&mut self, click_x: f32, click_y: f32) -> bool {
        let toggle_x = self.x + self.width - TOGGLE_WIDTH;
        let toggle_y = self.y + (self.height - TOGGLE_HEIGHT) / 2.0;

        if click_x >= toggle_x
            && click_x <= toggle_x + TOGGLE_WIDTH
            && click_y >= toggle_y
            && click_y <= toggle_y + TOGGLE_HEIGHT
        {
            self.value = !self.value;
            self.changed = true;
            self.animation_from = self.animation_progress;
            self.animation_active = true;
            self.animation_start = std::time::Instant::now();
            return true;
        }
        false
    }

    pub fn update_animation(&mut self) {
        if !self.animation_active {
            return;
        }

        let elapsed = self.animation_start.elapsed().as_secs_f32();
        let t = (elapsed / ANIMATION_DURATION_SECS).min(1.0);
        // Ease-out cubic
        let eased = 1.0 - (1.0 - t).powi(3);

        let target = if self.value { 1.0 } else { 0.0 };
        self.animation_progress = self.animation_from + (target - self.animation_from) * eased;

        if t >= 1.0 {
            self.animation_active = false;
            self.animation_progress = target;
        }
    }

    pub fn set_value(&mut self, value: bool) {
        self.value = value;
        self.animation_progress = if value { 1.0 } else { 0.0 };
        self.animation_from = self.animation_progress;
        self.animation_active = false;
    }

    pub fn take_changed(&mut self) -> Option<bool> {
        if self.changed {
            self.changed = false;
            Some(self.value)
        } else {
            None
        }
    }

    pub fn render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        widget_renderer: &WidgetRenderer,
        text_items: &mut Vec<TextItem>,
        queue: &wgpu::Queue,
        window_width: u32,
        window_height: u32,
    ) {
        // Collect label text item
        text_items.push(TextItem {
            text: self.label.clone(),
            x: self.x + 4.0,
            y: self.y + 4.0,
            scale: 1.0,
            color: [0.604, 0.604, 0.670, 1.0],
            max_width: self.width - TOGGLE_WIDTH - 8.0,
        });

        let toggle_x = self.x + self.width - TOGGLE_WIDTH;
        let toggle_y = self.y + (self.height - TOGGLE_HEIGHT) / 2.0;

        // Interpolate background color between off (gray) and on (accent green)
        let off_color: [f32; 4] = [0.064, 0.064, 0.083, 1.0];
        let on_color: [f32; 4] = [0.010, 0.787, 0.214, 1.0];
        let t = self.animation_progress;
        let bg_color = [
            off_color[0] + (on_color[0] - off_color[0]) * t,
            off_color[1] + (on_color[1] - off_color[1]) * t,
            off_color[2] + (on_color[2] - off_color[2]) * t,
            off_color[3] + (on_color[3] - off_color[3]) * t,
        ];

        // Draw toggle track (pill shape)
        widget_renderer.draw_rounded_rect(
            encoder,
            view,
            queue,
            toggle_x,
            toggle_y,
            TOGGLE_WIDTH,
            TOGGLE_HEIGHT,
            TOGGLE_HEIGHT / 2.0,
            bg_color,
            window_width,
            window_height,
        );

        // Draw knob (circle)
        let knob_left = toggle_x + KNOB_PADDING + KNOB_RADIUS;
        let knob_right = toggle_x + TOGGLE_WIDTH - KNOB_PADDING - KNOB_RADIUS;
        let knob_center_x = knob_left + (knob_right - knob_left) * self.animation_progress;
        let knob_center_y = toggle_y + TOGGLE_HEIGHT / 2.0;

        widget_renderer.draw_rounded_rect(
            encoder,
            view,
            queue,
            knob_center_x - KNOB_RADIUS,
            knob_center_y - KNOB_RADIUS,
            KNOB_RADIUS * 2.0,
            KNOB_RADIUS * 2.0,
            KNOB_RADIUS,
            [1.0, 1.0, 1.0, 1.0],
            window_width,
            window_height,
        );
    }
}
