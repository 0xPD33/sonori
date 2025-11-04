use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use wgpu::{self, util::DeviceExt};
use winit::{
    dpi::{PhysicalPosition, PhysicalSize},
    event::{ElementState, MouseButton},
};

use crate::real_time_transcriber::TranscriptionMode;

use super::button_texture::ButtonTexture;

// Button base sizes (will be scaled dynamically)
const COPY_BUTTON_BASE_SIZE: f32 = 16.0; // Base size for scaling calculations
const CLOSE_BUTTON_BASE_SIZE: f32 = 12.0; // Base size for close button (slightly smaller)
const BUTTON_MARGIN_RATIO: f32 = 0.025; // Margin as ratio of window width
const BUTTON_SPACING_RATIO: f32 = 0.02; // Spacing as ratio of window width

// Animation constants
const ANIMATION_DURATION: f32 = 0.15; // Slightly longer for smoother feel
const HOVER_SCALE: f32 = 1.15; // More noticeable hover effect
const PRESS_SCALE: f32 = 0.95; // Less aggressive press for better feel
const HOVER_ROTATION: f32 = 0.261799; // 15 degrees in radians (π/12)
const ANIMATION_SPEED: f32 = 1.0 / ANIMATION_DURATION; // Pre-calculated animation speed

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ButtonType {
    Copy,
    Reset,
    Close,
    Pause,
    Play,

    // Manual mode buttons
    RecordToggle, // Toggle manual recording (play/pause)
    Accept,       // Accept and finish current manual session (texture only, not in layout)
    ModeToggle,   // Switch between real-time/manual modes
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ButtonState {
    Normal,
    Hover,
    Pressed,
}

/// Layout parameters for button positioning and sizing
#[derive(Debug, Clone, Copy)]
struct ButtonLayoutParams {
    regular_button_size: u32,
    close_button_size: u32,
    margin: u32,
    spacing: u32,
}

pub struct Button {
    button_type: ButtonType,
    state: ButtonState,
    position: (u32, u32),
    size: (u32, u32),
    vertices: wgpu::Buffer,
    pipeline: wgpu::RenderPipeline,
    texture: Option<ButtonTexture>,
    animation_progress: f32,
    previous_state: ButtonState,
    animation_active: bool,
    animation_start_time: std::time::Instant,
    scale: f32,
    rotation: f32,
    rotation_buffer: Option<wgpu::Buffer>,
    rotation_bind_group: Option<wgpu::BindGroup>,
}

pub struct ButtonManager {
    buttons: std::collections::HashMap<ButtonType, Button>,
    text_area_height: u32,
    gap: u32,
    active_button: Option<ButtonType>,
    recording: Option<Arc<AtomicBool>>,
    transcription_mode: crate::real_time_transcriber::TranscriptionMode,
    // Texture cache
    copy_texture: Option<ButtonTexture>,
    reset_texture: Option<ButtonTexture>,
    pause_texture: Option<ButtonTexture>,
    play_texture: Option<ButtonTexture>,
    accept_texture: Option<ButtonTexture>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::TextureFormat,
    window_width: u32,
    window_height: u32,
}

impl Button {
    fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        button_type: ButtonType,
        position: (u32, u32),
        size: (u32, u32),
        format: wgpu::TextureFormat,
        texture: Option<ButtonTexture>,
    ) -> Self {
        // Create default texture if none provided and it's not a close button
        let texture_for_button = if texture.is_none() && button_type != ButtonType::Close {
            match ButtonTexture::create_default(device, queue, format) {
                Ok(texture) => Some(texture),
                Err(e) => {
                    println!("Failed to create default texture: {}", e);
                    None
                }
            }
        } else {
            texture
        };

        // Create shader for button
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Button Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("button.wgsl").into()),
        });

        // Create rotation uniform buffer and bind group for shader-based buttons
        let (rotation_buffer, rotation_bind_group) = if button_type == ButtonType::Close
            || button_type == ButtonType::ModeToggle
        {
            // Create rotation uniform buffer (now includes mode for ModeToggle)
            let initial_data = if button_type == ButtonType::ModeToggle {
                [0.0f32, 0.0f32] // rotation, mode (0.0 = RealTime by default)
            } else {
                [0.0f32, 0.0f32] // rotation, unused mode field
            };
            let rotation_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Shader Button Uniform Buffer"),
                contents: bytemuck::cast_slice(&initial_data),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });

            // Create bind group layout with correct visibility for this button type
            let bind_group_visibility = if button_type == ButtonType::ModeToggle {
                // ModeToggle fragment shader needs access to the mode uniform
                wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT
            } else {
                // Close and CancelRecording only need vertex access
                wgpu::ShaderStages::VERTEX
            };

            let bind_group_layout =
                device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    entries: &[wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: bind_group_visibility,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    }],
                    label: Some("Shader Button Bind Group Layout"),
                });

            // Create bind group
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                layout: &bind_group_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: rotation_buffer.as_entire_binding(),
                }],
                label: Some("Shader Button Bind Group"),
            });

            (Some(rotation_buffer), Some(bind_group))
        } else {
            (None, None)
        };

        // Create appropriate pipeline layout based on button type
        let pipeline_layout = if button_type == ButtonType::Close
            || button_type == ButtonType::ModeToggle
        {
            // For shader-based buttons - use the same visibility logic as the bind group
            let pipeline_visibility = if button_type == ButtonType::ModeToggle {
                // ModeToggle fragment shader needs access to the mode uniform
                wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT
            } else {
                // Close and CancelRecording only need vertex access
                wgpu::ShaderStages::VERTEX
            };

            let bind_group_layout =
                device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    entries: &[wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: pipeline_visibility,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    }],
                    label: Some("Shader Button Pipeline Bind Group Layout"),
                });

            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Shader Button Pipeline Layout"),
                bind_group_layouts: &[&bind_group_layout],
                push_constant_ranges: &[],
            })
        } else {
            // For buttons that use textures
            // Get the texture bind group layout for the shader
            let bind_group_layout = if let Some(tex) = &texture_for_button {
                &tex.bind_group_layout
            } else {
                // Create a dummy bind group layout if no texture
                &device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    entries: &[
                        wgpu::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Texture {
                                multisampled: false,
                                view_dimension: wgpu::TextureViewDimension::D2,
                                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 1,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                            count: None,
                        },
                    ],
                    label: Some("button_texture_bind_group_layout"),
                })
            };

            // Create pipeline layout with texture bindings
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Button Pipeline Layout"),
                bind_group_layouts: &[bind_group_layout],
                push_constant_ranges: &[],
            })
        };

        // Create render pipeline
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Button Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: match button_type {
                    ButtonType::Copy => Some("vs_copy"),
                    ButtonType::Reset => Some("vs_reset"),
                    ButtonType::Close => Some("vs_close"),
                    ButtonType::Pause | ButtonType::Play => Some("vs_copy"),
                    ButtonType::RecordToggle => Some("vs_copy"),
                    ButtonType::Accept => Some("vs_copy"), // Use texture-based rendering
                    ButtonType::ModeToggle => Some("vs_close"), // Use close vertex shader
                },
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 8,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: match button_type {
                    ButtonType::Copy => Some("fs_copy"),
                    ButtonType::Reset => Some("fs_reset"),
                    ButtonType::Close => Some("fs_close"),
                    ButtonType::Pause | ButtonType::Play => Some("fs_copy"),
                    ButtonType::RecordToggle => Some("fs_copy"),
                    ButtonType::Accept => Some("fs_copy"), // Use texture-based rendering
                    ButtonType::ModeToggle => Some("fs_mode_toggle"), // Custom shader for R/M text
                },
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

        // Create vertices for button (simple quad)
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Button Vertices"),
            contents: bytemuck::cast_slice(&[
                -1.0f32, -1.0, // top-left
                1.0, -1.0, // top-right
                -1.0, 1.0, // bottom-left
                1.0, 1.0, // bottom-right
            ]),
            usage: wgpu::BufferUsages::VERTEX,
        });

        Self {
            button_type,
            state: ButtonState::Normal,
            position,
            size,
            vertices,
            pipeline,
            texture: texture_for_button,
            animation_progress: 0.0,
            previous_state: ButtonState::Normal,
            animation_active: false,
            animation_start_time: std::time::Instant::now(),
            scale: 1.0,
            rotation: 0.0,
            rotation_buffer,
            rotation_bind_group,
        }
    }

    fn contains_point(&self, x: f64, y: f64) -> bool {
        let (button_x, button_y) = self.position;
        let (button_width, button_height) = self.size;

        x >= button_x as f64
            && x <= (button_x + button_width) as f64
            && y >= button_y as f64
            && y <= (button_y + button_height) as f64
    }

    fn set_state(&mut self, state: ButtonState) {
        if self.state != state {
            // Store previous state for animation transition
            self.previous_state = self.state;
            self.state = state;

            // Start animation
            self.animation_active = true;
            self.animation_start_time = std::time::Instant::now();
            self.animation_progress = 0.0;
        }
    }

    // Simplified update_animation method using smooth interpolation
    fn update_animation(&mut self) {
        if !self.animation_active {
            return;
        }

        // Calculate animation progress with easing
        let elapsed = self.animation_start_time.elapsed().as_secs_f32();
        self.animation_progress = (elapsed * ANIMATION_SPEED).min(1.0);

        if self.animation_progress >= 1.0 {
            self.animation_active = false;
            self.set_final_animation_values();
        } else {
            self.interpolate_animation_values();
        }
    }

    // Set final target values based on current state
    fn set_final_animation_values(&mut self) {
        match (self.button_type, self.state) {
            (ButtonType::Close, ButtonState::Hover) => {
                self.rotation = HOVER_ROTATION;
                self.scale = 1.0;
            }
            (ButtonType::Close, _) => {
                self.rotation = 0.0;
                self.scale = 1.0;
            }
            (_, ButtonState::Hover) => {
                self.scale = HOVER_SCALE;
                self.rotation = 0.0;
            }
            (_, ButtonState::Pressed) => {
                self.scale = PRESS_SCALE;
                self.rotation = 0.0;
            }
            (_, ButtonState::Normal) => {
                self.scale = 1.0;
                self.rotation = 0.0;
            }
        }
    }

    // Smooth interpolation between states
    fn interpolate_animation_values(&mut self) {
        let (target_scale, target_rotation) = self.get_target_values();

        // Simple linear interpolation
        self.scale += (target_scale - self.scale) * 0.2; // Smooth factor
        self.rotation += (target_rotation - self.rotation) * 0.2;
    }

    // Get target values for current state
    fn get_target_values(&self) -> (f32, f32) {
        match (self.button_type, self.state) {
            (ButtonType::Close, ButtonState::Hover) => (1.0, HOVER_ROTATION),
            (ButtonType::Close, _) => (1.0, 0.0),
            (_, ButtonState::Hover) => (HOVER_SCALE, 0.0),
            (_, ButtonState::Pressed) => (PRESS_SCALE, 0.0),
            (_, ButtonState::Normal) => (1.0, 0.0),
        }
    }

    // Update rotation buffer with current rotation and mode values
    fn update_rotation_buffer(&self, queue: &wgpu::Queue, mode: Option<f32>) {
        if let Some(buffer) = &self.rotation_buffer {
            let data = if self.button_type == ButtonType::ModeToggle {
                [self.rotation, mode.unwrap_or(0.0)] // Include mode for ModeToggle
            } else {
                [self.rotation, 0.0] // Only rotation for other buttons
            };
            queue.write_buffer(buffer, 0, bytemuck::cast_slice(&data));
        }
    }

    fn render(
        &self,
        view: &wgpu::TextureView,
        encoder: &mut wgpu::CommandEncoder,
        queue: &wgpu::Queue,
        transcription_mode: Option<crate::real_time_transcriber::TranscriptionMode>,
    ) {
        // Update rotation buffer if needed
        if self.button_type == ButtonType::Close || self.button_type == ButtonType::ModeToggle {
            let mode_value = if self.button_type == ButtonType::ModeToggle {
                transcription_mode.map(|mode| match mode {
                    crate::real_time_transcriber::TranscriptionMode::RealTime => 0.0,
                    crate::real_time_transcriber::TranscriptionMode::Manual => 1.0,
                })
            } else {
                None
            };
            self.update_rotation_buffer(queue, mode_value);
        }

        // Create a new render pass for this button
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Button Render Pass"),
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

        // Calculate scaling for animation
        let (center_x, center_y) = (
            self.position.0 as f32 + (self.size.0 as f32 / 2.0),
            self.position.1 as f32 + (self.size.1 as f32 / 2.0),
        );

        // Calculate scaled dimensions
        let scaled_width = self.size.0 as f32 * self.scale;
        let scaled_height = self.size.1 as f32 * self.scale;

        // Calculate top-left position with scaling from center
        let scaled_x = center_x - (scaled_width / 2.0);
        let scaled_y = center_y - (scaled_height / 2.0);

        // Set viewport with animation scaling
        render_pass.set_viewport(scaled_x, scaled_y, scaled_width, scaled_height, 0.0, 1.0);

        render_pass.set_pipeline(&self.pipeline);

        // Set the appropriate bind group
        if self.button_type == ButtonType::Close || self.button_type == ButtonType::ModeToggle {
            // Set rotation uniform bind group for shader-based buttons
            if let Some(bind_group) = &self.rotation_bind_group {
                render_pass.set_bind_group(0, bind_group, &[]);
            }
        } else if let Some(texture) = &self.texture {
            // Set texture bind group for other buttons
            render_pass.set_bind_group(0, &texture.bind_group, &[]);
        }

        render_pass.set_vertex_buffer(0, self.vertices.slice(..));
        render_pass.draw(0..4, 0..1);
    }
}

