use std::process::Command;

/// Simple Wayland connection for clipboard operations
pub struct WlCopy;

impl WlCopy {
    /// Copy text to clipboard using wl-copy
    pub fn copy_to_clipboard(text: &str) -> Result<(), String> {
        match Command::new("wl-copy").arg(text).status() {
            Ok(status) if status.success() => {
                println!("Copied '{}' to clipboard", text);
                Ok(())
            }
            Ok(status) => Err(format!("wl-copy failed with status {}", status)),
            Err(e) => Err(format!("Error executing wl-copy: {}", e)),
        }
    }
}

/// Simulate a paste keystroke using available tools (wtype → dotool fallback chain)
pub fn paste_via_keystroke(paste_shortcut: &str) -> Result<(), String> {
    if paste_shortcut == "ctrl_v" {
        paste_ctrl_v()
    } else {
        paste_ctrl_shift_v()
    }
}

fn paste_ctrl_v() -> Result<(), String> {
    if let Ok(status) = Command::new("wtype")
        .args(["-M", "ctrl", "-k", "v", "-m", "ctrl"])
        .status()
    {
        if status.success() {
            return Ok(());
        }
    }

    use std::io::Write;
    use std::process::Stdio;
    let mut child = Command::new("dotool")
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Neither wtype nor dotool available: {}", e))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(b"key ctrl+v\n")
            .map_err(|e| format!("Failed to write to dotool: {}", e))?;
    }
    let status = child.wait().map_err(|e| format!("dotool failed: {}", e))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("dotool exited with status {}", status))
    }
}

fn paste_ctrl_shift_v() -> Result<(), String> {
    if let Ok(status) = Command::new("wtype")
        .args([
            "-M", "ctrl", "-M", "shift", "-k", "v", "-m", "shift", "-m", "ctrl",
        ])
        .status()
    {
        if status.success() {
            return Ok(());
        }
    }

    use std::io::Write;
    use std::process::Stdio;
    let mut child = Command::new("dotool")
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Neither wtype nor dotool available: {}", e))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(b"key ctrl+shift+v\n")
            .map_err(|e| format!("Failed to write to dotool: {}", e))?;
    }
    let status = child.wait().map_err(|e| format!("dotool failed: {}", e))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("dotool exited with status {}", status))
    }
}
