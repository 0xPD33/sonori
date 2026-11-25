use std::io::{Read, Write};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

/// Hard deadline for the external clipboard/paste helpers. Without it a hung
/// wtype/dotool blocks the paste worker for the rest of the session.
const HELPER_TIMEOUT: Duration = Duration::from_secs(2);
const CLIPBOARD_TIMEOUT: Duration = Duration::from_secs(10);

/// Simple Wayland connection for clipboard operations
pub struct WlCopy;

impl WlCopy {
    /// Copy text to clipboard using wl-copy.
    ///
    /// The transcript is fed over stdin, never as an argument: argv is world
    /// readable through /proc, so `wl-copy <transcript>` leaks what the user
    /// just dictated to every process on the machine. Stdin also copies the
    /// text verbatim, where argv mode appends a trailing newline.
    pub fn copy_to_clipboard(text: &str) -> Result<(), String> {
        // On a fully loaded machine wl-copy took over 2 s to set the selection,
        // and the transcript was not pasted.
        run_helper_with_timeout(
            Command::new("wl-copy"),
            "wl-copy",
            Some(text.as_bytes()),
            CLIPBOARD_TIMEOUT,
        )
    }
}

/// Simulate a paste keystroke using available tools (wtype → dotool fallback chain)
pub fn paste_via_keystroke(with_shift: bool) -> Result<(), String> {
    let (wtype_args, dotool_key): (&[&str], &str) = if with_shift {
        (
            &[
                "-M", "ctrl", "-M", "shift", "-k", "v", "-m", "shift", "-m", "ctrl",
            ],
            "key ctrl+shift+v\n",
        )
    } else {
        (&["-M", "ctrl", "-k", "v", "-m", "ctrl"], "key ctrl+v\n")
    };

    let mut wtype = Command::new("wtype");
    wtype.args(wtype_args);
    match run_helper(wtype, "wtype", None) {
        Ok(()) => Ok(()),
        Err(wtype_err) => run_helper(
            Command::new("dotool"),
            "dotool",
            Some(dotool_key.as_bytes()),
        )
        .map_err(|dotool_err| format!("wtype ({wtype_err}); dotool ({dotool_err})")),
    }
}

/// The focused app reads the paste in well under this, so a slow compositor
/// query must not hold the paste back for long.
const FOCUS_QUERY_TIMEOUT: Duration = Duration::from_millis(500);

/// App IDs of terminals, lower case. In a terminal Ctrl+V goes to the program
/// and Ctrl+Shift+V pastes. Any app ID with "term" in it also counts.
const TERMINALS: &[&str] = &[
    "foot",
    "footclient",
    "kitty",
    "alacritty",
    "com.mitchellh.ghostty",
    "ghostty",
    "org.wezfurlong.wezterm",
    "wezterm",
    "org.kde.konsole",
    "konsole",
    "org.gnome.console",
    "kgx",
    "org.gnome.ptyxis",
    "com.raggesilver.blackbox",
    "urxvt",
    "st-256color",
    "contour",
    "rio",
    "herdr-app",
    "dev.warp.warp",
];

/// Whether a paste needs Ctrl+Shift+V. "auto" asks the compositor for the
/// focused app; without an answer it uses Ctrl+Shift+V, the old default.
pub fn paste_with_shift(paste_shortcut: &str) -> bool {
    match paste_shortcut {
        "ctrl_v" => false,
        "auto" => {
            let app = focused_app_id();
            let with_shift = app.as_deref().is_none_or(|app| {
                let app = app.to_lowercase();
                TERMINALS.contains(&app.as_str()) || app.contains("term")
            });
            let keys = if with_shift { "Ctrl+Shift+V" } else { "Ctrl+V" };
            println!("Paste shortcut: {keys} for app {app:?}");
            with_shift
        }
        _ => true,
    }
}

/// The app ID of the focused window, from the compositor's IPC. None on a
/// compositor without one (GNOME, KDE) and when no window has focus.
pub fn focused_app_id() -> Option<String> {
    let (program, args): (&str, &[&str]) = if std::env::var_os("NIRI_SOCKET").is_some() {
        ("niri", &["msg", "--json", "focused-window"])
    } else if std::env::var_os("SWAYSOCK").is_some() {
        ("swaymsg", &["-t", "get_tree"])
    } else if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
        ("hyprctl", &["-j", "activewindow"])
    } else {
        return None;
    };
    let json: serde_json::Value = serde_json::from_slice(&helper_output(program, args)?).ok()?;
    let window = if program == "swaymsg" {
        focused_node(&json)?
    } else {
        &json
    };
    // niri and Sway give app_id, Hyprland gives class, and Sway gives
    // window_properties.class for an Xwayland window.
    let app = window
        .get("app_id")
        .and_then(|v| v.as_str())
        .or_else(|| window.get("class").and_then(|v| v.as_str()))
        .or_else(|| window.get("window_properties")?.get("class")?.as_str())?;
    Some(app.to_string())
}

