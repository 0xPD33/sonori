//! `sonori doctor`: a report of what Sonori needs from the system and what it
//! found. It only reads state: it opens no window, records no audio, and leaves
//! the clipboard alone. The exit status is 1 when Sonori cannot work as set up.

use std::path::PathBuf;
use std::time::Duration;

use crate::config::{self, AppConfig, OutputMode};
use crate::hotkey::HotkeyState;
use crate::ipc::{self, IpcCommand};

#[derive(Clone, Copy, PartialEq)]
enum Level {
    Pass,
    Warn,
    Fail,
    Info,
}

#[derive(Default)]
struct Report {
    failures: usize,
    warnings: usize,
}

impl Report {
    fn section(&self, title: &str) {
        println!("\n{title}");
    }

    fn item(&mut self, level: Level, label: &str, detail: impl AsRef<str>) {
        let mark = match level {
            Level::Pass => "ok  ",
            Level::Warn => "warn",
            Level::Fail => "FAIL",
            Level::Info => "    ",
        };
        match level {
            Level::Fail => self.failures += 1,
            Level::Warn => self.warnings += 1,
            _ => {}
        }
        let detail = detail.as_ref();
        if detail.is_empty() {
            println!("  {mark}  {label}");
        } else {
            println!("  {mark}  {label}: {detail}");
        }
    }
}

/// What the compositor offers, from its Wayland registry.
struct Globals(Vec<(String, u32)>);

impl Globals {
    fn version(&self, interface: &str) -> Option<u32> {
        self.0
            .iter()
            .find(|(name, _)| name == interface)
            .map(|(_, v)| *v)
    }

    fn count(&self, interface: &str) -> usize {
        self.0.iter().filter(|(name, _)| name == interface).count()
    }
}

pub async fn run() -> anyhow::Result<()> {
    let mut report = Report::default();
    println!("Sonori doctor {}", env!("CARGO_PKG_VERSION"));
    println!("You can paste this report into a bug report. It holds no transcripts.");

    let config = config::peek_app_config();

    system(&mut report);
    let globals = wayland(&mut report);
    let portals = portals(&mut report).await;
    let helpers = helpers(&mut report);
    capabilities(
        &mut report,
        globals.as_ref(),
        &portals,
        &helpers,
        config.as_ref(),
    );
    gpu(&mut report);
    audio(&mut report, config.as_ref());
    settings(&mut report, config.as_ref());
    instance(&mut report).await;

    println!(
        "\nResult: {} problem(s), {} warning(s)",
        report.failures, report.warnings
    );
    if report.failures > 0 {
        std::process::exit(1);
    }
    Ok(())
}

fn system(report: &mut Report) {
    report.section("System");
    let os = std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|text| {
            text.lines()
                .find_map(|l| l.strip_prefix("PRETTY_NAME="))
                .map(|v| v.trim_matches('"').to_string())
        })
        .unwrap_or_else(|| "unknown".into());
    report.item(Level::Info, "OS", os);
    let kernel = std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default();
    report.item(Level::Info, "Kernel", kernel.trim());
    let env = |key: &str| std::env::var(key).unwrap_or_else(|_| "unset".into());
    report.item(Level::Info, "Session type", env("XDG_SESSION_TYPE"));
    report.item(Level::Info, "Desktop", env("XDG_CURRENT_DESKTOP"));
}

