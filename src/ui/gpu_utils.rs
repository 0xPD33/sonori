use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
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

/// Opens the device every Sonori surface shares. The settings panel uses push
/// constants, so any device that may host it needs them.
pub fn request_device(adapter: &wgpu::Adapter) -> Result<(wgpu::Device, wgpu::Queue), String> {
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: None,
        required_features: wgpu::Features::PUSH_CONSTANTS,
        required_limits: wgpu::Limits {
            max_push_constant_size: 128,
            ..wgpu::Limits::default()
        },
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
    }))
    .map_err(|e| {
        format!("Failed to open the GPU device ({e}). Check that Vulkan drivers are installed.")
    })
}

/// Set once the GPU device is lost, for example after a driver reset. Every
/// surface then stops drawing, and the app builds a new device and windows.
#[derive(Clone, Default)]
pub struct DeviceLost(Arc<AtomicBool>);

impl DeviceLost {
    pub fn get(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// `SONORI_TEST_GPU_LOSS=<seconds>` destroys the first device after that many
/// seconds, so the VM test can check the recovery.
fn lose_for_test(device: &wgpu::Device) {
    static ARMED: AtomicBool = AtomicBool::new(false);
    let Some(secs) = std::env::var("SONORI_TEST_GPU_LOSS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
    else {
        return;
    };
    if ARMED.swap(true, Ordering::Relaxed) {
        return;
    }
    let device = device.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(secs));
        eprintln!("Test: destroying the GPU device");
        device.destroy();
        // A destroyed device reports the loss on its next poll.
        let _ = device.poll(wgpu::PollType::Poll);
    });
}

/// The one GPU instance and device of the app; every window draws with it.
/// Windows come and go as the display mode changes. When each had its own
/// Vulkan instance, dropping one crashed the NVIDIA driver in another's present.
#[derive(Clone)]
pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub lost: DeviceLost,
}

impl Gpu {
    pub fn new() -> Result<Self, String> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .map_err(|e| {
            format!("No suitable GPU adapter found ({e}). Sonori needs Vulkan drivers.")
        })?;
        let (device, queue) = request_device(&adapter)?;
        // A drawing bug must not end the dictation app.
        device.on_uncaptured_error(Box::new(|e| eprintln!("GPU error: {e}")));
        let lost = DeviceLost::default();
        let flag = lost.0.clone();
        device.set_device_lost_callback(move |reason, message| {
            eprintln!("GPU device lost ({reason:?}): {message}");
            flag.store(true, Ordering::Release);
        });
        lose_for_test(&device);
        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            lost,
        })
    }
}
