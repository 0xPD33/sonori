use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// What the global shortcuts portal did with the configured hotkey.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum HotkeyState {
    /// Global shortcuts are off in the config.
    Disabled,
    /// Waiting for the portal to answer.
    Pending,
    /// The portal bound the shortcut. `trigger` is what the user presses.
    Bound { trigger: String },
    /// The portal did not bind the shortcut. `reason` says why.
    Unavailable { reason: String },
}

pub type SharedHotkeyState = Arc<RwLock<HotkeyState>>;

impl HotkeyState {
    /// One line for the UI, with the IPC fallback when there is no hotkey. The
    /// portal's reason is left to `sonori status` and the log.
    pub fn describe(&self) -> String {
        match self {
            HotkeyState::Disabled => {
                "Hotkey: off. Bind `sonori toggle` in your compositor.".to_string()
            }
            HotkeyState::Pending => "Hotkey: waiting for the portal".to_string(),
            HotkeyState::Bound { trigger } => format!("Hotkey: {trigger}"),
            HotkeyState::Unavailable { .. } => {
                "Hotkey: not set by the portal. Bind `sonori toggle` in your compositor."
                    .to_string()
            }
        }
    }

    /// The trigger to show in hints, if one is bound.
    pub fn trigger(&self) -> Option<&str> {
        match self {
            HotkeyState::Bound { trigger } => Some(trigger),
            _ => None,
        }
    }
}
