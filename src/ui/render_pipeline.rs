use std::sync::Arc;
use wgpu::{self, util::DeviceExt};
use winit::dpi::PhysicalSize;

pub struct RenderPipelines {
    pub rounded_rect_pipeline: wgpu::RenderPipeline,
    pub rounded_rect_vertices: wgpu::Buffer,
    pub hover_uniform_buffer: wgpu::Buffer,
    pub hover_bind_group: wgpu::BindGroup,
    pub hover_bind_group_layout: wgpu::BindGroupLayout,
}

impl RenderPipelines {
    pub fn new(device: &wgpu::Device, config: &wgpu::SurfaceConfiguration) -> Self {
        // Create rounded rect shader
        let rounded_rect_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Rounded Rect Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("rounded_rect.wgsl").into()),
        });

        // Create bind group layout for hover uniform
        let hover_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Hover Bind Group Layout"),
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

        // Create rounded rect pipeline layout
        let rounded_rect_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Rounded Rect Pipeline Layout"),
                bind_group_layouts: &[&hover_bind_group_layout],
                push_constant_ranges: &[],
            });

        // Create rounded rect pipeline
        let rounded_rect_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Rounded Rect Pipeline"),
                layout: Some(&rounded_rect_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &rounded_rect_shader,
                    entry_point: Some("vs_main"),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: 8,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0 => Float32x2],
                    }],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &rounded_rect_shader,
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
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });

        // Create a vertex buffer for our quad vertices
        #[repr(C)]
        #[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
        struct Vertex {
            position: [f32; 2],
        }

        let vertices = [
            Vertex {
                position: [-1.0, -1.0],
            }, // Bottom left
            Vertex {
                position: [1.0, -1.0],
            }, // Bottom right
            Vertex {
                position: [-1.0, 1.0],
            }, // Top left
            Vertex {
                position: [1.0, 1.0],
            }, // Top right
        ];

        let rounded_rect_vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Vertex Buffer"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        // Create uniform buffer for hover animation
        // Start with opacity_multiplier = 0.0 (base opacity)
        let hover_uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Hover Uniform Buffer"),
            contents: bytemuck::cast_slice(&[0.0f32]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        // Create bind group for hover uniform
        let hover_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Hover Bind Group"),
            layout: &hover_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: hover_uniform_buffer.as_entire_binding(),
            }],
        });

        Self {
            rounded_rect_pipeline,
            rounded_rect_vertices,
            hover_uniform_buffer,
            hover_bind_group,
            hover_bind_group_layout,
        }
    }

    pub fn draw_background(&self, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Clear Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 0.0, // Fully transparent
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
    }

    pub fn draw_spectrogram_background(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        text_area_height: u32,
        gap: u32,
        spectrogram_width: u32,
        spectrogram_height: u32,
    ) {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Spectrogram Background Pass"),
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

        // Set viewport to cover just the spectrogram area
        render_pass.set_viewport(
            0.0,                             // x position
            (text_area_height + gap) as f32, // y position - start below text area with a gap
            spectrogram_width as f32,        // width
            spectrogram_height as f32,       // height
            0.0,                             // min depth
            1.0,                             // max depth
        );

        render_pass.set_pipeline(&self.rounded_rect_pipeline);
        render_pass.set_bind_group(0, &self.hover_bind_group, &[]);
        render_pass.set_vertex_buffer(0, self.rounded_rect_vertices.slice(..));
        render_pass.draw(0..4, 0..1); // 4 vertices for the quad
    }
}
