use std::sync::Arc;
use wgpu::{self, util::DeviceExt};

/// Shared utility for GPU quad rendering to reduce code duplication
pub struct GpuQuadRenderer {
    _device: Arc<wgpu::Device>,
    vertex_buffer: wgpu::Buffer,
    _pipeline_layout: wgpu::PipelineLayout,
    pipeline: wgpu::RenderPipeline,
}

impl GpuQuadRenderer {
    /// Create a new quad renderer with the specified shader and format
    pub fn new_simple(
        device: &Arc<wgpu::Device>,
        format: wgpu::TextureFormat,
        label: &str,
    ) -> Self {
        // Create a simple pipeline for now - can be extended later
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(&format!("{} Shader", label)),
            source: wgpu::ShaderSource::Wgsl(include_str!("quad.wgsl").into()),
        });

        // Create shared vertex buffer for quad rendering
        let vertices = [
            [-1.0f32, -1.0], // Bottom left
            [1.0, -1.0],     // Bottom right
            [-1.0, 1.0],     // Top left
            [1.0, 1.0],      // Top right
        ];

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("{} Vertex Buffer", label)),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        // Create pipeline layout
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(&format!("{} Pipeline Layout", label)),
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });

        // Create render pipeline
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(&format!("{} Pipeline", label)),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[wgpu::VertexAttribute {
                        offset: 0,
                        shader_location: 0,
                        format: wgpu::VertexFormat::Float32x2,
                    }],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
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

        Self {
            _device: device.clone(),
            vertex_buffer,
            _pipeline_layout: pipeline_layout,
            pipeline,
        }
    }

    /// Create a render pass with common configuration
    pub fn create_render_pass<'a>(
        encoder: &'a mut wgpu::CommandEncoder,
        view: &'a wgpu::TextureView,
        label: &str,
    ) -> wgpu::RenderPass<'a> {
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
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
        })
    }

    /// Render a quad with the specified viewport and bind groups
    pub fn render_quad(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        viewport_x: f32,
        viewport_y: f32,
        viewport_width: f32,
        viewport_height: f32,
        bind_groups: &[Option<&wgpu::BindGroup>],
        label: &str,
    ) {
        let mut render_pass =
            Self::create_render_pass(encoder, view, &format!("{} Render Pass", label));

        // Set viewport
        render_pass.set_viewport(
            viewport_x,
            viewport_y,
            viewport_width,
            viewport_height,
            0.0,
            1.0,
        );

        // Set pipeline and bind groups
        render_pass.set_pipeline(&self.pipeline);
        for (i, bind_group) in bind_groups.iter().enumerate() {
            if let Some(bg) = bind_group {
                render_pass.set_bind_group(i as u32, *bg, &[]);
            }
        }

        // Set vertex buffer and draw
        render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        render_pass.draw(0..4, 0..1);
    }

    /// Get the pipeline for custom rendering
    pub fn pipeline(&self) -> &wgpu::RenderPipeline {
        &self.pipeline
    }
}