/// The node with `"focused": true` in a Sway tree.
fn focused_node(node: &serde_json::Value) -> Option<&serde_json::Value> {
    if node.get("focused").and_then(|v| v.as_bool()) == Some(true) {
        return Some(node);
    }
    ["nodes", "floating_nodes"]
        .iter()
        .filter_map(|key| node.get(key)?.as_array())
        .flatten()
        .find_map(focused_node)
}

/// The stdout of a helper that exited with success before the timeout.
fn helper_output(program: &str, args: &[&str]) -> Option<Vec<u8>> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    // A big Sway tree fills the pipe before swaymsg exits, so read while it runs.
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        stdout.read_to_end(&mut out).map(|_| out)
    });
    let status = wait_with_timeout(&mut child, program, FOCUS_QUERY_TIMEOUT).ok()?;
    let out = reader.join().ok()?.ok()?;
    status.success().then_some(out)
}

/// Type text into the focused window (wtype → dotool fallback chain). The text
/// goes over stdin, never argv, for the same reason as `copy_to_clipboard`.
pub fn type_text(text: &str) -> Result<(), String> {
    // Typing takes a few ms per character, so long text needs a longer deadline.
    let timeout = HELPER_TIMEOUT + Duration::from_millis(10 * text.chars().count() as u64);
    let mut wtype = Command::new("wtype");
    wtype.arg("-");
    match run_helper_with_timeout(wtype, "wtype", Some(text.as_bytes()), timeout) {
        Ok(()) => Ok(()),
        Err(wtype_err) => {
            // dotool reads one command per line, so each newline becomes an Enter.
            let script = text
                .split('\n')
                .map(|line| format!("type {line}\n"))
                .collect::<Vec<_>>()
                .join("key enter\n");
            run_helper_with_timeout(
                Command::new("dotool"),
                "dotool",
                Some(script.as_bytes()),
                timeout,
            )
            .map_err(|dotool_err| format!("wtype ({wtype_err}); dotool ({dotool_err})"))
        }
    }
}

/// Spawn a helper, optionally feed it stdin, and wait for it under a deadline.
fn run_helper(cmd: Command, name: &str, stdin_data: Option<&[u8]>) -> Result<(), String> {
    run_helper_with_timeout(cmd, name, stdin_data, HELPER_TIMEOUT)
}

fn run_helper_with_timeout(
    mut cmd: Command,
    name: &str,
    stdin_data: Option<&[u8]>,
    timeout: Duration,
) -> Result<(), String> {
    let stdin_cfg = if stdin_data.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    };

    let mut child = cmd
        .stdin(stdin_cfg)
        .spawn()
        .map_err(|e| format!("{name} unavailable: {e}"))?;

    if let Some(data) = stdin_data {
        // Scoped so the handle drops after the write, closing the pipe and
        // giving the helper the EOF it waits for.
        // ponytail: a blocking write_all could outlast HELPER_TIMEOUT if a
        // helper stopped reading with a full pipe buffer. wl-copy and dotool
        // both drain to EOF, so this only matters if that ever changes.
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| format!("{name}: stdin unavailable"))?;
        if let Err(e) = stdin.write_all(data) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("failed writing to {name}: {e}"));
        }
    }

    let status = wait_with_timeout(&mut child, name, timeout)?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{name} exited with status {status}"))
    }
}

/// ponytail: poll rather than spawn a waiter thread. 10ms granularity is well
/// under what a paste needs, and these helpers normally exit in single digits.
fn wait_with_timeout(
    child: &mut Child,
    name: &str,
    timeout: Duration,
) -> Result<ExitStatus, String> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("{name} timed out after {timeout:?}"));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(format!("{name} failed: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_receives_stdin_not_argv() {
        let mut cat = Command::new("cat");
        cat.stdout(Stdio::null());
        assert!(run_helper(cat, "cat", Some(b"secret transcript")).is_ok());
    }

    #[test]
    fn missing_helper_is_an_error_not_a_hang() {
        let cmd = Command::new("sonori-no-such-helper");
        assert!(run_helper(cmd, "nope", None).is_err());
    }

    #[test]
    fn hung_helper_is_killed_at_the_deadline() {
        let mut child = Command::new("sleep")
            .arg("30")
            .stdin(Stdio::null())
            .spawn()
            .expect("sleep should be available");

        let started = Instant::now();
        let err = wait_with_timeout(&mut child, "sleep", HELPER_TIMEOUT).unwrap_err();

        assert!(err.contains("timed out"), "unexpected error: {err}");
        assert!(started.elapsed() < HELPER_TIMEOUT * 2);
        // Killed, so it is reapable immediately rather than still running.
        assert!(child.try_wait().is_ok());
    }
}
