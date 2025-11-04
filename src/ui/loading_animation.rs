use std::sync::Arc;
use wgpu::{self, util::DeviceExt};
use winit::dpi::PhysicalSize;

use super::gpu_utils::GpuQuadRenderer;
use super::common::ProcessingState;

/// Loading animation component for transcription processing
pub struct LoadingAnimation {
    renderer: GpuQuadRenderer,
    state: AnimationState,
    start_time: std::time::Instant,
    animation_duration: std::time::Duration,
    last_processing_state: Option<ProcessingState>,
}

#[derive(Debug, Clone, Copy)]
enum AnimationState {
    /// Dots animation (for loading/transcribing)
    Dots,
    /// Spinner animation (for processing)
    Spinner,
    /// Success animation (for completion)
    Success,
    /// Error animation (for errors)
    Error,
}

impl LoadingAnimation {
    /// Create a new loading animation renderer
    pub fn new(device: &Arc<wgpu::Device>, format: wgpu::TextureFormat) -> Self {
        let renderer = GpuQuadRenderer::new_simple(device, format, "Loading Animation");

        Self {
            renderer,
            state: AnimationState::Dots,
            start_time: std::time::Instant::now(),
            animation_duration: std::time::Duration::from_millis(800), // Faster 0.8 second cycle
            last_processing_state: None,
        }
    }

    /// Set animation state based on processing state
    pub fn set_processing_state(&mut self, processing_state: ProcessingState) {
        // Only reset start time if the processing state actually changed
        let state_changed = self.last_processing_state != Some(processing_state);

        self.state = match processing_state {
            ProcessingState::Loading | ProcessingState::Transcribing => AnimationState::Dots,
            ProcessingState::Completed => AnimationState::Success,
            ProcessingState::Error => AnimationState::Error,
            ProcessingState::Idle => AnimationState::Dots, // Default to dots
        };

        if state_changed {
            self.start_time = std::time::Instant::now();
            self.last_processing_state = Some(processing_state);
        }
    }

    /// Render the loading animation centered in the given area
    pub fn render(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        center_x: f32,
        center_y: f32,
        size: f32,
        color: [f32; 4],
    ) {
        let elapsed = self.start_time.elapsed();
        let progress = (elapsed.as_secs_f32() / self.animation_duration.as_secs_f32()) % 1.0;

        match self.state {
            AnimationState::Dots => self.render_dots_animation(encoder, view, center_x, center_y, size, color, progress),
            AnimationState::Spinner => self.render_spinner_animation(encoder, view, center_x, center_y, size, color, progress),
            AnimationState::Success => self.render_success_animation(encoder, view, center_x, center_y, size, color),
            AnimationState::Error => self.render_error_animation(encoder, view, center_x, center_y, size, color),
        }
    }

    /// Render animated dots (loading/transcribing state)
    fn render_dots_animation(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        center_x: f32,
        center_y: f32,
        size: f32,
        color: [f32; 4],
        progress: f32,
    ) {
        // Render 3 smaller dots that animate in sequence
        let dot_size = size * 0.08; // Much smaller dots
        let spacing = size * 0.12; // Tighter spacing
        let total_width = 3.0 * dot_size + 2.0 * spacing;
        let start_x = center_x - total_width / 2.0;

        for i in 0..3 {
            // Calculate animation phase for each dot - faster progression
            let dot_progress = (progress + i as f32 * 0.15) % 1.0;
            let scale = 0.4 + 0.6 * (std::f32::consts::PI * 2.0 * dot_progress).sin().abs();
            let opacity = 0.15 + 0.25 * scale; // More translucent overall

            let dot_x = start_x + i as f32 * (dot_size + spacing);
            let dot_y = center_y;
            let actual_size = dot_size * scale;

            // Create a colored circle for the dot with translucency
            let dot_color = [color[0], color[1], color[2], color[3] * opacity];

            self.renderer.render_quad(
                encoder,
                view,
                dot_x - actual_size / 2.0,
                dot_y - actual_size / 2.0,
                actual_size,
                actual_size,
                &[],
                "Loading Dot",
            );
        }
    }

