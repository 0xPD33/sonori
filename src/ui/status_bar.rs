use std::sync::Arc;

use parking_lot::RwLock;
use wgpu::{self, util::DeviceExt};
use winit::dpi::PhysicalSize;

use super::common::{BackendStatus, BackendStatusState};
use super::text_renderer::TextRenderer;

const ERROR_FADE_DURATION_SECS: f64 = 10.0;

pub struct StatusBar {
    pipeline: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    left_text_renderer: TextRenderer,
    right_text_renderer: TextRenderer,
    recording_dot_renderer: TextRenderer,
    recording_timer_renderer: TextRenderer,
    status: Arc<RwLock<BackendStatus>>,
    pulse_phase: f32,
    last_update: std::time::Instant,
}

impl StatusBar {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        config: &wgpu::SurfaceConfiguration,
        size: PhysicalSize<u32>,
        status: Arc<RwLock<BackendStatus>>,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Status Bar Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("status_bar.wgsl").into()),
        });

        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Status Bar Uniform Buffer"),
            contents: bytemuck::cast_slice(&[0.0f32, -1.0f32, -1.0f32, 0.0f32]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Status Bar Bind Group Layout"),
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

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Status Bar Bind Group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Status Bar Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Status Bar Pipeline"),
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

        let vertices: [[f32; 2]; 4] = [
            [-1.0, -1.0],
            [1.0, -1.0],
            [-1.0, 1.0],
            [1.0, 1.0],
        ];

        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Status Bar Vertices"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let left_text_renderer = TextRenderer::new(
            Arc::new(device.clone()),
            Arc::new(queue.clone()),
            size,
            config.format,
        );

        let right_text_renderer = TextRenderer::new(
            Arc::new(device.clone()),
            Arc::new(queue.clone()),
            size,
            config.format,
        );

        let recording_dot_renderer = TextRenderer::new(
            Arc::new(device.clone()),
            Arc::new(queue.clone()),
            size,
            config.format,
        );

        let recording_timer_renderer = TextRenderer::new(
            Arc::new(device.clone()),
            Arc::new(queue.clone()),
            size,
            config.format,
        );

        Self {
            pipeline,
            vertices,
            uniform_buffer,
            bind_group,
            left_text_renderer,
            right_text_renderer,
            recording_dot_renderer,
            recording_timer_renderer,
            status,
            pulse_phase: 0.0,
            last_update: std::time::Instant::now(),
        }
    }

    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        self.left_text_renderer.resize(size);
        self.right_text_renderer.resize(size);
        self.recording_dot_renderer.resize(size);
        self.recording_timer_renderer.resize(size);
    }

    pub fn render(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        queue: &wgpu::Queue,
        bar_x: u32,
        bar_y: u32,
        bar_width: u32,
        bar_height: u32,
    ) {
        // Update pulse phase for recording dot animation
        let now = std::time::Instant::now();
        let delta = now.duration_since(self.last_update).as_secs_f32();
        self.last_update = now;
        self.pulse_phase = (self.pulse_phase + delta * 3.0) % (2.0 * std::f32::consts::PI);

        // Extract all data from the locked status
        let (error_tint, download_progress, prefix_text, status_text, status_color, is_recording, recording_elapsed, is_loading) = {
            let mut status = self.status.write();

            // Auto-clear errors after ERROR_FADE_DURATION_SECS
            if let BackendStatusState::Error(_) = &status.state {
                if let Some(error_time) = status.error_time {
                    if error_time.elapsed().as_secs_f64() >= ERROR_FADE_DURATION_SECS {
                        status.state = BackendStatusState::Ready;
                        status.error_time = None;
                    }
                }
            }

            let error_tint = match &status.state {
                BackendStatusState::Error(_) => {
                    if let Some(error_time) = status.error_time {
                        let elapsed = error_time.elapsed().as_secs_f64();
                        let fade_start = ERROR_FADE_DURATION_SECS - 2.0;
                        if elapsed > fade_start {
                            1.0 - ((elapsed - fade_start) / 2.0).min(1.0) as f32
                        } else {
                            1.0
                        }
                    } else {
                        1.0
                    }
                }
                _ => 0.0,
            };

            let (status_text, status_color) = if let Some(progress) = status.download_progress {
                (
                    format!("Downloading {:.0}%", progress * 100.0),
                    [0.2, 0.7, 1.0, 0.9], // blue
                )
            } else {
                match &status.state {
                    BackendStatusState::Ready => ("Ready".to_string(), [0.3, 0.85, 0.4, 0.9]),
                    BackendStatusState::Loading(msg) => {
                        let text = if msg.is_empty() {
                            "Loading...".to_string()
                        } else {
                            format!("Loading: {}", msg)
                        };
                        (text, [1.0, 0.85, 0.2, 0.9])
                    }
                    BackendStatusState::Error(msg) => (msg.clone(), [1.0, 0.3, 0.3, 0.9]),
                }
            };

            let prefix_text = format!(
                "{} \u{00B7} {} \u{00B7} ",
                status.backend_name, status.model_name
            );

            let is_recording = status.is_recording;
            let recording_elapsed = status.recording_start.map(|start| start.elapsed());
            let download_progress = status.download_progress.unwrap_or(-1.0);
            let is_loading = matches!(status.state, BackendStatusState::Loading(_));

            (error_tint, download_progress, prefix_text, status_text, status_color, is_recording, recording_elapsed, is_loading)
        };

        // Compute loading sweep phase (animated indeterminate bar)
        let loading_phase = if is_loading && download_progress < 0.0 {
            // Only show sweep when loading without a download progress bar
            (self.pulse_phase * 0.3).fract()
        } else {
            -1.0
        };

        // Update uniform buffer
        queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::cast_slice(&[error_tint, download_progress, loading_phase, 0.0f32]),
        );

        // Render background
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Status Bar Pass"),
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
                bar_x as f32,
                bar_y as f32,
                bar_width as f32,
                bar_height as f32,
                0.0,
                1.0,
            );

            render_pass.set_pipeline(&self.pipeline);
            render_pass.set_bind_group(0, &self.bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.vertices.slice(..));
            render_pass.draw(0..4, 0..1);
        }

        let left_color: [f32; 4] = [0.7, 0.7, 0.7, 0.8];
        let text_scale = 1.0;
        let text_y_offset = 5.0;
        let left_padding = 10.0;

        // Measure prefix width for precise status text placement
        let prefix_width = self.left_text_renderer.measure_text(&prefix_text, text_scale);

        // Render prefix text: "BackendName · ModelName · "
        self.left_text_renderer.render_text(
            view,
            encoder,
            &prefix_text,
            bar_x as f32 + left_padding,
            bar_y as f32 + text_y_offset,
            text_scale,
            left_color,
            bar_width,
            bar_height,
        );

        // Render status text right after the prefix, with its own color
        let status_x = bar_x as f32 + left_padding + prefix_width;

        self.right_text_renderer.render_text(
            view,
            encoder,
            &status_text,
            status_x,
            bar_y as f32 + text_y_offset,
            text_scale,
            status_color,
            bar_width,
            bar_height,
        );

        // Render recording indicator on the right side
        if is_recording {
            if let Some(elapsed) = recording_elapsed {
                let total_secs = elapsed.as_secs();
                let minutes = total_secs / 60;
                let seconds = total_secs % 60;
                let timer_text = format!("{minutes}:{seconds:02}");

                let pulse_alpha = 0.5 + 0.5 * self.pulse_phase.sin();
                let dot_text = "\u{25CF}"; // filled circle

                let dot_color: [f32; 4] = [1.0, 0.2, 0.2, pulse_alpha];
                let timer_color: [f32; 4] = [0.9, 0.9, 0.9, 0.9];

                let dot_width = self.recording_dot_renderer.measure_text(dot_text, text_scale);
                let timer_width = self.recording_timer_renderer.measure_text(&timer_text, text_scale);
                let spacing = 4.0;
                let total_right_width = dot_width + spacing + timer_width;
                let right_start_x =
                    bar_x as f32 + bar_width as f32 - total_right_width - left_padding;

                self.recording_dot_renderer.render_text(
                    view,
                    encoder,
                    dot_text,
                    right_start_x,
                    bar_y as f32 + text_y_offset,
                    text_scale,
                    dot_color,
                    bar_width,
                    bar_height,
                );

                self.recording_timer_renderer.render_text(
                    view,
                    encoder,
                    &timer_text,
                    right_start_x + dot_width + spacing,
                    bar_y as f32 + text_y_offset,
                    text_scale,
                    timer_color,
                    bar_width,
                    bar_height,
                );
            }
        }
    }
}
