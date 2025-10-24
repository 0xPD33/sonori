// Re-export common types and functions for easier imports
pub use crate::config::{AppConfig, VadConfigSerde};
pub use crate::download::download_file;
pub use crate::silero_audio_processor::{AudioSegment, SileroVad, VadConfig, VadState};
pub use crate::ui::common::AudioVisualizationData;

// Re-export common external dependencies
pub use anyhow::{anyhow, Context, Result};
pub use serde::{Deserialize, Serialize};
pub use std::collections::VecDeque;
pub use std::path::PathBuf;
pub use std::sync::{Arc, Mutex};
pub use std::time::Duration;
