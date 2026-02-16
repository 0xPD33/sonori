use std::time::Instant;
use wgpu::{util::DeviceExt, Device, Queue, RenderPipeline, TextureView};

use super::buttons::ButtonType;

// Animation constants
const SHOW_DELAY: f32 = 0.15; // 150ms delay before showing
const FADE_DURATION: f32 = 0.1; // 100ms fade-in animation

// Tooltip styling
const TOOLTIP_PADDING_X: f32 = 6.0;
const TOOLTIP_PADDING_Y: f32 = 6.0; // Equal padding on all sides
const TOOLTIP_GAP: f32 = 8.0; // Gap between button and tooltip
const TOOLTIP_FONT_SIZE: f32 = 14.0;
const TOOLTIP_LINE_HEIGHT_MULTIPLIER: f32 = 1.2;

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 2],
}

impl Vertex {
    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x2,
            }],
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct TooltipUniforms {
    opacity: f32,
    _padding: [f32; 3], // Pad to 16 bytes for uniform buffer alignment
}

#[derive(Debug, Clone)]
enum TooltipState {
    Hidden,
    WaitingToShow {
        button_type: ButtonType,
        button_center_x: f32,
        button_top_y: f32,
        button_bottom_y: f32,
        button_left_x: f32,
        start_time: Instant,
    },
    Showing {
        button_type: ButtonType,
        button_center_x: f32,
        button_top_y: f32,
        button_bottom_y: f32,
        button_left_x: f32,
        show_start_time: Instant,
    },
}

pub struct Tooltip {
    render_pipeline: RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    state: TooltipState,
    text_dimensions: std::collections::HashMap<ButtonType, (f32, f32)>,
    cached_buffers: std::collections::HashMap<ButtonType, glyphon::Buffer>,
    device: Device,
    queue: Queue,
    font_system: glyphon::FontSystem,
    swash_cache: glyphon::SwashCache,
    text_atlas: glyphon::TextAtlas,
    text_renderer: glyphon::TextRenderer,
    _cache: glyphon::Cache,
    viewport: glyphon::Viewport,
}

