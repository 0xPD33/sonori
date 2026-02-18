use super::widget_renderer::WidgetRenderer;
use crate::ui::batch_text_renderer::TextItem;

const TRACK_HEIGHT: f32 = 4.0;
const HANDLE_RADIUS: f32 = 8.0;
const TRACK_WIDTH: f32 = 100.0;
const VALUE_DISPLAY_WIDTH: f32 = 40.0;

pub struct Slider {
    pub label: String,
    pub value: f32,
    pub min: f32,
    pub max: f32,
    pub step: f32,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    dragging: bool,
    changed: bool,
}

impl Slider {
    pub fn new(
        label: &str,
        value: f32,
        min: f32,
        max: f32,
        step: f32,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> Self {
        Self {
            label: label.to_string(),
            value: value.clamp(min, max),
            min,
            max,
            step,
            x,
            y,
            width,
            height,
            dragging: false,
            changed: false,
        }
    }

    fn track_x(&self) -> f32 {
        self.x + self.width - TRACK_WIDTH - VALUE_DISPLAY_WIDTH - 8.0
    }

    fn track_y(&self) -> f32 {
        self.y + (self.height - TRACK_HEIGHT) / 2.0
    }

    fn value_to_x(&self, val: f32) -> f32 {
        let t = (val - self.min) / (self.max - self.min);
        self.track_x() + t * TRACK_WIDTH
    }

    fn x_to_value(&self, px: f32) -> f32 {
        let t = ((px - self.track_x()) / TRACK_WIDTH).clamp(0.0, 1.0);
        let raw = self.min + t * (self.max - self.min);
        // Snap to step
        if self.step > 0.0 {
            (raw / self.step).round() * self.step
        } else {
            raw
        }
        .clamp(self.min, self.max)
    }

    pub fn handle_click(&mut self, click_x: f32, click_y: f32) -> bool {
        let handle_x = self.value_to_x(self.value);
        let handle_y = self.y + self.height / 2.0;

        // Check if click is on handle (generous hit area)
        let dx = click_x - handle_x;
        let dy = click_y - handle_y;
        if dx * dx + dy * dy <= (HANDLE_RADIUS + 4.0) * (HANDLE_RADIUS + 4.0) {
            self.dragging = true;
            return true;
        }

        // Check if click is on track
        let track_x = self.track_x();
        let track_y = self.track_y();
        if click_x >= track_x
            && click_x <= track_x + TRACK_WIDTH
            && click_y >= track_y - HANDLE_RADIUS
            && click_y <= track_y + TRACK_HEIGHT + HANDLE_RADIUS
        {
            let new_val = self.x_to_value(click_x);
            if (new_val - self.value).abs() > f32::EPSILON {
                self.value = new_val;
                self.changed = true;
            }
            self.dragging = true;
            return true;
        }

        false
    }

    pub fn handle_drag(&mut self, x: f32, _y: f32) {
        if !self.dragging {
            return;
        }
        let new_val = self.x_to_value(x);
        if (new_val - self.value).abs() > f32::EPSILON {
            self.value = new_val;
            self.changed = true;
        }
    }

    pub fn handle_release(&mut self) {
        self.dragging = false;
    }

    pub fn take_changed(&mut self) -> Option<f32> {
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
        let label_width = self.width - TRACK_WIDTH - VALUE_DISPLAY_WIDTH - 16.0;
        text_items.push(TextItem {
            text: self.label.clone(),
            x: self.x + 4.0,
            y: self.y + 4.0,
            scale: 1.0,
            color: [0.604, 0.604, 0.670, 1.0],
            max_width: label_width,
        });

        let track_x = self.track_x();
        let track_y = self.track_y();

        // Draw track background
        widget_renderer.draw_rounded_rect(
            encoder,
            view,
            queue,
            track_x,
            track_y,
            TRACK_WIDTH,
            TRACK_HEIGHT,
            TRACK_HEIGHT / 2.0,
            [0.064, 0.064, 0.083, 1.0],
            window_width,
            window_height,
        );

        // Draw filled portion of track
        let fill_width = self.value_to_x(self.value) - track_x;
        if fill_width > 0.0 {
            widget_renderer.draw_rounded_rect(
                encoder,
                view,
                queue,
                track_x,
                track_y,
                fill_width,
                TRACK_HEIGHT,
                TRACK_HEIGHT / 2.0,
                [0.010, 0.787, 0.214, 1.0],
                window_width,
                window_height,
            );
        }

        // Draw handle
        let handle_x = self.value_to_x(self.value);
        let handle_y = self.y + self.height / 2.0;

        widget_renderer.draw_rounded_rect(
            encoder,
            view,
            queue,
            handle_x - HANDLE_RADIUS,
            handle_y - HANDLE_RADIUS,
            HANDLE_RADIUS * 2.0,
            HANDLE_RADIUS * 2.0,
            HANDLE_RADIUS,
            [1.0, 1.0, 1.0, 1.0],
            window_width,
            window_height,
        );

        // Collect value text item
        let value_text = if self.step >= 1.0 {
            format!("{}", self.value as i32)
        } else {
            format!("{:.1}", self.value)
        };

        let value_x = track_x + TRACK_WIDTH + 8.0;
        text_items.push(TextItem {
            text: value_text,
            x: value_x,
            y: self.y + 4.0,
            scale: 1.0,
            color: [0.262, 0.262, 0.318, 0.9],
            max_width: VALUE_DISPLAY_WIDTH,
        });
    }
}
