use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

/// Persisted restore tokens for portal integrations.
#[derive(Default, Serialize, Deserialize)]
pub struct PortalTokens {
    pub remote_keyboard: Option<String>,
    pub remote_screencast: Option<String>,
}

impl PortalTokens {
    pub fn load() -> Self {
        let path = match tokens_file_path() {
            Some(path) => path,
            None => return Self::default(),
        };

        match fs::read_to_string(path) {
            Ok(contents) => toml::from_str(&contents).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> io::Result<()> {
        if let Some(path) = tokens_file_path() {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }

            let toml = toml::to_string(self)
                .map_err(|e| io::Error::new(ErrorKind::Other, format!("{e}")))?;
            fs::write(path, toml)?;
        }
        Ok(())
    }
}

fn tokens_file_path() -> Option<PathBuf> {
    cache_dir().map(|dir| dir.join("portal_session.toml"))
}

fn cache_dir() -> Option<PathBuf> {
    if let Some(path) = env::var_os("XDG_CACHE_HOME") {
        return Some(PathBuf::from(path).join("sonori"));
    }

    env::var_os("HOME").map(|home| Path::new(&home).join(".cache").join("sonori"))
}