fn wayland(report: &mut Report) -> Option<Globals> {
    use wayland_client::globals::{registry_queue_init, GlobalListContents};
    use wayland_client::protocol::wl_registry;
    use wayland_client::{Connection, Dispatch, QueueHandle};

    struct State;
    impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
        fn event(
            _: &mut Self,
            _: &wl_registry::WlRegistry,
            _: wl_registry::Event,
            _: &GlobalListContents,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    report.section("Wayland");
    let conn = match Connection::connect_to_env() {
        Ok(conn) => conn,
        Err(e) => {
            report.item(Level::Fail, "Compositor", format!("cannot connect ({e})"));
            return None;
        }
    };
    let globals = match registry_queue_init::<State>(&conn) {
        Ok((globals, _queue)) => Globals(globals.contents().with_list(|list| {
            list.iter()
                .map(|g| (g.interface.clone(), g.version))
                .collect()
        })),
        Err(e) => {
            report.item(Level::Fail, "Compositor", format!("registry failed ({e})"));
            return None;
        }
    };

    let protocols = [
        ("zwlr_layer_shell_v1", "overlay and cursor mode"),
        ("zwp_virtual_keyboard_manager_v1", "paste with wtype"),
        ("ext_data_control_manager_v1", "clipboard without focus"),
        (
            "zwlr_data_control_manager_v1",
            "clipboard without focus (older)",
        ),
        (
            "zwlr_foreign_toplevel_manager_v1",
            "focused app, wlroots and niri",
        ),
        ("org_kde_plasma_window_management", "focused app, KDE"),
        (
            "wp_fractional_scale_manager_v1",
            "sharp text at fractional scale",
        ),
    ];
    for (interface, purpose) in protocols {
        match globals.version(interface) {
            Some(v) => report.item(Level::Info, interface, format!("v{v} ({purpose})")),
            None => report.item(Level::Info, interface, format!("missing ({purpose})")),
        }
    }
    report.item(
        Level::Info,
        "Outputs",
        globals.count("wl_output").to_string(),
    );
    Some(globals)
}

struct Portals {
    remote_desktop: Option<u32>,
    global_shortcuts: Option<u32>,
}

async fn portals(report: &mut Report) -> Portals {
    report.section("XDG portals");
    let conn = match tokio::time::timeout(Duration::from_secs(3), zbus::Connection::session()).await
    {
        Ok(Ok(conn)) => Some(conn),
        Ok(Err(e)) => {
            report.item(Level::Warn, "Session bus", format!("unavailable ({e})"));
            None
        }
        Err(_) => {
            report.item(Level::Warn, "Session bus", "timed out");
            None
        }
    };
    let mut found = Portals {
        remote_desktop: None,
        global_shortcuts: None,
    };
    let Some(conn) = conn else { return found };
    for (interface, purpose, slot) in [
        (
            "org.freedesktop.portal.RemoteDesktop",
            "paste through the portal",
            &mut found.remote_desktop,
        ),
        (
            "org.freedesktop.portal.GlobalShortcuts",
            "global hotkey",
            &mut found.global_shortcuts,
        ),
    ] {
        *slot = portal_version(&conn, interface).await;
        let short = interface.rsplit('.').next().unwrap_or(interface);
        match *slot {
            Some(v) => report.item(Level::Info, short, format!("v{v} ({purpose})")),
            None => report.item(Level::Info, short, format!("missing ({purpose})")),
        }
    }
    found
}

async fn portal_version(conn: &zbus::Connection, interface: &str) -> Option<u32> {
    let query = async {
        let proxy = zbus::Proxy::new(
            conn,
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            interface.to_string(),
        )
        .await
        .ok()?;
        proxy.get_property::<u32>("version").await.ok()
    };
    // A portal service that does not answer must not hang the report.
    tokio::time::timeout(Duration::from_secs(5), query)
        .await
        .ok()
        .flatten()
}

struct Helpers {
    wl_copy: bool,
    wtype: bool,
    dotool: bool,
    uinput: bool,
}

fn helpers(report: &mut Report) -> Helpers {
    report.section("Helper programs");
    let mut check = |name: &str, purpose: &str| {
        let found = find_in_path(name);
        report.item(
            Level::Info,
            name,
            match found {
                true => format!("found ({purpose})"),
                false => format!("missing ({purpose})"),
            },
        );
        found
    };
    let wl_copy = check("wl-copy", "clipboard");
    let wtype = check("wtype", "paste keystroke");
    let dotool = check("dotool", "paste keystroke fallback");
    // Opening /dev/uinput creates no device; dotool needs write access to it.
    let uinput = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/uinput")
        .is_ok();
    report.item(
        Level::Info,
        "/dev/uinput",
        if uinput { "writable" } else { "not writable" },
    );
    Helpers {
        wl_copy,
        wtype,
        dotool,
        uinput,
    }
}

fn find_in_path(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|dir: PathBuf| dir.join(name).is_file())
    })
}

