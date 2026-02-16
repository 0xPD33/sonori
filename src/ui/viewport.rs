/// Viewport utilities for managing rendering viewports
///
/// This module provides helper functions and types for calculating and managing
/// viewports in the WGPU rendering pipeline, reducing code duplication.

#[derive(Debug, Clone, Copy)]
pub struct Viewport {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub min_depth: f32,
    pub max_depth: f32,
}

impl Viewport {
    /// Create a new viewport
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
            min_depth: 0.0,
            max_depth: 1.0,
        }
    }

    /// Create a viewport covering the full screen
    pub fn full_screen(window_width: u32, window_height: u32) -> Self {
        Self::new(0.0, 0.0, window_width as f32, window_height as f32)
    }

    /// Create a viewport for the text area
    pub fn for_text_area(text_area_width: u32, text_area_height: u32, gap: u32) -> Self {
        Self::new(
            0.0,
            0.0,
            text_area_width as f32,
            (text_area_height - gap) as f32,
        )
    }

    /// Create a viewport for the spectrogram area
    pub fn for_spectrogram(
        text_area_height: u32,
        gap: u32,
        spectrogram_width: u32,
        spectrogram_height: u32,
    ) -> Self {
        Self::new(
            0.0,
            (text_area_height + gap) as f32,
            spectrogram_width as f32,
            spectrogram_height as f32,
        )
    }

    /// Create a viewport for a scrollbar track
    pub fn for_scrollbar_track(window_width: u32, text_area_height: u32, gap: u32) -> Self {
        Self::new(
            (window_width as f32) - 8.0, // Right edge, 8px wide
            0.0,
            8.0,
            (text_area_height - gap) as f32,
        )
    }

    /// Create a viewport for a scrollbar thumb with dynamic positioning
    pub fn for_scrollbar_thumb(
        window_width: u32,
        _text_area_height: u32,
        _gap: u32,
        thumb_y: f32,
        thumb_height: f32,
    ) -> Self {
        Self::new(
            (window_width as f32) - 8.0, // Right edge, 8px wide
            thumb_y,
            8.0,
            thumb_height,
        )
    }

    /// Apply animation scaling to viewport (center-based scaling)
    pub fn with_animation_scale(&self, scale: f32) -> Self {
        if (scale - 1.0).abs() < 0.001 {
            return *self;
        }

        let center_x = self.x + self.width / 2.0;
        let center_y = self.y + self.height / 2.0;

        let scaled_width = self.width * scale;
        let scaled_height = self.height * scale;

        Self::new(
            center_x - scaled_width / 2.0,
            center_y - scaled_height / 2.0,
            scaled_width,
            scaled_height,
        )
    }

    /// Apply offset to viewport position
    pub fn with_offset(&self, dx: f32, dy: f32) -> Self {
        Self::new(self.x + dx, self.y + dy, self.width, self.height)
    }

    /// Get viewport as tuple for wgpu set_viewport() call
    pub fn as_tuple(&self) -> (f32, f32, f32, f32, f32, f32) {
        (
            self.x,
            self.y,
            self.width,
            self.height,
            self.min_depth,
            self.max_depth,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(actual: f32, expected: f32) {
        assert!((actual - expected).abs() < 1e-4);
    }

    #[test]
    fn test_full_screen_viewport() {
        let vp = Viewport::full_screen(1920, 1080);
        assert_eq!(vp.x, 0.0);
        assert_eq!(vp.y, 0.0);
        assert_eq!(vp.width, 1920.0);
        assert_eq!(vp.height, 1080.0);
    }

    #[test]
    fn test_animation_scale() {
        let vp = Viewport::new(0.0, 0.0, 100.0, 100.0);
        let scaled = vp.with_animation_scale(1.2);
        assert_close(scaled.width, 120.0);
        assert_close(scaled.height, 120.0);
        // Should be centered
        assert_close(scaled.x, -10.0);
        assert_close(scaled.y, -10.0);
    }
}
