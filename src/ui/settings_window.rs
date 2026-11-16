//! The Settings window: a movable frosted-glass layer surface. Every change
//! saves at once; see `docs/design/settings.md`.

mod model;
mod view;

pub(crate) use model::fit_language;

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Instant;

use parking_lot::RwLock;
use speechcore::{AudioVisualizationData, BackendStatus, TranscriptionMode};
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{KeyEvent, MouseScrollDelta};
use winit::platform::wayland::Anchor;
use winit::window::Window;

use super::shapes::ShapeRenderer;
use super::text_renderer::TextRenderer;
use crate::config::AppConfig;
use crate::hotkey::SharedHotkeyState;
use crate::ipc::AppCommand;
use model::{Change, Effect};
use view::{Action, Context, View, HEIGHT, MARGIN, WIDTH};

/// Lowest peak the meter measures against, so room noise stays small.
const PEAK_FLOOR: f32 = 0.02;
/// A line of the scroll wheel, in logical px.
const WHEEL_LINE: f32 = 40.0;
/// Part of the window, in logical px, that stays on the output while dragging.
const KEEP_VISIBLE: f64 = 120.0;

/// Size of the layer surface: the window and room for its shadow.
pub fn surface_size() -> LogicalSize<f64> {
    LogicalSize::new(
        (WIDTH + 2.0 * MARGIN) as f64,
        (HEIGHT + 2.0 * MARGIN) as f64,
    )
}

/// Shared state the window reads and the channels it acts through.
pub struct SettingsHandles {
    pub backend_command_tx: Option<tokio::sync::mpsc::UnboundedSender<speechcore::BackendCommand>>,
    pub backend_status: Option<Arc<RwLock<BackendStatus>>>,
    pub hotkey_state: SharedHotkeyState,
    pub app_tx: tokio::sync::mpsc::UnboundedSender<AppCommand>,
    pub recording: Option<Arc<AtomicBool>>,
    pub audio_data: Option<Arc<RwLock<AudioVisualizationData>>>,
    pub magic_mode_enabled: Option<Arc<AtomicBool>>,
    pub transcription_mode: Arc<AtomicU8>,
}

/// A window drag in progress. Positions are logical px.
struct Drag {
    /// Where the pointer pressed, relative to the surface.
    grab: (f64, f64),
    /// Latest pointer position relative to the surface.
    latest: (f64, f64),
    /// Relative pointer motion not applied yet.
    pending: (f64, f64),
    raw_events: u32,
}

pub struct SettingsWindow {
    // Dropped before `window`, which it borrows.
    surface: wgpu::Surface<'static>,
    pub window: Arc<dyn Window>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    shapes: ShapeRenderer,
    text: TextRenderer,
    text_size: PhysicalSize<u32>,
    view: View,
    /// The config the window shows, with Magic Mode and listening taken from
    /// the running app.
    app_config: AppConfig,
    handles: SettingsHandles,
    /// Found once: the lookup logs when `SONORI_CONFIG_PATH` is set.
    config_path: String,
    applied_config: Option<AppConfig>,
    mode_request: Option<TranscriptionMode>,
    close_requested: bool,
    /// Top-left corner on the output in logical px; `None` while the compositor
    /// centres the window.
    position: Option<(f64, f64)>,
    drag: Option<Drag>,
    pointer: (f32, f32),
    peak: f32,
    /// Runtime state the last frame showed: recording, Magic Mode, listening mode.
    shown: (bool, bool, u8),
    started: Instant,
}

impl SettingsWindow {
    pub fn new(
        window: Box<dyn Window>,
        instance: &wgpu::Instance,
        adapter: &wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,
        initial_config: &AppConfig,
        handles: SettingsHandles,
        position: Option<(f64, f64)>,
    ) -> Result<Self, String> {
        let window: Arc<dyn Window> = Arc::from(window);
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| format!("Failed to create GPU surface for settings window: {e}"))?;
        let caps = surface.get_capabilities(adapter);
        // Non-sRGB, so blending matches the compositor's and colours are sRGB values.
        let format = [
            wgpu::TextureFormat::Bgra8Unorm,
            wgpu::TextureFormat::Rgba8Unorm,
        ]
        .into_iter()
        .find(|f| caps.formats.contains(f))
        .ok_or_else(|| {
            format!(
                "No 8-bit surface format for settings among {:?}",
                caps.formats
            )
        })?;
        let alpha_mode = [
            wgpu::CompositeAlphaMode::PreMultiplied,
            wgpu::CompositeAlphaMode::PostMultiplied,
            wgpu::CompositeAlphaMode::Inherit,
        ]
        .into_iter()
        .find(|m| caps.alpha_modes.contains(m))
        .unwrap_or(caps.alpha_modes[0]);
        let size = window.surface_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        // Presses on the shadow around the glass reach the windows below.
        if let Some(wayland) = window.cast_ref::<winit::platform::wayland::Window>() {
            if let Ok(region) = wayland.create_region() {
                region.add(MARGIN as i32, MARGIN as i32, WIDTH as i32, HEIGHT as i32);
                wayland.set_region(Some(&region));
            }
        }

