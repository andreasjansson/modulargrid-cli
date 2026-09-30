//! Persisted login session (cookies) on disk.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cookie {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Session {
    pub cookies: Vec<Cookie>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub user_id: Option<String>,
}

pub fn config_dir() -> Result<PathBuf> {
    if let Ok(d) = std::env::var("MODULARGRID_CONFIG_DIR") {
        return Ok(PathBuf::from(d));
    }
    let base = dirs::config_dir().context("could not determine config directory")?;
    Ok(base.join("modulargrid"))
}

fn session_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("session.json"))
}

impl Session {
    pub fn load() -> Result<Option<Session>> {
        let p = session_path()?;
        match std::fs::read_to_string(&p) {
            Ok(s) => {
                Ok(Some(serde_json::from_str(&s).with_context(|| {
                    format!("corrupt session file {}", p.display())
                })?))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save(&self) -> Result<()> {
        let p = session_path()?;
        std::fs::create_dir_all(p.parent().unwrap())?;
        std::fs::write(&p, serde_json::to_string_pretty(self)?)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    pub fn delete() -> Result<bool> {
        let p = session_path()?;
        match std::fs::remove_file(&p) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e.into()),
        }
    }
}
