pub struct TextProcessor {
    pub char_width: f32,
    pub line_height: f32,
    pub buffer_lines: f32,
}

impl TextProcessor {
    pub fn new(char_width: f32, line_height: f32, buffer_lines: f32) -> Self {
        Self {
            char_width,
            line_height,
            buffer_lines,
        }
    }

    /// Update metrics to match the actual rendered font size
    pub fn update_metrics(&mut self, text_scale: f32) {
        let font_size = 10.0 * text_scale;
        self.line_height = font_size * 1.1;
        // Average character width for SansSerif proportional font
        self.char_width = font_size * 0.55;
    }

    /// Clean up whitespace in the text, removing consecutive spaces
    pub fn clean_whitespace(&self, text: &str) -> String {
        let text = text.trim();

        // Replace consecutive whitespace with a single space
        let mut result = String::with_capacity(text.len());
        let mut last_was_whitespace = false;

        for c in text.chars() {
            if c.is_whitespace() {
                if !last_was_whitespace {
                    result.push(' ');
                    last_was_whitespace = true;
                }
            } else {
                result.push(c);
                last_was_whitespace = false;
            }
        }

        result
    }

    /// Calculate the number of lines and whether scrolling is needed
    pub fn calculate_layout(
        &self,
        text: &str,
        viewport_width: f32,
        visible_height: f32,
    ) -> TextLayoutInfo {
        // Estimate characters per line based on average character width
        let chars_per_line = (viewport_width - 8.0) / self.char_width;

        // Calculate visible lines in the viewport
        let visible_lines = visible_height / self.line_height;

        // Count words and estimate line breaks
        let mut line_count = 1.0;
        let mut current_line_chars = 0.0;

        for word in text.split_whitespace() {
            let word_len = word.len() as f32;

            if current_line_chars + word_len + 1.0 > chars_per_line {
                line_count += 1.0;
                current_line_chars = word_len + 1.0;
            } else {
                current_line_chars += word_len + 1.0;
            }
        }

        // Determine if scrollbar is needed
        let need_scrollbar = line_count > visible_lines + self.buffer_lines;

        // Calculate the maximum scroll offset
        let max_scroll_offset = if need_scrollbar {
            ((line_count - visible_lines) * self.line_height).max(0.0)
        } else {
            0.0
        };

        TextLayoutInfo {
            line_count,
            need_scrollbar,
            max_scroll_offset,
            visible_lines,
        }
    }
}

pub struct TextLayoutInfo {
    pub line_count: f32,
    pub need_scrollbar: bool,
    pub max_scroll_offset: f32,
    pub visible_lines: f32,
}
