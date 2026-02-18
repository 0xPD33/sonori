use super::widget_renderer::WidgetRenderer;
use crate::ui::batch_text_renderer::TextItem;

const DROPDOWN_ITEM_HEIGHT: f32 = 22.0;
const CHEVRON_WIDTH: f32 = 16.0;
const SELECT_BOX_WIDTH: f32 = 140.0;

pub struct SelectOption {
    pub label: String,
    pub value: String,
}

pub struct Select {
    pub label: String,
    pub options: Vec<SelectOption>,
    pub selected_index: usize,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub expanded: bool,
    hovered_index: Option<usize>,
    changed: bool,
}

impl Select {
    pub fn new(
        label: &str,
        options: Vec<SelectOption>,
        selected_index: usize,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> Self {
        Self {
            label: label.to_string(),
            options,
            selected_index,
            x,
            y,
            width,
            height,
            expanded: false,
            hovered_index: None,
            changed: false,
        }
    }

    fn select_box_x(&self) -> f32 {
        self.x + self.width - SELECT_BOX_WIDTH
    }

    fn dropdown_height(&self) -> f32 {
        self.options.len() as f32 * DROPDOWN_ITEM_HEIGHT
    }

    pub fn handle_click(&mut self, click_x: f32, click_y: f32) -> bool {
        let box_x = self.select_box_x();

        if self.expanded {
            // Check if click is on a dropdown item
            let dropdown_y = self.y + self.height;
            if click_x >= box_x
                && click_x <= box_x + SELECT_BOX_WIDTH
                && click_y >= dropdown_y
                && click_y <= dropdown_y + self.dropdown_height()
            {
                let index = ((click_y - dropdown_y) / DROPDOWN_ITEM_HEIGHT) as usize;
                if index < self.options.len() && index != self.selected_index {
                    self.selected_index = index;
                    self.changed = true;
                }
                self.expanded = false;
                return true;
            }

            // Click anywhere else closes the dropdown
            self.expanded = false;
            // Check if click was on the select box itself (toggle)
            if click_x >= box_x
                && click_x <= box_x + SELECT_BOX_WIDTH
                && click_y >= self.y
                && click_y <= self.y + self.height
            {
                return true;
            }
            return true; // Consume click to close dropdown
        }

        // Check if click is on the select box to expand
        if click_x >= box_x
            && click_x <= box_x + SELECT_BOX_WIDTH
            && click_y >= self.y
            && click_y <= self.y + self.height
        {
            self.expanded = true;
            return true;
        }

        false
    }

    pub fn handle_mouse_move(&mut self, x: f32, y: f32) {
        if !self.expanded {
            self.hovered_index = None;
            return;
        }

        let box_x = self.select_box_x();
        let dropdown_y = self.y + self.height;

        if x >= box_x
            && x <= box_x + SELECT_BOX_WIDTH
            && y >= dropdown_y
            && y <= dropdown_y + self.dropdown_height()
        {
            let index = ((y - dropdown_y) / DROPDOWN_ITEM_HEIGHT) as usize;
            if index < self.options.len() {
                self.hovered_index = Some(index);
            } else {
                self.hovered_index = None;
            }
        } else {
            self.hovered_index = None;
        }
    }

    pub fn take_changed(&mut self) -> Option<usize> {
        if self.changed {
            self.changed = false;
            Some(self.selected_index)
        } else {
            None
        }
    }

    pub fn selected_value(&self) -> &str {
        self.options
            .get(self.selected_index)
            .map(|o| o.value.as_str())
            .unwrap_or("")
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
        let box_x = self.select_box_x();

        // Collect label text item
        let label_width = self.width - SELECT_BOX_WIDTH - 8.0;
        text_items.push(TextItem {
            text: self.label.clone(),
            x: self.x + 4.0,
            y: self.y + 4.0,
            scale: 1.0,
            color: [0.604, 0.604, 0.670, 1.0],
            max_width: label_width,
        });

        // Draw select box background
        widget_renderer.draw_rounded_rect(
            encoder,
            view,
            queue,
            box_x,
            self.y,
            SELECT_BOX_WIDTH,
            self.height,
            4.0,
            [0.033, 0.033, 0.047, 1.0],
            window_width,
            window_height,
        );

        // Collect selected option text item
        if let Some(option) = self.options.get(self.selected_index) {
            text_items.push(TextItem {
                text: option.label.clone(),
                x: box_x + 6.0,
                y: self.y + 4.0,
                scale: 1.0,
                color: [0.604, 0.604, 0.670, 1.0],
                max_width: SELECT_BOX_WIDTH - CHEVRON_WIDTH - 10.0,
            });
        }

        // Collect chevron text item
        let chevron = if self.expanded { "\u{25B2}" } else { "\u{25BC}" };
        text_items.push(TextItem {
            text: chevron.to_string(),
            x: box_x + SELECT_BOX_WIDTH - CHEVRON_WIDTH - 4.0,
            y: self.y + 4.0,
            scale: 0.85,
            color: [0.171, 0.171, 0.214, 0.7],
            max_width: CHEVRON_WIDTH,
        });

    }

    /// Render the dropdown overlay separately, so it draws on top of all other widgets.
    pub fn render_dropdown(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        widget_renderer: &WidgetRenderer,
        text_items: &mut Vec<TextItem>,
        queue: &wgpu::Queue,
        window_width: u32,
        window_height: u32,
    ) {
        if !self.expanded {
            return;
        }

        let box_x = self.select_box_x();
        let dropdown_y = self.y + self.height;

        // Draw dropdown background
        widget_renderer.draw_rounded_rect(
            encoder, view, queue,
            box_x, dropdown_y,
            SELECT_BOX_WIDTH, self.dropdown_height(),
            4.0,
            [0.005, 0.005, 0.010, 1.0],
            window_width, window_height,
        );

        for (i, option) in self.options.iter().enumerate() {
            let item_y = dropdown_y + i as f32 * DROPDOWN_ITEM_HEIGHT;

            let is_hovered = self.hovered_index == Some(i);
            let is_selected = i == self.selected_index;

            if is_hovered || is_selected {
                let highlight_color = if is_hovered {
                    [0.051, 0.051, 0.073, 0.9]
                } else {
                    [0.005, 0.262, 0.073, 0.5]
                };

                widget_renderer.draw_rounded_rect(
                    encoder, view, queue,
                    box_x + 2.0, item_y + 1.0,
                    SELECT_BOX_WIDTH - 4.0, DROPDOWN_ITEM_HEIGHT - 2.0,
                    3.0, highlight_color,
                    window_width, window_height,
                );
            }

            let text_color = if is_selected {
                [1.0, 1.0, 1.0, 1.0]
            } else {
                [0.604, 0.604, 0.604, 0.9]
            };

            text_items.push(TextItem {
                text: option.label.clone(),
                x: box_x + 6.0,
                y: item_y + 4.0,
                scale: 1.0,
                color: text_color,
                max_width: SELECT_BOX_WIDTH - 12.0,
            });
        }
    }
}
