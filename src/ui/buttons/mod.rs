mod button;

use button::{Button, ButtonState};

pub use button::ButtonType;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use wgpu;
use winit::{
    dpi::{PhysicalPosition, PhysicalSize},
    event::{ElementState, MouseButton},
};

use speechcore::TranscriptionMode;

use super::button_texture::ButtonTexture;

// Button base sizes (will be scaled dynamically)
const COPY_BUTTON_BASE_SIZE: f32 = 16.0; // Base size for scaling calculations
const CLOSE_BUTTON_BASE_SIZE: f32 = 12.0; // Base size for close button (slightly smaller)
const BUTTON_MARGIN_RATIO: f32 = 0.025; // Margin as ratio of window width
const BUTTON_SPACING_RATIO: f32 = 0.02; // Spacing as ratio of window width

/// Layout parameters for button positioning and sizing
#[derive(Debug, Clone, Copy)]
struct ButtonLayoutParams {
    regular_button_size: u32,
    close_button_size: u32,
    margin: u32,
    spacing: u32,
}

pub struct ButtonManager {
    buttons: std::collections::HashMap<ButtonType, Button>,
    text_area_height: u32,
    _gap: u32,
    active_button: Option<ButtonType>,
    recording: Option<Arc<AtomicBool>>,
    transcription_mode: speechcore::TranscriptionMode,
    enhancement_enabled: bool, // Config: whether MagicMode feature is available (shows button)
    magic_mode_active: bool, // Runtime: whether MagicMode is currently toggled on (affects opacity)
    // Texture cache
    copy_texture: Option<ButtonTexture>,
    reset_texture: Option<ButtonTexture>,
    pause_texture: Option<ButtonTexture>,
    play_texture: Option<ButtonTexture>,
    accept_texture: Option<ButtonTexture>,
    magic_wand_texture: Option<ButtonTexture>, // Single texture with opacity control
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::TextureFormat,
    window_width: u32,
    window_height: u32,
}
impl ButtonManager {
    /// Calculate dynamic button layout parameters based on window dimensions
    fn calculate_layout_params(window_width: u32) -> ButtonLayoutParams {
        let scale_factor = (window_width as f32 / 240.0).clamp(0.7, 1.2);

        ButtonLayoutParams {
            regular_button_size: (COPY_BUTTON_BASE_SIZE * scale_factor) as u32,
            close_button_size: (CLOSE_BUTTON_BASE_SIZE * scale_factor) as u32,
            margin: ((window_width as f32) * BUTTON_MARGIN_RATIO).clamp(6.0, 16.0) as u32,
            spacing: ((window_width as f32) * BUTTON_SPACING_RATIO).clamp(4.0, 12.0) as u32,
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
        enhancement_enabled: bool,
    ) -> Self {
        // Store the original text_area_height for button positioning
        // Buttons should be positioned within the text area, above the gap

        // Define button sets based on transcription mode
        let button_types = Self::get_button_types(transcription_mode, enhancement_enabled);

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
            _gap: gap,
            active_button: None,
            recording: None,
            transcription_mode,
            enhancement_enabled,
            magic_mode_active: false,
            copy_texture: None,
            reset_texture: None,
            pause_texture: None,
            play_texture: None,
            accept_texture: None,
            magic_wand_texture: None,
            device: device.clone(),
            queue: queue.clone(),
            config: format,
            window_width: window_size.width,
            window_height: window_size.height,
        }
    }

    /// Get button types based on transcription mode and enhancement config
    fn get_button_types(mode: TranscriptionMode, enhancement_enabled: bool) -> Vec<ButtonType> {
        match mode {
            TranscriptionMode::RealTime => vec![
                ButtonType::Pause,
                ButtonType::Copy,
                ButtonType::Reset,
                ButtonType::ModeToggle,
                ButtonType::Settings,
                ButtonType::Close,
            ],
            TranscriptionMode::Manual => {
                let mut buttons = vec![ButtonType::RecordToggle];
                if enhancement_enabled {
                    buttons.push(ButtonType::MagicMode);
                }
                buttons.extend([
                    ButtonType::Copy,
                    ButtonType::Reset,
                    ButtonType::ModeToggle,
                    ButtonType::Settings,
                    ButtonType::Close,
                ]);
                buttons
            }
        }
    }

