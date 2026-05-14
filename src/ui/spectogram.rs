use rustfft::{num_complex::Complex, FftPlanner};
use std::sync::Arc;
use std::time::Instant;
use wgpu::{util::DeviceExt, Buffer, Device, Queue, RenderPipeline, TextureView};
use winit::dpi::PhysicalSize;

/// Configuration for spectrogram visualization parameters
#[derive(Debug, Clone)]
pub struct SpectrogramConfig {
    pub skin: crate::config::SpectrogramSkin,
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
    pub bar_spacing_multiplier: f32,
    pub bar_color: [f32; 4],
}

impl Default for SpectrogramConfig {
    fn default() -> Self {
        Self {
            skin: crate::config::SpectrogramSkin::Bars,
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
            bar_spacing_multiplier: 1.0,
            bar_color: [1.0, 1.0, 1.0, 1.0],
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
    cached_instances: Vec<BarInstance>,
}

/// Internal structure for pre-computing bar instance properties
#[derive(Clone, Debug)]
struct BarInstanceTemplate {
    _position_factor: f32, // Position factor for edge tapering
    edge_factor: f32,      // Pre-computed edge tapering factor
    norm_x: f32,           // Normalized x position
    norm_width: f32,       // Normalized width
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

        let config = SpectrogramConfig::default();
        let bar_instance_template = create_bar_instance_template(num_bins, size.width, &config);

        let mut cached_instances = Vec::with_capacity(num_bins);
        fill_bar_instances(
            &bar_data,
            &bar_instance_template,
            size.height,
            &config,
            &mut cached_instances,
        );

        let instance_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Instance Buffer"),
            contents: bytemuck::cast_slice(&cached_instances),
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

        Self {
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
            cached_instances,
        }
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

                self.bar_instance_template =
                    create_bar_instance_template(optimal_bins, new_size.width, &self.config);

                self.cached_instances.clear();
                self.cached_instances.reserve(optimal_bins);
            }
        }

