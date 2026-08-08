use wgpu::{self, util::DeviceExt};

use crate::ui::button_texture::ButtonTexture;

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
    MagicMode,    // Toggle LFM enhancement mode (manual mode only)
    Settings,     // Open settings panel
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum ButtonState {
    Normal,
    Hover,
    Pressed,
}
pub struct Button {
    pub(super) button_type: ButtonType,
    pub(super) state: ButtonState,
    pub(super) position: (u32, u32),
    pub(super) size: (u32, u32),
    vertices: wgpu::Buffer,
    pipeline: wgpu::RenderPipeline,
    pub(super) texture: Option<ButtonTexture>,
    animation_progress: f32,
    previous_state: ButtonState,
    animation_active: bool,
    animation_start_time: std::time::Instant,
    scale: f32,
    rotation: f32,
    rotation_buffer: Option<wgpu::Buffer>,
    rotation_bind_group: Option<wgpu::BindGroup>,
    // Opacity support for single-texture buttons with state-based opacity
    pub(super) opacity: f32,
    pub(super) opacity_buffer: Option<wgpu::Buffer>,
    pub(super) opacity_bind_group: Option<wgpu::BindGroup>,
}
impl Button {
    pub(super) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        button_type: ButtonType,
        position: (u32, u32),
        size: (u32, u32),
        format: wgpu::TextureFormat,
        texture: Option<ButtonTexture>,
    ) -> Self {
        // Create default texture if none provided and it's not a shader-based button
        let texture_for_button = if texture.is_none()
            && button_type != ButtonType::Close
            && button_type != ButtonType::Settings
        {
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
            source: wgpu::ShaderSource::Wgsl(include_str!("../button.wgsl").into()),
        });

        // Create opacity buffer and bind group for opacity-based texture buttons (MagicMode)
        let (opacity_buffer, opacity_bind_group) = if button_type == ButtonType::MagicMode {
            // Create opacity uniform buffer with default opacity
            let opacity_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Opacity Uniform Buffer"),
                contents: bytemuck::cast_slice(&[1.0f32]), // Default to full opacity
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });

            // We'll create the bind group later when we have the texture in load_textures
            (Some(opacity_buffer), None::<wgpu::BindGroup>)
        } else {
            (None, None)
        };

        // Create rotation uniform buffer and bind group for shader-based buttons
        let (rotation_buffer, rotation_bind_group) = if button_type == ButtonType::Close
            || button_type == ButtonType::ModeToggle
            || button_type == ButtonType::Settings
        {
            // rotation, mode/unused field
            let initial_data = [0.0f32, 0.0f32];
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
                // Close and Settings only need vertex access
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
            || button_type == ButtonType::Settings
        {
            // For shader-based buttons - use the same visibility logic as the bind group
            let pipeline_visibility = if button_type == ButtonType::ModeToggle {
                // ModeToggle fragment shader needs access to the mode uniform
                wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT
            } else {
                // Close and Settings only need vertex access
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
        } else if button_type == ButtonType::MagicMode {
            // For MagicMode: texture + sampler + opacity uniform
            let bind_group_layout =
                device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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
                        wgpu::BindGroupLayoutEntry {
                            binding: 2,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Uniform,
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                    ],
                    label: Some("MagicMode Texture Opacity Bind Group Layout"),
                });

            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("MagicMode Pipeline Layout"),
                bind_group_layouts: &[&bind_group_layout],
                push_constant_ranges: &[],
            })
        } else {
            // For buttons that use textures only (no opacity)
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
                    ButtonType::MagicMode => Some("vs_copy"), // Use texture-based rendering
                    ButtonType::Settings => Some("vs_close"), // Use close vertex shader (rotation support)
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
                    ButtonType::MagicMode => Some("fs_texture_opacity"), // Texture with dynamic opacity
                    ButtonType::Settings => Some("fs_settings"),         // Gear icon shader
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
            opacity: 1.0,
            opacity_buffer,
            opacity_bind_group,
        }
    }

    pub(super) fn contains_point(&self, x: f64, y: f64) -> bool {
        let (button_x, button_y) = self.position;
        let (button_width, button_height) = self.size;

        x >= button_x as f64
            && x <= (button_x + button_width) as f64
            && y >= button_y as f64
            && y <= (button_y + button_height) as f64
    }

    pub(super) fn set_state(&mut self, state: ButtonState) {
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
    pub(super) fn update_animation(&mut self) {
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
            (ButtonType::Close | ButtonType::Settings, ButtonState::Hover) => {
                self.rotation = HOVER_ROTATION;
                self.scale = 1.0;
            }
            (ButtonType::Close | ButtonType::Settings, _) => {
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
            (ButtonType::Close | ButtonType::Settings, ButtonState::Hover) => (1.0, HOVER_ROTATION),
            (ButtonType::Close | ButtonType::Settings, _) => (1.0, 0.0),
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

    // Update opacity buffer for opacity-based buttons
    fn update_opacity_buffer(&self, queue: &wgpu::Queue) {
        if let Some(buffer) = &self.opacity_buffer {
            queue.write_buffer(buffer, 0, bytemuck::cast_slice(&[self.opacity]));
        }
    }

    // Set opacity for this button
    pub(super) fn set_opacity(&mut self, opacity: f32) {
        self.opacity = opacity;
    }

    pub(super) fn render(
        &self,
        view: &wgpu::TextureView,
        encoder: &mut wgpu::CommandEncoder,
        queue: &wgpu::Queue,
        transcription_mode: Option<speechcore::TranscriptionMode>,
    ) {
        // Update rotation buffer if needed
        if self.button_type == ButtonType::Close
            || self.button_type == ButtonType::ModeToggle
            || self.button_type == ButtonType::Settings
        {
            let mode_value = if self.button_type == ButtonType::ModeToggle {
                transcription_mode.map(|mode| match mode {
                    speechcore::TranscriptionMode::RealTime => 0.0,
                    speechcore::TranscriptionMode::Manual => 1.0,
                })
            } else {
                None
            };
            self.update_rotation_buffer(queue, mode_value);
        }

        // Update opacity buffer for MagicMode
        if self.button_type == ButtonType::MagicMode {
            self.update_opacity_buffer(queue);
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
        if self.button_type == ButtonType::Close
            || self.button_type == ButtonType::ModeToggle
            || self.button_type == ButtonType::Settings
        {
            // Set rotation uniform bind group for shader-based buttons
            if let Some(bind_group) = &self.rotation_bind_group {
                render_pass.set_bind_group(0, bind_group, &[]);
            }
        } else if self.button_type == ButtonType::MagicMode {
            // Set opacity bind group for MagicMode (texture + sampler + opacity)
            if let Some(bind_group) = &self.opacity_bind_group {
                render_pass.set_bind_group(0, bind_group, &[]);
            } else {
                // Skip rendering if bind group not set up (can happen after mode switch)
                return;
            }
        } else if let Some(texture) = &self.texture {
            // Set texture bind group for other buttons
            render_pass.set_bind_group(0, &texture.bind_group, &[]);
        }

        render_pass.set_vertex_buffer(0, self.vertices.slice(..));
        render_pass.draw(0..4, 0..1);
    }
}
