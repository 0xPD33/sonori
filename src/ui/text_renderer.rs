use glyphon::{
    Attrs, Buffer, Cache, Color, Family, FontSystem, Metrics, Resolution, Shaping, SwashCache,
    TextArea, TextAtlas, TextBounds, TextRenderer as GlyphonTextRenderer, Viewport,
};
use std::sync::Arc;
use wgpu::{Device, Queue, TextureView};
use winit::dpi::PhysicalSize;

// Import window constants for consistent margins
use super::window::{LEFT_MARGIN, RIGHT_MARGIN};

/// A text renderer that uses glyphon to render text
pub struct TextRenderer {
    font_system: FontSystem,
    cache: SwashCache,
    atlas: TextAtlas,
    renderer: GlyphonTextRenderer,
    buffer: Buffer,
    device: Arc<Device>,
    queue: Arc<Queue>,
    size: PhysicalSize<u32>,
    surface_format: wgpu::TextureFormat,
    cache_ref: Cache,
    viewport: Viewport,
}

impl TextRenderer {
    /// Create a new text renderer
    pub fn new(
        device: Arc<Device>,
        queue: Arc<Queue>,
        size: PhysicalSize<u32>,
        surface_format: wgpu::TextureFormat,
    ) -> Self {
        // Create font system and cache
        let mut font_system = FontSystem::new();
        let cache = SwashCache::new();

        // Add system fonts - this is critical for text to appear
        font_system.db_mut().load_system_fonts();

        // Create a cache for the TextAtlas
        let cache_ref = Cache::new(&device);

        // Create viewport
        let viewport = Viewport::new(&device, &cache_ref);

        // Create the text atlas with the correct parameters
        let mut atlas = TextAtlas::new(&device, &queue, &cache_ref, surface_format);

        // Create the text renderer with the correct parameters
        let renderer =
            GlyphonTextRenderer::new(&mut atlas, &device, wgpu::MultisampleState::default(), None);

        // Create text buffer with smaller default metrics for less intrusive text
        let mut buffer = Buffer::new(&mut font_system, Metrics::new(16.0, 20.0));

        // Set the buffer size to match the window size
        buffer.set_size(
            &mut font_system,
            Some(size.width as f32),
            Some(size.height as f32),
        );

        Self {
            font_system,
            cache,
            atlas,
            renderer,
            buffer,
            device,
            queue,
            size,
            surface_format,
            cache_ref,
            viewport,
        }
    }

    /// Resize the text renderer
    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        self.size = size;

        // Update the buffer size
        self.buffer.set_size(
            &mut self.font_system,
            Some(size.width as f32),
            Some(size.height as f32),
        );

        // Update the viewport resolution
        self.viewport.update(
            &self.queue,
            Resolution {
                width: size.width,
                height: size.height,
            },
        );
    }

    /// Render text at a specific position with proper wrapping and clipping
    pub fn render_text(
        &mut self,
        view: &TextureView,
        encoder: &mut wgpu::CommandEncoder,
        text: &str,
        x: f32,
        y: f32,
        scale: f32,
        color: [f32; 4],
        area_width: u32,
        area_height: u32,
    ) {
        if text.is_empty() {
            return;
        }

        // Clear the buffer for new text
        self.buffer.lines.clear();

        let font_size = 10.0 * scale; // Reduced from 12.0 to 10.0 for smaller text
        let metrics = Metrics::new(font_size, font_size * 1.1);
        self.buffer.set_metrics(&mut self.font_system, metrics);

        let text_color = Color::rgba(
            (color[0] * 255.0) as u8,
            (color[1] * 255.0) as u8,
            (color[2] * 255.0) as u8,
            (color[3] * 255.0) as u8,
        );

        // Set the buffer width to match the text area width for proper wrapping
        // But allow unlimited height for scrolling
        self.buffer.set_size(
            &mut self.font_system,
            Some(area_width as f32 - (LEFT_MARGIN + RIGHT_MARGIN)),
            None,
        );

        self.buffer.set_text(
            &mut self.font_system,
            text,
            Attrs::new().family(Family::SansSerif).color(text_color),
            Shaping::Advanced,
        );

        self.buffer.shape_until_scroll(&mut self.font_system, true);

        self.viewport.update(
            &self.queue,
            Resolution {
                width: self.size.width,
                height: self.size.height,
            },
        );

        let text_area = TextArea {
            buffer: &self.buffer,
            left: x,
            top: y,
            scale: 1.0,
            bounds: TextBounds {
                left: 0,
                top: -10000,
                right: area_width as i32,
                bottom: 10000,
            },
            default_color: text_color,
            custom_glyphs: &[],
        };

        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Text Render Pass"),
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

        render_pass.set_scissor_rect(0, 0, area_width, area_height);

        if let Ok(_) = self.renderer.prepare(
            &self.device,
            &self.queue,
            &mut self.font_system,
            &mut self.atlas,
            &self.viewport,
            [text_area],
            &mut self.cache,
        ) {
            let _ = self
                .renderer
                .render(&self.atlas, &self.viewport, &mut render_pass);
        }

        // Trim the atlas to free up memory
        self.atlas.trim();
    }
}
