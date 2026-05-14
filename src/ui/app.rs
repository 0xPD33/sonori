use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use winit::{
    application::ApplicationHandler,
    cursor::CursorIcon,
    dpi::{LogicalPosition, LogicalSize, PhysicalSize},
    event::{DeviceEvent, DeviceId, ElementState, KeyEvent, Modifiers, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, DeviceEvents, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    monitor::{MonitorHandle, VideoMode},
    platform::wayland::ActiveEventLoopExtWayland,
    window::{WindowAttributes, WindowId},
};

use winit::platform::wayland::{Anchor, KeyboardInteractivity, Layer, WindowAttributesWayland};

use super::common::{AudioVisualizationData, BackendStatus};
use super::settings_window::SettingsWindow;
use super::window::WindowState;

// Constants from window.rs
use super::window::MARGIN;
use crate::config::{AppConfig, CustomWindowPosition, DisplayConfig, WindowPosition};

const DRAG_DEBUG_ENV: &str = "SONORI_DRAG_DEBUG";
const DRAG_RAW_MOTION_SCALE: f64 = 1.25;
const DRAG_CATCH_UP_FACTOR: f64 = 0.04;
const DRAG_MAX_CATCH_UP_PX: f64 = 2.0;
const DRAG_MAX_CATCH_UP_TO_RAW_RATIO: f64 = 0.25;
const DRAG_MAX_FRAME_DELTA_PX: f64 = 96.0;
const DRAG_EDGE_INSET_PX: i32 = 32;

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
        window_drag: None,
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
        window_drag: None,
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
    window_drag: Option<WindowDragState>,
}