impl ButtonManager {
    /// Calculate dynamic button layout parameters based on window dimensions
    fn calculate_layout_params(window_width: u32) -> ButtonLayoutParams {
        let scale_factor = (window_width as f32 / 240.0).max(0.7).min(1.2);

        ButtonLayoutParams {
            regular_button_size: (COPY_BUTTON_BASE_SIZE * scale_factor) as u32,
            close_button_size: (CLOSE_BUTTON_BASE_SIZE * scale_factor) as u32,
            margin: ((window_width as f32) * BUTTON_MARGIN_RATIO).max(6.0).min(16.0) as u32,
            spacing: ((window_width as f32) * BUTTON_SPACING_RATIO).max(4.0).min(12.0) as u32,
        }
    }

    /// Calculate button layout for a set of button types
    fn calculate_button_layout(&self, button_types: &[ButtonType]) -> Vec<(ButtonType, (u32, u32))> {
        let params = Self::calculate_layout_params(self.window_width);
        let bottom_buttons: Vec<_> = button_types
            .iter()
            .filter(|&&bt| bt != ButtonType::Close)
            .cloned()
            .collect();

        if bottom_buttons.is_empty() {
            return Vec::new();
        }

        let button_count = bottom_buttons.len();
        let total_width = (button_count as u32) * params.regular_button_size
            + (button_count.saturating_sub(1) as u32) * params.spacing;
        let start_x = self.window_width / 2 - total_width / 2;

        let mut layout = Vec::new();

        // Calculate positions for bottom buttons
        for (i, &button_type) in bottom_buttons.iter().enumerate() {
            let button_x = start_x + (i as u32) * (params.regular_button_size + params.spacing);
            let button_y = (self.text_area_height as f32 * 0.95) as u32 - params.regular_button_size - params.margin;
            layout.push((button_type, (button_x, button_y)));
        }

        // Add close button position if it exists
        if button_types.contains(&ButtonType::Close) {
            let close_x = self.window_width - 4 - params.margin - params.close_button_size;
            let close_y = params.margin;
            layout.push((ButtonType::Close, (close_x, close_y)));
        }

        layout
    }

