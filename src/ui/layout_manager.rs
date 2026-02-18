use super::scrollbar::SCROLLBAR_WIDTH;

pub struct LayoutManager {
    pub window_width: u32,
    pub window_height: u32,
    pub spectrogram_width: u32,
    pub spectrogram_height: u32,
    pub text_area_height: u32,
    pub status_bar_height: u32,
    pub right_margin: f32,
    pub left_margin: f32,
    pub gap: u32,
}

impl LayoutManager {
    pub fn new(
        window_width: u32,
        window_height: u32,
        spectrogram_width: u32,
        spectrogram_height: u32,
        text_area_height: u32,
        status_bar_height: u32,
        right_margin: f32,
        left_margin: f32,
        gap: u32,
    ) -> Self {
        Self {
            window_width,
            window_height,
            spectrogram_width,
            spectrogram_height,
            text_area_height,
            status_bar_height,
            right_margin,
            left_margin,
            gap,
        }
    }

    /// Update the window dimensions
    pub fn update_dimensions(&mut self, width: u32, height: u32) {
        self.window_width = width;
        self.window_height = height;
    }

    /// Calculate the text area width, considering scrollbar if needed
    pub fn calculate_text_area_width(&self, need_scrollbar: bool) -> u32 {
        if need_scrollbar {
            self.window_width.saturating_sub(SCROLLBAR_WIDTH + 1) // Reduced margin for slimmer scrollbar
        } else {
            // Use the full window width when no scrollbar is needed (minus margins on both sides)
            self.window_width
        }
    }

    /// Get the effective text area height (without the gap)
    pub fn get_text_area_height(&self) -> u32 {
        self.text_area_height - self.gap
    }

    /// Get text positioning
    pub fn get_text_position(&self, scroll_offset: f32) -> (f32, f32) {
        // Fixed position for text (left margin)
        let text_x = self.left_margin;

        // Apply scroll offset to text position
        let text_y = 4.0 - scroll_offset;

        (text_x, text_y)
    }

    /// Calculate the status bar position (between text area and spectrogram)
    pub fn get_status_bar_position(&self) -> (u32, u32, u32, u32) {
        (
            0,                    // x position
            self.text_area_height, // y position (right after text area)
            self.window_width,    // width
            self.status_bar_height, // height
        )
    }

    /// Calculate the spectrogram position
    pub fn get_spectrogram_position(&self) -> (f32, f32, f32, f32) {
        let status_bar_bottom_margin = 3u32;
        (
            0.0, // x position
            (self.text_area_height + self.status_bar_height + self.gap + status_bar_bottom_margin) as f32, // y position
            self.spectrogram_width as f32,  // width
            self.spectrogram_height as f32, // height
        )
    }

    /// Calculate the button panel position to encompass all buttons
    /// Returns (x, y, width, height) for the button panel background
    /// The button panel renders full-screen in normalized coordinates, so this returns the virtual bounds
    pub fn get_button_panel_position(&self) -> (f32, f32, f32, f32) {
        // Button panel renders full-screen, but logically encompasses the button area
        // with padding. This could be used for optimization or culling in the future.
        let button_padding = 10.0; // Padding around button area in pixels

        // Buttons are positioned within the text area
        // Return a rect that encompasses the text area with some padding
        (
            -button_padding, // x position (left edge with padding)
            -button_padding, // y position (top edge with padding)
            (self.window_width as f32) + (button_padding * 2.0), // width (full width with padding)
            (self.text_area_height as f32) + (button_padding * 2.0), // height (text area with padding)
        )
    }
}
