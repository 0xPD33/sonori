use std::sync::Arc;

use winit::dpi::PhysicalSize;
use winit::keyboard::Key;
use winit::window::Window;

use super::settings_panel::SettingsPanel;
use crate::config::AppConfig;

pub struct SettingsWindow {
    pub window: Arc<dyn Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    panel: SettingsPanel,
    backend_command_tx:
        Option<tokio::sync::mpsc::UnboundedSender<crate::backend_manager::BackendCommand>>,
    applied_config: Option<AppConfig>,
}

impl SettingsWindow {
    pub fn new(
        window: Box<dyn Window>,
        instance: &wgpu::Instance,
        adapter: &wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,
        surface_format: wgpu::TextureFormat,
        initial_config: &AppConfig,
        backend_command_tx: Option<
            tokio::sync::mpsc::UnboundedSender<crate::backend_manager::BackendCommand>,
        >,
    ) -> Result<Self, String> {
        let window: Arc<dyn Window> = Arc::from(window);

        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| format!("Failed to create GPU surface for settings window: {e}"))?;

        let size = window.outer_size();

        let surface_caps = surface.get_capabilities(adapter);

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
            wgpu::CompositeAlphaMode::Inherit
        } else {
            surface_caps.alpha_modes[0]
        };

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        surface.configure(&device, &config);

        let mut panel = SettingsPanel::new(&device, &queue, &config, size);
        panel.is_open = true;
        panel.animation_progress = 1.0;
        panel.animation_active = false;
        panel.populate_from_config(initial_config);

        Ok(Self {
            window,
            surface,
            device,
            queue,
            config,
            panel,
            backend_command_tx,
            applied_config: None,
        })
    }

    pub fn draw(&mut self) {
        let output = match self.surface.get_current_texture() {
            Ok(t) => t,
            Err(wgpu::SurfaceError::Outdated | wgpu::SurfaceError::Lost) => {
                // Reconfigure and retry
                self.surface.configure(&self.device, &self.config);
                match self.surface.get_current_texture() {
                    Ok(t) => t,
                    Err(_) => return,
                }
            }
            Err(_) => return,
        };
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Settings Window Encoder"),
            });

        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Settings Window Clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.003,
                            g: 0.003,
                            b: 0.004,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }

        self.panel.render(
            &mut encoder,
            &view,
            &self.queue,
            self.config.width,
            self.config.height,
        );

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        if self.panel.needs_redraw() {
            self.window.request_redraw();
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.panel.resize(PhysicalSize::new(width, height));
    }

    pub fn handle_click(&mut self, x: f32, y: f32) {
        self.panel
            .handle_click(x, y, self.config.width, self.config.height);
        if self.panel.take_apply_request() {
            self.apply_settings_changes();
        }
        self.window.request_redraw();
    }

    pub fn handle_mouse_move(&mut self, x: f32, y: f32) {
        self.panel.handle_mouse_move(x, y);
        self.window.request_redraw();
    }

    pub fn handle_mouse_release(&mut self) {
        self.panel.handle_mouse_release();
        self.window.request_redraw();
    }

    pub fn handle_key(&mut self, key: &Key, shift: bool) -> bool {
        self.panel.handle_key(key, shift)
    }

    pub fn close_requested(&self) -> bool {
        self.panel.close_requested
    }

    pub fn take_applied_config(&mut self) -> Option<AppConfig> {
        self.applied_config.take()
    }

    fn apply_settings_changes(&mut self) {
        let (mut app_config, _) = crate::config::read_app_config_with_path();
        let previous_config = app_config.clone();
        let (any_changed, needs_reload) = self.panel.apply_pending_changes(&mut app_config);
        if any_changed {
            log_settings_changes(&previous_config, &app_config, needs_reload);

            if let Err(e) = crate::config::write_app_config(&app_config) {
                eprintln!("Failed to write config: {}", e);
                return;
            }
            self.panel.populate_from_config(&app_config);
            self.panel.clear_pending_changes();
            self.applied_config = Some(app_config.clone());
            if needs_reload {
                if let Some(tx) = &self.backend_command_tx {
                    let _ = tx.send(crate::backend_manager::BackendCommand::Reload {
                        backend_config: app_config.backend_config.clone(),
                        model_name: app_config.general_config.model.clone(),
                    });
                }
            }
        }
    }
}

fn log_settings_changes(previous: &AppConfig, next: &AppConfig, needs_reload: bool) {
    let mut changes = Vec::new();

    if previous.backend_config.backend != next.backend_config.backend {
        changes.push(format!(
            "backend {} -> {}",
            previous.backend_config.backend, next.backend_config.backend
        ));
    }
    if previous.general_config.model != next.general_config.model {
        changes.push(format!(
            "model {} -> {}",
            previous.general_config.model, next.general_config.model
        ));
    }
    if previous.ui_config.typewriter_effect != next.ui_config.typewriter_effect {
        changes.push(format!(
            "typewriter {} -> {}",
            previous.ui_config.typewriter_effect, next.ui_config.typewriter_effect
        ));
    }

    if changes.is_empty() && !needs_reload {
        return;
    }

    if changes.is_empty() {
        println!("Applying settings changes: backend reload queued");
    } else if needs_reload {
        println!(
            "Applying settings changes: {} (backend reload queued)",
            changes.join(", ")
        );
    } else {
        println!("Applying settings changes: {}", changes.join(", "));
    }
}
