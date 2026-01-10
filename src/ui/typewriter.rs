use std::time::Instant;

/// Typewriter effect that reveals text character by character
/// Speed scales with text length - longer text reveals faster
pub struct TypewriterEffect {
    /// The full text to reveal
    target_text: String,
    /// When the effect started
    start_time: Option<Instant>,
    /// Number of characters currently visible
    visible_chars: usize,
    /// Whether the effect is active
    active: bool,
    /// Target duration for the effect (seconds)
    target_duration: f32,
}

impl TypewriterEffect {
    pub fn new() -> Self {
        Self {
            target_text: String::new(),
            start_time: None,
            visible_chars: 0,
            active: false,
            target_duration: 1.5, // Effect always takes ~1.5 seconds regardless of length
        }
    }

    /// Start the typewriter effect with new text
    pub fn start(&mut self, text: String) {
        if text.is_empty() {
            self.active = false;
            return;
        }

        // Scale duration based on text length
        // Short text (< 50 chars): 0.3 - 0.8 seconds
        // Medium text (50-200 chars): 0.8 - 1.2 seconds
        // Long text (> 200 chars): 1.2 - 1.5 seconds
        let char_count = text.chars().count();
        self.target_duration = if char_count < 50 {
            0.3 + (char_count as f32 / 50.0) * 0.5
        } else if char_count < 200 {
            0.8 + ((char_count - 50) as f32 / 150.0) * 0.4
        } else {
            1.2 + ((char_count - 200) as f32 / 300.0).min(1.0) * 0.3
        };

        self.target_text = text;
        self.start_time = Some(Instant::now());
        self.visible_chars = 0;
        self.active = true;
    }

    /// Stop the effect and show all text immediately
    pub fn complete(&mut self) {
        self.visible_chars = self.target_text.chars().count();
        self.active = false;
    }

    /// Check if the effect is currently running
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Update the effect and return the visible text
    pub fn update(&mut self) -> &str {
        if !self.active {
            return &self.target_text;
        }

        let Some(start) = self.start_time else {
            return &self.target_text;
        };

        let elapsed = start.elapsed().as_secs_f32();
        let total_chars = self.target_text.chars().count();

        if total_chars == 0 {
            self.active = false;
            return &self.target_text;
        }

        // Calculate progress (0.0 to 1.0) with ease-out curve
        let linear_progress = (elapsed / self.target_duration).min(1.0);
        // Ease-out: starts fast, slows down at end
        let eased_progress = 1.0 - (1.0 - linear_progress).powi(2);

        // Calculate visible characters based on progress
        self.visible_chars = (eased_progress * total_chars as f32).ceil() as usize;
        self.visible_chars = self.visible_chars.min(total_chars);

        // Check if complete
        if self.visible_chars >= total_chars || elapsed >= self.target_duration {
            self.active = false;
            return &self.target_text;
        }

        // Return slice of visible characters
        self.get_visible_slice()
    }

    /// Get slice of text up to visible_chars
    fn get_visible_slice(&self) -> &str {
        let char_indices: Vec<_> = self.target_text.char_indices().collect();
        if self.visible_chars >= char_indices.len() {
            &self.target_text
        } else {
            let end_byte = char_indices.get(self.visible_chars)
                .map(|(i, _)| *i)
                .unwrap_or(self.target_text.len());
            &self.target_text[..end_byte]
        }
    }

    /// Get the current visible text without updating
    pub fn get_visible_text(&self) -> &str {
        if !self.active || self.visible_chars >= self.target_text.chars().count() {
            return &self.target_text;
        }
        self.get_visible_slice()
    }

    /// Reset the effect
    pub fn reset(&mut self) {
        self.target_text.clear();
        self.start_time = None;
        self.visible_chars = 0;
        self.active = false;
    }
}

impl Default for TypewriterEffect {
    fn default() -> Self {
        Self::new()
    }
}