        let shapes = ShapeRenderer::new(&device, format);
        let text = TextRenderer::with_color_mode(
            Arc::new(device.clone()),
            Arc::new(queue.clone()),
            size,
            format,
            glyphon::ColorMode::Web,
        );
        let now = Instant::now();
        window.request_redraw();
        Ok(Self {
            surface,
            window,
            device,
            queue,
            config,
            shapes,
            text,
            text_size: size,
            view: View::new(now),
            app_config: initial_config.clone(),
            handles,
            config_path: crate::config::config_path_for_display(),
            applied_config: None,
            mode_request: None,
            close_requested: false,
            position,
            drag: None,
            pointer: (0.0, 0.0),
            peak: PEAK_FLOOR,
            shown: (false, false, 0),
            started: now,
        })
    }

    /// Where the window sits on its output, if the user moved it.
    pub fn position(&self) -> Option<(f64, f64)> {
        self.position
    }

    pub fn close_requested(&self) -> bool {
        self.close_requested
    }

    pub fn take_applied_config(&mut self) -> Option<AppConfig> {
        self.applied_config.take()
    }

    /// The listening mode the user picked, for the app to switch to.
    pub fn take_mode_request(&mut self) -> Option<TranscriptionMode> {
        self.mode_request.take()
    }

    /// Shows a config the app applied, e.g. from the quick menu.
    pub fn sync_config(&mut self, config: &AppConfig) {
        self.app_config = config.clone();
        self.window.request_redraw();
    }

    /// Redraws when runtime state that the window shows changed.
    pub fn poll(&mut self) {
        if self.runtime_state() != self.shown {
            self.window.request_redraw();
        }
    }

    fn runtime_state(&self) -> (bool, bool, u8) {
        let load = |flag: &Option<Arc<AtomicBool>>| {
            flag.as_ref().is_some_and(|f| f.load(Ordering::Relaxed))
        };
        (
            load(&self.handles.recording),
            load(&self.handles.magic_mode_enabled),
            self.handles.transcription_mode.load(Ordering::Relaxed),
        )
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.window.request_redraw();
    }

    pub fn draw(&mut self) {
        self.apply_drag();
        let frame = match self.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(wgpu::SurfaceError::Outdated | wgpu::SurfaceError::Lost) => {
                self.surface.configure(&self.device, &self.config);
                self.window.request_redraw();
                return;
            }
            Err(_) => return,
        };
        let size = PhysicalSize::new(self.config.width, self.config.height);
        if self.text_size != size {
            self.text.resize(size);
            self.text_size = size;
        }
        let now = Instant::now();
        let shown = self.runtime_state();
        self.shown = shown;
        self.app_config.enhancement_config.active = shown.1;
        self.app_config.general_config.transcription_mode =
            match TranscriptionMode::from_u8(shown.2) {
                TranscriptionMode::Manual => "manual",
                TranscriptionMode::RealTime => "realtime",
            }
            .to_string();
        let level = shown.0.then(|| self.mic_level());
        let hotkey = self.handles.hotkey_state.read();
        let cx = Context {
            config: &self.app_config,
            hotkey: hotkey.trigger().map(str::to_string),
            hotkey_note: hotkey.describe(),
            config_path: self.config_path.clone(),
            history_path: home_relative(&self.app_config.history_config.path),
            level,
            sf: self.window.scale_factor() as f32,
            now,
        };
        drop(hotkey);
        let built = self.view.build(&cx, &mut self.text);

        let (width, height) = (size.width as f32, size.height as f32);
        self.shapes.upload(
            &self.device,
            &self.queue,
            [width, height],
            now.duration_since(self.started).as_secs_f32(),
            &built.shapes,
        );
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut first_slot = 0;
        for (i, layer) in built.layers.iter().enumerate() {
            if i > 0 && layer.batches.is_empty() && layer.labels.is_empty() {
                continue;
            }
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Settings Encoder"),
                });
            {
                let load = if i == 0 {
                    wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                } else {
                    wgpu::LoadOp::Load
                };
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Settings Pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                for (range, clip) in &layer.batches {
                    let Some([x, y, w, h]) = scissor(*clip, size) else {
                        continue;
                    };
                    pass.set_scissor_rect(x, y, w, h);
                    self.shapes.draw(&mut pass, range.clone());
                }
            }
            self.text
                .render_label_layer(&view, &mut encoder, &layer.labels, first_slot);
            first_slot += layer.labels.len();
            // Each layer prepares glyphon again, so it must reach the GPU first.
            self.queue.submit(Some(encoder.finish()));
        }
        frame.present();
        self.text.trim_atlas();
        if built.animating || self.drag.is_some() {
            self.window.request_redraw();
        }
    }

    /// RMS of the newest samples relative to the recent peak, as the cursor tile measures it.
    fn mic_level(&mut self) -> f32 {
        let Some(audio) = &self.handles.audio_data else {
            return 0.0;
        };
        let rms = {
            let data = audio.read();
            let tail = &data.samples[data.samples.len().saturating_sub(512)..];
            if tail.is_empty() {
                return 0.0;
            }
            (tail.iter().map(|s| s * s).sum::<f32>() / tail.len() as f32).sqrt()
        };
        self.peak = (self.peak * 0.985).max(rms).max(PEAK_FLOOR);
        (((rms / self.peak - 0.08) / 0.92).clamp(0.0, 1.0)).powf(0.7)
    }

    pub fn pointer_moved(&mut self, position: PhysicalPosition<f64>) {
        self.pointer = (position.x as f32, position.y as f32);
        if let Some(drag) = self.drag.as_mut() {
            let sf = self.window.scale_factor();
            drag.latest = (position.x / sf, position.y / sf);
            self.window.request_redraw();
            return;
        }
        if self.view.pointer_moved(self.pointer) {
            self.window.request_redraw();
        }
    }

    pub fn pointer_left(&mut self) {
        self.view.pointer_left();
        self.window.request_redraw();
    }

    pub fn press(&mut self, position: PhysicalPosition<f64>) {
        self.pointer = (position.x as f32, position.y as f32);
        let action = self.view.press(self.pointer, &self.app_config);
        self.act(action);
        self.window.request_redraw();
    }

    pub fn release(&mut self) {
        if self.drag.is_some() {
            self.apply_drag();
            self.drag = None;
            return;
        }
        if let Some(change) = self.view.release(&self.app_config) {
            self.save(change);
        }
        self.window.request_redraw();
    }

    pub fn scroll(&mut self, delta: MouseScrollDelta) {
        let dy = match delta {
            MouseScrollDelta::LineDelta(_, y) => -y * WHEEL_LINE,
            MouseScrollDelta::PixelDelta(p) => -(p.y / self.window.scale_factor()) as f32,
        };
        self.view.scroll(dy, self.pointer);
        self.window.request_redraw();
    }

    pub fn key(&mut self, event: &KeyEvent) {
        let action = self.view.key(event, &self.app_config);
        self.act(action);
        self.window.request_redraw();
    }

    fn act(&mut self, action: Action) {
        match action {
            Action::None => {}
            Action::Close => self.close_requested = true,
            Action::StartDrag => self.start_drag(),
            Action::Save(change) => self.save(change),
            Action::ToggleMagic => {
                let _ = self.handles.app_tx.send(AppCommand::ToggleMagicMode);
                if !self.app_config.enhancement_config.enabled {
                    self.view
                        .toast("Turn on Magic Mode model in Advanced, then restart");
                }
            }
            Action::OpenConfig => {
                if let Some(path) = crate::config::config_path() {
                    if let Err(e) = std::process::Command::new("xdg-open").arg(&path).spawn() {
                        eprintln!("Failed to open {}: {e}", path.display());
                        self.view.toast("Could not open the config file");
                    }
                }
            }
        }
    }

    /// Writes `change` to the config file at once and applies it to the app.
    fn save(&mut self, change: Change) {
        // Change only this field on disk, so other edits to the file survive.
        let (mut config, _) = crate::config::read_app_config_with_path();
        change.apply(&mut config);
        if let Err(e) = crate::config::write_app_config(&config) {
            eprintln!("Failed to write config: {e}");
            if let Some(status) = &self.handles.backend_status {
                status
                    .write()
                    .report_error(format!("Settings not saved: {e}"));
            }
            self.view.toast("Not saved: the config file has an error");
            return;
        }
        println!("Settings saved: {change:?}");
        let effect = change.effect();
        if matches!(effect, Effect::Reload | Effect::Model) {
            if let Some(tx) = &self.handles.backend_command_tx {
                let _ = tx.send(speechcore::BackendCommand::Reload {
                    backend_config: config.backend_config.clone(),
                    model_name: config.general_config.model.clone(),
                });
            }
        }
        if let Change::Choose(model::Choice::Listening, _) = change {
            self.mode_request = Some(TranscriptionMode::from(
                config.general_config.transcription_mode.as_str(),
            ));
        }
        self.view.toast(match effect {
            Effect::Now => "Saved",
            Effect::Restart => "Saved · applies after a restart",
            Effect::Reload => "Saved · reloading the model",
            Effect::Model => "Switching model\u{2026}",
        });
        self.app_config = config.clone();
        self.applied_config = Some(config);
    }

    // ----- dragging -----

    fn start_drag(&mut self) {
        let sf = self.window.scale_factor();
        let local = (self.pointer.0 as f64 / sf, self.pointer.1 as f64 / sf);
        let start = self.position.unwrap_or_else(|| {
            let (mw, mh) = self.output_size();
            let size = surface_size();
            ((mw - size.width) / 2.0, (mh - size.height) / 2.0)
        });
        self.position = Some(start);
        self.place(start);
        self.drag = Some(Drag {
            grab: local,
            latest: local,
            pending: (0.0, 0.0),
            raw_events: 0,
        });
    }

    /// Relative pointer motion while a drag runs.
    pub fn raw_motion(&mut self, delta: (f64, f64)) {
        let Some(drag) = self.drag.as_mut() else {
            return;
        };
        drag.pending.0 += delta.0 * super::app::DRAG_RAW_MOTION_SCALE;
        drag.pending.1 += delta.1 * super::app::DRAG_RAW_MOTION_SCALE;
        drag.raw_events += 1;
        self.window.request_redraw();
    }

    /// Moves the window by the motion since the last frame. Relative pointer
    /// motion drives it, because surface-local positions lag behind each move.
    fn apply_drag(&mut self) {
        let Some(drag) = self.drag.as_mut() else {
            return;
        };
        let (dx, dy) = if drag.raw_events > 0 {
            let catch_up = |error: f64, raw: f64| super::app::bounded_drag_catch_up(error, raw);
            let d = (
                drag.pending.0 + catch_up(drag.latest.0 - drag.grab.0, drag.pending.0),
                drag.pending.1 + catch_up(drag.latest.1 - drag.grab.1, drag.pending.1),
            );
            drag.pending = (0.0, 0.0);
            d
        } else {
            // Without relative motion, follow the pointer inside the surface.
            let d = (drag.latest.0 - drag.grab.0, drag.latest.1 - drag.grab.1);
            drag.latest = drag.grab;
            d
        };
        if dx == 0.0 && dy == 0.0 {
            return;
        }
        let (x, y) = self.position.unwrap_or_default();
        let (mw, mh) = self.output_size();
        let size = surface_size();
        let m = MARGIN as f64;
        let x = (x + dx).clamp(KEEP_VISIBLE - size.width, mw - KEEP_VISIBLE);
        let y = (y + dy).clamp(-m, mh - KEEP_VISIBLE);
        self.position = Some((x, y));
        self.place((x, y));
    }

    fn place(&self, (x, y): (f64, f64)) {
        if let Some(wayland) = self.window.cast_ref::<winit::platform::wayland::Window>() {
            wayland.set_anchor(Anchor::TOP | Anchor::LEFT);
            wayland.set_margin(y.round() as i32, 0, 0, x.round() as i32);
        }
        self.window.request_redraw();
    }

    /// Logical size of the output that shows the window.
    fn output_size(&self) -> (f64, f64) {
        let sf = self.window.scale_factor();
        self.window
            .current_monitor()
            .and_then(|m| m.current_video_mode())
            .map(|mode| {
                let size = mode.size().to_logical::<f64>(sf);
                (size.width, size.height)
            })
            .unwrap_or((1920.0, 1080.0))
    }
}

/// The scissor rectangle for `clip` on a surface of `size`, or `None` if empty.
fn scissor(clip: Option<[f32; 4]>, size: PhysicalSize<u32>) -> Option<[u32; 4]> {
    let [x, y, w, h] = clip.unwrap_or([0.0, 0.0, size.width as f32, size.height as f32]);
    let x0 = x.max(0.0).floor() as u32;
    let y0 = y.max(0.0).floor() as u32;
    let x1 = ((x + w).ceil() as u32).min(size.width);
    let y1 = ((y + h).ceil() as u32).min(size.height);
    (x1 > x0 && y1 > y0).then(|| [x0, y0, x1 - x0, y1 - y0])
}

/// `path` with the home directory written as `~`.
fn home_relative(path: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    match std::path::Path::new(path).strip_prefix(&home) {
        Ok(rest) if !home.is_empty() => format!("~/{}", rest.display()),
        _ => path.to_string(),
    }
}
