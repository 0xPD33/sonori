use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

/// Append a transcript entry to the history file with timestamp
pub fn append_to_transcript_history(
    text: &str,
    history_path: &str,
    enabled: bool,
) -> Result<(), std::io::Error> {
    if !enabled || text.trim().is_empty() {
        return Ok(());
    }

    let path = Path::new(history_path);
    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    let entry = format!("[{}] {}\n", timestamp, text.trim());

    // Create parent directory if needed
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Append to file (create if doesn't exist)
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;

    file.write_all(entry.as_bytes())?;
    Ok(())
}
