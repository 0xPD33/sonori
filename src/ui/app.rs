use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use winit::{
    application::ApplicationHandler,
    cursor::CursorIcon,
    dpi::{LogicalPosition, PhysicalSize},
    event::{ElementState, KeyEvent, Modifiers, WindowEvent},
    event_loop::{ActiveEventLoop, DeviceEvents, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    monitor::{MonitorHandle, VideoMode},
    platform::wayland::ActiveEventLoopExtWayland,
    window::{WindowAttributes, WindowId},
};

use winit::platform::wayland::{KeyboardInteractivity, Layer, WindowAttributesWayland};

use super::common::{AudioVisualizationData, BackendStatus};
use super::settings_window::SettingsWindow;
use super::window::WindowState;

// Constants from window.rs
use super::window::MARGIN;
use crate::config::AppConfig;

pub fn run() {
    let event_loop = EventLoop::new()
        .expect("Failed to create event loop. Ensure a display server (Wayland/X11) is available.");
    let app_config = crate::config::AppConfig::default();
    let mut app = WindowApp {
        windows: HashMap::new(),
        audio_data: None,
        running: None,
        recording: None,
        magic_mode_enabled: None,
        current_modifiers: Modifiers::default(),
        config: app_config,
        manual_session_sender: None,
        transcription_mode_ref: Arc::new(AtomicU8::new(
            crate::real_time_transcriber::TranscriptionMode::RealTime.as_u8(),
        )),
        tray_update_tx: None,
        tray_command_rx: None,
        backend_status: None,
        backend_command_tx: None,
        settings_window: None,
        settings_window_id: None,
    };
    event_loop
        .run_app(&mut app)
        .expect("Event loop exited with error");
}

pub fn run_with_audio_data(
    audio_data: Arc<RwLock<AudioVisualizationData>>,
    running: Arc<AtomicBool>,
    recording: Arc<AtomicBool>,
    magic_mode_enabled: Arc<AtomicBool>,
    config: AppConfig,
    manual_session_sender: Option<
        tokio::sync::mpsc::Sender<crate::real_time_transcriber::ManualSessionCommand>,
    >,
    transcription_mode_ref: Arc<AtomicU8>,
    tray_update_tx: Option<tokio::sync::mpsc::UnboundedSender<crate::system_tray::TrayUpdate>>,
    tray_command_rx: Option<tokio::sync::mpsc::UnboundedReceiver<crate::system_tray::TrayCommand>>,
    backend_status: Option<Arc<RwLock<BackendStatus>>>,
    backend_command_tx: Option<
        tokio::sync::mpsc::UnboundedSender<crate::backend_manager::BackendCommand>,
    >,
) {
    let event_loop = EventLoop::new()
        .expect("Failed to create event loop. Ensure a display server (Wayland/X11) is available.");
    let mut app = WindowApp {
        windows: HashMap::new(),
        audio_data: Some(audio_data),
        running: Some(running),
        recording: Some(recording),
        magic_mode_enabled: Some(magic_mode_enabled),
        current_modifiers: Modifiers::default(),
        config,
        manual_session_sender,
        transcription_mode_ref,
        tray_update_tx,
        tray_command_rx,
        backend_status,
        backend_command_tx,
        settings_window: None,
        settings_window_id: None,
    };

    event_loop
        .run_app(&mut app)
        .expect("Event loop exited with error");
}

pub struct WindowApp {
    pub windows: HashMap<WindowId, WindowState>,
    pub audio_data: Option<Arc<RwLock<AudioVisualizationData>>>,
    pub running: Option<Arc<AtomicBool>>,
    pub recording: Option<Arc<AtomicBool>>,
    pub magic_mode_enabled: Option<Arc<AtomicBool>>,
    pub current_modifiers: Modifiers,
    pub config: AppConfig,
    pub manual_session_sender:
        Option<tokio::sync::mpsc::Sender<crate::real_time_transcriber::ManualSessionCommand>>,
    pub transcription_mode_ref: Arc<AtomicU8>,
    pub tray_update_tx: Option<tokio::sync::mpsc::UnboundedSender<crate::system_tray::TrayUpdate>>,
    pub tray_command_rx:
        Option<tokio::sync::mpsc::UnboundedReceiver<crate::system_tray::TrayCommand>>,
    pub backend_status: Option<Arc<RwLock<BackendStatus>>>,
    pub backend_command_tx:
        Option<tokio::sync::mpsc::UnboundedSender<crate::backend_manager::BackendCommand>>,
    pub settings_window: Option<SettingsWindow>,
    pub settings_window_id: Option<WindowId>,
}

impl WindowApp {
    fn open_settings_window(&mut self, event_loop: &dyn ActiveEventLoop) {
        if self.settings_window.is_some() {
            return;
        }

        let mut attrs = WindowAttributes::default()
            .with_title("Sonori Settings")
            .with_surface_size(winit::dpi::LogicalSize::new(400, 500))
            .with_decorations(false)
            .with_transparent(false)
            .with_resizable(false);

        if event_loop.is_wayland() {
            // Must use layer-shell for the settings window too, because this winit
            // fork doesn't deliver pointer events to non-layer-shell windows.
            // No anchor = compositor centers the surface.
            let wayland_attrs = WindowAttributesWayland::default()
                .with_layer_shell()
                .with_layer(Layer::Overlay)
                .with_exclusive_zone(-1)
                .with_keyboard_interactivity(KeyboardInteractivity::OnDemand);
            attrs = attrs.with_platform_attributes(Box::new(wayland_attrs));
        }

        // Get instance/device/queue/format from the main window to share GPU context
        let (instance, device, queue, format) = match self.windows.values().next() {
            Some(main_win) => (
                &main_win.instance,
                main_win.device.clone(),
                main_win.queue.clone(),
                main_win.config.format,
            ),
            None => return,
        };

        match event_loop.create_window(attrs) {
            Ok(window) => {
                let settings_win = SettingsWindow::new(
                    window,
                    instance,
                    device,
                    queue,
                    format,
                    self.backend_command_tx.clone(),
                );
                let id = settings_win.window.id();
                self.settings_window_id = Some(id);
                self.settings_window = Some(settings_win);
            }
            Err(e) => {
                eprintln!("Failed to create settings window: {}", e);
            }
        }
    }

    fn notify_tray_about_recording(&self) {
        if let (Some(recording_flag), Some(tray_tx)) = (&self.recording, &self.tray_update_tx) {
            let is_recording = recording_flag.load(Ordering::Relaxed);
            let _ = tray_tx.send(crate::system_tray::TrayUpdate::Recording(is_recording));
        }
    }
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
        // Poll tokio runtime to allow async tasks to progress
        tokio::task::spawn(async {
            tokio::task::yield_now().await;
        });

        // Periodic check when event loop is idle - ensures shutdown happens even without events
        if let Some(running) = &self.running {
            if !running.load(std::sync::atomic::Ordering::Relaxed) {
                println!("Event loop idle but running flag is false - exiting event loop");
                event_loop.exit();
                return;
            }
        }

        // Process tray commands if available
        if let Some(tray_rx) = &mut self.tray_command_rx {
            let mut notify_recording = false;
            while let Ok(command) = tray_rx.try_recv() {
                match command {
                    crate::system_tray::TrayCommand::ToggleRecording => {
                        // Toggle recording in real-time mode
                        for window in self.windows.values_mut() {
                            window.toggle_recording();
                        }
                        notify_recording = true;
                    }
                    crate::system_tray::TrayCommand::ToggleManualSession => {
                        // Toggle manual session in manual mode
                        for window in self.windows.values_mut() {
                            window.toggle_manual_session();
                        }
                        notify_recording = true;
                    }
                    crate::system_tray::TrayCommand::SwitchMode => {
                        // Switch between manual and real-time modes
                        for window in self.windows.values_mut() {
                            window.toggle_mode();
                        }
                        notify_recording = true;
                    }
                    crate::system_tray::TrayCommand::Quit => {
                        println!("Quit requested from system tray");
                        if let Some(running) = &self.running {
                            running.store(false, std::sync::atomic::Ordering::Relaxed);
                        }
                        event_loop.exit();
                    }
                }
            }
            if notify_recording {
                self.notify_tray_about_recording();
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
            let backend_name = match self.config.backend_config.backend {
                crate::backend::BackendType::CTranslate2 => "CTranslate2",
                crate::backend::BackendType::WhisperCpp => "WhisperCpp",
                crate::backend::BackendType::Moonshine => "Moonshine",
                crate::backend::BackendType::Parakeet => "Parakeet",
            }
            .to_string();
            let model_name = self.config.general_config.model.clone();
            let mut window_state = create_window(
                event_loop,
                window_attributes.with_title("Sonori"),
                1.0,
                mode,
                screen,
                self.running.clone(),
                self.recording.clone(),
                self.magic_mode_enabled.clone(),
                crate::real_time_transcriber::TranscriptionMode::from_u8(
                    self.transcription_mode_ref.load(Ordering::Relaxed),
                ),
                self.manual_session_sender.clone(),
                self.transcription_mode_ref.clone(),
                &self.config.display_config,
                self.config.enhancement_config.enabled,
                &backend_name,
                &model_name,
                self.backend_status.clone(),
                self.backend_command_tx.clone(),
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
        // Route events to settings window if it's the target
        if Some(window_id) == self.settings_window_id {
            if let Some(sw) = &mut self.settings_window {
                match event {
                    WindowEvent::CloseRequested => {
                        self.settings_window = None;
                        self.settings_window_id = None;
                        return;
                    }
                    WindowEvent::SurfaceResized(size) => {
                        sw.resize(size.width, size.height);
                        return;
                    }
                    WindowEvent::RedrawRequested => {
                        sw.draw();
                        return;
                    }
                    WindowEvent::KeyboardInput {
                        event:
                            KeyEvent {
                                state: ElementState::Pressed,
                                ref logical_key,
                                physical_key: PhysicalKey::Code(key_code),
                                ..
                            },
                        ..
                    } => {
                        if key_code == KeyCode::Escape {
                            self.settings_window = None;
                            self.settings_window_id = None;
                        } else {
                            let shift = self.current_modifiers.state().shift_key();
                            if sw.handle_key(logical_key, shift) {
                                sw.window.request_redraw();
                            }
                        }
                        return;
                    }
                    WindowEvent::PointerButton {
                        state,
                        position,
                        ..
                    } => {
                        if state == ElementState::Pressed {
                            sw.handle_click(position.x as f32, position.y as f32);
                            if sw.close_requested() {
                                self.settings_window = None;
                                self.settings_window_id = None;
                                return;
                            }
                        } else if state == ElementState::Released {
                            sw.handle_mouse_release();
                        }
                        return;
                    }
                    WindowEvent::PointerMoved { position, .. } => {
                        sw.handle_mouse_move(position.x as f32, position.y as f32);
                        return;
                    }
                    _ => {
                        return;
                    }
                }
            }
            return;
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
                if let Some(window) = self.windows.get_mut(&window_id) {
                    // Tab - Toggle manual session (temporary, works when window focused)
                    // TODO: Once global shortcut (Super+Tab) works unfocused, remove this
                    if key_code == KeyCode::Tab {
                        let current_mode = crate::real_time_transcriber::TranscriptionMode::from_u8(
                            self.transcription_mode_ref.load(Ordering::Relaxed),
                        );
                        if current_mode == crate::real_time_transcriber::TranscriptionMode::Manual {
                            window.toggle_manual_session();
                        }
                    }
                }
                return;
            }
            _ => {}
        }

        // Handle other window events
        if let Some(window) = self.windows.get_mut(&window_id) {
            let mut should_notify_recording = false;
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
                    should_notify_recording = true;
                }
                WindowEvent::PointerLeft { .. } => {
                    window.handle_cursor_leave();
                }
                _ => {}
            }

            if should_notify_recording {
                self.notify_tray_about_recording();
            }
        }

        // Check if settings was requested (outside the window borrow scope)
        let settings_requested = self
            .windows
            .get_mut(&window_id)
            .map(|w| w.check_settings_requested())
            .unwrap_or(false);
        if settings_requested {
            self.open_settings_window(event_loop);
        }
    }
}