    /// Update all button positions based on current transcription mode
    fn update_all_button_positions(&mut self) {
        let button_types = match self.transcription_mode {
            TranscriptionMode::RealTime => vec![
                ButtonType::Pause,
                ButtonType::Copy,
                ButtonType::Reset,
                ButtonType::ModeToggle,
                ButtonType::Close,
            ],
            TranscriptionMode::Manual => vec![
                ButtonType::RecordToggle,
                ButtonType::Copy,
                ButtonType::Reset,
                ButtonType::ModeToggle,
                ButtonType::Close,
            ],
        };

        let layout = self.calculate_button_layout(&button_types);
        let params = Self::calculate_layout_params(self.window_width);

        for (button_type, (x, y)) in layout {
            if let Some(button) = self.buttons.get_mut(&button_type) {
                button.position = (x, y);
                let size = if button_type == ButtonType::Close {
                    params.close_button_size
                } else {
                    params.regular_button_size
                };
                button.size = (size, size);
            }
        }
    }

    /// Calculate dynamic button size based on window dimensions (legacy method for compatibility)
    fn calculate_button_size(window_width: u32, is_close: bool) -> u32 {
        let params = Self::calculate_layout_params(window_width);
        if is_close {
            params.close_button_size
        } else {
            params.regular_button_size
        }
    }

