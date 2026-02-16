use std::time::Instant;
use wgpu::{util::DeviceExt, RenderPipeline, TextureView};
use winit::dpi::PhysicalSize;

// Animation constants
const FADE_ANIMATION_DURATION: f32 = 0.2; // 200ms
const FADE_ANIMATION_SPEED: f32 = 5.0; // Speed factor for easing

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

/// Button panel background with fade animation
pub struct ButtonPanel {
    _device: wgpu::Device,
    _queue: wgpu::Queue,
    render_pipeline: RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    animation_progress: f32, // 0.0 = hidden, 1.0 = visible
    target_progress: f32,    // Target animation state
    animation_start_time: Option<Instant>,
    size: PhysicalSize<u32>,
}

impl ButtonPanel {
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        size: PhysicalSize<u32>,
        surface_format: wgpu::TextureFormat,
        hover_bind_group_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Button Panel Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("button_panel.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Button Panel Pipeline Layout"),
            bind_group_layouts: &[hover_bind_group_layout],
            push_constant_ranges: &[],
        });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Button Panel Render Pipeline"),
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

        // Create vertices for a full-screen quad
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
            label: Some("Button Panel Vertex Buffer"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        Self {
            _device: device,
            _queue: queue,
            render_pipeline,
            vertex_buffer,
            animation_progress: 0.0,
            target_progress: 0.0,
            animation_start_time: None,
            size,
        }
    }

    /// Set whether the panel should be visible
    pub fn set_visible(&mut self, visible: bool) {
        self.target_progress = if visible { 1.0 } else { 0.0 };
        if self.animation_start_time.is_none() {
            self.animation_start_time = Some(Instant::now());
        }
    }

    /// Update animation progress
    pub fn update(&mut self) {
        if let Some(start_time) = self.animation_start_time {
            let elapsed = start_time.elapsed().as_secs_f32();
            let animation_duration = FADE_ANIMATION_DURATION / FADE_ANIMATION_SPEED;

            if elapsed >= animation_duration {
                // Animation complete
                self.animation_progress = self.target_progress;
                self.animation_start_time = None;
            } else {
                // Animate towards target
                let progress = elapsed / animation_duration;
                let eased_progress = progress * progress; // Quadratic easing

                if self.target_progress > self.animation_progress {
                    // Fading in
                    self.animation_progress = self
                        .animation_progress
                        .max(self.animation_progress + eased_progress * 0.01)
                        .min(self.target_progress);
                } else {
                    // Fading out
                    self.animation_progress = self
                        .animation_progress
                        .min(self.animation_progress - eased_progress * 0.01)
                        .max(self.target_progress);
                }
            }
        }
    }

    /// Get current animation progress (0.0 = hidden, 1.0 = visible)
    pub fn animation_progress(&self) -> f32 {
        self.animation_progress
    }

    /// Check if panel is currently visible or animating
    pub fn should_render(&self) -> bool {
        self.animation_progress > 0.01
    }

    /// Render the button panel with a specific viewport
    /// bounds: (x, y, width, height) in pixels
    pub fn render_with_bounds(
        &self,
        view: &TextureView,
        encoder: &mut wgpu::CommandEncoder,
        bounds: Option<(f32, f32, f32, f32)>,
        hover_bind_group: &wgpu::BindGroup,
    ) {
        if !self.should_render() {
            return;
        }

        // Don't render if no bounds provided
        let Some((x, y, width, height)) = bounds else {
            return;
        };

        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Button Panel Render Pass"),
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

        // Set viewport to restrict rendering to button area only
        render_pass.set_viewport(x, y, width, height, 0.0, 1.0);

        render_pass.set_pipeline(&self.render_pipeline);
        render_pass.set_bind_group(0, hover_bind_group, &[]);
        render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        render_pass.draw(0..4, 0..1);
    }

    /// Resize the panel
    pub fn resize(&mut self, new_size: PhysicalSize<u32>) {
        self.size = new_size;
    }
}