fn create_window(
    ev: &dyn ActiveEventLoop,
    w: WindowAttributes,
    scale_factor: f64,
    monitor_mode: VideoMode,
    _monitor: MonitorHandle,
    running: Option<Arc<AtomicBool>>,
    recording: Option<Arc<AtomicBool>>,
    magic_mode_enabled: Option<Arc<AtomicBool>>,
    transcription_mode: crate::real_time_transcriber::TranscriptionMode,
    manual_session_sender: Option<
        tokio::sync::mpsc::Sender<crate::real_time_transcriber::ManualSessionCommand>,
    >,
    transcription_mode_ref: Arc<AtomicU8>,
    display_config: &crate::config::DisplayConfig,
    enhancement_enabled: bool,
    backend_name: &str,
    model_name: &str,
    backend_status: Option<Arc<RwLock<BackendStatus>>>,
    backend_command_tx: Option<
        tokio::sync::mpsc::UnboundedSender<crate::backend_manager::BackendCommand>,
    >,
) -> WindowState {
    // Get monitor dimensions from video mode
    let monitor_size = monitor_mode.size();
    let monitor_width = monitor_size.width;
    let monitor_height = monitor_size.height;

    // Calculate 10% of screen size
    let window_width = (monitor_width as f32 * 0.10) as u32;
    let window_height = (monitor_height as f32 * 0.10) as u32;

    // Ensure minimum viable size (240x174 as current minimum)
    let window_width = window_width.max(240);
    let window_height = window_height.max(174);

    let dynamic_size = PhysicalSize::new(window_width, window_height);
    let logical_size = dynamic_size.to_logical::<i32>(scale_factor);

    // Calculate layout based on logical size (what the surface will actually be)
    let logical_width = logical_size.width.max(240) as u32;
    let logical_height = logical_size.height.max(174) as u32;

    // Calculate proportional layout (make spectrogram more rectangular)
    let spectrogram_width = logical_width;
    let spectrogram_height = (logical_height as f32 * 0.32) as u32;
    let status_bar_height = 20u32;
    let text_area_height = ((logical_height as f32 * 0.66) as u32).saturating_sub(status_bar_height);
    let gap = 0u32;

    // Set the fixed size in the window attributes
    let mut w = w.with_surface_size(logical_size);

    // TEMPORARY: Use OnDemand to restore Tab key functionality while debugging portal
    // TODO: Switch to None once portal works (None prevents window from stealing keys)
    let keyboard_mode = KeyboardInteractivity::OnDemand;

    if ev.is_wayland() {
        // For Wayland, create platform-specific attributes using WindowAttributesWayland
        // Get anchor from display configuration
        let anchor = display_config.window_position.to_wayland_anchor();

        let wayland_attrs = WindowAttributesWayland::default()
            .with_layer_shell()
            .with_anchor(anchor)
            .with_layer(Layer::Overlay)
            .with_margin(MARGIN as i32, MARGIN as i32, MARGIN as i32, MARGIN as i32)
            // FIXME: Specifying output causes crashes on niri - let compositor choose
            // .with_output(monitor.native_id())
            .with_keyboard_interactivity(keyboard_mode);

        w = w
            .with_platform_attributes(Box::new(wayland_attrs))
            .with_resizable(false);
    } else {
        w = w
            .with_position(LogicalPosition::new(0, 0))
            .with_window_level(winit::window::WindowLevel::AlwaysOnTop)
            // Don't use fullscreen as it would override our fixed size
            // .with_fullscreen(Some(winit::window::Fullscreen::Borderless(None)))
            .with_resizable(false);
    }

    w = w.with_cursor(CursorIcon::Default);

    ev.listen_device_events(DeviceEvents::Always);

    WindowState::new(
        ev.create_window(w)
            .expect("Failed to create application window"),
        running,
        recording,
        magic_mode_enabled,
        transcription_mode,
        manual_session_sender,
        transcription_mode_ref,
        display_config,
        logical_width,
        logical_height,
        spectrogram_width,
        spectrogram_height,
        text_area_height,
        gap,
        enhancement_enabled,
        backend_name,
        model_name,
        backend_status,
        backend_command_tx,
    )
}