    /// Render spinning animation (processing state)
    fn render_spinner_animation(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        center_x: f32,
        center_y: f32,
        size: f32,
        color: [f32; 4],
        progress: f32,
    ) {
        // Render a spinning circle effect
        let segments = 8;
        let segment_size = size * 0.1;

        for i in 0..segments {
            let angle = (i as f32 / segments as f32) * 2.0 * std::f32::consts::PI + progress * 2.0 * std::f32::consts::PI;
            let radius = size * 0.3;
            let x = center_x + angle.cos() * radius;
            let y = center_y + angle.sin() * radius;

            // Calculate opacity based on position in the cycle
            let opacity = (angle.sin() + 1.0) / 2.0;
            let segment_color = [color[0], color[1], color[2], color[3] * opacity];

            self.renderer.render_quad(
                encoder,
                view,
                x - segment_size / 2.0,
                y - segment_size / 2.0,
                segment_size,
                segment_size,
                &[],
                "Spinner Segment",
            );
        }
    }

    /// Render success animation
    fn render_success_animation(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        center_x: f32,
        center_y: f32,
        size: f32,
        color: [f32; 4],
    ) {
        // Render a checkmark or success indicator
        let check_size = size * 0.6;

        // Green color for success
        let success_color = [0.2, 0.8, 0.2, color[3]];

        // Simple checkmark made of two lines (rendered as rotated quads)
        // Vertical line
        self.renderer.render_quad(
            encoder,
            view,
            center_x - check_size * 0.1,
            center_y,
            check_size * 0.2,
            check_size * 0.6,
            &[],
            "Success Check Vertical",
        );

        // Horizontal line
        self.renderer.render_quad(
            encoder,
            view,
            center_x,
            center_y + check_size * 0.2,
            check_size * 0.6,
            check_size * 0.2,
            &[],
            "Success Check Horizontal",
        );
    }

    /// Render error animation
    fn render_error_animation(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        center_x: f32,
        center_y: f32,
        size: f32,
        color: [f32; 4],
    ) {
        // Render an X for error
        let x_size = size * 0.6;

        // Red color for error
        let error_color = [0.9, 0.2, 0.2, color[3]];

        // Render X as two diagonal lines
        let line_width = x_size * 0.15;

        // First diagonal (top-left to bottom-right)
        self.renderer.render_quad(
            encoder,
            view,
            center_x - line_width / 2.0,
            center_y - x_size / 2.0,
            line_width,
            x_size,
            &[],
            "Error X First Line",
        );

        // Second diagonal (top-right to bottom-left)
        self.renderer.render_quad(
            encoder,
            view,
            center_x - line_width / 2.0,
            center_y - x_size / 2.0,
            line_width,
            x_size,
            &[],
            "Error X Second Line",
        );
    }

    /// Get appropriate text for the current processing state
    pub fn get_processing_text(&self, processing_state: ProcessingState) -> &'static str {
        match processing_state {
            ProcessingState::Loading => "Loading model...",
            ProcessingState::Transcribing => "Transcribing...",
            ProcessingState::Completed => "Transcription complete",
            ProcessingState::Error => "Transcription failed",
            ProcessingState::Idle => "Ready",
        }
    }

    /// Get appropriate color for the current processing state
    pub fn get_processing_color(&self, processing_state: ProcessingState) -> [f32; 4] {
        match processing_state {
            ProcessingState::Loading => [0.7, 0.7, 0.7, 0.6],      // More translucent gray
            ProcessingState::Transcribing => [0.1, 0.9, 0.5, 0.6], // More translucent teal
            ProcessingState::Completed => [0.2, 0.8, 0.2, 0.7],   // More translucent green
            ProcessingState::Error => [0.9, 0.2, 0.2, 0.7],     // More translucent red
            ProcessingState::Idle => [1.0, 0.85, 0.15, 0.6],    // More translucent gold
        }
    }
}