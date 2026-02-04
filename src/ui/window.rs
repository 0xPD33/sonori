use parking_lot::Mutex;
use std::error::Error;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use wgpu::{self, util::DeviceExt};
use winit::{
    dpi::{PhysicalPosition, PhysicalSize},
    event::{ElementState, MouseButton, MouseScrollDelta},
    event_loop::ActiveEventLoop,
    window::Window,
};

use super::button_panel::ButtonPanel;
use super::buttons::ButtonManager;
use super::common::AudioVisualizationData;
use super::tooltip::Tooltip;
use super::event_handler::EventHandler;
use super::layout_manager::LayoutManager;
use super::loading_animation::LoadingAnimation;
use super::render_pipeline::RenderPipelines;
use super::scroll_state::ScrollState;
use super::scrollbar::Scrollbar;
use super::spectogram::Spectrogram;
use super::text_processor::TextProcessor;
use super::text_window::TextWindow;
use super::timer_badge::TimerBadge;
use parking_lot::RwLock;

pub const SPECTROGRAM_WIDTH: u32 = 240; // Width of the spectrogram
pub const SPECTROGRAM_HEIGHT: u32 = 80; // Height of the spectrogram
pub const TEXT_AREA_HEIGHT: u32 = 90; // Additional height for text above spectrogram
pub const MARGIN: i32 = 32; // Margin from the bottom of the screen
pub const GAP: u32 = 4; // Gap between text area and spectrogram
pub const RIGHT_MARGIN: f32 = 4.0; // Right margin for text area
pub const LEFT_MARGIN: f32 = 4.0; // Left margin for text area

pub struct WindowState {
    pub window: Arc<dyn Window>,
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,
    pub spectrogram: Option<Spectrogram>,
    pub audio_data: Option<Arc<RwLock<AudioVisualizationData>>>,
    pub render_pipelines: RenderPipelines,
    pub text_window: TextWindow,
    pub button_manager: ButtonManager,
    pub button_panel: ButtonPanel,
    pub tooltip: Tooltip,
    pub text_processor: TextProcessor,
    pub layout_manager: LayoutManager,
    pub scrollbar: Scrollbar,
    pub scroll_state: ScrollState,
    pub event_handler: EventHandler,
    pub loading_animation: LoadingAnimation,
    pub timer_badge: TimerBadge,
    pub running: Option<Arc<AtomicBool>>,
    pub recording: Option<Arc<AtomicBool>>,
    pub magic_mode_enabled: Option<Arc<AtomicBool>>,
    transcription_mode_ref:
        Arc<parking_lot::Mutex<crate::real_time_transcriber::TranscriptionMode>>,
    last_known_mode: crate::real_time_transcriber::TranscriptionMode,
    // Dynamic sizing
    pub window_width: u32,
    pub window_height: u32,
    pub spectrogram_width: u32,
    pub spectrogram_height: u32,
    pub text_area_height: u32,
    pub gap: u32,
    // Frame rate limiting
    last_frame_time: Option<std::time::Instant>,
    target_frame_duration: std::time::Duration,
    present_mode: wgpu::PresentMode,
    // Hover animation state
    hover_animation_progress: f32, // 0.0 to 1.0
    is_hovering: bool,
    last_hover_update: std::time::Instant,
    // Typewriter effect for transcription reveal
    typewriter: super::typewriter::TypewriterEffect,
    typewriter_enabled: bool,
    last_processing_state: crate::ui::common::ProcessingState,
}

