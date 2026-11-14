//! Signed-distance shapes in the style of the Sonori icon: frosted glass, silver
//! capsules, control surfaces, and line icons, drawn by one instanced pipeline.
//! Cursor mode and the Settings window draw with them; see `shapes.wgsl`.

use std::ops::Range;

use bytemuck::{Pod, Zeroable};

use super::text_renderer::{Label, TextRenderer, LABEL_LINE};

/// One quad and the signed-distance shape the fragment shader draws in it.
/// `params[0]` selects the shape; see `shapes.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Shape {
    pub rect: [f32; 4],
    pub color: [f32; 4],
    pub params: [f32; 4],
    pub extra: [f32; 4],
}

// Shape kinds.
pub const OUTLINE: f32 = 0.0;
pub const GLASS_TILE: f32 = 1.0;
pub const SILVER_BAR: f32 = 2.0;
pub const PANEL: f32 = 3.0;
pub const ICON: f32 = 4.0;

// Panel styles.
pub const FLAT: f32 = 0.0;
pub const RAISED: f32 = 1.0;
pub const SUNK: f32 = 2.0;
pub const SLATE_FILL: f32 = 3.0;
pub const HAIRLINE: f32 = 4.0;

// Icons.
pub const GLOBE: f32 = 0.0;
pub const SPARKLES: f32 = 1.0;
pub const PAGE: f32 = 2.0;
pub const BARS: f32 = 3.0;
pub const POINTER: f32 = 4.0;
pub const GEAR: f32 = 5.0;
pub const CHEVRON: f32 = 6.0;
pub const TICK: f32 = 7.0;
pub const MIC: f32 = 8.0;
pub const WORDS: f32 = 9.0;
pub const CROSS: f32 = 10.0;

// Text colours of the icon palette.
pub const INK: [f32; 3] = [0.125, 0.165, 0.224];
pub const INK_2: [f32; 3] = [0.357, 0.404, 0.475];
pub const SLATE: [f32; 3] = [0.231, 0.29, 0.376];

/// A frosted glass tile centred on (`cx`, `cy`) with half-size (`hw`, `hh`);
/// `margin` leaves room for its shadow, `glow` lights its edge.
#[allow(clippy::too_many_arguments)]
pub fn glass(
    cx: f32,
    cy: f32,
    hw: f32,
    hh: f32,
    corner: f32,
    margin: f32,
    fill_alpha: f32,
    glow: f32,
    opacity: f32,
) -> Shape {
    Shape {
        rect: [
            cx - hw - margin,
            cy - hh - margin,
            2.0 * (hw + margin),
            2.0 * (hh + margin),
        ],
        color: [1.0, 1.0, 1.0, opacity],
        params: [GLASS_TILE, corner.min(hw).min(hh), margin, glow],
        extra: [fill_alpha, 0.0, 0.0, 0.0],
    }
}

/// A damped spring, stepped with semi-implicit Euler in small slices for stability.
#[derive(Debug, Clone, Copy, Default)]
pub struct Spring {
    pub x: f32,
    pub v: f32,
}

impl Spring {
    pub fn at(x: f32) -> Self {
        Self { x, v: 0.0 }
    }

    pub fn step(&mut self, target: f32, stiffness: f32, damping: f32, dt: f32) {
        let slices = (dt / (1.0 / 240.0)).ceil().max(1.0);
        let h = dt / slices;
        for _ in 0..slices as u32 {
            self.v += (stiffness * (target - self.x) - damping * self.v) * h;
            self.x += self.v * h;
        }
    }

    /// Whether the spring rests at `target`, to within `tolerance`.
    pub fn settled(&self, target: f32, tolerance: f32) -> bool {
        (self.x - target).abs() < tolerance && self.v.abs() < tolerance * 10.0
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    screen: [f32; 2],
    time: f32,
    alpha: f32,
}

/// The pipeline and buffers that draw `Shape`s.
pub struct ShapeRenderer {
    pipeline: wgpu::RenderPipeline,
    globals: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    shapes: wgpu::Buffer,
    capacity: usize,
}

impl ShapeRenderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Shapes Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shapes.wgsl").into()),
        });
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Shapes Globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let capacity = 128;
        let shapes = Self::shape_buffer(device, capacity);
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Shapes Layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Shapes Bind Group"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Shapes Pipeline Layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Shapes Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Shape>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x4,
                        1 => Float32x4,
                        2 => Float32x4,
                        3 => Float32x4
                    ],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        Self {
            pipeline,
            globals,
            bind_group,
            shapes,
            capacity,
        }
    }

    fn shape_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Shapes"),
            size: (capacity * std::mem::size_of::<Shape>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Writes the frame's shapes and the surface size in physical px. Call it
    /// once per frame, before any `draw`.
    pub fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen: [f32; 2],
        time: f32,
        shapes: &[Shape],
    ) {
        if shapes.len() > self.capacity {
            self.capacity = shapes.len().next_power_of_two();
            self.shapes = Self::shape_buffer(device, self.capacity);
        }
        let globals = Globals {
            screen,
            time,
            alpha: 1.0,
        };
        queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&globals));
        if !shapes.is_empty() {
            queue.write_buffer(&self.shapes, 0, bytemuck::cast_slice(shapes));
        }
    }

    /// Draws the uploaded shapes in `range`.
    pub fn draw(&self, pass: &mut wgpu::RenderPass, range: Range<u32>) {
        if range.is_empty() {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.shapes.slice(..));
        pass.draw(0..6, range);
    }
}