/// The verdict per feature, from the raw findings above.
fn capabilities(
    report: &mut Report,
    globals: Option<&Globals>,
    portals: &Portals,
    helpers: &Helpers,
    config: Option<&AppConfig>,
) {
    report.section("What works");
    let has = |interface: &str| globals.is_some_and(|g| g.version(interface).is_some());

    if has("zwlr_layer_shell_v1") {
        report.item(Level::Pass, "Overlay and cursor mode", "");
    } else {
        report.item(
            Level::Fail,
            "Overlay and cursor mode",
            "the compositor has no layer shell (GNOME); run `sonori --cli`",
        );
    }

    if helpers.wl_copy {
        report.item(Level::Pass, "Clipboard", "wl-copy");
    } else {
        report.item(Level::Fail, "Clipboard", "install wl-clipboard");
    }

    let output_mode = config.map(|c| c.portal_config.output_mode);
    let portal_enabled = config.is_none_or(|c| c.portal_config.enable_xdg_portal);
    let keystrokes = if helpers.wtype && has("zwp_virtual_keyboard_manager_v1") {
        Some("wtype")
    } else if helpers.dotool && helpers.uinput {
        Some("dotool")
    } else {
        None
    };
    let portal = (portal_enabled && portals.remote_desktop.is_some())
        .then_some("portal (asks for permission once)");
    // Typing goes through the input method, else wtype or dotool; pasting can
    // also use the portal.
    let (label, way) = match output_mode.unwrap_or_default() {
        OutputMode::Clipboard => {
            report.item(Level::Info, "Paste", "off (output mode is clipboard only)");
            return hotkey(report, portals, config);
        }
        OutputMode::Paste => ("Paste", portal.or(keystrokes)),
        OutputMode::Type if has("zwp_input_method_manager_v2") => (
            "Type",
            Some(keystrokes.map_or("input method", |_| "input method, else keystrokes")),
        ),
        OutputMode::Type => ("Type", keystrokes),
    };
    match way {
        Some(way) => report.item(Level::Pass, label, way),
        None => report.item(
            Level::Fail,
            label,
            "no way to send keystrokes; install wtype, or dotool with uinput access",
        ),
    }
    hotkey(report, portals, config);
}

fn hotkey(report: &mut Report, portals: &Portals, config: Option<&AppConfig>) {
    let shortcuts_enabled = config.is_none_or(|c| c.portal_config.enable_global_shortcuts);
    if shortcuts_enabled && portals.global_shortcuts.is_some() {
        report.item(Level::Pass, "Global hotkey", "portal");
    } else {
        report.item(
            Level::Warn,
            "Global hotkey",
            "no portal; bind `sonori toggle` to a key in your compositor",
        );
    }
}

fn gpu(report: &mut Report) {
    report.section("GPU");
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..Default::default()
    });
    // The GL backend probes X11 and prints "Authorization required" on Wayland.
    for adapter in quiet_stderr(|| instance.enumerate_adapters(wgpu::Backends::all())) {
        let info = adapter.get_info();
        report.item(
            Level::Info,
            &format!("{:?}", info.backend),
            format!(
                "{} ({:?}, driver {} {})",
                info.name, info.device_type, info.driver, info.driver_info
            ),
        );
    }
    drop(instance);

    match quiet_stderr(crate::ui::gpu_utils::Gpu::new) {
        Ok(gpu) => {
            let info = gpu.adapter.get_info();
            match draw_test(&gpu.device, &gpu.queue) {
                Ok(()) => report.item(
                    Level::Pass,
                    "Draw test",
                    format!("{} drew a frame", info.name),
                ),
                Err(e) => report.item(Level::Fail, "Draw test", format!("{} ({e})", info.name)),
            }
        }
        Err(e) => report.item(Level::Fail, "GPU", e),
    }
}

/// Clears an offscreen texture and reads it back: proves the device runs work.
fn draw_test(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<(), String> {
    const WIDTH: u32 = 64;
    let size = wgpu::Extent3d {
        width: WIDTH,
        height: 1,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("doctor"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("doctor readback"),
        size: (WIDTH * 4) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let view = texture.create_view(&Default::default());
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: None,
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color {
                    r: 1.0,
                    g: 0.0,
                    b: 1.0,
                    a: 1.0,
                }),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(WIDTH * 4),
                rows_per_image: None,
            },
        },
        size,
    );
    queue.submit([encoder.finish()]);
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device
        .poll(wgpu::PollType::Wait)
        .map_err(|e| format!("GPU did not finish: {e}"))?;
    let pixel: [u8; 4] = slice.get_mapped_range()[..4]
        .try_into()
        .map_err(|_| "short readback")?;
    if pixel == [255, 0, 255, 255] {
        Ok(())
    } else {
        Err(format!("read back {pixel:?}, expected [255, 0, 255, 255]"))
    }
}

