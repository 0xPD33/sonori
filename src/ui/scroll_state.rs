/// Scroll state management for the UI
///
/// Centralizes all scroll-related state and logic in one place,
/// reducing clutter in WindowState.

#[derive(Debug, Clone)]
pub struct ScrollState {
    /// Current scroll offset in pixels
    pub scroll_offset: f32,
    /// Maximum scroll offset (when text is taller than window)
    pub max_scroll_offset: f32,
    /// Target scroll offset for smooth scrolling animation
    pub target_scroll_offset: f32,
    /// Whether auto-scroll is enabled (follows new text)
    pub auto_scroll: bool,
    /// Length of the last rendered transcript (for change detection)
    pub last_transcript_len: usize,
}

impl Default for ScrollState {
    fn default() -> Self {
        Self {
            scroll_offset: 0.0,
            max_scroll_offset: 0.0,
            target_scroll_offset: 0.0,
            auto_scroll: true,
            last_transcript_len: 0,
        }
    }
}

impl ScrollState {
    /// Create a new scroll state with default values
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset scroll state
    pub fn reset(&mut self) {
        self.scroll_offset = 0.0;
        self.max_scroll_offset = 0.0;
        self.target_scroll_offset = 0.0;
        self.last_transcript_len = 0;
    }

    /// Clamp scroll offset to valid range
    pub fn clamp_scroll_offset(&mut self) {
        self.scroll_offset = self.scroll_offset.min(self.max_scroll_offset).max(0.0);
    }

    /// Update maximum scroll offset and clamp current offset
    pub fn set_max_scroll_offset(&mut self, max: f32) {
        self.max_scroll_offset = max.max(0.0);
        self.clamp_scroll_offset();
    }

    /// Set target scroll offset and clamp it
    pub fn set_target_scroll_offset(&mut self, target: f32) {
        self.target_scroll_offset = target.min(self.max_scroll_offset).max(0.0);
    }

    /// Update scroll offset with auto-scroll animation
    ///
    /// Smoothly interpolates scroll offset towards target when auto-scroll is enabled.
    /// Returns true if scroll position changed.
    pub fn update_with_auto_scroll(&mut self) -> bool {
        if !self.auto_scroll {
            self.target_scroll_offset = self.scroll_offset;
            return false;
        }

        let old_offset = self.scroll_offset;

        // Set target to bottom
        self.target_scroll_offset = self.max_scroll_offset;

        // Smoothly interpolate towards target (20% per frame)
        const LERP_FACTOR: f32 = 0.2;
        self.scroll_offset += (self.target_scroll_offset - self.scroll_offset) * LERP_FACTOR;

        // Snap to target when very close to avoid infinite approach
        const SNAP_THRESHOLD: f32 = 0.5;
        if (self.target_scroll_offset - self.scroll_offset).abs() < SNAP_THRESHOLD {
            self.scroll_offset = self.target_scroll_offset;
        }

        self.clamp_scroll_offset();

        (self.scroll_offset - old_offset).abs() > 0.001
    }

    /// Update scroll offset when not auto-scrolling
    /// Target follows current position
    pub fn update_without_auto_scroll(&mut self) {
        self.target_scroll_offset = self.scroll_offset;
    }

    /// Check if transcript has changed since last update
    pub fn transcript_changed(&self, current_transcript_len: usize, is_recording: bool) -> bool {
        is_recording && current_transcript_len != self.last_transcript_len
    }

    /// Update transcript length tracker
    pub fn update_transcript_len(&mut self, len: usize) {
        self.last_transcript_len = len;
    }

    /// Check if scrollbar should be visible
    pub fn needs_scrollbar(&self) -> bool {
        self.max_scroll_offset > 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_state() {
        let state = ScrollState::default();
        assert_eq!(state.scroll_offset, 0.0);
        assert_eq!(state.max_scroll_offset, 0.0);
        assert!(state.auto_scroll);
    }

    #[test]
    fn test_clamp_scroll_offset() {
        let mut state = ScrollState::new();
        state.max_scroll_offset = 100.0;
        state.scroll_offset = 150.0;
        state.clamp_scroll_offset();
        assert_eq!(state.scroll_offset, 100.0);
    }

    #[test]
    fn test_transcript_changed() {
        let state = ScrollState::new();
        assert!(state.transcript_changed(10, true));
        assert!(!state.transcript_changed(0, true));
        assert!(!state.transcript_changed(10, false)); // Not recording
    }

    #[test]
    fn test_needs_scrollbar() {
        let mut state = ScrollState::new();
        assert!(!state.needs_scrollbar());
        state.max_scroll_offset = 10.0;
        assert!(state.needs_scrollbar());
    }
}