    /// Calculate dynamic button margin based on window dimensions (legacy method for compatibility)
    fn calculate_button_margin(window_width: u32) -> u32 {
        Self::calculate_layout_params(window_width).margin
    }

    /// Calculate dynamic button spacing based on window dimensions (legacy method for compatibility)
    fn calculate_button_spacing(window_width: u32) -> u32 {
        Self::calculate_layout_params(window_width).spacing
    }

    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        window_size: PhysicalSize<u32>,
        format: wgpu::TextureFormat,
        transcription_mode: TranscriptionMode,
        text_area_height: u32,
        gap: u32,
    ) -> Self {
        // Store the original text_area_height for button positioning
        // Buttons should be positioned within the text area, above the gap

        // Define button sets based on transcription mode
        let button_types = match transcription_mode {
            TranscriptionMode::RealTime => {
                vec![
                    ButtonType::Pause,
                    ButtonType::Copy,
                    ButtonType::Reset,
                    ButtonType::ModeToggle, // Always visible
                    ButtonType::Close,
                ]
            }
            TranscriptionMode::Manual => {
                vec![
                    ButtonType::RecordToggle,
                    ButtonType::Copy,
                    ButtonType::Reset,
                    ButtonType::ModeToggle, // Always visible
                    ButtonType::Close,
                ]
            }
        };

        // Calculate button layout
        let bottom_buttons: Vec<_> = button_types
            .iter()
            .filter(|&&bt| bt != ButtonType::Close)
            .cloned()
            .collect();
        let button_count = bottom_buttons.len();
        let button_size = Self::calculate_button_size(window_size.width, false);
        let button_spacing = Self::calculate_button_spacing(window_size.width);
        let total_buttons_width = (button_count as u32) * button_size
            + (button_count.saturating_sub(1) as u32) * button_spacing;
        let center_x = window_size.width / 2;
        let start_x = center_x - total_buttons_width / 2;

        // Create buttons with calculated positions
        let mut buttons = HashMap::new();

        // Position bottom buttons (all except Close)
        for (i, &button_type) in bottom_buttons.iter().enumerate() {
            let button_x = start_x + (i as u32) * (button_size + button_spacing);
            let button_margin = Self::calculate_button_margin(window_size.width);
            let button_y = text_area_height - button_size - button_margin;

            let button = Button::new(
                device,
                queue,
                button_type,
                (button_x, button_y),
                (button_size, button_size),
                format,
                None,
            );
            buttons.insert(button_type, button);
        }

        // Add close button in top right corner, aligned with text area right edge
        if button_types.contains(&ButtonType::Close) {
            let close_button_size = Self::calculate_button_size(window_size.width, true);
            let button_margin = Self::calculate_button_margin(window_size.width);
            let close_button = Button::new(
                device,
                queue,
                ButtonType::Close,
                (
                    window_size.width - 4 - button_margin - close_button_size, // 4 = RIGHT_MARGIN from text area
                    button_margin,
                ),
                (close_button_size, close_button_size),
                format,
                None,
            );
            buttons.insert(ButtonType::Close, close_button);
        }

        Self {
            buttons,
            text_area_height,
            gap,
            active_button: None,
            recording: None,
            transcription_mode,
            copy_texture: None,
            reset_texture: None,
            pause_texture: None,
            play_texture: None,
            accept_texture: None,
            device: device.clone(),
            queue: queue.clone(),
            config: format,
            window_width: window_size.width,
            window_height: window_size.height,
        }
    }

    /// Helper function to load a single texture and assign it to the corresponding button
    fn load_single_texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        image_bytes: &[u8],
        texture_name: &str,
        button_type: ButtonType,
        format: wgpu::TextureFormat,
    ) {
        if let Ok(texture) = ButtonTexture::from_bytes(
            device,
            queue,
            image_bytes,
            Some(texture_name),
            format,
        ) {
            // Store the texture in the appropriate cache field
            match button_type {
                ButtonType::Copy => self.copy_texture = Some(texture.clone()),
                ButtonType::Reset => self.reset_texture = Some(texture.clone()),
                ButtonType::Pause => self.pause_texture = Some(texture.clone()),
                ButtonType::Play => self.play_texture = Some(texture.clone()),
                ButtonType::Accept => self.accept_texture = Some(texture.clone()),
                _ => {} // Other buttons don't have texture cache fields
            }

            // Assign the texture to the button if it exists
            if let Some(button) = self.buttons.get_mut(&button_type) {
                button.texture = Some(texture);
            }
        }
    }

    pub fn load_textures(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        copy_image_bytes: Option<&[u8]>,
        reset_image_bytes: Option<&[u8]>,
        pause_image_bytes: Option<&[u8]>,
        play_image_bytes: Option<&[u8]>,
        accept_image_bytes: Option<&[u8]>,
        format: wgpu::TextureFormat,
    ) {
        // Load all button textures using the helper function
        if let Some(image_bytes) = copy_image_bytes {
            self.load_single_texture(device, queue, image_bytes, "Copy Button Texture", ButtonType::Copy, format);
        }

        if let Some(image_bytes) = reset_image_bytes {
            self.load_single_texture(device, queue, image_bytes, "Reset Button Texture", ButtonType::Reset, format);
        }

        if let Some(image_bytes) = pause_image_bytes {
            self.load_single_texture(device, queue, image_bytes, "Pause Button Texture", ButtonType::Pause, format);
        }

        if let Some(image_bytes) = play_image_bytes {
            self.load_single_texture(device, queue, image_bytes, "Play Button Texture", ButtonType::Play, format);
        }

        if let Some(image_bytes) = accept_image_bytes {
            self.load_single_texture(device, queue, image_bytes, "Accept Button Texture", ButtonType::Accept, format);
        }

        // Manual mode buttons use play/pause textures:
        // RecordToggle button will dynamically switch between play/pause textures
    }

    pub fn resize(&mut self, window_size: PhysicalSize<u32>) {
        // Update stored window dimensions
        self.window_width = window_size.width;
        self.window_height = window_size.height;

        // Define the correct button order based on current mode
        let button_order = match self.transcription_mode {
            TranscriptionMode::RealTime => {
                vec![
                    ButtonType::Pause,
                    ButtonType::Copy,
                    ButtonType::Reset,
                    ButtonType::ModeToggle,
                ]
            }
            TranscriptionMode::Manual => {
                vec![
                    ButtonType::RecordToggle,
                    ButtonType::Copy,
                    ButtonType::Reset,
                    ButtonType::ModeToggle,
                ]
            }
        };

        // Filter to only buttons that actually exist
        let bottom_buttons: Vec<_> = button_order
            .into_iter()
            .filter(|bt| self.buttons.contains_key(bt))
            .collect();
        let button_count = bottom_buttons.len();
        let button_size = Self::calculate_button_size(self.window_width, false);
        let button_spacing = Self::calculate_button_spacing(self.window_width);
        let total_buttons_width = (button_count as u32) * button_size
            + (button_count.saturating_sub(1) as u32) * button_spacing;
        let center_x = window_size.width / 2;
        let start_x = center_x - total_buttons_width / 2;

        // Update positions for bottom buttons in the correct order
        for (i, &button_type) in bottom_buttons.iter().enumerate() {
            if let Some(button) = self.buttons.get_mut(&button_type) {
                let button_x = start_x + (i as u32) * (button_size + button_spacing);
                // Position buttons much closer to the bottom of the text area (95% of text area height)
                let button_size = Self::calculate_button_size(self.window_width, false);
            let button_spacing = Self::calculate_button_spacing(self.window_width);
            let button_margin = Self::calculate_button_margin(self.window_width);
            let button_y = (self.text_area_height as f32 * 0.95) as u32 - button_size - button_margin;
                button.position = (button_x, button_y);
                button.size = (button_size, button_size);
            }
        }

        // Update close button position
        if let Some(close_button) = self.buttons.get_mut(&ButtonType::Close) {
            let close_button_size = Self::calculate_button_size(self.window_width, true);
            let button_margin = Self::calculate_button_margin(self.window_width);
            close_button.position = (
                window_size.width - 4 - button_margin - close_button_size, // 4 = RIGHT_MARGIN from text area
                button_margin,
            );
            close_button.size = (close_button_size, close_button_size);
        }
    }

    pub fn reset_hover_states(&mut self) {
        for button in self.buttons.values_mut() {
            button.set_state(ButtonState::Normal);
        }
        self.active_button = None;
    }

    pub fn handle_mouse_move(&mut self, position: PhysicalPosition<f64>) {
        let x = position.x;
        let y = position.y;

        // Find which button (if any) contains the mouse position
        let current_hover = self
            .buttons
            .iter()
            .find(|(_, button)| button.contains_point(x, y))
            .map(|(&button_type, _)| {
                // Handle special case for pause/play button state detection
                if button_type == ButtonType::Pause {
                    if let Some(recording) = &self.recording {
                        if recording.load(Ordering::Relaxed) {
                            ButtonType::Pause
                        } else {
                            ButtonType::Play
                        }
                    } else {
                        ButtonType::Pause
                    }
                } else {
                    button_type
                }
            });

        // Only update states if there's an actual change to avoid unnecessary updates
        if current_hover != self.active_button {
            // Reset all buttons to normal state first
            self.reset_hover_states();

            // Set the newly hovered button to hover state
            if let Some(hovered_button_type) = current_hover {
                // Find the actual button to update (handle pause/play mapping)
                let target_button_type = match hovered_button_type {
                    ButtonType::Play => ButtonType::Pause, // Play state is handled by pause button
                    _ => hovered_button_type,
                };

                if let Some(button) = self.buttons.get_mut(&target_button_type) {
                    button.set_state(ButtonState::Hover);
                }
            }

            // Update active button tracking
            self.active_button = current_hover;
        }
    }

    pub fn handle_pointer_event(
        &mut self,
        button: MouseButton,
        state: ElementState,
        position: PhysicalPosition<f64>,
    ) -> Option<ButtonType> {
        let x = position.x;
        let y = position.y;
        let mut result = None;

        match state {
            ElementState::Pressed => {
                // Find and set pressed state for any button containing the point
                for (_, button) in self.buttons.iter_mut() {
                    if button.contains_point(x, y) {
                        button.set_state(ButtonState::Pressed);
                        break;
                    }
                }
            }
            ElementState::Released => {
                // Check for clicks - only register if mouse released on a pressed button
                for (&button_type, button) in self.buttons.iter_mut() {
                    if button.contains_point(x, y) && matches!(button.state, ButtonState::Pressed) {
                        // Handle special case for pause/play button
                        result = Some(if button_type == ButtonType::Pause {
                            if let Some(recording) = &self.recording {
                                if recording.load(Ordering::Relaxed) {
                                    ButtonType::Pause
                                } else {
                                    ButtonType::Play
                                }
                            } else {
                                ButtonType::Pause
                            }
                        } else {
                            button_type
                        });
                        break;
                    }
                }

                // Reset all buttons to appropriate state (hover if mouse over, normal otherwise)
                for (_, button) in self.buttons.iter_mut() {
                    let new_state = if button.contains_point(x, y) {
                        ButtonState::Hover
                    } else {
                        ButtonState::Normal
                    };
                    button.set_state(new_state);
                }
            }
        }

        result
    }

    pub fn render(
        &mut self,
        view: &wgpu::TextureView,
        encoder: &mut wgpu::CommandEncoder,
        is_hovering_transcript: bool,
        queue: &wgpu::Queue,
    ) {
        // Only render buttons when hovering over the transcript
        if is_hovering_transcript {
            // Handle pause/play button texture switching for real-time mode
            if self.transcription_mode == TranscriptionMode::RealTime {
                let is_recording = self
                    .recording
                    .as_ref()
                    .map(|rec| rec.load(Ordering::Relaxed))
                    .unwrap_or(false);

                let current_type = if is_recording {
                    ButtonType::Pause
                } else {
                    ButtonType::Play
                };

                // Update pause button texture if recording state changed
                if let Some(pause_button) = self.buttons.get_mut(&ButtonType::Pause) {
                    if pause_button.button_type != current_type {
                        let texture_option = if is_recording {
                            self.pause_texture.clone()
                        } else {
                            self.play_texture.clone()
                        };

                        if let Some(texture) = texture_option {
                            let current_state = pause_button.state;
                            pause_button.texture = Some(texture);
                            pause_button.button_type = current_type;
                            pause_button.set_state(current_state);
                        }
                    }
                }
            } else if self.transcription_mode == TranscriptionMode::Manual {
                // Update record toggle button texture based on recording state
                self.update_record_toggle_button_texture();
            }

            // Update animations for all buttons
            self.update_animations();

            // Render all buttons
            for button in self.buttons.values() {
                button.render(view, encoder, queue, Some(self.transcription_mode));
            }
        }
    }

    pub fn update_animations(&mut self) {
        for button in self.buttons.values_mut() {
            button.update_animation();
        }
    }

    pub fn set_recording(&mut self, recording: Option<Arc<AtomicBool>>) {
        self.recording = recording;
    }

    pub fn set_transcription_mode(&mut self, mode: TranscriptionMode) {
        if self.transcription_mode != mode {
            let old_mode = self.transcription_mode;
            self.transcription_mode = mode;
            println!(
                "ButtonManager: Switching from {:?} to {:?} mode",
                old_mode, mode
            );

            // Update button layout for the new mode
            self.update_button_layout_for_mode();
        }
    }

    fn update_button_layout_for_mode(&mut self) {
        // Define button sets based on transcription mode
        let new_button_types = match self.transcription_mode {
            TranscriptionMode::RealTime => {
                vec![
                    ButtonType::Pause,
                    ButtonType::Copy,
                    ButtonType::Reset,
                    ButtonType::ModeToggle,
                    ButtonType::Close,
                ]
            }
            TranscriptionMode::Manual => {
                vec![
                    ButtonType::RecordToggle,
                    ButtonType::Copy,
                    ButtonType::Reset,
                    ButtonType::ModeToggle,
                    ButtonType::Close,
                ]
            }
        };

        // Remove buttons that are no longer needed
        let current_types: Vec<ButtonType> = self.buttons.keys().cloned().collect();
        for button_type in current_types {
            if !new_button_types.contains(&button_type) {
                self.buttons.remove(&button_type);
                println!("ButtonManager: Removed button {:?}", button_type);
            }
        }

        // Add new buttons that don't exist yet
        for &button_type in &new_button_types {
            if !self.buttons.contains_key(&button_type) {
                self.add_button(button_type);
                println!("ButtonManager: Added button {:?}", button_type);
            }
        }

        // Update button positions for the new layout
        self.recalculate_button_positions();
    }

    fn add_button(&mut self, button_type: ButtonType) {
        let position = (0, 0); // Temporary position, will be recalculated
        let button_size = Self::calculate_button_size(self.window_width, button_type == ButtonType::Close);
        let size = (button_size, button_size);

        let button = Button::new(
            &self.device,
            &self.queue,
            button_type,
            position,
            size,
            self.config,
            None, // Texture will be assigned later if needed
        );

        self.buttons.insert(button_type, button);

        // Assign appropriate textures
        match button_type {
            ButtonType::RecordToggle => {
                // RecordToggle starts with play texture (not recording)
                if let Some(play_texture) = &self.play_texture {
                    if let Some(button) = self.buttons.get_mut(&ButtonType::RecordToggle) {
                        button.texture = Some(play_texture.clone());
                    }
                }
            }
            ButtonType::Pause => {
                // Assign pause or play texture based on current recording state
                let is_recording = self
                    .recording
                    .as_ref()
                    .map(|rec| rec.load(Ordering::Relaxed))
                    .unwrap_or(false);

                let texture = if is_recording {
                    self.pause_texture.clone()
                } else {
                    self.play_texture.clone()
                };

                if let Some(tex) = texture {
                    if let Some(button) = self.buttons.get_mut(&ButtonType::Pause) {
                        button.texture = Some(tex);
                    }
                }
            }
            // Other textures are already handled by the existing load_textures method
            _ => {}
        }
    }

    fn recalculate_button_positions(&mut self) {
        // Define the correct button order based on current mode
        let button_order = match self.transcription_mode {
            TranscriptionMode::RealTime => {
                vec![
                    ButtonType::Pause,
                    ButtonType::Copy,
                    ButtonType::Reset,
                    ButtonType::ModeToggle,
                ]
            }
            TranscriptionMode::Manual => {
                vec![
                    ButtonType::RecordToggle,
                    ButtonType::Copy,
                    ButtonType::Reset,
                    ButtonType::ModeToggle,
                ]
            }
        };

        // Filter to only buttons that actually exist
        let bottom_buttons: Vec<_> = button_order
            .into_iter()
            .filter(|bt| self.buttons.contains_key(bt))
            .collect();
        let button_count = bottom_buttons.len();
        let button_size = Self::calculate_button_size(self.window_width, false);
        let button_spacing = Self::calculate_button_spacing(self.window_width);
        let total_buttons_width = (button_count as u32) * button_size
            + (button_count.saturating_sub(1) as u32) * button_spacing;
        let center_x = self.window_width / 2;
        let start_x = center_x - total_buttons_width / 2;

        // Position bottom buttons (all except Close) in the correct order
        for (i, &button_type) in bottom_buttons.iter().enumerate() {
            if let Some(button) = self.buttons.get_mut(&button_type) {
                let button_x = start_x + (i as u32) * (button_size + button_spacing);
                // Position buttons much closer to the bottom of the text area (95% of text area height)
                let button_size = Self::calculate_button_size(self.window_width, false);
            let button_spacing = Self::calculate_button_spacing(self.window_width);
            let button_margin = Self::calculate_button_margin(self.window_width);
            let button_y = (self.text_area_height as f32 * 0.95) as u32 - button_size - button_margin;
                button.position = (button_x, button_y);
                button.size = (button_size, button_size);
            }
        }

        // Update close button position
        if let Some(close_button) = self.buttons.get_mut(&ButtonType::Close) {
            let close_button_size = Self::calculate_button_size(self.window_width, true);
            let button_margin = Self::calculate_button_margin(self.window_width);
            close_button.position = (
                self.window_width - 4 - button_margin - close_button_size, // 4 = RIGHT_MARGIN from text area
                button_margin,
            );
            close_button.size = (close_button_size, close_button_size);
        }
    }

    pub fn update_pause_button_texture(&mut self) {
        if let Some(pause_button) = self.buttons.get_mut(&ButtonType::Pause) {
            let is_recording = self
                .recording
                .as_ref()
                .map(|rec| rec.load(Ordering::Relaxed))
                .unwrap_or(false);

            if is_recording {
                // We're recording, show the pause button
                if let Some(texture) = &self.pause_texture {
                    pause_button.texture = Some(texture.clone());
                }
            } else {
                // We're not recording, show the play button
                if let Some(texture) = &self.play_texture {
                    pause_button.texture = Some(texture.clone());
                }
            }
        }
    }

    pub fn update_record_toggle_button_texture(&mut self) {
        if let Some(record_button) = self.buttons.get_mut(&ButtonType::RecordToggle) {
            let is_recording = self
                .recording
                .as_ref()
                .map(|rec| rec.load(Ordering::Relaxed))
                .unwrap_or(false);

            // In manual mode, check current transcription mode to determine behavior
            let current_mode = self.transcription_mode;

            if current_mode == crate::real_time_transcriber::TranscriptionMode::Manual {
                if is_recording {
                    // We're recording in manual mode, show the accept button (to finish recording)
                    if let Some(texture) = &self.accept_texture {
                        record_button.texture = Some(texture.clone());
                    }
                } else {
                    // We're not recording in manual mode, show the play button (to start recording)
                    if let Some(texture) = &self.play_texture {
                        record_button.texture = Some(texture.clone());
                    }
                }
            } else {
                // Real-time mode: use pause/play logic
                if is_recording {
                    if let Some(texture) = &self.pause_texture {
                        record_button.texture = Some(texture.clone());
                    }
                } else {
                    if let Some(texture) = &self.play_texture {
                        record_button.texture = Some(texture.clone());
                    }
                }
            }
        }
    }

    /// Get the bounding box for the bottom button panel (excludes Close button)
    /// Returns (x, y, width, height) in pixels
    pub fn get_button_panel_bounds(&self) -> Option<(f32, f32, f32, f32)> {
        // Get the button order based on current mode
        let button_order = match self.transcription_mode {
            TranscriptionMode::RealTime => {
                vec![
                    ButtonType::Pause,
                    ButtonType::Copy,
                    ButtonType::Reset,
                    ButtonType::ModeToggle,
                ]
            }
            TranscriptionMode::Manual => {
                vec![
                    ButtonType::RecordToggle,
                    ButtonType::Copy,
                    ButtonType::Reset,
                    ButtonType::ModeToggle,
                ]
            }
        };

        // Filter to only buttons that exist
        let bottom_buttons: Vec<_> = button_order
            .into_iter()
            .filter_map(|bt| self.buttons.get(&bt))
            .collect();

        if bottom_buttons.is_empty() {
            return None;
        }

        // Find min/max x and y coordinates for bottom buttons only
        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;
        let mut max_x = f32::MIN;
        let mut max_y = f32::MIN;

        for button in &bottom_buttons {
            let x = button.position.0 as f32;
            let y = button.position.1 as f32;
            let width = button.size.0 as f32;
            let height = button.size.1 as f32;

            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x + width);
            max_y = max_y.max(y + height);
        }

        // Add padding around the buttons
        const PADDING: f32 = 4.0;

        // Clamp to ensure viewport bounds are valid (non-negative and within window)
        let x = (min_x - PADDING).max(0.0);
        let y = (min_y - PADDING).max(0.0);
        let width = (max_x - min_x + (PADDING * 2.0)).min(self.window_width as f32 - x);
        let height = (max_y - min_y + (PADDING * 2.0)).min(self.window_height as f32 - y);

        Some((x, y, width, height))
    }

    /// Get the bounding box for the Close button panel
    /// Returns (x, y, width, height) in pixels
    pub fn get_close_button_panel_bounds(&self) -> Option<(f32, f32, f32, f32)> {
        let close_button = self.buttons.get(&ButtonType::Close)?;

        let x = close_button.position.0 as f32;
        let y = close_button.position.1 as f32;
        let width = close_button.size.0 as f32;
        let height = close_button.size.1 as f32;

        // Add padding around the close button
        const PADDING: f32 = 4.0;

        // Clamp to ensure viewport bounds are valid (non-negative and within window)
        let panel_x = (x - PADDING).max(0.0);
        let panel_y = (y - PADDING).max(0.0);
        let panel_width = (width + (PADDING * 2.0)).min(self.window_width as f32 - panel_x);
        let panel_height = (height + (PADDING * 2.0)).min(self.window_height as f32 - panel_y);

        Some((panel_x, panel_y, panel_width, panel_height))
    }
}
