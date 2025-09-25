use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalPosition, PhysicalSize},
    event::{ElementState, KeyEvent, Modifiers, WindowEvent},
    event_loop::{ActiveEventLoop, DeviceEvents, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    monitor::VideoModeHandle,
    platform::wayland::{
        ActiveEventLoopExtWayland, MonitorHandleExtWayland, WindowAttributesExtWayland,
    },
    window::{CursorIcon, WindowAttributes, WindowId},
};

use smithay_client_toolkit::shell::wlr_layer::{Anchor, KeyboardInteractivity, Layer};

use super::common::AudioVisualizationData;
use super::window::WindowState;

// Constants from window.rs
use super::window::{GAP, MARGIN, SPECTROGRAM_HEIGHT, SPECTROGRAM_WIDTH, TEXT_AREA_HEIGHT};
use crate::config::AppConfig;

pub fn run() {
    let event_loop = EventLoop::new().unwrap();
    let app_config = crate::config::AppConfig::default();
    let mut app = WindowApp {
        windows: HashMap::new(),
        audio_data: None,
        running: None,
        recording: None,
        current_modifiers: Modifiers::default(),
        config: app_config,
        manual_session_sender: None,
        transcription_mode_ref: Arc::new(parking_lot::Mutex::new(
            crate::real_time_transcriber::TranscriptionMode::RealTime,
        )),
    };
    event_loop.run_app(&mut app).unwrap();
}

pub fn run_with_audio_data(
    audio_data: Arc<RwLock<AudioVisualizationData>>,
    running: Arc<AtomicBool>,
    recording: Arc<AtomicBool>,
    config: AppConfig,
    manual_session_sender: Option<
        tokio::sync::mpsc::Sender<crate::real_time_transcriber::ManualSessionCommand>,
    >,
    transcription_mode_ref: Arc<
        parking_lot::Mutex<crate::real_time_transcriber::TranscriptionMode>,
    >,
) {
    let event_loop = EventLoop::new().unwrap();
    let mut app = WindowApp {
        windows: HashMap::new(),
        audio_data: Some(audio_data),
        running: Some(running),
        recording: Some(recording),
        current_modifiers: Modifiers::default(),
        config,
        manual_session_sender,
        transcription_mode_ref,
    };

    event_loop.run_app(&mut app).unwrap();
}

pub struct WindowApp {
    pub windows: HashMap<WindowId, WindowState>,
    pub audio_data: Option<Arc<RwLock<AudioVisualizationData>>>,
    pub running: Option<Arc<AtomicBool>>,
    pub recording: Option<Arc<AtomicBool>>,
    pub current_modifiers: Modifiers,
    pub config: AppConfig,
    pub manual_session_sender:
        Option<tokio::sync::mpsc::Sender<crate::real_time_transcriber::ManualSessionCommand>>,
    pub transcription_mode_ref:
        Arc<parking_lot::Mutex<crate::real_time_transcriber::TranscriptionMode>>,
}

