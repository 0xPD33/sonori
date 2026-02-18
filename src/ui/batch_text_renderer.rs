use glyphon::{
    Attrs, Buffer, Cache, Color, Family, FontSystem, Metrics, Resolution, Shaping, SwashCache,
    TextArea, TextAtlas, TextBounds, TextRenderer as GlyphonTextRenderer, Viewport,
};
use std::sync::Arc;
use wgpu::{Device, Queue, TextureView};
use winit::dpi::PhysicalSize;

pub struct TextItem {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub scale: f32,
    pub color: [f32; 4],
    pub max_width: f32,
}

pub struct BatchTextRenderer {
    font_system: FontSystem,
    swash_cache: SwashCache,
    atlas: TextAtlas,
    renderer: GlyphonTextRenderer,
    viewport: Viewport,
    device: Arc<Device>,
    queue: Arc<Queue>,
    size: PhysicalSize<u32>,
    _cache_ref: Cache,
}

impl BatchTextRenderer {
    pub fn new(
        device: Arc<Device>,
        queue: Arc<Queue>,
        size: PhysicalSize<u32>,
        surface_format: wgpu::TextureFormat,
    ) -> Self {
        let mut font_system = FontSystem::new();
        let swash_cache = SwashCache::new();

        font_system.db_mut().load_system_fonts();

        let cache_ref = Cache::new(&device);
        let viewport = Viewport::new(&device, &cache_ref);
        let mut atlas = TextAtlas::new(&device, &queue, &cache_ref, surface_format);
        let renderer =
            GlyphonTextRenderer::new(&mut atlas, &device, wgpu::MultisampleState::default(), None);

        Self {
            font_system,
            swash_cache,
            atlas,
            renderer,
            viewport,
            device,
            queue,
            size,
            _cache_ref: cache_ref,
        }
    }

    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        self.size = size;
        self.viewport.update(
            &self.queue,
            Resolution {
                width: size.width,
                height: size.height,
            },
        );
    }

    pub fn render_batch(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        view: &TextureView,
        items: &[TextItem],
    ) {
        if items.is_empty() {
            return;
        }

        self.viewport.update(
            &self.queue,
            Resolution {
                width: self.size.width,
                height: self.size.height,
            },
        );

        let mut buffers: Vec<Buffer> = Vec::with_capacity(items.len());

        for item in items {
            let font_size = 10.0 * item.scale;
            let mut buffer =
                Buffer::new(&mut self.font_system, Metrics::new(font_size, font_size * 1.1));

            buffer.set_size(&mut self.font_system, Some(item.max_width), None);

            let color = Color::rgba(
                (item.color[0] * 255.0) as u8,
                (item.color[1] * 255.0) as u8,
                (item.color[2] * 255.0) as u8,
                (item.color[3] * 255.0) as u8,
            );

            buffer.set_text(
                &mut self.font_system,
                &item.text,
                &Attrs::new().family(Family::SansSerif).color(color),
                Shaping::Advanced,
            );

            buffer.shape_until_scroll(&mut self.font_system, true);
            buffers.push(buffer);
        }

        let text_areas: Vec<TextArea> = buffers
            .iter()
            .zip(items.iter())
            .map(|(buffer, item)| {
                let color = Color::rgba(
                    (item.color[0] * 255.0) as u8,
                    (item.color[1] * 255.0) as u8,
                    (item.color[2] * 255.0) as u8,
                    (item.color[3] * 255.0) as u8,
                );

                TextArea {
                    buffer,
                    left: item.x,
                    top: item.y,
                    scale: 1.0,
                    bounds: TextBounds {
                        left: 0,
                        top: 0,
                        right: self.size.width as i32,
                        bottom: self.size.height as i32,
                    },
                    default_color: color,
                    custom_glyphs: &[],
                }
            })
            .collect();

        if self
            .renderer
            .prepare(
                &self.device,
                &self.queue,
                &mut self.font_system,
                &mut self.atlas,
                &self.viewport,
                text_areas,
                &mut self.swash_cache,
            )
            .is_ok()
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Batch Text Render Pass"),
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

            render_pass.set_scissor_rect(0, 0, self.size.width, self.size.height);

            let _ = self
                .renderer
                .render(&self.atlas, &self.viewport, &mut render_pass);
        }

        self.atlas.trim();
    }
}
