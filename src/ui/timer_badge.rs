use std::sync::Arc;
use std::time::{Duration, Instant};
use wgpu::{self, Device, Queue, TextureView};

/// Timer badge overlay component for showing recording duration
pub struct TimerBadge {
    recording_start: Option<Instant>,
    fade_progress: f32,
    pulse_phase: f32,
    last_update: Instant,

    // Recording indicator config
    indicator_color: [f32; 4],
    show_indicator: bool,

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
    pub fn new(
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        format: wgpu::TextureFormat,
        ui_config: &crate::config::UiConfig,
    ) -> Self {
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
            pulse_phase: 0.0,
            last_update: Instant::now(),
            indicator_color: ui_config.recording_indicator_color,
            show_indicator: ui_config.show_recording_indicator,
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
        // Update animations
        let now = Instant::now();
        let delta = now.duration_since(self.last_update).as_secs_f32();
        self.last_update = now;

        // Fade animation
        let target_fade = if self.is_recording() { 1.0 } else { 0.0 };
        let fade_speed = 5.0;
        self.fade_progress += (target_fade - self.fade_progress) * fade_speed * delta;
        self.fade_progress = self.fade_progress.clamp(0.0, 1.0);

        // Pulse animation for recording dot (cycle every ~0.8s)
        if self.is_recording() {
            self.pulse_phase += delta * 1.25 * std::f32::consts::TAU;
            if self.pulse_phase > std::f32::consts::TAU {
                self.pulse_phase -= std::f32::consts::TAU;
            }
        }

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

            use glyphon::{Attrs, Buffer, Color, Family, Metrics, Shaping, TextArea, TextBounds};

            let metrics = Metrics::new(font_size, line_height);
            let mut timer_buffer = Buffer::new(&mut self.font_system, metrics);
            timer_buffer.set_size(&mut self.font_system, Some(1000.0), Some(50.0));
            timer_buffer.set_text(
                &mut self.font_system,
                &time_str,
                &Attrs::new().family(Family::Monospace),
                Shaping::Advanced,
            );

            // Calculate actual text width from layout
            let layout = timer_buffer.layout_runs().collect::<Vec<_>>();
            let text_width = layout
                .iter()
                .map(|run| run.line_w)
                .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                .unwrap_or(0.0);

            // Calculate recording indicator size and position
            let indicator_size = font_size * 0.6;
            let indicator_gap = 6.0;
            let total_width = if self.show_indicator {
                indicator_size + indicator_gap + text_width
            } else {
                text_width
            };

            // Position in bottom-right corner of text area
            let content_x = text_area_x + text_area_width - total_width - padding;
            let text_y = text_area_y + text_area_height - line_height - padding;

            let text_alpha = (0.9 * self.fade_progress * 255.0) as u8;
            let timer_x = if self.show_indicator {
                content_x + indicator_size + indicator_gap
            } else {
                content_x
            };

            // Build text areas
            let mut text_areas: Vec<TextArea> = Vec::new();

            // Add indicator dot if enabled
            let indicator_buffer;
            if self.show_indicator {
                // Pulse opacity between 0.5 and 1.0
                let pulse_value = 0.75 + 0.25 * self.pulse_phase.sin();
                let indicator_alpha = (pulse_value * self.fade_progress * 255.0) as u8;
                let indicator_color = Color::rgba(
                    (self.indicator_color[0] * 255.0) as u8,
                    (self.indicator_color[1] * 255.0) as u8,
                    (self.indicator_color[2] * 255.0) as u8,
                    indicator_alpha,
                );

                indicator_buffer = {
                    let mut buf = Buffer::new(&mut self.font_system, metrics);
                    buf.set_size(&mut self.font_system, Some(50.0), Some(50.0));
                    buf.set_text(
                        &mut self.font_system,
                        "●",
                        &Attrs::new().family(Family::SansSerif).color(indicator_color),
                        Shaping::Advanced,
                    );
                    buf
                };

                text_areas.push(TextArea {
                    buffer: &indicator_buffer,
                    left: content_x,
                    top: text_y,
                    scale: 1.0,
                    bounds: TextBounds {
                        left: content_x as i32,
                        top: text_y as i32,
                        right: (content_x + indicator_size + 20.0) as i32,
                        bottom: (text_y + line_height) as i32,
                    },
                    default_color: indicator_color,
                    custom_glyphs: &[],
                });
            }

            // Add timer text
            text_areas.push(TextArea {
                buffer: &timer_buffer,
                left: timer_x,
                top: text_y,
                scale: 1.0,
                bounds: TextBounds {
                    left: timer_x as i32,
                    top: text_y as i32,
                    right: (timer_x + text_width) as i32,
                    bottom: (text_y + line_height) as i32,
                },
                default_color: Color::rgba(255, 255, 255, text_alpha),
                custom_glyphs: &[],
            });

            // Prepare and render text
            self.text_renderer
                .prepare(
                    &self.device,
                    queue,
                    &mut self.font_system,
                    &mut self.text_atlas,
                    &self.viewport,
                    text_areas,
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