        self.size = new_size;
        self.update_instance_buffer();
    }

    pub fn apply_ui_config(&mut self, ui_config: &crate::config::UiConfig) {
        self.config.bar_color = ui_config.effective_spectrogram_color();
        self.config.skin = ui_config.spectrogram_skin;
        apply_skin_config(&mut self.config);
        let new_template =
            create_bar_instance_template(self.bar_data.len(), self.size.width, &self.config);
        self.bar_instance_template = new_template;
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
            let count = audio_samples.len().div_ceil(sample_step);

            for i in (0..audio_samples.len()).step_by(sample_step) {
                sum += audio_samples[i].abs();
            }

            sum / count as f32
        };

        self.is_speaking = audio_energy > self.config.speaking_threshold;

        // Pre-allocate a working buffer for smoothing to avoid allocation in hot path
        let mut smoothed_data = std::mem::take(&mut self.target_bar_data);
        smoothed_data.resize(num_bars, 0.0);

        // Process audio samples to calculate bar heights. Each visual bar gets a
        // small bucket of raw samples so skins can use peak/RMS dynamics without
        // depending on one arbitrary sample.
        for (i, value) in smoothed_data.iter_mut().enumerate().take(num_bars) {
            let (start_idx, end_idx) = sample_range_for_bar(i, num_bars, audio_samples.len());
            let stats = audio_bucket_stats(&audio_samples[start_idx..end_idx]);

            *value = match self.config.skin {
                crate::config::SpectrogramSkin::Waveform => {
                    let sign = if stats.signed_peak >= 0.0 { 1.0 } else { -1.0 };
                    let envelope = stats.rms * 0.70 + stats.peak_abs * 0.30;
                    let shaped = (envelope * self.config.sample_amplification).sqrt()
                        * self.config.scaled_amplification;
                    (sign * shaped).clamp(-self.config.max_bar_height, self.config.max_bar_height)
                }
                crate::config::SpectrogramSkin::Meter => {
                    let envelope = stats.peak_abs * 0.78 + stats.rms * 0.22;
                    (envelope.sqrt() * self.config.scaled_amplification)
                        .min(self.config.max_bar_height)
                        .max(self.config.min_amplitude)
                }
                crate::config::SpectrogramSkin::Bars => {
                    let envelope = stats.avg_abs * 0.55 + stats.rms * 0.30 + stats.peak_abs * 0.15;
                    (envelope.sqrt() * self.config.scaled_amplification)
                        .min(self.config.max_bar_height)
                        .max(self.config.min_amplitude)
                }
            };
        }

        // Apply smoothing without cloning the entire array
        // Handle edge cases separately
        if num_bars > 2 {
            // Apply filter to first element
            let first = smoothed_data[0] * self.config.current_bar_weight
                + smoothed_data[1] * self.config.next_bar_weight;

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
            (
                self.config.animation_speed * 4.0,
                self.config.animation_speed * 2.0,
                0.0,
            )
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
            if self.config.skin == crate::config::SpectrogramSkin::Waveform {
                *bar = (*bar).clamp(-self.config.max_amplitude, self.config.max_amplitude);
            } else {
                *bar = (*bar).clamp(self.config.min_amplitude, self.config.max_amplitude);
            }
        }

        self.update_instance_buffer();
    }

    fn update_instance_buffer(&mut self) {
        fill_bar_instances(
            &self.bar_data,
            &self.bar_instance_template,
            self.size.height,
            &self.config,
            &mut self.cached_instances,
        );
        self.queue.write_buffer(
            &self.instance_buffer,
            0,
            bytemuck::cast_slice(&self.cached_instances),
        );
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
fn create_bar_instance_template(
    num_bars: usize,
    width: u32,
    config: &SpectrogramConfig,
) -> Vec<BarInstanceTemplate> {
    let total_width = width as f32;
    let bar_width = total_width / num_bars as f32;

    // Calculate spacing dynamically based on number of bars
    let spacing_factor =
        (0.2 * (50.0 / num_bars as f32) * config.bar_spacing_multiplier).clamp(0.03, 0.70);
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

fn fill_bar_instances(
    bar_data: &[f32],
    templates: &[BarInstanceTemplate],
    height: u32,
    config: &SpectrogramConfig,
    instances: &mut Vec<BarInstance>,
) {
    instances.clear();
    instances.reserve(bar_data.len());

    for (&amplitude, template) in bar_data.iter().zip(templates.iter()) {
        let adjusted_amplitude = amplitude * template.edge_factor;

        let bar_height = (adjusted_amplitude.abs() * height as f32).max(2.0);

        let norm_height = bar_height / height as f32 * 2.0;
        let norm_y = match config.skin {
            crate::config::SpectrogramSkin::Bars => {
                (height as f32 - bar_height) / (2.0 * height as f32) * 2.0 - 1.0
            }
            crate::config::SpectrogramSkin::Waveform => {
                if adjusted_amplitude >= 0.0 {
                    0.0
                } else {
                    -norm_height
                }
            }
            crate::config::SpectrogramSkin::Meter => -1.0,
        };

        let color = [
            config.bar_color[0],
            config.bar_color[1],
            config.bar_color[2],
            adjusted_amplitude.abs().max(config.min_opacity) * config.bar_color[3],
        ];

        instances.push(BarInstance {
            position: [template.norm_x, norm_y],
            size: [template.norm_width, norm_height],
            color,
        });
    }
}

fn apply_skin_config(config: &mut SpectrogramConfig) {
    match config.skin {
        crate::config::SpectrogramSkin::Bars => {
            config.animation_speed = 0.85;
            config.min_amplitude = 0.025;
            config.max_bar_height = 0.9;
            config.sample_amplification = 1.1;
            config.scaled_amplification = 1.5;
            config.prev_bar_weight = 0.2;
            config.current_bar_weight = 0.6;
            config.next_bar_weight = 0.2;
            config.min_edge_factor = 0.75;
            config.edge_factor_range = 0.25;
            config.bar_spacing_multiplier = 1.0;
        }
        crate::config::SpectrogramSkin::Waveform => {
            config.animation_speed = 1.15;
            config.min_amplitude = 0.0;
            config.max_bar_height = 0.78;
            config.sample_amplification = 1.9;
            config.scaled_amplification = 1.08;
            config.prev_bar_weight = 0.16;
            config.current_bar_weight = 0.68;
            config.next_bar_weight = 0.16;
            config.min_edge_factor = 1.0;
            config.edge_factor_range = 0.0;
            config.bar_spacing_multiplier = 0.35;
        }
        crate::config::SpectrogramSkin::Meter => {
            config.animation_speed = 1.4;
            config.min_amplitude = 0.015;
            config.max_bar_height = 0.95;
            config.sample_amplification = 1.7;
            config.scaled_amplification = 2.1;
            config.prev_bar_weight = 0.08;
            config.current_bar_weight = 0.84;
            config.next_bar_weight = 0.08;
            config.min_edge_factor = 0.95;
            config.edge_factor_range = 0.05;
            config.bar_spacing_multiplier = 3.0;
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct AudioBucketStats {
    avg_abs: f32,
    rms: f32,
    peak_abs: f32,
    signed_peak: f32,
}

fn sample_range_for_bar(index: usize, bar_count: usize, sample_count: usize) -> (usize, usize) {
    if sample_count == 0 || bar_count == 0 {
        return (0, 0);
    }

    let start = index * sample_count / bar_count;
    let mut end = (index + 1) * sample_count / bar_count;
    if end <= start {
        end = start + 1;
    }
    (start.min(sample_count), end.min(sample_count))
}

fn audio_bucket_stats(samples: &[f32]) -> AudioBucketStats {
    if samples.is_empty() {
        return AudioBucketStats::default();
    }

    let mut sum_abs = 0.0;
    let mut sum_sq = 0.0;
    let mut signed_peak = 0.0f32;

    for &sample in samples {
        let abs = sample.abs();
        sum_abs += abs;
        sum_sq += sample * sample;
        if abs > signed_peak.abs() {
            signed_peak = sample;
        }
    }

    let len = samples.len() as f32;
    AudioBucketStats {
        avg_abs: sum_abs / len,
        rms: (sum_sq / len).sqrt(),
        peak_abs: signed_peak.abs(),
        signed_peak,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_range_for_bar_is_non_empty_for_sparse_samples() {
        assert_eq!(sample_range_for_bar(0, 4, 2), (0, 1));
        assert_eq!(sample_range_for_bar(3, 4, 2), (1, 2));
    }

    #[test]
    fn audio_bucket_stats_keeps_dominant_signed_peak() {
        let stats = audio_bucket_stats(&[-0.2, 0.1, 0.35, -0.25]);

        assert_eq!(stats.signed_peak, 0.35);
        assert_eq!(stats.peak_abs, 0.35);
        assert!(stats.rms > stats.avg_abs * 0.9);
    }
}