/// Drawing helpers for one frame. Sizes are in logical px; `sf` scales them
/// to the physical px that positions use.
pub struct Ui<'a> {
    pub sf: f32,
    pub alpha: f32,
    pub shapes: &'a mut Vec<Shape>,
    pub labels: &'a mut Vec<Label>,
    pub text: &'a mut TextRenderer,
    /// Labels outside this rectangle (physical px) are cut off.
    pub clip: Option<[f32; 4]>,
}

impl Ui<'_> {
    pub fn measure(&mut self, text: &str, size: f32, bold: bool) -> f32 {
        self.text.measure_label(text, size * self.sf, bold)
    }

    /// A label vertically centred on `cy`.
    #[allow(clippy::too_many_arguments)]
    pub fn label(
        &mut self,
        text: &str,
        x: f32,
        cy: f32,
        size: f32,
        bold: bool,
        color: [f32; 3],
        opacity: f32,
    ) {
        let top = cy - size * self.sf * LABEL_LINE / 2.0;
        self.label_top(text, x, top, size, bold, color, opacity);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn label_top(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        size: f32,
        bold: bool,
        color: [f32; 3],
        opacity: f32,
    ) {
        if opacity * self.alpha <= 0.0 {
            return;
        }
        self.labels.push(Label {
            text: text.to_string(),
            x,
            y,
            px: size * self.sf,
            color: [color[0], color[1], color[2], 0.96 * opacity * self.alpha],
            bold,
            clip: self.clip,
        });
    }

    /// Icon `id` in a `size` logical px square centred on (`cx`, `cy`).
    #[allow(clippy::too_many_arguments)]
    pub fn icon(
        &mut self,
        id: f32,
        cx: f32,
        cy: f32,
        size: f32,
        angle: f32,
        color: [f32; 3],
        opacity: f32,
    ) {
        let half = size * self.sf / 2.0;
        self.shapes.push(Shape {
            rect: [cx - half, cy - half, 2.0 * half, 2.0 * half],
            color: [color[0], color[1], color[2], opacity * self.alpha],
            params: [ICON, id, 0.0, angle],
            extra: [0.0; 4],
        });
    }

    pub fn panel(&mut self, rect: [f32; 4], corner: f32, style: f32, opacity: f32) {
        if opacity <= 0.0 {
            return;
        }
        let m = if style == RAISED { 3.0 * self.sf } else { 0.0 };
        self.shapes.push(Shape {
            rect: [
                rect[0] - m,
                rect[1] - m,
                rect[2] + 2.0 * m,
                rect[3] + 2.0 * m,
            ],
            color: [1.0, 1.0, 1.0, opacity * self.alpha],
            params: [PANEL, corner * self.sf, style, m],
            extra: [0.0; 4],
        });
    }

    /// A segmented control ending at `right`; returns the button rectangles.
    pub fn segmented(
        &mut self,
        names: &[&str],
        selected: Option<usize>,
        pill: [Spring; 2],
        right: f32,
        cy: f32,
    ) -> Vec<[f32; 4]> {
        let sf = self.sf;
        let widths: Vec<f32> = names
            .iter()
            .map(|n| self.measure(n, 12.5, true) + 24.0 * sf)
            .collect();
        let button_h = 27.0 * sf;
        let track_w = widths.iter().sum::<f32>() + 2.0 * sf * (names.len() - 1) as f32 + 6.0 * sf;
        let track = [
            right - track_w,
            cy - button_h / 2.0 - 3.0 * sf,
            track_w,
            button_h + 6.0 * sf,
        ];
        self.panel(track, 11.0, SUNK, 1.0);
        if selected.is_some() && pill[1].x > 0.0 {
            self.panel(
                [pill[0].x, cy - button_h / 2.0, pill[1].x, button_h],
                8.0,
                RAISED,
                1.0,
            );
        }
        let mut x = track[0] + 3.0 * sf;
        let mut buttons = Vec::new();
        for (i, (name, w)) in names.iter().zip(&widths).enumerate() {
            let color = if selected == Some(i) { INK } else { INK_2 };
            self.label(name, x + 12.0 * sf, cy, 12.5, true, color, 1.0);
            buttons.push([x, cy - button_h / 2.0, *w, button_h]);
            x += w + 2.0 * sf;
        }
        buttons
    }
}