    fn ordered_button_keys(&self, include_close: bool) -> Vec<ButtonType> {
        Self::get_button_types(self.transcription_mode, self.enhancement_enabled)
            .into_iter()
            .filter(|&bt| include_close || bt != ButtonType::Close)
            .filter(|bt| self.buttons.contains_key(bt))
            .collect()
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
        if let Ok(texture) =
            ButtonTexture::from_bytes(device, queue, image_bytes, Some(texture_name), format)
        {
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
        magic_wand_image_bytes: Option<&[u8]>,
        format: wgpu::TextureFormat,
    ) {
        // Load all button textures using the helper function
        if let Some(image_bytes) = copy_image_bytes {
            self.load_single_texture(
                device,
                queue,
                image_bytes,
                "Copy Button Texture",
                ButtonType::Copy,
                format,
            );
        }

        if let Some(image_bytes) = reset_image_bytes {
            self.load_single_texture(
                device,
                queue,
                image_bytes,
                "Reset Button Texture",
                ButtonType::Reset,
                format,
            );
        }

        if let Some(image_bytes) = pause_image_bytes {
            self.load_single_texture(
                device,
                queue,
                image_bytes,
                "Pause Button Texture",
                ButtonType::Pause,
                format,
            );
        }

        if let Some(image_bytes) = play_image_bytes {
            self.load_single_texture(
                device,
                queue,
                image_bytes,
                "Play Button Texture",
                ButtonType::Play,
                format,
            );
        }

        if let Some(image_bytes) = accept_image_bytes {
            self.load_single_texture(
                device,
                queue,
                image_bytes,
                "Accept Button Texture",
                ButtonType::Accept,
                format,
            );
        }

        // Load magic wand texture (single texture with opacity control)
        if let Some(image_bytes) = magic_wand_image_bytes {
            if let Ok(texture) = ButtonTexture::from_bytes(
                device,
                queue,
                image_bytes,
                Some("Magic Wand Texture"),
                format,
            ) {
                self.magic_wand_texture = Some(texture.clone());

                // Create the opacity bind group for the MagicMode button
                if let Some(button) = self.buttons.get_mut(&ButtonType::MagicMode) {
                    if let Some(opacity_buffer) = &button.opacity_buffer {
                        // Create bind group layout for texture + sampler + opacity
                        let bind_group_layout =
                            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                                entries: &[
                                    wgpu::BindGroupLayoutEntry {
                                        binding: 0,
                                        visibility: wgpu::ShaderStages::FRAGMENT,
                                        ty: wgpu::BindingType::Texture {
                                            multisampled: false,
                                            view_dimension: wgpu::TextureViewDimension::D2,
                                            sample_type: wgpu::TextureSampleType::Float {
                                                filterable: true,
                                            },
                                        },
                                        count: None,
                                    },
                                    wgpu::BindGroupLayoutEntry {
                                        binding: 1,
                                        visibility: wgpu::ShaderStages::FRAGMENT,
                                        ty: wgpu::BindingType::Sampler(
                                            wgpu::SamplerBindingType::Filtering,
                                        ),
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
                                label: Some("MagicMode Opacity Bind Group Layout"),
                            });

                        // Create the bind group with texture, sampler, and opacity
                        let opacity_bind_group =
                            device.create_bind_group(&wgpu::BindGroupDescriptor {
                                layout: &bind_group_layout,
                                entries: &[
                                    wgpu::BindGroupEntry {
                                        binding: 0,
                                        resource: wgpu::BindingResource::TextureView(&texture.view),
                                    },
                                    wgpu::BindGroupEntry {
                                        binding: 1,
                                        resource: wgpu::BindingResource::Sampler(&texture.sampler),
                                    },
                                    wgpu::BindGroupEntry {
                                        binding: 2,
                                        resource: opacity_buffer.as_entire_binding(),
                                    },
                                ],
                                label: Some("MagicMode Opacity Bind Group"),
                            });

                        button.opacity_bind_group = Some(opacity_bind_group);
                        button.texture = Some(texture);

                        // Set initial opacity based on magic mode active state
                        button.opacity = if self.magic_mode_active { 0.85 } else { 0.4 };
                    }
                }
            }
        }

        // Manual mode buttons use play/pause textures:
        // RecordToggle button will dynamically switch between play/pause textures
    }

    pub fn resize(&mut self, window_size: PhysicalSize<u32>) {
        // Update stored window dimensions
        self.window_width = window_size.width;
        self.window_height = window_size.height;

        // Define the correct button order based on current mode
        let button_order: Vec<_> =
            Self::get_button_types(self.transcription_mode, self.enhancement_enabled)
                .into_iter()
                .filter(|&bt| bt != ButtonType::Close)
                .collect();

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
                let _button_spacing = Self::calculate_button_spacing(self.window_width);
                let button_margin = Self::calculate_button_margin(self.window_width);
                let button_y =
                    (self.text_area_height as f32 * 0.95) as u32 - button_size - button_margin;
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
        let current_hover =
            self.ordered_button_keys(true)
                .into_iter()
                .rev()
                .find_map(|button_type| {
                    let button = self.buttons.get(&button_type)?;
                    if !button.contains_point(x, y) {
                        return None;
                    }

                    Some(if button_type == ButtonType::Pause {
                        if self
                            .recording
                            .as_ref()
                            .map(|recording| recording.load(Ordering::Relaxed))
                            .unwrap_or(false)
                        {
                            ButtonType::Pause
                        } else {
                            ButtonType::Play
                        }
                    } else {
                        button_type
                    })
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
        _button: MouseButton,
        state: ElementState,
        position: PhysicalPosition<f64>,
    ) -> Option<ButtonType> {
        let x = position.x;
        let y = position.y;
        let mut result = None;

        match state {
            ElementState::Pressed => {
                // Find and set pressed state for any button containing the point
                for button_type in self.ordered_button_keys(true).into_iter().rev() {
                    if let Some(button) = self.buttons.get_mut(&button_type) {
                        if !button.contains_point(x, y) {
                            continue;
                        }
                        button.set_state(ButtonState::Pressed);
                        break;
                    }
                }
            }
            ElementState::Released => {
                // Check for clicks - only register if mouse released on a pressed button
                for button_type in self.ordered_button_keys(true).into_iter().rev() {
                    if let Some(button) = self.buttons.get_mut(&button_type) {
                        if !button.contains_point(x, y)
                            || !matches!(button.state, ButtonState::Pressed)
                        {
                            continue;
                        }

                        result = Some(if button_type == ButtonType::Pause {
                            if self
                                .recording
                                .as_ref()
                                .map(|recording| recording.load(Ordering::Relaxed))
                                .unwrap_or(false)
                            {
                                ButtonType::Pause
                            } else {
                                ButtonType::Play
                            }
                        } else {
                            button_type
                        });
                        break;
                    }
                }

                // Reset all buttons to appropriate state (hover if mouse over, normal otherwise)
                for button in self.buttons.values_mut() {
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

            // Update magic wand opacity based on magic mode active state
            if let Some(magic_button) = self.buttons.get_mut(&ButtonType::MagicMode) {
                magic_button.set_opacity(if self.magic_mode_active { 0.85 } else { 0.4 });
            }

            // Update animations for all buttons
            self.update_animations();

            // Render all buttons
            for button_type in self.ordered_button_keys(true) {
                if let Some(button) = self.buttons.get(&button_type) {
                    button.render(view, encoder, queue, Some(self.transcription_mode));
                }
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

            // Reset magic_mode_active when leaving Manual mode to prevent stale state
            if old_mode == TranscriptionMode::Manual && self.magic_mode_active {
                self.magic_mode_active = false;
                println!("ButtonManager: Reset magic_mode_active when leaving Manual mode");
            }

            self.transcription_mode = mode;
            println!(
                "ButtonManager: Switching from {:?} to {:?} mode",
                old_mode, mode
            );

            // Update button layout for the new mode
            self.update_button_layout_for_mode();
        }
    }

    /// Set whether magic mode feature is enabled (config - controls button visibility)
    pub fn set_enhancement_enabled(&mut self, enabled: bool) {
        self.enhancement_enabled = enabled;
    }

    /// Set whether magic mode is currently active (runtime toggle state - controls opacity)
    pub fn set_magic_mode_active(&mut self, active: bool) {
        self.magic_mode_active = active;
    }

    /// Toggle magic mode active state
    pub fn toggle_magic_mode(&mut self) {
        self.magic_mode_active = !self.magic_mode_active;
    }

    /// Check if magic mode is currently active (toggled on)
    pub fn is_magic_mode_active(&self) -> bool {
        self.magic_mode_active
    }

    /// Check if magic mode feature is enabled (config)
    pub fn is_enhancement_enabled(&self) -> bool {
        self.enhancement_enabled
    }

    fn update_button_layout_for_mode(&mut self) {
        // Define button sets based on transcription mode
        let new_button_types =
            Self::get_button_types(self.transcription_mode, self.enhancement_enabled);

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
        let button_size =
            Self::calculate_button_size(self.window_width, button_type == ButtonType::Close);
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
            ButtonType::MagicMode => {
                // Assign magic wand texture and set up opacity bind group
                if let Some(texture) = &self.magic_wand_texture {
                    if let Some(button) = self.buttons.get_mut(&ButtonType::MagicMode) {
                        if let Some(opacity_buffer) = &button.opacity_buffer {
                            // Create bind group layout for texture + sampler + opacity
                            let bind_group_layout = self.device.create_bind_group_layout(
                                &wgpu::BindGroupLayoutDescriptor {
                                    entries: &[
                                        wgpu::BindGroupLayoutEntry {
                                            binding: 0,
                                            visibility: wgpu::ShaderStages::FRAGMENT,
                                            ty: wgpu::BindingType::Texture {
                                                multisampled: false,
                                                view_dimension: wgpu::TextureViewDimension::D2,
                                                sample_type: wgpu::TextureSampleType::Float {
                                                    filterable: true,
                                                },
                                            },
                                            count: None,
                                        },
                                        wgpu::BindGroupLayoutEntry {
                                            binding: 1,
                                            visibility: wgpu::ShaderStages::FRAGMENT,
                                            ty: wgpu::BindingType::Sampler(
                                                wgpu::SamplerBindingType::Filtering,
                                            ),
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
                                    label: Some("MagicMode Opacity Bind Group Layout"),
                                },
                            );

                            // Create the bind group with texture, sampler, and opacity
                            let opacity_bind_group =
                                self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                                    layout: &bind_group_layout,
                                    entries: &[
                                        wgpu::BindGroupEntry {
                                            binding: 0,
                                            resource: wgpu::BindingResource::TextureView(
                                                &texture.view,
                                            ),
                                        },
                                        wgpu::BindGroupEntry {
                                            binding: 1,
                                            resource: wgpu::BindingResource::Sampler(
                                                &texture.sampler,
                                            ),
                                        },
                                        wgpu::BindGroupEntry {
                                            binding: 2,
                                            resource: opacity_buffer.as_entire_binding(),
                                        },
                                    ],
                                    label: Some("MagicMode Opacity Bind Group"),
                                });

                            button.opacity_bind_group = Some(opacity_bind_group);
                            button.texture = Some(texture.clone());

                            // Set initial opacity based on magic mode active state (inactive after mode switch)
                            button.opacity = 0.4; // Inactive state
                        }
                    }
                }
            }
            ButtonType::Settings => {
                // Settings button is shader-based (gear icon), no texture needed
            }
            // Other textures are already handled by the existing load_textures method
            _ => {}
        }
    }

    fn recalculate_button_positions(&mut self) {
        // Define the correct button order based on current mode
        let button_order: Vec<_> =
            Self::get_button_types(self.transcription_mode, self.enhancement_enabled)
                .into_iter()
                .filter(|&bt| bt != ButtonType::Close)
                .collect();

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
                let _button_spacing = Self::calculate_button_spacing(self.window_width);
                let button_margin = Self::calculate_button_margin(self.window_width);
                let button_y =
                    (self.text_area_height as f32 * 0.95) as u32 - button_size - button_margin;
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

            if current_mode == speechcore::TranscriptionMode::Manual {
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
        // Get the button order based on current mode (excluding Close)
        let button_order: Vec<_> =
            Self::get_button_types(self.transcription_mode, self.enhancement_enabled)
                .into_iter()
                .filter(|&bt| bt != ButtonType::Close)
                .collect();

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

    /// Get currently hovered button info for tooltip
    /// Returns (button_type, center_x, top_y, bottom_y, left_x) if a button is hovered
    pub fn get_hovered_button(&self) -> Option<(ButtonType, f32, f32, f32, f32)> {
        let button_type = self.active_button?;
        let button = self.buttons.get(&button_type)?;

        let center_x = button.position.0 as f32 + (button.size.0 as f32 / 2.0);
        let top_y = button.position.1 as f32;
        let bottom_y = button.position.1 as f32 + button.size.1 as f32;
        let left_x = button.position.0 as f32;

        Some((button_type, center_x, top_y, bottom_y, left_x))
    }
}
