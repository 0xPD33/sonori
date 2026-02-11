use rustfft::{num_complex::Complex, FftPlanner};
use std::sync::Arc;
use std::time::Instant;
use wgpu::{util::DeviceExt, Buffer, Device, Queue, RenderPipeline, TextureView};
use winit::dpi::PhysicalSize;


/// Configuration for spectrogram visualization parameters
#[derive(Debug, Clone)]
pub struct SpectrogramConfig {
    pub fft_size: usize,
    pub animation_speed: f32,
    pub min_amplitude: f32,
    pub max_amplitude: f32,
    pub speaking_threshold: f32,
    pub min_opacity: f32,
    pub max_bar_height: f32,
    pub sample_amplification: f32,
    pub scaled_amplification: f32,
    pub min_diff_threshold: f32,
    pub prev_bar_weight: f32,
    pub current_bar_weight: f32,
    pub next_bar_weight: f32,
    pub min_edge_factor: f32,
    pub edge_factor_range: f32,
}

impl Default for SpectrogramConfig {
    fn default() -> Self {
        Self {
            fft_size: 512,
            animation_speed: 0.85,
            min_amplitude: 0.025,
            max_amplitude: 1.0,
            speaking_threshold: 0.2,
            min_opacity: 0.15,
            max_bar_height: 0.9,
            sample_amplification: 1.1,
            scaled_amplification: 1.5,
            min_diff_threshold: 0.001,
            prev_bar_weight: 0.2,
            current_bar_weight: 0.6,
            next_bar_weight: 0.2,
            min_edge_factor: 0.75,
            edge_factor_range: 0.25,
        }
    }
}

// Legacy constants for backward compatibility (will be removed in future)
const FFT_SIZE: usize = 512;

pub struct Spectrogram {
    // Configuration
    config: SpectrogramConfig,

    // WGPU resources
    _device: Arc<Device>,
    queue: Arc<Queue>,
    render_pipeline: RenderPipeline,
    vertex_buffer: Buffer,
    instance_buffer: Buffer,

    // Spectrogram data
    bar_data: Vec<f32>,
    target_bar_data: Vec<f32>,
    size: PhysicalSize<u32>,

    // Animation state
    last_update: Instant,
    is_speaking: bool,

    // FFT resources
    _fft: Arc<dyn rustfft::Fft<f32>>,
    _fft_input: Vec<Complex<f32>>,
    _fft_output: Vec<Complex<f32>>,
    _window: Vec<f32>, // Hann window for better frequency resolution

    // Performance optimization: cached values
    bar_instance_template: Vec<BarInstanceTemplate>,
}

/// Internal structure for pre-computing bar instance properties
#[derive(Clone, Debug)]
struct BarInstanceTemplate {
    _position_factor: f32, // Position factor for edge tapering
    edge_factor: f32,     // Pre-computed edge tapering factor
    norm_x: f32,          // Normalized x position
    norm_width: f32,      // Normalized width
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 2],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct BarInstance {
    position: [f32; 2],
    size: [f32; 2],
    color: [f32; 4],
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

impl BarInstance {
    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<BarInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 4]>() as wgpu::BufferAddress,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Float32x4,
                },
            ],
        }
    }
}