impl ApplicationHandler for WindowApp {
    fn resumed(&mut self, event_loop: &dyn ActiveEventLoop) {
        // Check running flag on resume and exit if shutting down
        if let Some(running) = &self.running {
            if !running.load(std::sync::atomic::Ordering::Relaxed) {
                println!("App resumed but running flag is false - exiting event loop");
                event_loop.exit();
                return;
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &dyn ActiveEventLoop) {
        // Periodic check when event loop is idle - ensures shutdown happens even without events
        if let Some(running) = &self.running {
            if !running.load(std::sync::atomic::Ordering::Relaxed) {
                println!("Event loop idle but running flag is false - exiting event loop");
                event_loop.exit();
                return;
            }
        }
    }

    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        let window_attributes = WindowAttributes::default()
            .with_decorations(false)
            .with_transparent(true);

        if let Some((_, screen)) = event_loop
            .available_monitors()
            .into_iter()
            .enumerate()
            .next()
        {
            let Some(mode) = screen.current_video_mode() else {
                return;
            };
            let window_attributes = window_attributes.clone();
            let mut window_state = create_window(
                event_loop,
                window_attributes.with_title("Sonori"),
                1.0,
                mode,
                self.running.clone(),
                self.recording.clone(),
                *self.transcription_mode_ref.lock(),
                self.manual_session_sender.clone(),
                self.transcription_mode_ref.clone(),
            );

            if let Some(audio_data) = &self.audio_data {
                window_state.set_audio_data(audio_data.clone());
            }

            let window_id = window_state.window.id();
            self.windows.insert(window_id, window_state);
        }
    }

    fn window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        // IMMEDIATE: Check running flag and exit event loop if shutting down
        if let Some(running) = &self.running {
            if !running.load(std::sync::atomic::Ordering::Relaxed) {
                println!("Running flag is false - exiting event loop immediately");
                event_loop.exit();
                return;
            }
        }
        match event {
            WindowEvent::ModifiersChanged(modifiers) => {
                // Update modifiers without borrowing the window
                self.current_modifiers = modifiers;
                return;
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(key_code),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => {
                // Get ctrl state before borrowing window
                let ctrl_pressed = self.current_modifiers.state().control_key();

                if let Some(window) = self.windows.get_mut(&window_id) {
                    // Debug key press
                    println!("Key pressed: {:?}", key_code);

                    // Get keyboard shortcuts from config
                    let shortcuts = &self.config.keyboard_shortcuts;

                    // Check for copy transcript shortcut
                    if ctrl_pressed
                        && key_code
                            == shortcuts
                                .to_key_code(&shortcuts.copy_transcript)
                                .unwrap_or(KeyCode::KeyC)
                    {
                        println!("Copy transcript shortcut pressed, copying transcript");
                        window.copy_transcript();
                    }
                    // Check for reset transcript shortcut
                    else if ctrl_pressed
                        && key_code
                            == shortcuts
                                .to_key_code(&shortcuts.reset_transcript)
                                .unwrap_or(KeyCode::KeyR)
                    {
                        println!("Reset transcript shortcut pressed, resetting transcript");
                        window.reset_transcript();
                    }
                    // Check for toggle recording shortcut (Space) - only in real-time mode
                    else if key_code
                        == shortcuts
                            .to_key_code(&shortcuts.toggle_recording)
                            .unwrap_or(KeyCode::Space)
                    {
                        let current_mode = *self.transcription_mode_ref.lock();
                        if current_mode == crate::real_time_transcriber::TranscriptionMode::RealTime
                        {
                            println!("Space pressed in real-time mode, toggling recording");
                            window.toggle_recording();
                        } else {
                            println!("Space pressed in manual mode, ignoring (use Tab instead)");
                        }
                    }
                    // Check for Tab key in manual mode (manual session start/stop)
                    else if key_code == KeyCode::Tab {
                        let current_mode = *self.transcription_mode_ref.lock();
                        if current_mode == crate::real_time_transcriber::TranscriptionMode::Manual {
                            println!("Tab pressed in manual mode, toggling manual session");
                            window.toggle_manual_session();
                        } else {
                            println!("Tab pressed in real-time mode, ignoring (use Space instead)");
                        }
                    }
                    // Check for exit application shortcut
                    else if key_code
                        == shortcuts
                            .to_key_code(&shortcuts.exit_application)
                            .unwrap_or(KeyCode::Escape)
                    {
                        println!("Exit application shortcut pressed, initiating shutdown");
                        window.quit();
                    }
                }
                return;
            }
            _ => {}
        }

        // Handle other window events
        if let Some(window) = self.windows.get_mut(&window_id) {
            match event {
                WindowEvent::CloseRequested => {
                    println!("Window close requested");
                    // First quit to set the running flag to false
                    window.quit();
                    // Don't call event_loop.exit() here as it can cause segfaults
                    // The shutdown monitor task will detect the running flag and exit properly
                }
                WindowEvent::SurfaceResized(size) => {
                    window.resize(size.width, size.height);
                }
                WindowEvent::RedrawRequested => {
                    window.draw(window.config.width);
                }
                WindowEvent::MouseWheel { delta, .. } => {
                    window.handle_scroll(delta);
                }
                WindowEvent::PointerMoved { position, .. } => {
                    window.handle_cursor_moved(position);
                }
                WindowEvent::PointerButton {
                    button,
                    state,
                    position,
                    ..
                } => {
                    window.handle_mouse_input(
                        button.mouse_button(),
                        state,
                        position,
                        Some(event_loop),
                    );
                }
                WindowEvent::PointerLeft { .. } => {
                    window.handle_cursor_leave();
                }
                _ => {}
            }
        }
    }
}

fn create_window(
    ev: &dyn ActiveEventLoop,
    w: WindowAttributes,
    scale_factor: f64,
    monitor_mode: VideoModeHandle,
    running: Option<Arc<AtomicBool>>,
    recording: Option<Arc<AtomicBool>>,
    transcription_mode: crate::real_time_transcriber::TranscriptionMode,
    manual_session_sender: Option<
        tokio::sync::mpsc::Sender<crate::real_time_transcriber::ManualSessionCommand>,
    >,
    transcription_mode_ref: Arc<
        parking_lot::Mutex<crate::real_time_transcriber::TranscriptionMode>,
    >,
) -> WindowState {
    // Use spectrogram size plus text area height and gap
    let fixed_size = PhysicalSize::new(
        SPECTROGRAM_WIDTH,
        SPECTROGRAM_HEIGHT + TEXT_AREA_HEIGHT + GAP,
    );
    let logical_size = fixed_size.to_logical::<i32>(scale_factor);

    // Set the fixed size in the window attributes
    let w = w.with_surface_size(logical_size);

    // Determine keyboard interactivity mode: request exclusive keyboard when in manual mode
    let keyboard_mode =
        if transcription_mode == crate::real_time_transcriber::TranscriptionMode::Manual {
            KeyboardInteractivity::Exclusive
        } else {
            KeyboardInteractivity::OnDemand
        };

    let w = if ev.is_wayland() {
        // For Wayland, we need to specify the output (monitor)
        w.with_anchor(Anchor::BOTTOM)
            .with_layer(Layer::Overlay)
            .with_margin(MARGIN as i32, MARGIN as i32, MARGIN as i32, MARGIN as i32)
            .with_output(monitor_mode.monitor().native_id())
            .with_resizable(false)
            .with_keyboard_interactivity(keyboard_mode)
    } else {
        w.with_position(LogicalPosition::new(0, 0))
            .with_window_level(winit::window::WindowLevel::AlwaysOnTop)
            // Don't use fullscreen as it would override our fixed size
            // .with_fullscreen(Some(winit::window::Fullscreen::Borderless(None)))
            .with_resizable(false)
            .with_keyboard_interactivity(keyboard_mode)
    };

    ev.listen_device_events(DeviceEvents::Always);

    WindowState::new(
        ev.create_window(w.with_cursor(CursorIcon::Default))
            .unwrap(),
        running,
        recording,
        transcription_mode,
        manual_session_sender,
        transcription_mode_ref,
    )
}
