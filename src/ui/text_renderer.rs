use glyphon::{
    Attrs, Buffer, Cache, Color, ColorMode, Family, FontSystem, Metrics, Resolution, Shaping,
    SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer as GlyphonTextRenderer, Viewport,
    Weight,
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
    _surface_format: wgpu::TextureFormat,
    _cache_ref: Cache,
    viewport: Viewport,
    cached_layout: Option<CachedTextLayout>,
    /// Last `measure_text` input and result; the status bar measures the same
    /// text every frame, and reshaping it each time costs more than drawing it.
    cached_measure: Option<(String, f32, f32)>,
    /// Shaped buffers for `render_labels`, by slot.
    label_buffers: Vec<(LabelKey, Buffer)>,
    label_widths: Vec<(LabelKey, f32)>,
}

/// Line height of a label, as a multiple of its font size.
pub const LABEL_LINE: f32 = 1.25;

/// One line of text at a position, for `render_labels`. `px` is the font size
/// in physical pixels; `y` is the top of the line box.
pub struct Label {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub px: f32,
    pub color: [f32; 4],
    pub bold: bool,
    /// Physical px rectangle outside which the label is cut off.
    pub clip: Option<[f32; 4]>,
}

#[derive(Default, PartialEq)]
struct LabelKey {
    text: String,
    px_bits: u32,
    bold: bool,
}

struct CachedTextLayout {
    text: String,
    font_size: f32,
    buffer_width: f32,
    color: [u8; 4],
}

impl TextRenderer {
    /// Create a new text renderer
    pub fn new(
        device: Arc<Device>,
        queue: Arc<Queue>,
        size: PhysicalSize<u32>,
        surface_format: wgpu::TextureFormat,
    ) -> Self {
        Self::with_color_mode(device, queue, size, surface_format, ColorMode::Accurate)
    }

    /// `ColorMode::Web` treats colors as sRGB values, for non-sRGB surfaces.
    pub fn with_color_mode(
        device: Arc<Device>,
        queue: Arc<Queue>,
        size: PhysicalSize<u32>,
        surface_format: wgpu::TextureFormat,
        color_mode: ColorMode,
    ) -> Self {
        // Create font system and cache
        // Parses all system fonts; slow, and far slower in debug builds.
        let mut font_system = FontSystem::new();
        let cache = SwashCache::new();

        // Create a cache for the TextAtlas
        let cache_ref = Cache::new(&device);

        // Create viewport
        let viewport = Viewport::new(&device, &cache_ref);

        // Create the text atlas with the correct parameters
        let mut atlas =
            TextAtlas::with_color_mode(&device, &queue, &cache_ref, surface_format, color_mode);

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
            _surface_format: surface_format,
            _cache_ref: cache_ref,
            viewport,
            cached_layout: None,
            cached_measure: None,
            label_buffers: Vec::new(),
            label_widths: Vec::new(),
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
        self.cached_layout = None;
        self.cached_measure = None;
    }

