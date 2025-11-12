/// Processing states for transcription
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessingState {
    /// No active processing
    Idle,
    /// Loading model or initializing
    Loading,
    /// Currently transcribing audio
    Transcribing,
    /// Recording paused
    Paused,
    /// Processing completed successfully
    Completed,
    /// Error occurred during processing
    Error,
}

/// Common data structure for audio visualization
/// Used across different UI components
#[derive(Debug, Clone)]
pub struct AudioVisualizationData {
    /// Audio samples to visualize
    pub samples: Vec<f32>,
    /// Flag indicating if speech is currently detected
    pub is_speaking: bool,
    /// Current transcript text
    pub transcript: String,
    /// Flag to request resetting the transcript history
    pub reset_requested: bool,
    /// Current processing state
    pub processing_state: ProcessingState,
    /// Processing state change timestamp for animations
    pub processing_state_changed: std::time::Instant,
}

impl AudioVisualizationData {
    /// Create a new AudioVisualizationData with pre-allocated capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            samples: Vec::with_capacity(capacity),
            is_speaking: false,
            transcript: String::new(),
            reset_requested: false,
            processing_state: ProcessingState::Idle,
            processing_state_changed: std::time::Instant::now(),
        }
    }

    /// Clear samples while preserving capacity for reuse
    pub fn clear_samples(&mut self) {
        self.samples.clear();
    }

    /// Update transcript efficiently by clearing and extending
    pub fn update_transcript(&mut self, new_transcript: &str) {
        self.transcript.clear();
        self.transcript.push_str(new_transcript);
    }

    /// Update samples efficiently by clearing and extending
    pub fn update_samples(&mut self, new_samples: &[f32]) {
        self.samples.clear();
        self.samples.extend_from_slice(new_samples);
    }

    /// Set processing state and update timestamp
    pub fn set_processing_state(&mut self, state: ProcessingState) {
        if self.processing_state != state {
            self.processing_state = state;
            self.processing_state_changed = std::time::Instant::now();
        }
    }

    /// Check if currently processing (loading, transcribing, or paused)
    pub fn is_processing(&self) -> bool {
        matches!(self.processing_state, ProcessingState::Loading | ProcessingState::Transcribing | ProcessingState::Paused)
    }

    /// Get duration since processing state changed
    pub fn processing_state_duration(&self) -> std::time::Duration {
        self.processing_state_changed.elapsed()
    }
}