impl Tooltip {
    pub fn new(device: Device, queue: Queue, surface_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Tooltip Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("tooltip.wgsl").into()),
        });

        // Create uniform buffer for opacity
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Tooltip Uniform Buffer"),
            contents: bytemuck::cast_slice(&[TooltipUniforms {
                opacity: 0.0,
                _padding: [0.0; 3],
            }]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        // Create bind group layout
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Tooltip Bind Group Layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        // Create bind group
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Tooltip Bind Group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Tooltip Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Tooltip Render Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Vertex::desc()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        });

        // Create vertex buffer (will be updated when showing tooltip)
        let vertices = [
            Vertex {
                position: [-1.0, -1.0],
            },
            Vertex {
                position: [1.0, -1.0],
            },
            Vertex {
                position: [-1.0, 1.0],
            },
            Vertex {
                position: [1.0, 1.0],
            },
        ];

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Tooltip Vertex Buffer"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        });

        // Initialize glyphon resources
        let mut font_system = glyphon::FontSystem::new();
        let swash_cache = glyphon::SwashCache::new();
        let cache = glyphon::Cache::new(&device);
        let viewport = glyphon::Viewport::new(&device, &cache);
        let mut text_atlas = glyphon::TextAtlas::new(&device, &queue, &cache, surface_format);
        let text_renderer = glyphon::TextRenderer::new(
            &mut text_atlas,
            &device,
            wgpu::MultisampleState::default(),
            None,
        );

        let mut text_dimensions = std::collections::HashMap::new();
        let mut cached_buffers = std::collections::HashMap::new();

        for button_type in [
            ButtonType::Copy,
            ButtonType::Reset,
            ButtonType::Close,
            ButtonType::Pause,
            ButtonType::Play,
            ButtonType::RecordToggle,
            ButtonType::ModeToggle,
            ButtonType::Accept,
            ButtonType::MagicMode,
        ] {
            let text = Self::get_tooltip_text(button_type);
            let (width, height) =
                Self::calculate_text_dimensions(text, TOOLTIP_FONT_SIZE, &mut font_system);
            text_dimensions.insert(button_type, (width, height));

            use glyphon::{Attrs, Buffer, Family, Metrics, Shaping};
            let line_height = TOOLTIP_FONT_SIZE * TOOLTIP_LINE_HEIGHT_MULTIPLIER;
            let metrics = Metrics::new(TOOLTIP_FONT_SIZE, line_height);
            let mut buffer = Buffer::new(&mut font_system, metrics);
            let tooltip_width = width + TOOLTIP_PADDING_X * 2.0;
            let tooltip_height = height + TOOLTIP_PADDING_Y * 2.0;
            buffer.set_size(&mut font_system, Some(tooltip_width), Some(tooltip_height));
            buffer.set_text(
                &mut font_system,
                text,
                &Attrs::new().family(Family::SansSerif),
                Shaping::Advanced,
            );
            cached_buffers.insert(button_type, buffer);
        }

        Self {
            render_pipeline,
            vertex_buffer,
            uniform_buffer,
            bind_group,
            state: TooltipState::Hidden,
            text_dimensions,
            cached_buffers,
            device,
            queue,
            font_system,
            swash_cache,
            text_atlas,
            text_renderer,
            _cache: cache,
            viewport,
        }
    }

    /// Get tooltip text for a button type
    fn get_tooltip_text(button_type: ButtonType) -> &'static str {
        match button_type {
            ButtonType::Copy => "Copy",
            ButtonType::Reset => "Reset",
            ButtonType::Close => "Close",
            ButtonType::Pause => "Pause",
            ButtonType::Play => "Resume",
            ButtonType::RecordToggle => "Record",
            ButtonType::ModeToggle => "Switch Mode",
            ButtonType::Accept => "Accept",
            ButtonType::MagicMode => "Magic Mode",
        }
    }

    /// Calculate text dimensions using glyphon
    fn calculate_text_dimensions(
        text: &str,
        font_size: f32,
        font_system: &mut glyphon::FontSystem,
    ) -> (f32, f32) {
        use glyphon::{Attrs, Buffer, Family, Metrics, Shaping};

        let line_height = font_size * TOOLTIP_LINE_HEIGHT_MULTIPLIER;
        let metrics = Metrics::new(font_size, line_height);
        let mut buffer = Buffer::new(font_system, metrics);
        buffer.set_size(font_system, Some(1000.0), Some(100.0));
        buffer.set_text(
            font_system,
            text,
            &Attrs::new().family(Family::SansSerif),
            Shaping::Advanced,
        );

        // Get the width and height
        let layout = buffer.layout_runs().collect::<Vec<_>>();
        let width = layout
            .iter()
            .map(|run| run.line_w)
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or(0.0);
        // Use line_height instead of font_size for proper vertical spacing
        let height = line_height;

        (width, height)
    }

    /// Update tooltip state based on current button hover
    pub fn update(&mut self, hovered_button: Option<(ButtonType, f32, f32, f32, f32)>) {
        match (&self.state, hovered_button) {
            // Hidden -> WaitingToShow: Start hover timer
            (TooltipState::Hidden, Some((button_type, center_x, top_y, bottom_y, left_x))) => {
                self.state = TooltipState::WaitingToShow {
                    button_type,
                    button_center_x: center_x,
                    button_top_y: top_y,
                    button_bottom_y: bottom_y,
                    button_left_x: left_x,
                    start_time: Instant::now(),
                };
            }

            // WaitingToShow -> Showing: Show after delay
            (
                TooltipState::WaitingToShow {
                    button_type,
                    button_center_x,
                    button_top_y,
                    button_bottom_y,
                    button_left_x,
                    start_time,
                },
                Some((hovered_type, _, _, _, _)),
            ) if *button_type == hovered_type => {
                if start_time.elapsed().as_secs_f32() >= SHOW_DELAY {
                    self.state = TooltipState::Showing {
                        button_type: *button_type,
                        button_center_x: *button_center_x,
                        button_top_y: *button_top_y,
                        button_bottom_y: *button_bottom_y,
                        button_left_x: *button_left_x,
                        show_start_time: Instant::now(),
                    };
                }
            }

            // Any state -> Hidden: Mouse left button
            (_, None) => {
                self.state = TooltipState::Hidden;
            }

            // Showing: Stay showing if still hovering same button
            (TooltipState::Showing { button_type, .. }, Some((hovered_type, _, _, _, _)))
                if *button_type == hovered_type =>
            {
                // Stay in showing state
            }

            // WaitingToShow but moved to different button -> Reset timer
            (
                TooltipState::WaitingToShow { .. },
                Some((button_type, center_x, top_y, bottom_y, left_x)),
            ) => {
                self.state = TooltipState::WaitingToShow {
                    button_type,
                    button_center_x: center_x,
                    button_top_y: top_y,
                    button_bottom_y: bottom_y,
                    button_left_x: left_x,
                    start_time: Instant::now(),
                };
            }

            // Showing but moved to different button -> Hide and restart
            (
                TooltipState::Showing { .. },
                Some((button_type, center_x, top_y, bottom_y, left_x)),
            ) => {
                self.state = TooltipState::WaitingToShow {
                    button_type,
                    button_center_x: center_x,
                    button_top_y: top_y,
                    button_bottom_y: bottom_y,
                    button_left_x: left_x,
                    start_time: Instant::now(),
                };
            }
        }
    }

    /// Render the tooltip
    pub fn render(
        &mut self,
        view: &TextureView,
        encoder: &mut wgpu::CommandEncoder,
        window_width: u32,
        window_height: u32,
    ) {
        // Early return if hidden or waiting
        let (button_type, button_center_x, button_top_y, button_bottom_y, button_left_x, opacity) =
            match &self.state {
                TooltipState::Hidden | TooltipState::WaitingToShow { .. } => return,
                TooltipState::Showing {
                    button_type,
                    button_center_x,
                    button_top_y,
                    button_bottom_y,
                    button_left_x,
                    show_start_time,
                } => {
                    let elapsed = show_start_time.elapsed().as_secs_f32();
                    let opacity = if elapsed < FADE_DURATION {
                        elapsed / FADE_DURATION
                    } else {
                        1.0
                    };
                    (
                        *button_type,
                        *button_center_x,
                        *button_top_y,
                        *button_bottom_y,
                        *button_left_x,
                        opacity,
                    )
                }
            };

        // Update opacity uniform
        self.queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::cast_slice(&[TooltipUniforms {
                opacity,
                _padding: [0.0; 3],
            }]),
        );

        // Get text dimensions
        let (text_width, text_height) = self
            .text_dimensions
            .get(&button_type)
            .copied()
            .unwrap_or((50.0, 14.0));

        // Calculate tooltip dimensions
        let tooltip_width = text_width + TOOLTIP_PADDING_X * 2.0;
        let tooltip_height = text_height + TOOLTIP_PADDING_Y * 2.0;

        // Smart positioning: show left of Close button, above others
        let (tooltip_x, tooltip_y) = if button_type == ButtonType::Close {
            // Close button - show tooltip to the left, vertically centered
            let button_center_y = (button_top_y + button_bottom_y) / 2.0;
            let x = button_left_x - tooltip_width - TOOLTIP_GAP;
            let y = button_center_y - tooltip_height / 2.0;
            (x, y)
        } else {
            // Other buttons - show tooltip above, horizontally centered
            let x = button_center_x - tooltip_width / 2.0;
            let y = button_top_y - tooltip_height - TOOLTIP_GAP;
            (x, y)
        };

        // Clamp to window bounds with extra margin for close button at edges
        let edge_margin = if button_type == ButtonType::Close {
            6.0
        } else {
            4.0
        };
        let tooltip_x = tooltip_x
            .max(edge_margin)
            .min(window_width as f32 - tooltip_width - edge_margin);
        let tooltip_y = tooltip_y
            .max(edge_margin)
            .min(window_height as f32 - tooltip_height - edge_margin);

        // Convert to clip space coordinates
        let x_min = (tooltip_x / window_width as f32) * 2.0 - 1.0;
        let x_max = ((tooltip_x + tooltip_width) / window_width as f32) * 2.0 - 1.0;
        let y_min = 1.0 - ((tooltip_y + tooltip_height) / window_height as f32) * 2.0;
        let y_max = 1.0 - (tooltip_y / window_height as f32) * 2.0;

        // Update vertex buffer with new position
        let vertices = [
            Vertex {
                position: [x_min, y_min],
            },
            Vertex {
                position: [x_max, y_min],
            },
            Vertex {
                position: [x_min, y_max],
            },
            Vertex {
                position: [x_max, y_max],
            },
        ];
        self.queue
            .write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&vertices));

        // Render background
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Tooltip Render Pass"),
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

        render_pass.set_pipeline(&self.render_pipeline);
        render_pass.set_bind_group(0, &self.bind_group, &[]);
        render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        render_pass.draw(0..4, 0..1);

        drop(render_pass);

        let text_x = tooltip_x + TOOLTIP_PADDING_X;
        let text_y = tooltip_y + TOOLTIP_PADDING_Y;

        use glyphon::{Color, TextArea, TextBounds};

        let buffer = self
            .cached_buffers
            .get(&button_type)
            .expect("Cached buffer not found");

        let text_area = TextArea {
            buffer,
            left: text_x,
            top: text_y,
            scale: 1.0,
            bounds: TextBounds {
                left: text_x as i32,
                top: text_y as i32,
                right: (text_x + text_width) as i32,
                bottom: (text_y + text_height) as i32,
            },
            default_color: Color::rgb(255, 255, 255),
            custom_glyphs: &[],
        };

        // Update viewport size
        self.viewport.update(
            &self.queue,
            glyphon::Resolution {
                width: window_width,
                height: window_height,
            },
        );

        // Prepare text
        self.text_renderer
            .prepare(
                &self.device,
                &self.queue,
                &mut self.font_system,
                &mut self.text_atlas,
                &self.viewport,
                [text_area],
                &mut self.swash_cache,
            )
            .expect("Failed to prepare tooltip text");

        // Render text
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Tooltip Text Render Pass"),
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
            .expect("Failed to render tooltip text");

        drop(render_pass);

        // Trim atlas
        self.text_atlas.trim();
    }
}