    /// Measure the rendered width of text at a given scale (single line, no wrapping)
    pub fn measure_text(&mut self, text: &str, scale: f32) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        if let Some((cached_text, cached_scale, width)) = &self.cached_measure {
            if cached_text == text && *cached_scale == scale {
                return *width;
            }
        }
        self.cached_layout = None;
        self.buffer.lines.clear();
        let font_size = 10.0 * scale;
        let metrics = Metrics::new(font_size, font_size * 1.1);
        self.buffer.set_metrics(&mut self.font_system, metrics);
        self.buffer.set_size(&mut self.font_system, None, None);
        self.buffer.set_text(
            &mut self.font_system,
            text,
            &Attrs::new().family(Family::SansSerif),
            Shaping::Advanced,
        );
        self.buffer.shape_until_scroll(&mut self.font_system, true);
        let width = self
            .buffer
            .layout_runs()
            .map(|run| run.line_w)
            .next()
            .unwrap_or(0.0);
        self.cached_measure = Some((text.to_string(), scale, width));
        width
    }

    /// Height of `text` wrapped to `area_width`, as `render_text` lays it out.
    /// It shapes with the same cache key, so a frame that measures and then
    /// renders the same text shapes it once.
    pub fn text_height(&mut self, text: &str, scale: f32, color: [f32; 4], area_width: u32) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        self.layout_text(text, scale, color, area_width);
        self.buffer
            .layout_runs()
            .last()
            .map_or(0.0, |run| run.line_top + run.line_height)
    }

    fn layout_text(&mut self, text: &str, scale: f32, color: [f32; 4], area_width: u32) -> Color {
        let font_size = 10.0 * scale;
        let metrics = Metrics::new(font_size, font_size * 1.1);

        let text_color_rgba = [
            (color[0] * 255.0) as u8,
            (color[1] * 255.0) as u8,
            (color[2] * 255.0) as u8,
            (color[3] * 255.0) as u8,
        ];
        let text_color = Color::rgba(
            text_color_rgba[0],
            text_color_rgba[1],
            text_color_rgba[2],
            text_color_rgba[3],
        );

        // Wrap at the text area width, with unlimited height for scrolling.
        let buffer_width = area_width as f32 - (LEFT_MARGIN + RIGHT_MARGIN);
        let needs_layout = self.cached_layout.as_ref().is_none_or(|cached| {
            cached.text != text
                || cached.font_size != font_size
                || cached.buffer_width != buffer_width
                || cached.color != text_color_rgba
        });

        if needs_layout {
            self.buffer.lines.clear();
            self.buffer.set_metrics(&mut self.font_system, metrics);
            self.buffer
                .set_size(&mut self.font_system, Some(buffer_width), None);
            self.buffer.set_text(
                &mut self.font_system,
                text,
                &Attrs::new().family(Family::SansSerif).color(text_color),
                Shaping::Advanced,
            );
            self.buffer.shape_until_scroll(&mut self.font_system, true);
            self.cached_layout = Some(CachedTextLayout {
                text: text.to_string(),
                font_size,
                buffer_width,
                color: text_color_rgba,
            });
        }
        text_color
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
        _area_height: u32,
        clip_rect: Option<(u32, u32, u32, u32)>,
    ) {
        if text.is_empty() {
            return;
        }

        let text_color = self.layout_text(text, scale, color, area_width);
        self.draw_buffer(view, encoder, x, y, text_color, clip_rect);
    }

    /// Renders `spans` (each text with its own color) as one wrapped paragraph,
    /// so single words can fade on their own. `line_height` is a multiple of the
    /// font size. It lays the text out on every call.
    pub fn render_spans(
        &mut self,
        view: &TextureView,
        encoder: &mut wgpu::CommandEncoder,
        spans: &[(&str, [f32; 4])],
        x: f32,
        y: f32,
        scale: f32,
        line_height: f32,
        area_width: u32,
    ) {
        let font_size = 10.0 * scale;
        self.cached_layout = None;
        self.buffer.lines.clear();
        self.buffer.set_metrics(
            &mut self.font_system,
            Metrics::new(font_size, font_size * line_height),
        );
        let buffer_width = area_width as f32 - (LEFT_MARGIN + RIGHT_MARGIN);
        self.buffer
            .set_size(&mut self.font_system, Some(buffer_width), None);
        let to_color = |c: &[f32; 4]| {
            Color::rgba(
                (c[0] * 255.0) as u8,
                (c[1] * 255.0) as u8,
                (c[2] * 255.0) as u8,
                (c[3] * 255.0) as u8,
            )
        };
        let base = Attrs::new().family(Family::SansSerif);
        self.buffer.set_rich_text(
            &mut self.font_system,
            spans
                .iter()
                .map(|(text, color)| (*text, base.clone().color(to_color(color)))),
            &base,
            Shaping::Advanced,
            None,
        );
        self.buffer.shape_until_scroll(&mut self.font_system, true);
        let default = spans
            .first()
            .map_or(Color::rgba(0, 0, 0, 0), |(_, c)| to_color(c));
        self.draw_buffer(view, encoder, x, y, default, None);
    }

    fn draw_buffer(
        &mut self,
        view: &TextureView,
        encoder: &mut wgpu::CommandEncoder,
        x: f32,
        y: f32,
        text_color: Color,
        clip_rect: Option<(u32, u32, u32, u32)>,
    ) {
        self.viewport.update(
            &self.queue,
            Resolution {
                width: self.size.width,
                height: self.size.height,
            },
        );

        let (clip_x, clip_y, clip_w, clip_h) =
            clip_rect.unwrap_or((0, 0, self.size.width, self.size.height));

        let text_area = TextArea {
            buffer: &self.buffer,
            left: x,
            top: y,
            scale: 1.0,
            bounds: TextBounds {
                left: clip_x as i32,
                top: clip_y as i32,
                right: (clip_x + clip_w) as i32,
                bottom: (clip_y + clip_h) as i32,
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

        render_pass.set_scissor_rect(clip_x, clip_y, clip_w, clip_h);

        if self
            .renderer
            .prepare(
                &self.device,
                &self.queue,
                &mut self.font_system,
                &mut self.atlas,
                &self.viewport,
                [text_area],
                &mut self.cache,
            )
            .is_ok()
        {
            let _ = self
                .renderer
                .render(&self.atlas, &self.viewport, &mut render_pass);
        }
    }

    /// Width of one line of `text` as `render_labels` draws it.
    pub fn measure_label(&mut self, text: &str, px: f32, bold: bool) -> f32 {
        let key = LabelKey {
            text: text.to_string(),
            px_bits: px.to_bits(),
            bold,
        };
        if let Some((_, width)) = self.label_widths.iter().find(|(k, _)| *k == key) {
            return *width;
        }
        let index = self.label_buffer(0, text, px, bold);
        let width = self.label_buffers[index]
            .1
            .layout_runs()
            .map(|run| run.line_w)
            .fold(0.0, f32::max);
        if self.label_widths.len() >= 256 {
            self.label_widths.clear();
        }
        self.label_widths.push((key, width));
        width
    }

    /// Draws single-line labels in one pass. Each label keeps its shaped buffer
    /// while its text, size, and weight stay the same, so only colour changes are free.
    pub fn render_labels(
        &mut self,
        view: &TextureView,
        encoder: &mut wgpu::CommandEncoder,
        labels: &[Label],
    ) {
        self.render_label_layer(view, encoder, labels, 0);
    }

    /// `render_labels` for a second layer of the same frame: its labels take the
    /// slots after `first_slot`, so the shaped buffers of the layer below stay.
    /// Submit the encoder of the layer below first: glyphon keeps one vertex buffer.
    pub fn render_label_layer(
        &mut self,
        view: &TextureView,
        encoder: &mut wgpu::CommandEncoder,
        labels: &[Label],
        first_slot: usize,
    ) {
        if labels.is_empty() {
            return;
        }
        // Slot 0 serves `measure_label`; labels use slots from 1 on.
        let base = first_slot + 1;
        for (i, label) in labels.iter().enumerate() {
            self.label_buffer(base + i, &label.text, label.px, label.bold);
        }
        self.viewport.update(
            &self.queue,
            Resolution {
                width: self.size.width,
                height: self.size.height,
            },
        );
        let to_color = |c: [f32; 4]| {
            Color::rgba(
                (c[0] * 255.0) as u8,
                (c[1] * 255.0) as u8,
                (c[2] * 255.0) as u8,
                (c[3].clamp(0.0, 1.0) * 255.0) as u8,
            )
        };
        let full = TextBounds {
            left: 0,
            top: 0,
            right: self.size.width as i32,
            bottom: self.size.height as i32,
        };
        let areas = labels.iter().enumerate().map(|(i, label)| TextArea {
            buffer: &self.label_buffers[base + i].1,
            left: label.x,
            top: label.y,
            scale: 1.0,
            bounds: label.clip.map_or(full, |[x, y, w, h]| TextBounds {
                left: x.floor() as i32,
                top: y.floor() as i32,
                right: (x + w).ceil() as i32,
                bottom: (y + h).ceil() as i32,
            }),
            default_color: to_color(label.color),
            custom_glyphs: &[],
        });
        if self
            .renderer
            .prepare(
                &self.device,
                &self.queue,
                &mut self.font_system,
                &mut self.atlas,
                &self.viewport,
                areas,
                &mut self.cache,
            )
            .is_err()
        {
            return;
        }
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Label Render Pass"),
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
        let _ = self
            .renderer
            .render(&self.atlas, &self.viewport, &mut render_pass);
    }

    /// Shapes `text` into label slot `index` unless it already holds it.
    fn label_buffer(&mut self, index: usize, text: &str, px: f32, bold: bool) -> usize {
        while self.label_buffers.len() <= index {
            let buffer = Buffer::new(&mut self.font_system, Metrics::new(16.0, 20.0));
            self.label_buffers.push((LabelKey::default(), buffer));
        }
        let key = LabelKey {
            text: text.to_string(),
            px_bits: px.to_bits(),
            bold,
        };
        let (cached, buffer) = &mut self.label_buffers[index];
        if *cached != key {
            buffer.set_metrics(&mut self.font_system, Metrics::new(px, px * LABEL_LINE));
            buffer.set_size(&mut self.font_system, None, None);
            let weight = if bold {
                Weight::SEMIBOLD
            } else {
                Weight::MEDIUM
            };
            buffer.set_text(
                &mut self.font_system,
                text,
                &Attrs::new().family(Family::SansSerif).weight(weight),
                Shaping::Advanced,
            );
            buffer.shape_until_scroll(&mut self.font_system, true);
            *cached = key;
        }
        index
    }

    /// Trim the atlas to free unused entries.
    /// Call once per frame AFTER queue.submit(), not between render_text calls.
    pub fn trim_atlas(&mut self) {
        self.atlas.trim();
    }
}