fn audio(report: &mut Report, config: Option<&AppConfig>) {
    report.section("Microphones (PortAudio)");
    // ALSA prints dozens of probe errors while PortAudio scans the devices.
    let pa = match quiet_stderr(portaudio::PortAudio::new) {
        Ok(pa) => pa,
        Err(e) => {
            report.item(Level::Fail, "PortAudio", format!("cannot start ({e})"));
            return;
        }
    };
    let default = pa.default_input_device().ok();
    let mut inputs = Vec::new();
    if let Ok(devices) = pa.devices() {
        for (index, info) in devices.flatten() {
            if info.max_input_channels > 0 {
                inputs.push((Some(index) == default, info.name.to_string()));
            }
        }
    }
    for (is_default, name) in &inputs {
        let mark = if *is_default { " (default)" } else { "" };
        report.item(Level::Info, &format!("{name}{mark}"), "");
    }

    let wanted = config.and_then(|c| c.audio_processor_config.input_device.clone());
    match wanted.filter(|w| !w.trim().is_empty()) {
        Some(wanted) => {
            let needle = wanted.to_lowercase();
            match inputs
                .iter()
                .find(|(_, n)| n.to_lowercase().contains(&needle))
            {
                Some((_, name)) => report.item(Level::Pass, "Configured microphone", name),
                None => report.item(
                    Level::Fail,
                    "Configured microphone",
                    format!("no input device matches \"{wanted}\""),
                ),
            }
        }
        None if default.is_some() => report.item(Level::Pass, "Microphone", "system default"),
        None => report.item(Level::Fail, "Microphone", "no default input device"),
    }
}

fn settings(report: &mut Report, config: Option<&AppConfig>) {
    report.section("Settings");
    report.item(
        Level::Info,
        "Config file",
        config::config_path_for_display(),
    );
    if let Some(problem) = config::config_file_problem() {
        report.item(Level::Warn, "Config", problem);
    }
    let Some(config) = config else {
        report.item(Level::Info, "Values", "defaults");
        return;
    };
    report.item(
        Level::Info,
        "Speech model",
        format!(
            "{} / {}",
            config.backend_config.backend, config.general_config.model
        ),
    );
    report.item(
        Level::Info,
        "Display mode",
        format!("{:?}", config.ui_config.display_mode),
    );
    report.item(
        Level::Info,
        "Output",
        format!(
            "{:?}, paste shortcut {}",
            config.portal_config.output_mode, config.portal_config.paste_shortcut
        ),
    );
}

async fn instance(report: &mut Report) {
    report.section("Running Sonori");
    match ipc::send_command(IpcCommand::Status).await {
        Ok(response) => match response.status {
            Some(status) => {
                report.item(
                    Level::Info,
                    "State",
                    format!(
                        "running, {} mode, recording {}",
                        status.mode, status.recording
                    ),
                );
                if let Some(backend) = status.backend {
                    let level = match &backend.last_error {
                        Some(_) => Level::Warn,
                        None => Level::Info,
                    };
                    let error = backend
                        .last_error
                        .map(|e| format!(", last error: {e}"))
                        .unwrap_or_default();
                    report.item(level, "Backend", format!("{}{error}", backend.state));
                }
                match status.hotkey {
                    Some(HotkeyState::Bound { trigger }) => {
                        report.item(Level::Pass, "Hotkey", format!("bound to {trigger}"))
                    }
                    Some(HotkeyState::Unavailable { reason }) => report.item(
                        Level::Warn,
                        "Hotkey",
                        format!("{reason}; bind `sonori toggle` in your compositor instead"),
                    ),
                    Some(HotkeyState::Pending) => {
                        report.item(Level::Info, "Hotkey", "waiting for the portal")
                    }
                    Some(HotkeyState::Disabled) => {
                        report.item(Level::Info, "Hotkey", "off in the config")
                    }
                    None => {}
                }
            }
            None => report.item(Level::Info, "State", "running, no status"),
        },
        Err(_) => report.item(Level::Info, "State", "not running"),
    }
}

/// Runs `f` with stderr sent to /dev/null, so library noise stays out of the report.
fn quiet_stderr<T>(f: impl FnOnce() -> T) -> T {
    use std::os::fd::AsRawFd;
    let Ok(null) = std::fs::OpenOptions::new().write(true).open("/dev/null") else {
        return f();
    };
    // SAFETY: fd 2 stays valid; the saved copy restores it and is closed after.
    unsafe {
        let saved = libc::dup(2);
        if saved < 0 {
            return f();
        }
        libc::dup2(null.as_raw_fd(), 2);
        let result = f();
        libc::dup2(saved, 2);
        libc::close(saved);
        result
    }
}
