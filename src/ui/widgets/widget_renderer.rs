use std::cell::RefCell;
use wgpu::{self, util::DeviceExt};

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct WidgetUniforms {
    color: [f32; 4],
    rect: [f32; 4], // x, y, width, height in pixels
    corner_radius: f32,
    viewport_width: f32,
    viewport_height: f32,
    _padding: f32,
}

struct PendingRect {
    uniforms: WidgetUniforms,
}

pub struct WidgetRenderer {
    pipeline: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
    pending_rects: RefCell<Vec<PendingRect>>,
}

impl WidgetRenderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Widget Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("widget.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Widget Pipeline Layout"),
            bind_group_layouts: &[],
            push_constant_ranges: &[wgpu::PushConstantRange {
                stages: wgpu::ShaderStages::FRAGMENT,
                range: 0..std::mem::size_of::<WidgetUniforms>() as u32,
            }],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Widget Pipeline"),
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

        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Widget Vertices"),
            contents: bytemuck::cast_slice(&[
                -1.0f32, -1.0, // bottom-left
                1.0, -1.0, // bottom-right
                -1.0, 1.0, // top-left
                1.0, 1.0, // top-right
            ]),
            usage: wgpu::BufferUsages::VERTEX,
        });

        Self {
            pipeline,
            vertices,
            pending_rects: RefCell::new(Vec::new()),
        }
    }

    pub fn draw_rounded_rect(
        &self,
        _encoder: &mut wgpu::CommandEncoder,
        _view: &wgpu::TextureView,
        _queue: &wgpu::Queue,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        corner_radius: f32,
        color: [f32; 4],
        window_width: u32,
        window_height: u32,
    ) {
        self.pending_rects.borrow_mut().push(PendingRect {
            uniforms: WidgetUniforms {
                color,
                rect: [x, y, width, height],
                corner_radius,
                viewport_width: window_width as f32,
                viewport_height: window_height as f32,
                _padding: 0.0,
            },
        });
    }

    pub fn flush(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        window_width: u32,
        window_height: u32,
    ) {
        let rects = self.pending_rects.borrow();
        if rects.is_empty() {
            return;
        }

        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Widget Render Pass"),
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
            0.0,
            0.0,
            window_width as f32,
            window_height as f32,
            0.0,
            1.0,
        );
        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_vertex_buffer(0, self.vertices.slice(..));

        for rect in rects.iter() {
            render_pass.set_push_constants(
                wgpu::ShaderStages::FRAGMENT,
                0,
                bytemuck::cast_slice(&[rect.uniforms]),
            );
            render_pass.draw(0..4, 0..1);
        }

        drop(render_pass);
        drop(rects);
        self.pending_rects.borrow_mut().clear();
    }
}
