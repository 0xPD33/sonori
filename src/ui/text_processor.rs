pub struct TextProcessor {
    pub line_height: f32,
}

impl TextProcessor {
    pub fn new(line_height: f32) -> Self {
        Self { line_height }
    }

    /// Update metrics to match the actual rendered font size
    pub fn update_metrics(&mut self, text_scale: f32) {
        let font_size = 10.0 * text_scale;
        self.line_height = font_size * 1.1;
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
}
