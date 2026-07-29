use std::io::Write;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

/// Hard deadline for the external clipboard/paste helpers. Without it a hung
/// wtype/dotool blocks the paste worker for the rest of the session.
const HELPER_TIMEOUT: Duration = Duration::from_secs(2);

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
        run_helper(Command::new("wl-copy"), "wl-copy", Some(text.as_bytes()))
    }
}

/// Simulate a paste keystroke using available tools (wtype → dotool fallback chain)
pub fn paste_via_keystroke(paste_shortcut: &str) -> Result<(), String> {
    let (wtype_args, dotool_key): (&[&str], &str) = if paste_shortcut == "ctrl_v" {
        (&["-M", "ctrl", "-k", "v", "-m", "ctrl"], "key ctrl+v\n")
    } else {
        (
            &[
                "-M", "ctrl", "-M", "shift", "-k", "v", "-m", "shift", "-m", "ctrl",
            ],
            "key ctrl+shift+v\n",
        )
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

/// Spawn a helper, optionally feed it stdin, and wait for it under a deadline.
fn run_helper(mut cmd: Command, name: &str, stdin_data: Option<&[u8]>) -> Result<(), String> {
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

    let status = wait_with_timeout(&mut child, name)?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{name} exited with status {status}"))
    }
}

/// ponytail: poll rather than spawn a waiter thread. 10ms granularity is well
/// under what a paste needs, and these helpers normally exit in single digits.
fn wait_with_timeout(child: &mut Child, name: &str) -> Result<ExitStatus, String> {
    let deadline = Instant::now() + HELPER_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("{name} timed out after {HELPER_TIMEOUT:?}"));
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
        let err = wait_with_timeout(&mut child, "sleep").unwrap_err();

        assert!(err.contains("timed out"), "unexpected error: {err}");
        assert!(started.elapsed() < HELPER_TIMEOUT * 2);
        // Killed, so it is reapable immediately rather than still running.
        assert!(child.try_wait().is_ok());
    }
}