impl Spectrogram {
    pub fn new(
        device: Arc<Device>,
        queue: Arc<Queue>,
        size: PhysicalSize<u32>,
        surface_format: wgpu::TextureFormat,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Spectrogram Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("spectogram.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Spectrogram Pipeline Layout"),
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Spectrogram Render Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Vertex::desc(), BarInstance::desc()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::SrcAlpha,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
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

        // Define square vertices for each bar (same for all instances)
        let vertices = [
            Vertex {
                position: [0.0, 0.0],
            },
            Vertex {
                position: [1.0, 0.0],
            },
            Vertex {
                position: [0.0, 1.0],
            },
            Vertex {
                position: [1.0, 1.0],
            },
        ];

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Vertex Buffer"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let num_bins = size.width as usize;
        let bar_data = vec![0.0; num_bins];
        let target_bar_data = vec![0.0; num_bins];

        // Pre-compute bar instance templates
        let config = SpectrogramConfig::default();
        let bar_instance_template = create_bar_instance_template(num_bins, size.width, &config);

        let instances = create_bar_instances(&bar_data, &bar_instance_template, size.height, &config);
        let instance_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Instance Buffer"),
            contents: bytemuck::cast_slice(&instances),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        });

        // Setup FFT processing
        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        let fft_input = vec![Complex { re: 0.0, im: 0.0 }; FFT_SIZE];
        let fft_output = vec![Complex { re: 0.0, im: 0.0 }; FFT_SIZE];

        // Pre-compute Hann window coefficients
        // The Hann window function is applied to audio samples to reduce spectral leakage
        // in the frequency domain. The formula is 0.5 * (1 - cos(2π * i / (N-1)))
        let window = (0..FFT_SIZE)
            .map(|i| {
                0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (FFT_SIZE - 1) as f32).cos())
            })
            .collect();

        let spectrogram = Self {
            config: SpectrogramConfig::default(),
            _device: device,
            queue,
            render_pipeline,
            vertex_buffer,
            instance_buffer,
            bar_data,
            target_bar_data,
            size,
            last_update: Instant::now(),
            is_speaking: false,
            _fft: fft,
            _fft_input: fft_input,
            _fft_output: fft_output,
            _window: window,
            bar_instance_template,
        };

        spectrogram
    }

    pub fn resize(&mut self, new_size: PhysicalSize<u32>) {
        if self.size.width != new_size.width {
            let optimal_bins = new_size.width as usize;

            if self.bar_data.len() != optimal_bins {
                // Resize bar data vectors while maintaining relative values
                let mut new_bar_data = vec![0.0; optimal_bins];
                let mut new_target_data = vec![0.0; optimal_bins];

                let old_len = self.bar_data.len();
                let scale_factor = old_len as f32 / optimal_bins as f32;

                for i in 0..optimal_bins {
                    let old_idx = (i as f32 * scale_factor) as usize;
                    if old_idx < old_len {
                        new_bar_data[i] = self.bar_data[old_idx];
                        new_target_data[i] = self.target_bar_data[old_idx];
                    }
                }

                self.bar_data = new_bar_data;
                self.target_bar_data = new_target_data;

                // Update the bar instance template for the new width
                self.bar_instance_template =
                    create_bar_instance_template(optimal_bins, new_size.width, &self.config);
            }
        }

        self.size = new_size;
        self.update_instance_buffer();
    }

    /// Processes audio samples and updates the target bar heights
    ///
    /// This is a key performance-critical function that converts audio samples
    /// into spectrogram bar heights.
    pub fn update(&mut self, audio_samples: &[f32]) {
        let num_bars = self.bar_data.len();

        // Check if we have silent audio (all zeros or empty)
        let is_silent = audio_samples.is_empty() || audio_samples.iter().all(|&x| x == 0.0);

        if is_silent {
            self.is_speaking = false;
            // Set target bars to zero for decay animation
            self.target_bar_data.fill(0.0);
            self.animate_bars();
            return;
        }

        let audio_energy = {
            let sample_step = (audio_samples.len() / 20).max(1);
            let mut sum = 0.0;
            let count = (audio_samples.len() + sample_step - 1) / sample_step; // Ceiling division

            for i in (0..audio_samples.len()).step_by(sample_step) {
                sum += audio_samples[i].abs();
            }

            sum / count as f32
        };

        self.is_speaking = audio_energy > self.config.speaking_threshold;

        // Pre-allocate a working buffer for smoothing to avoid allocation in hot path
        let mut smoothed_data = std::mem::take(&mut self.target_bar_data);
        smoothed_data.resize(num_bars, 0.0);

        // Process audio samples to calculate bar heights
        if audio_samples.len() < num_bars {
            // Optimize for fewer samples than bars
            let step = audio_samples.len().max(1) / num_bars.max(1);

            for i in 0..num_bars {
                let idx = (i * step).min(audio_samples.len().saturating_sub(1));
                let sample = if idx < audio_samples.len() {
                    audio_samples[idx].abs()
                } else {
                    0.0
                };

                // Apply a non-linear scaling (capped at MAX_BAR_HEIGHT)
                smoothed_data[i] = (sample * self.config.sample_amplification).min(self.config.max_bar_height);
            }
        } else {
            // Optimize for more samples than bars
            let samples_per_bar = audio_samples.len() / num_bars;

            for i in 0..num_bars {
                let start_idx = i * samples_per_bar;
                let end_idx = ((i + 1) * samples_per_bar).min(audio_samples.len());
                let segment_len = end_idx - start_idx;

                if segment_len > 0 {
                    // Use iterator for better optimization potential
                    let sum: f32 = audio_samples[start_idx..end_idx]
                        .iter()
                        .map(|&x| x.abs())
                        .sum();

                    let avg = sum / segment_len as f32;

                    // Apply non-linear scaling
                    smoothed_data[i] = (avg.sqrt() * self.config.scaled_amplification).min(self.config.max_bar_height);
                } else {
                    smoothed_data[i] = self.config.min_amplitude;
                }
            }
        }

        // Apply smoothing without cloning the entire array
        // Handle edge cases separately
        if num_bars > 2 {
            // Apply filter to first element
            let first = smoothed_data[0] * self.config.current_bar_weight + smoothed_data[1] * self.config.next_bar_weight;

            // Apply filter to last element
            let last = smoothed_data[num_bars - 2] * self.config.prev_bar_weight
                + smoothed_data[num_bars - 1] * self.config.current_bar_weight;

            // Save temporary values for each bar to avoid allocation
            let mut prev_val = smoothed_data[0];
            let mut curr_val = smoothed_data[1];

            // Apply in-place smoothing for middle elements
            for i in 1..num_bars - 1 {
                let next_val = if i + 1 < num_bars {
                    smoothed_data[i + 1]
                } else {
                    0.0
                };
                let smoothed = prev_val * self.config.prev_bar_weight
                    + curr_val * self.config.current_bar_weight
                    + next_val * self.config.next_bar_weight;

                prev_val = curr_val;
                curr_val = next_val;
                smoothed_data[i] = smoothed;
            }

            // Set edge values
            smoothed_data[0] = first;
            smoothed_data[num_bars - 1] = last;
        }

        // Swap back the buffer
        self.target_bar_data = smoothed_data;

        self.animate_bars();
    }

    /// Animates bar heights toward their target values with appropriate easing
    fn animate_bars(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_update).as_secs_f32();
        self.last_update = now;

        // Cap delta time
        let capped_dt = dt.min(0.1);

        // Pre-compute animation parameters based on speaking state
        // Different animation speeds are used when speaking vs silent
        // to create a more natural-looking visualization
        let (rise_speed, fall_speed, idle_decay) = if self.is_speaking {
            // When speaking: fast rise, moderate fall
            (self.config.animation_speed * 4.0, self.config.animation_speed * 2.0, 0.0)
        } else {
            // When silent: gentle decay toward minimum
            (
                self.config.animation_speed * 2.0,
                self.config.animation_speed * 3.0,
                self.config.animation_speed * 0.5,
            )
        };

        // Pre-compute common factors to avoid redundant calculations
        let rise_factor = rise_speed * capped_dt;
        let fall_factor = fall_speed * capped_dt;
        let decay_factor = 1.0 - (idle_decay * capped_dt);

        // Update all bars in a single pass
        for (i, bar) in self.bar_data.iter_mut().enumerate() {
            let target = self.target_bar_data[i];
            let diff = target - *bar;

            if self.is_speaking {
                // When speaking, use asymmetric animation speeds for rise/fall
                let speed_factor = if diff > 0.0 { rise_factor } else { fall_factor };
                *bar += diff * speed_factor;
            } else {
                // When silent, animate toward minimum with gentle decay
                if diff.abs() > self.config.min_diff_threshold {
                    *bar += diff * fall_factor;
                } else {
                    // Apply exponential decay
                    *bar *= decay_factor;
                    *bar = (*bar).max(self.config.min_amplitude);
                }
            }

            // Keep values in valid range
            *bar = (*bar).clamp(self.config.min_amplitude, self.config.max_amplitude);
        }

        self.update_instance_buffer();
    }

    /// Updates GPU buffer with current bar instance data
    fn update_instance_buffer(&mut self) {
        let instances = create_bar_instances(
            &self.bar_data,
            &self.bar_instance_template,
            self.size.height,
            &self.config,
        );
        self.queue
            .write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(&instances));
    }

    pub fn render(&self, view: &TextureView, encoder: &mut wgpu::CommandEncoder) {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Spectrogram Render Pass"),
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
        render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        render_pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
        render_pass.draw(0..4, 0..self.bar_data.len() as u32);
    }

    pub fn render_with_custom_pass<'a, 'b>(&'a self, render_pass: &mut wgpu::RenderPass<'b>)
    where
        'a: 'b,
    {
        render_pass.set_pipeline(&self.render_pipeline);
        render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        render_pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
        render_pass.draw(0..4, 0..self.bar_data.len() as u32);
    }

    pub fn animate_and_render(&mut self, view: &TextureView, encoder: &mut wgpu::CommandEncoder) {
        self.animate_bars();
        self.render(view, encoder);
    }
}