#[derive(Debug, Clone, Copy)]
struct WindowDragState {
    window_id: WindowId,
    position: LogicalPosition<f64>,
    scale_factor: f64,
    grab_offset: LogicalPosition<f64>,
    latest_pointer_position: LogicalPosition<f64>,
    pending_delta_x: f64,
    pending_delta_y: f64,
    monitor_size: LogicalSize<u32>,
    window_size: LogicalSize<u32>,
    pointer_event_count: u32,
    raw_event_count: u32,
    update_count: u32,
    debug_enabled: bool,
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
            // Keep keyboard focus on the user's active app so recording shortcuts
            // do not cause the compositor to dismiss the settings layer surface.
            let wayland_attrs = WindowAttributesWayland::default()
                .with_layer_shell()
                .with_layer(Layer::Overlay)
                .with_exclusive_zone(-1)
                .with_keyboard_interactivity(KeyboardInteractivity::None);
            attrs = attrs.with_platform_attributes(Box::new(wayland_attrs));
        }

        // Get instance/device/queue/format from the main window to share GPU context
        let (instance, adapter, device, queue, format) = match self.windows.values().next() {
            Some(main_win) => (
                &main_win.instance,
                main_win.adapter.clone(),
                main_win.device.clone(),
                main_win.queue.clone(),
                main_win.config.format,
            ),
            None => return,
        };

        match event_loop.create_window(attrs) {
            Ok(window) => {
                let settings_win = match SettingsWindow::new(
                    window,
                    instance,
                    &adapter,
                    device,
                    queue,
                    format,
                    &self.config,
                    self.backend_command_tx.clone(),
                ) {
                    Ok(settings_win) => settings_win,
                    Err(e) => {
                        eprintln!("{}", e);
                        return;
                    }
                };
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

    fn drag_modifier_active(&self) -> bool {
        let modifiers = self.current_modifiers.state();
        modifiers.alt_key() || modifiers.meta_key()
    }

    fn begin_window_drag(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        window_id: WindowId,
        pointer_position: winit::dpi::PhysicalPosition<f64>,
    ) -> bool {
        let Some(window) = self.windows.get(&window_id) else {
            return false;
        };

        let Some(physical_monitor_size) = current_monitor_size(event_loop) else {
            return false;
        };

        let scale_factor = window.window.scale_factor();
        let monitor_size = physical_monitor_size.to_logical::<u32>(scale_factor);
        let window_size = LogicalSize::new(window.fixed_window_width, window.fixed_window_height);
        let start_position =
            configured_window_position(&self.config.display_config, monitor_size, window_size);

        if event_loop.is_wayland() {
            set_wayland_layer_custom_positioning(window.window.as_ref());
        }

        let pointer_position = pointer_position.to_logical(scale_factor);
        let debug_enabled = std::env::var_os(DRAG_DEBUG_ENV).is_some();

        if debug_enabled {
            eprintln!(
                "drag:start scale={scale_factor:.3} pos=({}, {}) pointer=({:.1}, {:.1}) monitor=({}, {}) window=({}, {})",
                start_position.x,
                start_position.y,
                pointer_position.x,
                pointer_position.y,
                monitor_size.width,
                monitor_size.height,
                window_size.width,
                window_size.height
            );
        }

        self.window_drag = Some(WindowDragState {
            window_id,
            position: LogicalPosition::new(start_position.x as f64, start_position.y as f64),
            scale_factor,
            grab_offset: pointer_position,
            latest_pointer_position: pointer_position,
            pending_delta_x: 0.0,
            pending_delta_y: 0.0,
            monitor_size,
            window_size,
            pointer_event_count: 0,
            raw_event_count: 0,
            update_count: 0,
            debug_enabled,
        });

        self.move_window_to(window_id, start_position);
        true
    }

    fn queue_window_drag_pointer_position(
        &mut self,
        window_id: WindowId,
        pointer_position: winit::dpi::PhysicalPosition<f64>,
    ) {
        let Some(window) = self.windows.get(&window_id) else {
            return;
        };

        if let Some(drag) = self.window_drag.as_mut() {
            if drag.window_id != window_id {
                return;
            }

            let pointer_position: LogicalPosition<f64> =
                pointer_position.to_logical(drag.scale_factor);
            drag.latest_pointer_position = pointer_position;
            drag.pointer_event_count = drag.pointer_event_count.saturating_add(1);

            window.window.request_redraw();
        }
    }

    fn queue_window_drag_raw_motion(&mut self, delta: (f64, f64)) {
        let Some(drag) = self.window_drag.as_mut() else {
            return;
        };

        drag.pending_delta_x += delta.0 * DRAG_RAW_MOTION_SCALE;
        drag.pending_delta_y += delta.1 * DRAG_RAW_MOTION_SCALE;
        drag.raw_event_count = drag.raw_event_count.saturating_add(1);

        if let Some(window) = self.windows.get(&drag.window_id) {
            window.window.request_redraw();
        }
    }

    fn apply_queued_window_drag(&mut self, window_id: WindowId) -> bool {
        let Some((window_id, position)) = self.window_drag.as_mut().and_then(|drag| {
            if drag.window_id != window_id {
                return None;
            }

            if drag.pending_delta_x == 0.0 && drag.pending_delta_y == 0.0 {
                return None;
            }

            let pointer_error_x = drag.latest_pointer_position.x - drag.grab_offset.x;
            let pointer_error_y = drag.latest_pointer_position.y - drag.grab_offset.y;
            let catch_up_x = bounded_drag_catch_up(pointer_error_x, drag.pending_delta_x);
            let catch_up_y = bounded_drag_catch_up(pointer_error_y, drag.pending_delta_y);
            let delta_x =
                (drag.pending_delta_x + catch_up_x).clamp(-DRAG_MAX_FRAME_DELTA_PX, DRAG_MAX_FRAME_DELTA_PX);
            let delta_y =
                (drag.pending_delta_y + catch_up_y).clamp(-DRAG_MAX_FRAME_DELTA_PX, DRAG_MAX_FRAME_DELTA_PX);

            drag.pending_delta_x = 0.0;
            drag.pending_delta_y = 0.0;

            drag.position.x += delta_x;
            drag.position.y += delta_y;
            drag.position =
                clamp_window_position_f64(drag.position, drag.monitor_size, drag.window_size);
            drag.update_count = drag.update_count.saturating_add(1);
            let rounded_position = round_window_position(drag.position);

            if drag.debug_enabled {
                eprintln!(
                    "drag:move update={} pointer_events={} raw_events={} raw=({:.1}, {:.1}) pointer=({:.1}, {:.1}) error=({:.1}, {:.1}) catch_up=({:.1}, {:.1}) applied=({:.1}, {:.1}) pos=({}, {})",
                    drag.update_count,
                    drag.pointer_event_count,
                    drag.raw_event_count,
                    delta_x - catch_up_x,
                    delta_y - catch_up_y,
                    drag.latest_pointer_position.x,
                    drag.latest_pointer_position.y,
                    pointer_error_x,
                    pointer_error_y,
                    catch_up_x,
                    catch_up_y,
                    delta_x,
                    delta_y,
                    rounded_position.x,
                    rounded_position.y
                );
            }

            Some((drag.window_id, rounded_position))
        }) else {
            return false;
        };

        self.move_window_to(window_id, position);
        true
    }

    fn end_window_drag(&mut self, window_id: WindowId) -> bool {
        self.apply_queued_window_drag(window_id);

        let Some(drag) = self.window_drag else {
            return false;
        };

        if drag.window_id != window_id {
            return false;
        }

        self.window_drag = None;
        let position = round_window_position(drag.position);
        if drag.debug_enabled {
            eprintln!(
                "drag:end pos=({}, {}) pointer_events={} raw_events={} updates={}",
                position.x,
                position.y,
                drag.pointer_event_count,
                drag.raw_event_count,
                drag.update_count
            );
        }
        self.persist_custom_window_position(position);
        true
    }

    fn move_window_to(&self, window_id: WindowId, position: LogicalPosition<i32>) {
        if let Some(window) = self.windows.get(&window_id) {
            if let Some(wayland_window) = window
                .window
                .as_ref()
                .cast_ref::<winit::platform::wayland::Window>()
            {
                wayland_window.set_anchor(Anchor::TOP | Anchor::LEFT);
                wayland_window.set_margin(position.y, 0, 0, position.x);
            } else {
                window.window.set_outer_position(position.into());
            }
            window.window.request_redraw();
        }
    }

    fn persist_custom_window_position(&mut self, position: LogicalPosition<i32>) {
        let custom_position = CustomWindowPosition {
            x: position.x,
            y: position.y,
        };

        self.config.display_config.window_position = WindowPosition::Custom;
        self.config.display_config.custom_window_position = Some(custom_position);

        let (mut app_config, _) = crate::config::read_app_config_with_path();
        app_config.display_config.window_position = WindowPosition::Custom;
        app_config.display_config.custom_window_position = Some(custom_position);

        if let Err(e) = crate::config::write_app_config(&app_config) {
            eprintln!("Failed to persist dragged window position: {}", e);
        }
    }

    fn apply_runtime_config(&mut self, event_loop: &dyn ActiveEventLoop, config: AppConfig) {
        let display_config = config.display_config.clone();
        let ui_config = config.ui_config.clone();
        self.config = config;

        let physical_monitor_size = current_monitor_size(event_loop);
        let window_ids: Vec<WindowId> = self.windows.keys().copied().collect();
        for window_id in window_ids {
            let position = {
                let Some(window) = self.windows.get_mut(&window_id) else {
                    continue;
                };

                window.apply_runtime_config(&display_config, &ui_config);

                let Some(physical_monitor_size) = physical_monitor_size else {
                    continue;
                };
                let scale_factor = window.window.scale_factor();
                let monitor_size = physical_monitor_size.to_logical::<u32>(scale_factor);
                let window_size =
                    LogicalSize::new(window.fixed_window_width, window.fixed_window_height);
                Some(configured_window_position(
                    &display_config,
                    monitor_size,
                    window_size,
                ))
            };

            if let Some(position) = position {
                self.move_window_to(window_id, position);
            }
        }
    }
}

fn current_monitor_size(event_loop: &dyn ActiveEventLoop) -> Option<PhysicalSize<u32>> {
    event_loop
        .available_monitors()
        .next()
        .and_then(|monitor| monitor.current_video_mode())
        .map(|mode| mode.size())
}

fn configured_window_position(
    display_config: &DisplayConfig,
    monitor_size: LogicalSize<u32>,
    window_size: LogicalSize<u32>,
) -> LogicalPosition<i32> {
    if display_config.window_position == WindowPosition::Custom {
        if let Some(position) = display_config.custom_window_position {
            return clamp_window_position(
                LogicalPosition::new(position.x, position.y),
                monitor_size,
                window_size,
            );
        }
    }

    preset_window_position(display_config.window_position, monitor_size, window_size)
}

fn preset_window_position(
    position: WindowPosition,
    monitor_size: LogicalSize<u32>,
    window_size: LogicalSize<u32>,
) -> LogicalPosition<i32> {
    let monitor_width = monitor_size.width as i32;
    let monitor_height = monitor_size.height as i32;
    let window_width = window_size.width as i32;
    let window_height = window_size.height as i32;

    let left = MARGIN;
    let center_x = (monitor_width - window_width) / 2;
    let right = monitor_width - window_width - MARGIN;
    let top = MARGIN;
    let center_y = (monitor_height - window_height) / 2;
    let bottom = monitor_height - window_height - MARGIN;

    let (x, y) = match position {
        WindowPosition::BottomLeft => (left, bottom),
        WindowPosition::BottomCenter => (center_x, bottom),
        WindowPosition::BottomRight => (right, bottom),
        WindowPosition::TopLeft => (left, top),
        WindowPosition::TopCenter => (center_x, top),
        WindowPosition::TopRight => (right, top),
        WindowPosition::MiddleLeft => (left, center_y),
        WindowPosition::MiddleCenter => (center_x, center_y),
        WindowPosition::MiddleRight => (right, center_y),
        WindowPosition::Custom => (center_x, bottom),
    };

    clamp_window_position(LogicalPosition::new(x, y), monitor_size, window_size)
}

fn clamp_window_position(
    position: LogicalPosition<i32>,
    monitor_size: LogicalSize<u32>,
    window_size: LogicalSize<u32>,
) -> LogicalPosition<i32> {
    let max_x = (monitor_size.width as i32 - window_size.width as i32 - DRAG_EDGE_INSET_PX).max(0);
    let max_y =
        (monitor_size.height as i32 - window_size.height as i32 - DRAG_EDGE_INSET_PX).max(0);
    let min_x = DRAG_EDGE_INSET_PX.min(max_x);
    let min_y = DRAG_EDGE_INSET_PX.min(max_y);

    LogicalPosition::new(
        position.x.clamp(min_x, max_x),
        position.y.clamp(min_y, max_y),
    )
}

fn clamp_window_position_f64(
    position: LogicalPosition<f64>,
    monitor_size: LogicalSize<u32>,
    window_size: LogicalSize<u32>,
) -> LogicalPosition<f64> {
    let inset = DRAG_EDGE_INSET_PX as f64;
    let max_x = (monitor_size.width as f64 - window_size.width as f64 - inset).max(0.0);
    let max_y = (monitor_size.height as f64 - window_size.height as f64 - inset).max(0.0);
    let min_x = inset.min(max_x);
    let min_y = inset.min(max_y);

    LogicalPosition::new(
        position.x.clamp(min_x, max_x),
        position.y.clamp(min_y, max_y),
    )
}

fn round_window_position(position: LogicalPosition<f64>) -> LogicalPosition<i32> {
    LogicalPosition::new(position.x.round() as i32, position.y.round() as i32)
}

fn bounded_drag_catch_up(pointer_error: f64, raw_delta: f64) -> f64 {
    if pointer_error == 0.0 || raw_delta == 0.0 || pointer_error.signum() != raw_delta.signum() {
        return 0.0;
    }

    let max_from_raw = raw_delta.abs() * DRAG_MAX_CATCH_UP_TO_RAW_RATIO;
    let correction = (pointer_error.abs() * DRAG_CATCH_UP_FACTOR)
        .min(DRAG_MAX_CATCH_UP_PX)
        .min(max_from_raw);

    raw_delta.signum() * correction
}

fn layer_shell_margin(
    display_config: &DisplayConfig,
    monitor_size: LogicalSize<u32>,
    window_size: LogicalSize<u32>,
) -> (i32, i32, i32, i32) {
    if display_config.window_position == WindowPosition::Custom {
        let position = configured_window_position(display_config, monitor_size, window_size);
        return (position.y, 0, 0, position.x);
    }

    (MARGIN, MARGIN, MARGIN, MARGIN)
}

fn set_wayland_layer_custom_positioning(window: &dyn winit::window::Window) {
    if let Some(wayland_window) = window.cast_ref::<winit::platform::wayland::Window>() {
        wayland_window.set_anchor(Anchor::TOP | Anchor::LEFT);
    }
}

impl ApplicationHandler for WindowApp {
    fn resumed(&mut self, event_loop: &dyn ActiveEventLoop) {
        // Check running flag on resume and exit if shutting down
        if let Some(running) = &self.running {
            if !running.load(std::sync::atomic::Ordering::Relaxed) {
                println!("App resumed but running flag is false - exiting event loop");
                event_loop.exit();
            }
        }
    }

    fn device_event(
        &mut self,
        _event_loop: &dyn ActiveEventLoop,
        _device_id: Option<DeviceId>,
        event: DeviceEvent,
    ) {
        if let DeviceEvent::PointerMotion { delta } = event {
            self.queue_window_drag_raw_motion(delta);
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

        if let Some((_, screen)) = event_loop.available_monitors().enumerate().next() {
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
                &self.config.ui_config,
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
            let mut applied_config = None;
            let mut close_settings = false;

            if let Some(sw) = &mut self.settings_window {
                match event {
                    WindowEvent::CloseRequested => {
                        // Layer-shell compositors may emit CloseRequested when focus moves away
                        // from the settings surface. Keep settings open unless the user closes it
                        // explicitly with Escape or the in-window close button.
                        sw.window.request_redraw();
                    }
                    WindowEvent::SurfaceResized(size) => {
                        sw.resize(size.width, size.height);
                    }
                    WindowEvent::RedrawRequested => {
                        sw.draw();
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
                            close_settings = true;
                        } else {
                            let shift = self.current_modifiers.state().shift_key();
                            if sw.handle_key(logical_key, shift) {
                                sw.window.request_redraw();
                            }
                        }
                    }
                    WindowEvent::PointerButton {
                        state, position, ..
                    } => {
                        if state == ElementState::Pressed {
                            sw.handle_click(position.x as f32, position.y as f32);
                            applied_config = sw.take_applied_config();
                            if sw.close_requested() {
                                close_settings = true;
                            }
                        } else if state == ElementState::Released {
                            sw.handle_mouse_release();
                        }
                    }
                    WindowEvent::PointerMoved { position, .. } => {
                        sw.handle_mouse_move(position.x as f32, position.y as f32);
                    }
                    _ => {}
                }
            }

            if close_settings {
                self.settings_window = None;
                self.settings_window_id = None;
            }
            if let Some(config) = applied_config {
                self.apply_runtime_config(event_loop, config);
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

        if let WindowEvent::PointerButton { button, state, .. } = &event {
            let mouse_button = (*button).mouse_button();
            if mouse_button == MouseButton::Left {
                if *state == ElementState::Released && self.end_window_drag(window_id) {
                    return;
                }

                if *state == ElementState::Pressed && self.drag_modifier_active() {
                    if let WindowEvent::PointerButton { position, .. } = &event {
                        if self.begin_window_drag(event_loop, window_id, *position) {
                            return;
                        }
                    }
                }
            }
        }

        if let WindowEvent::PointerMoved { position, .. } = &event {
            if self
                .window_drag
                .is_some_and(|drag| drag.window_id == window_id)
            {
                self.queue_window_drag_pointer_position(window_id, *position);
                return;
            }
        }

        if matches!(event, WindowEvent::RedrawRequested) {
            self.apply_queued_window_drag(window_id);
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
    ui_config: &crate::config::UiConfig,
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
    let text_area_height =
        ((logical_height as f32 * 0.66) as u32).saturating_sub(status_bar_height);
    let gap = 0u32;

    // Set the fixed size in the window attributes
    let mut w = w.with_surface_size(logical_size);
    let logical_monitor_size = monitor_size.to_logical::<u32>(scale_factor);
    let positioning_window_size = LogicalSize::new(logical_width, logical_height);
    let initial_position = configured_window_position(
        display_config,
        logical_monitor_size,
        positioning_window_size,
    );

    // TEMPORARY: Use OnDemand to restore Tab key functionality while debugging portal
    // TODO: Switch to None once portal works (None prevents window from stealing keys)
    let keyboard_mode = KeyboardInteractivity::OnDemand;

    if ev.is_wayland() {
        // For Wayland, create platform-specific attributes using WindowAttributesWayland
        // Get anchor from display configuration
        let anchor = display_config.window_position.to_wayland_anchor();
        let (top_margin, right_margin, bottom_margin, left_margin) = layer_shell_margin(
            display_config,
            logical_monitor_size,
            positioning_window_size,
        );

        let wayland_attrs = WindowAttributesWayland::default()
            .with_layer_shell()
            .with_anchor(anchor)
            .with_layer(Layer::Overlay)
            .with_margin(top_margin, right_margin, bottom_margin, left_margin)
            // FIXME: Specifying output causes crashes on niri - let compositor choose
            // .with_output(monitor.native_id())
            .with_keyboard_interactivity(keyboard_mode);

        w = w
            .with_platform_attributes(Box::new(wayland_attrs))
            .with_resizable(false);
    } else {
        w = w
            .with_position(initial_position)
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
        ui_config,
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
