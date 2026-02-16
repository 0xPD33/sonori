use wgpu::util::DeviceExt;

pub const SCROLLBAR_WIDTH: u32 = 6;

pub struct Scrollbar {
    pub vertices: wgpu::Buffer,
    pub pipeline: wgpu::RenderPipeline,
    pub track_bind_group: wgpu::BindGroup,
    pub thumb_bind_group: wgpu::BindGroup,
    pub scroll_offset: f32,
    pub max_scroll_offset: f32,
    pub auto_scroll: bool,
}

impl Scrollbar {
    pub fn new(
        device: &wgpu::Device,
        config: &wgpu::SurfaceConfiguration,
        _hover_bind_group_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        // Create vertices for the scrollbar
        let scrollbar_vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Scrollbar Vertices"),
            contents: bytemuck::cast_slice(&[
                // Scrollbar track (full height)
                -1.0f32, -1.0, // top-left
                1.0, -1.0, // top-right
                -1.0, 1.0, // bottom-left
                1.0, 1.0, // bottom-right
                // Scrollbar thumb (partial height) - make slimmer with less width
                -0.7f32, -0.8, // top-left
                0.7, -0.8, // top-right
                -0.7, 0.8, // bottom-left
                0.7, 0.8, // bottom-right
            ]),
            usage: wgpu::BufferUsages::VERTEX,
        });

        // Create bind group layout for color uniforms
        let color_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Scrollbar Color Bind Group Layout"),
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

        // Create color buffers
        // Track: dark semi-transparent
        let track_color_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Scrollbar Track Color Buffer"),
            contents: bytemuck::cast_slice(&[0.0f32, 0.0, 0.0, 0.15]), // Very dark, low opacity
            usage: wgpu::BufferUsages::UNIFORM,
        });

        // Thumb: lighter and more visible
        let thumb_color_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Scrollbar Thumb Color Buffer"),
            contents: bytemuck::cast_slice(&[0.3f32, 0.3, 0.3, 0.5]), // Gray, medium opacity
            usage: wgpu::BufferUsages::UNIFORM,
        });

        // Create bind groups
        let track_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Scrollbar Track Bind Group"),
            layout: &color_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: track_color_buffer.as_entire_binding(),
            }],
        });

        let thumb_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Scrollbar Thumb Bind Group"),
            layout: &color_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: thumb_color_buffer.as_entire_binding(),
            }],
        });

        // Create a pipeline for the scrollbar
        let scrollbar_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Scrollbar Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("scrollbar.wgsl").into()),
        });

        let scrollbar_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Scrollbar Pipeline Layout"),
                bind_group_layouts: &[&color_bind_group_layout],
                push_constant_ranges: &[],
            });

        let scrollbar_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Scrollbar Pipeline"),
            layout: Some(&scrollbar_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &scrollbar_shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 8,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x2,
                        offset: 0,
                        shader_location: 0,
                    }],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &scrollbar_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
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

        Self {
            vertices: scrollbar_vertices,
            pipeline: scrollbar_pipeline,
            track_bind_group,
            thumb_bind_group,
            scroll_offset: 0.0,
            max_scroll_offset: 0.0,
            auto_scroll: true,
        }
    }

    pub fn render(
        &self,
        view: &wgpu::TextureView,
        encoder: &mut wgpu::CommandEncoder,
        window_width: u32,
        text_area_height: u32,
        gap: u32,
    ) {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Scrollbar Render Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: view,
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

        // Set viewport for scrollbar track - Ensure exact height calculation
        // Use the full text_area_height value
        let track_height = (text_area_height - gap) as f32;
        render_pass.set_viewport(
            (window_width - SCROLLBAR_WIDTH) as f32,
            0.0,
            SCROLLBAR_WIDTH as f32,
            track_height,
            0.0,
            1.0,
        );

        // Draw scrollbar track
        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, &self.track_bind_group, &[]);
        render_pass.set_vertex_buffer(0, self.vertices.slice(..4 * 8));
        render_pass.draw(0..4, 0..1);

        // Calculate thumb position and size
        // Ensure the ratio calculation is correct to size the thumb correctly
        let content_height = track_height + self.max_scroll_offset;
        let visible_ratio = if content_height > 0.0 {
            track_height / content_height
        } else {
            1.0
        };

        // Minimum height for the thumb, but make sure it's proportional to content
        let thumb_height = (track_height * visible_ratio).max(20.0).min(track_height);

        // Calculate scroll progress (0.0 to 1.0)
        let scroll_progress = if self.max_scroll_offset > 0.0 {
            self.scroll_offset / self.max_scroll_offset
        } else {
            0.0
        };

        // Calculate where to place the thumb within the track
        let available_track = track_height - thumb_height;
        let thumb_top = scroll_progress * available_track;

        // Set viewport for scrollbar thumb
        render_pass.set_viewport(
            (window_width - SCROLLBAR_WIDTH) as f32,
            thumb_top,
            SCROLLBAR_WIDTH as f32,
            thumb_height,
            0.0,
            1.0,
        );

        // Draw scrollbar thumb
        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, &self.thumb_bind_group, &[]);
        render_pass.set_vertex_buffer(0, self.vertices.slice(4 * 8..));
        render_pass.draw(0..4, 0..1);

        // Draw auto-scroll indicator
        if self.auto_scroll {
            render_pass.set_viewport(
                (window_width - SCROLLBAR_WIDTH) as f32,
                track_height - 5.0,
                SCROLLBAR_WIDTH as f32,
                5.0,
                0.0,
                1.0,
            );

            render_pass.set_pipeline(&self.pipeline);
            render_pass.set_bind_group(0, &self.thumb_bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.vertices.slice(4 * 8..));
            render_pass.draw(0..4, 0..1);
        }
    }
}