impl WindowState {
    pub fn new(
        window: Box<dyn Window>,
        running: Option<Arc<AtomicBool>>,
        recording: Option<Arc<AtomicBool>>,
        magic_mode_enabled: Option<Arc<AtomicBool>>,
        transcription_mode: crate::real_time_transcriber::TranscriptionMode,
        manual_session_sender: Option<
            tokio::sync::mpsc::Sender<crate::real_time_transcriber::ManualSessionCommand>,
        >,
        transcription_mode_ref: Arc<
            parking_lot::Mutex<crate::real_time_transcriber::TranscriptionMode>,
        >,
        display_config: &crate::config::DisplayConfig,
        window_width: u32,
        window_height: u32,
        spectrogram_width: u32,
        spectrogram_height: u32,
        text_area_height: u32,
        gap: u32,
        enhancement_enabled: bool,
    ) -> Self {
        let window: Arc<dyn Window> = Arc::from(window);

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let surface = match instance.create_surface(window.clone()) {
            Ok(surface) => surface,
            Err(e) => {
                eprintln!("Failed to create surface: {:?}", e.source());
                panic!("Surface creation failed");
            }
        };

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .unwrap_or_else(|_| {
            eprintln!("Failed to find a suitable GPU adapter");
            panic!("No suitable GPU adapter found");
        });

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: None,
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            },
        ))
        .unwrap();

        // Use dynamic sizing values
        let fixed_width = window_width;
        let fixed_height = window_height;

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .filter(|f| f.is_srgb())
            .next()
            .unwrap_or(surface_caps.formats[0]);

        // Select present mode based on display configuration
        let present_mode = display_config.to_present_mode(&surface_caps.present_modes);

        // Select alpha mode for transparency support
        // Prefer modes that support transparency, with Inherit as fallback for X11
        let alpha_mode = if surface_caps
            .alpha_modes
            .contains(&wgpu::CompositeAlphaMode::PreMultiplied)
        {
            wgpu::CompositeAlphaMode::PreMultiplied
        } else if surface_caps
            .alpha_modes
            .contains(&wgpu::CompositeAlphaMode::PostMultiplied)
        {
            wgpu::CompositeAlphaMode::PostMultiplied
        } else if surface_caps
            .alpha_modes
            .contains(&wgpu::CompositeAlphaMode::Inherit)
        {
            // Inherit from windowing system - may help on X11
            wgpu::CompositeAlphaMode::Inherit
        } else {
            // Fallback to first available (likely Opaque)
            // On X11, transparency may still work through the compositor
            let is_wayland = std::env::var("WAYLAND_DISPLAY").is_ok();
            if !is_wayland && surface_caps.alpha_modes[0] == wgpu::CompositeAlphaMode::Opaque {
                eprintln!("Warning: GPU reports only Opaque alpha mode. Window transparency may not work.");
                eprintln!("Try: enabling compositor, or using a different compositor like picom.");
            }
            surface_caps.alpha_modes[0]
        };

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: fixed_width,
            height: fixed_height,
            present_mode,
            alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        surface.configure(&device, &config);

        // Create render pipelines
        let render_pipelines = RenderPipelines::new(&device, &config);

        // Initialize TextWindow
        let text_window = TextWindow::new(
            &device,
            &queue,
            &config,
            PhysicalSize::new(config.width, config.height),
            &render_pipelines.hover_bind_group_layout,
        );

        // Create the button manager
        let mut button_manager = ButtonManager::new(
            &device,
            &queue,
            PhysicalSize::new(config.width, config.height),
            config.format,
            transcription_mode,
            text_area_height,
            gap,
            enhancement_enabled,
        );

        // Load button icons
        let copy_icon = include_bytes!("../../assets/copy.png");
        let reset_icon = include_bytes!("../../assets/reset.png");
        let pause_icon = include_bytes!("../../assets/pause.png");
        let play_icon = include_bytes!("../../assets/play.png");
        let accept_icon = include_bytes!("../../assets/accept.png");
        let magic_wand_icon = include_bytes!("../../assets/magic-wand.png");

        // ModeToggle will use shader-based text rendering (R/M)

        button_manager.load_textures(
            &device,
            &queue,
            Some(copy_icon),
            Some(reset_icon),
            Some(pause_icon),
            Some(play_icon),
            Some(accept_icon),
            Some(magic_wand_icon),
            config.format,
        );

        // Set recording state in button manager
        button_manager.set_recording(recording.clone());

        // Create the button panel with fade animation
        let button_panel = ButtonPanel::new(
            device.clone(),
            queue.clone(),
            PhysicalSize::new(config.width, config.height),
            config.format,
            &render_pipelines.hover_bind_group_layout,
        );

        // Create the tooltip
        let tooltip = Tooltip::new(
            device.clone(),
            queue.clone(),
            config.format,
        );

        // Create the scrollbar
        let scrollbar = Scrollbar::new(&device, &config, &render_pipelines.hover_bind_group_layout);

        // Create text processor with default values
        let text_processor = TextProcessor::new(8.0, 20.0, 4.0);

        // Create layout manager
        let layout_manager = LayoutManager::new(
            config.width,
            config.height,
            spectrogram_width,
            spectrogram_height,
            text_area_height,
            RIGHT_MARGIN,
            LEFT_MARGIN,
            gap,
        );

        // Create event handler
        let event_handler = EventHandler::new(
            recording.clone(),
            magic_mode_enabled.clone(),
            manual_session_sender,
            transcription_mode_ref.clone(),
        );
        let last_known_mode = transcription_mode;

        // Create loading animation
        let loading_animation = LoadingAnimation::new(&Arc::new(device.clone()), config.format);

        // Load UI config for timer badge
        let (app_config, _) = crate::config::read_app_config_with_path();
        let ui_config = app_config.ui_config;

        // Create timer badge
        let timer_badge = TimerBadge::new(&Arc::new(device.clone()), &Arc::new(queue.clone()), config.format, &ui_config);

        // Calculate target frame duration from display config
        let target_frame_duration =
            std::time::Duration::from_secs_f64(1.0 / display_config.target_fps as f64);

        Self {
            window,
            surface,
            device,
            queue,
            config,
            spectrogram: None,
            audio_data: None,
            render_pipelines,
            text_window,
            button_manager,
            button_panel,
            tooltip,
            text_processor,
            layout_manager,

            // Scrollbar and scroll state
            scrollbar,
            scroll_state: ScrollState::new(),

            // Event handler
            event_handler,

            // Loading animation
            loading_animation,

            // Timer badge
            timer_badge,

            // Transcriber state references
            running,
            recording,
            magic_mode_enabled,
            transcription_mode_ref,
            last_known_mode,

            // Dynamic sizing
            window_width,
            window_height,
            spectrogram_width,
            spectrogram_height,
            text_area_height,
            gap,

            // Frame rate limiting
            last_frame_time: None,
            target_frame_duration,
            present_mode,

            // Hover animation state
            hover_animation_progress: 0.0,
            is_hovering: false,
            last_hover_update: std::time::Instant::now(),

            // Typewriter effect
            typewriter: super::typewriter::TypewriterEffect::new(),
            typewriter_enabled: ui_config.typewriter_effect,
            last_processing_state: crate::ui::common::ProcessingState::Idle,
        }
    }

    
    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);

            // Update layout manager dimensions
            self.layout_manager.update_dimensions(width, height);

            if let Some(spectrogram) = &mut self.spectrogram {
                spectrogram.resize(PhysicalSize::new(width, height));
            }

            self.text_window.resize(PhysicalSize::new(width, height));
            self.button_manager.resize(PhysicalSize::new(width, height));
            self.button_panel.resize(PhysicalSize::new(width, height));
        }
    }

    pub fn set_audio_data(&mut self, audio_data: Arc<RwLock<AudioVisualizationData>>) {
        self.audio_data = Some(audio_data);

        // Initialize spectrogram if not already created
        if self.spectrogram.is_none() {
            // Create the spectrogram with the dedicated spectrogram size, not the full window size
            let size = PhysicalSize::new(self.spectrogram_width, self.spectrogram_height);
            let spectrogram = Spectrogram::new(
                Arc::new(self.device.clone()),
                Arc::new(self.queue.clone()),
                size,
                self.config.format,
            );
            self.spectrogram = Some(spectrogram);
        }
    }

    pub fn draw(&mut self, _width: u32) {
        // Frame rate limiting for Immediate present mode (no vsync)
        if self.present_mode == wgpu::PresentMode::Immediate {
            if let Some(last_time) = self.last_frame_time {
                let elapsed = last_time.elapsed();
                if elapsed < self.target_frame_duration {
                    // Not enough time has passed, skip this frame
                    // Still request redraw for next opportunity
                    self.window.request_redraw();
                    return;
                }
            }
        }

        // Check if transcription mode has changed
        let current_mode = *self.transcription_mode_ref.lock();
        if current_mode != self.last_known_mode {
            self.button_manager.set_transcription_mode(current_mode);
            self.last_known_mode = current_mode;
        }

        // Update hover animation state
        let now = std::time::Instant::now();
        let delta_time = now.duration_since(self.last_hover_update).as_secs_f32();
        self.last_hover_update = now;

        // Smooth animation: 0.0 to 1.0 over ~300ms
        let animation_speed = 3.5; // Units per second
        if self.event_handler.hovering_transcript {
            // Fade in when hovering
            self.hover_animation_progress = (self.hover_animation_progress + delta_time * animation_speed).min(1.0);
        } else {
            // Fade out when not hovering
            self.hover_animation_progress = (self.hover_animation_progress - delta_time * animation_speed).max(0.0);
        }
        let output = match self.surface.get_current_texture() {
            Ok(output) => output,
            Err(wgpu::SurfaceError::Outdated | wgpu::SurfaceError::Lost) => {
                // Reconfigure the surface if it's outdated or lost
                self.surface.configure(&self.device, &self.config);
                match self.surface.get_current_texture() {
                    Ok(output) => output,
                    Err(e) => {
                        eprintln!("Failed to get surface texture after reconfigure: {:?}", e);
                        return;
                    }
                }
            }
            Err(wgpu::SurfaceError::Timeout) => {
                eprintln!("Surface texture acquisition timed out");
                return;
            }
            Err(wgpu::SurfaceError::OutOfMemory) => {
                panic!("GPU out of memory");
            }
            Err(e) => {
                eprintln!("Surface error: {:?}", e);
                return;
            }
        };
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        // First clear the screen to transparent
        self.render_pipelines.draw_background(&mut encoder, &view);

        // Update hover animation uniform buffer
        self.queue.write_buffer(
            &self.render_pipelines.hover_uniform_buffer,
            0,
            bytemuck::cast_slice(&[self.hover_animation_progress]),
        );

        // Draw the rounded rectangle background for the spectrogram only
        self.render_pipelines.draw_spectrogram_background(
            &mut encoder,
            &view,
            self.text_area_height,
            self.gap,
            self.spectrogram_width,
            self.spectrogram_height,
        );

        // Get audio data once
        let mut display_text: String = String::new();
        let mut is_speaking: bool = false;
        let empty_samples: Vec<f32> = vec![0.0; 1024]; // Buffer of silence for decay animation

        // Check recording state
        let is_recording = self
            .recording
            .as_ref()
            .map(|rec| rec.load(Ordering::Relaxed))
            .unwrap_or(false);

        // Update timer badge based on recording state
        if is_recording && !self.timer_badge.is_recording() {
            self.timer_badge.start_recording();
        } else if !is_recording && self.timer_badge.is_recording() {
            self.timer_badge.stop_recording();
        }

        // Determine if scrollbar is needed and the actual width to use for text area
        let mut need_scrollbar: bool = false;
        let mut text_area_width: u32;
        let text_area_height = self.layout_manager.get_text_area_height();

        // Always ensure the spectrogram is initialized
        if self.spectrogram.is_none() {
            let size = PhysicalSize::new(self.spectrogram_width, self.spectrogram_height);
            let spectrogram = Spectrogram::new(
                Arc::new(self.device.clone()),
                Arc::new(self.queue.clone()),
                size,
                self.config.format,
            );
            self.spectrogram = Some(spectrogram);
        }

        // Render the spectrogram with either the available audio data or empty data
        if let Some(spectrogram) = &mut self.spectrogram {
            let samples = if let Some(audio_data) = &self.audio_data {
                {
                    let audio_data_lock = audio_data.read();
                    // Always show the current samples - when paused, these will be the decaying samples
                    // The audio processor handles the decay animation, not the UI
                    is_speaking = is_recording && audio_data_lock.is_speaking; // Only show speaking state when recording
                    let transcript_ref = &audio_data_lock.transcript;
                    display_text = self.text_processor.clean_whitespace(transcript_ref);

                    // Convert to owned vector before dropping the lock
                    let samples_vec = audio_data_lock.samples.to_vec();
                    samples_vec
                }
            } else {
                if is_recording {
                    display_text = "Sonori is ready".to_string();
                }
                is_speaking = false;
                self.scroll_state.reset();
                need_scrollbar = false;
                text_area_width = self.layout_manager.calculate_text_area_width(false);
                self.scrollbar.max_scroll_offset = 0.0;
                self.scrollbar.scroll_offset = 0.0;
                empty_samples.clone() // Use silence buffer for decay animation
            };

            // Always update and render the spectrogram
            spectrogram.update(&samples);

            // Create a render pass with a viewport that positions the spectrogram below the text area
            {
                let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Spectrogram Render Pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load, // Load existing content
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });

                // Set the viewport using the layout manager
                let (x, y, width, height) = self.layout_manager.get_spectrogram_position();
                render_pass.set_viewport(x, y, width, height, 0.0, 1.0);

                // Use the custom render pass
                spectrogram.render_with_custom_pass(&mut render_pass);
            }
        }

        // Check if transcript has changed - only when recording
        let _transcript_changed = self.scroll_state.transcript_changed(display_text.len(), is_recording);
        if is_recording {
            self.scroll_state.update_transcript_len(display_text.len());
        }

        // Calculate text layout using the text processor
        let layout_info = self.text_processor.calculate_layout(
            &display_text,
            self.config.width as f32,
            text_area_height as f32,
        );

        need_scrollbar = layout_info.need_scrollbar;

        // Set text area width based on whether scrollbar is needed
        text_area_width = self
            .layout_manager
            .calculate_text_area_width(need_scrollbar);

        // Update scroll state
        self.scroll_state.set_max_scroll_offset(layout_info.max_scroll_offset);
        self.scroll_state.auto_scroll = self.event_handler.auto_scroll;

        // Update with auto-scroll animation
        self.scroll_state.update_with_auto_scroll();

        // Sync scrollbar state
        self.scrollbar.max_scroll_offset = self.scroll_state.max_scroll_offset;
        self.scrollbar.scroll_offset = self.scroll_state.scroll_offset;
        self.scrollbar.auto_scroll = self.scroll_state.auto_scroll;

        // Get text position from the layout manager
        let (text_x, text_y) = self.layout_manager.get_text_position(self.scroll_state.scroll_offset);

        // Calculate text scale with constrained growth to keep text smaller
        let base_width = 240.0;
        let max_scale = 1.4; // Reduced from 1.5 to 1.4 for better proportions
        let raw_scale = self.window_width as f32 / base_width;
        let text_scale = raw_scale.min(max_scale).max(0.85); // Increased minimum to 0.85x for better readability

        // Get current transcription mode
        let transcription_mode = *self.transcription_mode_ref.lock();

        // Check if we should show processing animation instead of text
        let (should_show_animation, processing_state) = if let Some(audio_data) = &self.audio_data {
            let audio_data_lock = audio_data.read();
            let state = audio_data_lock.processing_state;
            let is_empty = display_text.is_empty();
            drop(audio_data_lock);

            // Simplified visibility logic - only show loading animation
            let should_show = match (state, is_empty) {
                // Show loading animation when loading
                (crate::ui::common::ProcessingState::Loading, _) => true,
                // Show loading animation when transcribing with no text yet
                (crate::ui::common::ProcessingState::Transcribing, true) => true,
                // Don't show animations for any other states
                _ => false,
            };

            (should_show, state)
        } else {
            (false, crate::ui::common::ProcessingState::Idle)
        };

        // Trigger typewriter effect when transcription completes (manual mode)
        if self.typewriter_enabled && transcription_mode == crate::real_time_transcriber::TranscriptionMode::Manual {
            // Detect transition from Transcribing to Idle with text
            let state_transition = self.last_processing_state == crate::ui::common::ProcessingState::Transcribing
                && processing_state == crate::ui::common::ProcessingState::Idle
                && !display_text.is_empty();

            // Also detect when text content changes while idle (e.g., enhancement result)
            let text_changed = processing_state == crate::ui::common::ProcessingState::Idle
                && !display_text.is_empty()
                && display_text != self.typewriter.get_visible_text()
                && !self.typewriter.is_active();

            if state_transition || text_changed {
                self.typewriter.start(display_text.clone());
            }

            self.last_processing_state = processing_state;
        }

        // Get the text to display (may be typewriter-animated)
        let render_text = if self.typewriter.is_active() {
            self.typewriter.update().to_string()
        } else {
            display_text.clone()
        };

        // Choose text color based on speaking state
        let text_color = if should_show_animation {
            self.loading_animation.get_processing_color(processing_state)
        } else if is_speaking {
            [0.1, 0.9, 0.5, 1.0] // Brighter teal-green for better visibility
        } else {
            [1.0, 0.85, 0.15, 1.0] // Slightly warmer gold for better readability
        };

        // Render loading animation if processing, otherwise render text
        if should_show_animation {
            // Update loading animation state
            self.loading_animation.set_processing_state(processing_state);

            // Render text window background first
            self.text_window.render_background(
                &mut encoder,
                &view,
                text_area_width,
                text_area_height,
                self.gap,
                text_x,
                text_y,
                &self.render_pipelines.hover_bind_group,
            );

            // Render loading animation centered in the text area
            let center_x = text_x + text_area_width as f32 / 2.0;
            let center_y = text_y + text_area_height as f32 / 2.0;
            let animation_size = text_area_height as f32 * 0.6; // 60% of text area height

            self.loading_animation.render(
                &mut encoder,
                &view,
                center_x,
                center_y,
                animation_size,
                text_color,
            );

            // Render processing text below animation
            let processing_text = self.loading_animation.get_processing_text(processing_state, transcription_mode);
            let text_y_for_status = center_y + animation_size * 0.8; // Position below animation

            self.text_window.render_text_only(
                &mut encoder,
                &view,
                processing_text,
                text_area_width,
                text_area_height,
                self.gap,
                text_x,
                text_y_for_status,
                text_scale * 0.8, // Slightly smaller text for status
                text_color,
            );
        } else {
            // Render text window (background and text) normally
            self.text_window.render(
                &mut encoder,
                &view,
                &render_text,
                text_area_width,
                text_area_height,
                self.gap,
                text_x,
                text_y,
                text_scale,
                text_color,
                &self.render_pipelines.hover_bind_group,
            );
        }

        // Draw scrollbar only if needed
        if need_scrollbar {
            // Use the scrollbar component to render
            self.scrollbar.render(
                &view,
                &mut encoder,
                self.config.width,
                text_area_height,
                self.gap,
            );
        }

        // Update button panel animation based on hover state
        self.button_panel.set_visible(self.event_handler.hovering_transcript);
        self.button_panel.update();

        // Render the buttons after the text - only when hovering over transcript
        // First make sure the RecordToggle button texture is up-to-date
        if self.event_handler.hovering_transcript {
            // Update RecordToggle button texture based on recording state
            self.button_manager.update_record_toggle_button_texture();

            // Update tooltip state
            let hovered_button = self.button_manager.get_hovered_button();
            self.tooltip.update(hovered_button);

            // Get button panel bounds from button manager
            let bottom_button_bounds = self.button_manager.get_button_panel_bounds();
            let close_button_bounds = self.button_manager.get_close_button_panel_bounds();

            // Render button panel backgrounds before buttons
            // Two separate panels: one for bottom buttons, one for close button
            self.button_panel.render_with_bounds(&view, &mut encoder, bottom_button_bounds, &self.render_pipelines.hover_bind_group);
            self.button_panel.render_with_bounds(&view, &mut encoder, close_button_bounds, &self.render_pipelines.hover_bind_group);

            // Only render buttons when hovering over transcript area
            (&mut self.button_manager).render(&view, &mut encoder, true, &self.queue);

            // Render tooltip (after buttons, so it appears on top)
            self.tooltip.render(&view, &mut encoder, self.window_width, self.window_height);
        } else {
            // Not hovering, hide tooltip
            self.tooltip.update(None);
        }

        // Render timer badge overlay (bottom-right of text area)
        self.timer_badge.render(
            &mut encoder,
            &view,
            &self.queue,
            text_x,
            text_y,
            text_area_width as f32,
            text_area_height as f32,
            self.config.width,
            self.config.height,
        );

        // Submit all rendering commands
        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        // Update frame time for frame rate limiting
        if self.present_mode == wgpu::PresentMode::Immediate {
            self.last_frame_time = Some(std::time::Instant::now());
        }

        // ALWAYS request redraw to keep animation loop going
        // This ensures spectrogram decay animation continues when paused
        self.window.request_redraw();
    }

    pub fn handle_scroll(&mut self, delta: MouseScrollDelta) {
        self.event_handler
            .handle_scroll(&mut self.scroll_state.target_scroll_offset, self.scroll_state.max_scroll_offset, delta);
        self.scroll_state.auto_scroll = self.event_handler.auto_scroll;
        self.scrollbar.auto_scroll = self.scroll_state.auto_scroll;
        self.scrollbar.scroll_offset = self.scroll_state.scroll_offset;
        self.window.request_redraw();
    }

    pub fn handle_cursor_moved(&mut self, position: PhysicalPosition<f64>) {
        // Calculate text area dimensions
        let text_area_width = self
            .layout_manager
            .calculate_text_area_width(self.scroll_state.needs_scrollbar());
        let text_area_height = self.layout_manager.get_text_area_height();

        // Get window size
        let window_size = self.window.outer_size();

        // Update event handler and button states
        self.event_handler.handle_cursor_moved(
            position,
            text_area_width,
            text_area_height,
            window_size.width,
            window_size.height,
            &mut self.button_manager,
        );

        self.window.request_redraw();
    }

    pub fn handle_cursor_leave(&mut self) {
        // Explicitly handle cursor leaving the window
        self.event_handler
            .handle_cursor_leave(&mut self.button_manager);
        self.window.request_redraw();
    }

    pub fn handle_mouse_input(
        &mut self,
        button: MouseButton,
        state: ElementState,
        position: PhysicalPosition<f64>,
        event_loop: Option<&dyn ActiveEventLoop>,
    ) {
        let redraw_needed = self.event_handler.handle_mouse_input(
            button,
            state,
            position,
            &mut self.button_manager,
            &self.audio_data,
            &mut self.scroll_state.last_transcript_len,
            &mut self.scroll_state.scroll_offset,
            &mut self.scroll_state.max_scroll_offset,
            &self.running,
            event_loop,
        );

        if redraw_needed {
            self.window.request_redraw();
        }
    }

    pub fn copy_transcript(&self) {
        EventHandler::copy_transcript(&self.audio_data);
    }

    pub fn reset_transcript(&mut self) {
        EventHandler::reset_transcript(
            &self.audio_data,
            &mut self.scroll_state.last_transcript_len,
            &mut self.scroll_state.scroll_offset,
            &mut self.scroll_state.max_scroll_offset,
        );
    }

    pub fn toggle_recording(&mut self) {
        if let Some(recording) = &self.recording {
            // IMMEDIATE: Toggle recording state atomically (non-blocking)
            let was_recording = recording.load(Ordering::Relaxed);
            let new_state = !was_recording;
            recording.store(new_state, Ordering::Relaxed);

            // IMMEDIATE: Update button texture (local UI state, non-blocking)
            self.button_manager.update_record_toggle_button_texture();

            // The transcription systems will detect this change asynchronously
            // via their polling of the atomic flag - no blocking here
        }
    }

    pub fn toggle_manual_session(&mut self) {
        // IMMEDIATE: Check current state and send command asynchronously
        let is_currently_recording = self
            .recording
            .as_ref()
            .map(|rec| rec.load(Ordering::Relaxed))
            .unwrap_or(false);

        if let Some(sender) = &self.event_handler.manual_session_sender {
            let sender = sender.clone();
            // ASYNC: Send command without blocking UI thread
            tokio::spawn(async move {
                let command = if is_currently_recording {
                    crate::real_time_transcriber::ManualSessionCommand::StopSession {
                        responder: None,
                    }
                } else {
                    crate::real_time_transcriber::ManualSessionCommand::StartSession {
                        responder: None,
                    }
                };

                if let Err(e) = sender.send(command).await {
                    eprintln!("Failed to send manual session command: {}", e);
                }
            });
        } else {
            eprintln!("Manual session sender not available");
        }
        // UI thread continues immediately - manual session processor handles the command
    }

    pub fn toggle_mode(&mut self) {
        // Switch between manual and real-time modes
        let current_mode = *self.transcription_mode_ref.lock();
        let new_mode = match current_mode {
            crate::real_time_transcriber::TranscriptionMode::RealTime => {
                crate::real_time_transcriber::TranscriptionMode::Manual
            }
            crate::real_time_transcriber::TranscriptionMode::Manual => {
                crate::real_time_transcriber::TranscriptionMode::RealTime
            }
        };

        if let Some(sender) = &self.event_handler.manual_session_sender {
            let sender = sender.clone();
            tokio::spawn(async move {
                if let Err(e) = sender
                    .send(crate::real_time_transcriber::ManualSessionCommand::SwitchMode(new_mode))
                    .await
                {
                    eprintln!("Failed to send mode switch command from tray: {}", e);
                }
            });
        } else {
            eprintln!("Manual session sender not available for mode switching from tray");
        }
    }

    pub fn quit(&mut self) {
        if let Some(running) = &self.running {
            running.store(false, Ordering::Relaxed);
        }
    }
}
