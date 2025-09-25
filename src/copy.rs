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
