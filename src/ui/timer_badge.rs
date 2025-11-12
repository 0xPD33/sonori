use std::sync::Arc;
use std::time::{Duration, Instant};
use wgpu::{self, Device, Queue, TextureView};

/// Timer badge overlay component for showing recording duration
pub struct TimerBadge {
    recording_start: Option<Instant>,
    fade_progress: f32,
    last_update: Instant,

    // Glyphon text rendering resources
    font_system: glyphon::FontSystem,
    swash_cache: glyphon::SwashCache,
    text_atlas: glyphon::TextAtlas,
    text_renderer: glyphon::TextRenderer,
    cache: glyphon::Cache,
    viewport: glyphon::Viewport,
    device: Arc<Device>,
}

impl TimerBadge {
    /// Create a new timer badge component
    pub fn new(device: &Arc<wgpu::Device>, queue: &Arc<wgpu::Queue>, format: wgpu::TextureFormat) -> Self {
        let mut font_system = glyphon::FontSystem::new();
        let swash_cache = glyphon::SwashCache::new();
        let cache = glyphon::Cache::new(device);
        let viewport = glyphon::Viewport::new(device, &cache);
        let mut text_atlas = glyphon::TextAtlas::new(device, queue, &cache, format);
        let text_renderer = glyphon::TextRenderer::new(
            &mut text_atlas,
            device,
            wgpu::MultisampleState::default(),
            None,
        );

        Self {
            recording_start: None,
            fade_progress: 0.0,
            last_update: Instant::now(),
            font_system,
            swash_cache,
            text_atlas,
            text_renderer,
            cache,
            viewport,
            device: Arc::clone(device),
        }
    }

    /// Start tracking recording time
    pub fn start_recording(&mut self) {
        self.recording_start = Some(Instant::now());
    }

    /// Stop tracking recording time
    pub fn stop_recording(&mut self) {
        self.recording_start = None;
    }

    /// Check if currently recording
    pub fn is_recording(&self) -> bool {
        self.recording_start.is_some()
    }

    /// Format duration as M:SS or H:MM:SS
    fn format_duration(duration: Duration) -> String {
        let total_secs = duration.as_secs();
        let hours = total_secs / 3600;
        let minutes = (total_secs % 3600) / 60;
        let seconds = total_secs % 60;

        if hours > 0 {
            format!("{}:{:02}:{:02}", hours, minutes, seconds)
        } else {
            format!("{}:{:02}", minutes, seconds)
        }
    }

    /// Render the timer badge as an overlay
    pub fn render(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        queue: &wgpu::Queue,
        text_area_x: f32,
        text_area_y: f32,
        text_area_width: f32,
        text_area_height: f32,
        viewport_width: u32,
        viewport_height: u32,
    ) {
        // Update fade animation
        let now = Instant::now();
        let delta = now.duration_since(self.last_update).as_secs_f32();
        self.last_update = now;

        let target_fade = if self.is_recording() { 1.0 } else { 0.0 };
        let fade_speed = 5.0; // Fade in/out over ~200ms
        self.fade_progress += (target_fade - self.fade_progress) * fade_speed * delta;
        self.fade_progress = self.fade_progress.clamp(0.0, 1.0);

        if self.fade_progress < 0.01 {
            return; // Don't render if fully faded out
        }

        if let Some(start_time) = self.recording_start {
            let elapsed = start_time.elapsed();
            let time_str = Self::format_duration(elapsed);

            // Calculate text dimensions and position
            let font_size = 16.0;
            let line_height = font_size * 1.2;
            let padding = 8.0;

            // Update viewport
            self.viewport.update(
                queue,
                glyphon::Resolution {
                    width: viewport_width,
                    height: viewport_height,
                },
            );

            // Prepare text buffer to calculate width
            use glyphon::{Attrs, Buffer, Color, Family, Metrics, Shaping, TextArea, TextBounds};

            let metrics = Metrics::new(font_size, line_height);
            let mut buffer = Buffer::new(&mut self.font_system, metrics);
            buffer.set_size(&mut self.font_system, Some(1000.0), Some(50.0));
            buffer.set_text(&mut self.font_system, &time_str, &Attrs::new().family(Family::Monospace), Shaping::Advanced);

            // Calculate actual text width from layout
            let layout = buffer.layout_runs().collect::<Vec<_>>();
            let text_width = layout
                .iter()
                .map(|run| run.line_w)
                .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                .unwrap_or(0.0);

            // Position in bottom-right corner of text area
            let text_x = text_area_x + text_area_width - text_width - padding;
            let text_y = text_area_y + text_area_height - line_height - padding;

            let text_alpha = (0.9 * self.fade_progress * 255.0) as u8;
            let text_area = TextArea {
                buffer: &buffer,
                left: text_x,
                top: text_y,
                scale: 1.0,
                bounds: TextBounds {
                    left: text_x as i32,
                    top: text_y as i32,
                    right: (text_x + text_width) as i32,
                    bottom: (text_y + line_height) as i32,
                },
                default_color: Color::rgba(255, 255, 255, text_alpha),
                custom_glyphs: &[],
            };

            // Prepare and render text
            self.text_renderer
                .prepare(
                    &self.device,
                    queue,
                    &mut self.font_system,
                    &mut self.text_atlas,
                    &self.viewport,
                    [text_area],
                    &mut self.swash_cache,
                )
                .expect("Failed to prepare timer text");

            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Timer Badge Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            self.text_renderer
                .render(&self.text_atlas, &self.viewport, &mut render_pass)
                .expect("Failed to render timer text");

            drop(render_pass);

            self.text_atlas.trim();
        }
    }
}
