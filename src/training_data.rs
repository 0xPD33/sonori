//! Saves the audio and the text of every dictation, as data to train or test
//! speech models. `manifest.jsonl` uses the NeMo manifest format
//! (`audio_filepath`, `duration`, `text`) plus Sonori's own fields.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::Arc;

use speechcore::SessionRecord;

use crate::transcript_writer::expand_home;
use crate::ui::DeliveryOutcome;

/// One dictation and what Sonori did with it.
pub struct Capture {
    pub record: Arc<SessionRecord>,
    pub session_id: Option<String>,
    /// speechcore's text, after the clean-up rules.
    pub text: String,
    /// The text after Magic Mode, when it ran.
    pub magic_text: Option<String>,
    pub app_id: Option<String>,
    pub model: String,
    pub gpu: bool,
    pub vad_sensitivity: String,
    pub output_mode: String,
}

/// Writes the session audio to `audio/` and appends one line to `manifest.jsonl`.
pub fn save(dir: &str, capture: &Capture, outcome: &DeliveryOutcome) -> io::Result<()> {
    let dir = expand_home(dir);
    let record = &capture.record;
    let now = chrono::Local::now();
    let id: String = capture
        .session_id
        .as_deref()
        .unwrap_or("session")
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(8)
        .collect();
    let name = format!("audio/{}-{id}.wav", now.format("%Y%m%d-%H%M%S"));
    fs::create_dir_all(dir.join("audio"))?;
    write_wav(&dir.join(&name), &record.audio, record.sample_rate)?;

    let raw: Vec<&str> = record
        .chunks
        .iter()
        .map(|c| c.raw_text.trim())
        .filter(|t| !t.is_empty())
        .collect();
    let (delivery, delivery_error) = match outcome {
        DeliveryOutcome::Pasted => ("pasted", None),
        DeliveryOutcome::Typed => ("typed", None),
        DeliveryOutcome::Copied => ("copied", None),
        DeliveryOutcome::Failed(message) => ("failed", Some(message.as_str())),
    };
    let line = serde_json::json!({
        "audio_filepath": name,
        "duration": record.audio.len() as f64 / record.sample_rate as f64,
        // The model's own words: the label to correct before training on it.
        "text": raw.join(" "),
        "created": now.to_rfc3339(),
        "session_id": capture.session_id,
        "sonori_version": env!("CARGO_PKG_VERSION"),
        "speechcore_version": speechcore::VERSION,
        "backend": record.backend,
        "model": capture.model,
        "language": record.language,
        "gpu": capture.gpu,
        "vad_sensitivity": capture.vad_sensitivity,
        "speech": record.speech,
        "chunks": record.chunks.iter().map(|c| serde_json::json!({
            "start": c.start,
            "end": c.end,
            "raw_text": c.raw_text,
            "text": c.text,
            "inference_secs": c.inference_secs,
        })).collect::<Vec<_>>(),
        "processed_text": capture.text,
        "magic_mode_text": capture.magic_text,
        "app_id": capture.app_id,
        "output_mode": capture.output_mode,
        "delivery": delivery,
        "delivery_error": delivery_error,
    });
    let mut manifest = OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("manifest.jsonl"))?;
    // One write per line, so a crash cannot leave half a line.
    manifest.write_all(format!("{line}\n").as_bytes())
}

/// 16-bit mono PCM WAV. The file appears only when it is complete.
fn write_wav(path: &Path, samples: &[i16], sample_rate: u32) -> io::Result<()> {
    let data_len = (samples.len() * 2) as u32;
    let mut bytes = Vec::with_capacity(44 + samples.len() * 2);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    let partial = path.with_extension("wav.part");
    fs::write(&partial, bytes)?;
    fs::rename(partial, path)
}
