use wgpu::{self, util::DeviceExt};
use winit::dpi::PhysicalSize;

use super::text_renderer::TextRenderer;

pub struct TextWindow {
    pipeline: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
    text_renderer: TextRenderer,
}

impl TextWindow {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        config: &wgpu::SurfaceConfiguration,
        size: PhysicalSize<u32>,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Text Window Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("text_window.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Text Window Pipeline Layout"),
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Text Window Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 8,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
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

        let vertices = [
            [-1.0f32, -1.0], // Bottom left
            [1.0, -1.0],     // Bottom right
            [-1.0, 1.0],     // Top left
            [1.0, 1.0],      // Top right
        ];

        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Text Window Vertices"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let text_renderer = TextRenderer::new(
            std::sync::Arc::new(device.clone()),
            std::sync::Arc::new(queue.clone()),
            size,
            config.format,
        );

        Self {
            pipeline,
            vertices,
            text_renderer,
        }
    }

    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        self.text_renderer.resize(size);
    }

    pub fn render(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        text: &str,
        text_area_width: u32,
        text_area_height: u32,
        gap: u32,
        text_x: f32,
        text_y: f32,
        text_scale: f32,
        text_color: [f32; 4],
    ) {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Text Window Pass"),
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

        render_pass.set_viewport(
            0.0,                             // x position
            0.0,                             // y position
            text_area_width as f32,          // width
            (text_area_height - gap) as f32, // height
            0.0,                             // min depth
            1.0,                             // max depth
        );

        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_vertex_buffer(0, self.vertices.slice(..));
        render_pass.draw(0..4, 0..1);

        drop(render_pass);

        // Render text
        self.text_renderer.render_text(
            view,
            encoder,
            text,
            text_x,
            text_y,
            text_scale,
            text_color,
            text_area_width,
            text_area_height,
        );
    }
}