/// Pre-computes bar instance template data to avoid recalculations
///
/// This function calculates position-dependent values that don't change
/// with bar height, significantly reducing per-frame calculations.
fn create_bar_instance_template(num_bars: usize, width: u32, config: &SpectrogramConfig) -> Vec<BarInstanceTemplate> {
    let total_width = width as f32;
    let bar_width = total_width / num_bars as f32;

    // Calculate spacing dynamically based on number of bars
    let spacing_factor = (0.2 * (50.0 / num_bars as f32)).clamp(0.05, 0.2);
    let bar_spacing = bar_width * spacing_factor;
    let actual_bar_width = bar_width - bar_spacing;

    // Pre-compute normalized width to avoid repeated division
    let norm_width = actual_bar_width / width as f32 * 2.0;

    (0..num_bars)
        .map(|i| {
            let x = i as f32 * bar_width;
            let position_factor = i as f32 / (num_bars - 1) as f32;

            // Edge tapering creates a bell curve effect for the visualization
            // with bars at the center being taller than those at the edges
            let edge_factor = config.min_edge_factor
                + config.edge_factor_range * (std::f32::consts::PI * (position_factor - 0.5)).cos();

            // Pre-compute normalized X position to avoid division later
            let norm_x = x / width as f32 * 2.0 - 1.0;

            BarInstanceTemplate {
                _position_factor: position_factor,
                edge_factor,
                norm_x,
                norm_width,
            }
        })
        .collect()
}

/// Creates bar instances for rendering based on current amplitude values
/// and pre-computed template data
fn create_bar_instances(
    bar_data: &[f32],
    templates: &[BarInstanceTemplate],
    height: u32,
    config: &SpectrogramConfig,
) -> Vec<BarInstance> {
    bar_data
        .iter()
        .zip(templates.iter())
        .map(|(&amplitude, template)| {
            // Apply edge tapering using pre-computed factor
            let adjusted_amplitude = amplitude * template.edge_factor;

            // Calculate bar height with minimum height of 2 pixels
            let bar_height = (adjusted_amplitude * height as f32).max(2.0);

            // Calculate normalized Y position
            let norm_y = (height as f32 - bar_height) / (2.0 * height as f32) * 2.0 - 1.0;
            let norm_height = bar_height / height as f32 * 2.0;

            // Ensure a minimum opacity so bars are always visible
            // Use MIN_OPACITY constant for consistent minimum values
            let color = [1.0, 1.0, 1.0, adjusted_amplitude.max(config.min_opacity)];

            BarInstance {
                position: [template.norm_x, norm_y],
                size: [template.norm_width, norm_height],
                color,
            }
        })
        .collect()
}
