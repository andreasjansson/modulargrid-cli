//! Persisted login session (cookies) on disk.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cookie {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

    /// Write the session atomically: to a private temp file that is then
    /// renamed over `session.json`, so concurrent processes never see a
    /// half-written file and the cookie is never readable by other users.
    pub fn save(&self) -> Result<()> {
        self.save_to(&session_path()?)
    }

    fn save_to(&self, p: &std::path::Path) -> Result<()> {
        use std::io::Write;
        let dir = p.parent().unwrap();
        std::fs::create_dir_all(dir)?;
        let tmp = dir.join(format!(".session.json.{}", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp)?;
        f.write_all(serde_json::to_string_pretty(self)?.as_bytes())?;
        f.sync_all()?;
        std::fs::rename(&tmp, p)?;
        Ok(())
    }

    /// Whether two cookie lists hold the same cookies, ignoring order.
    pub fn same_cookies(a: &[Cookie], b: &[Cookie]) -> bool {
        let key = |v: &[Cookie]| {
            let mut v: Vec<_> = v
                .iter()
                .map(|c| (c.name.clone(), c.value.clone()))
                .collect();
            v.sort();
            v
        };
        key(a) == key(b)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn cookie(name: &str, value: &str) -> Cookie {
        Cookie {
            name: name.into(),
            value: value.into(),
        }
    }

    #[test]
    fn same_cookies_ignores_order() {
        let a = [cookie("CAKEPHP", "1"), cookie("x", "2")];
        let b = [cookie("x", "2"), cookie("CAKEPHP", "1")];
        assert!(Session::same_cookies(&a, &b));
        assert!(!Session::same_cookies(&a, &[cookie("CAKEPHP", "1")]));
        assert!(!Session::same_cookies(
            &[cookie("CAKEPHP", "1")],
            &[cookie("CAKEPHP", "2")]
        ));
    }

    #[test]
    fn save_replaces_file_atomically_and_privately() {
        let dir = std::env::temp_dir().join(format!("modulargrid-test-{}", std::process::id()));
        let p = dir.join("session.json");
        let a = Session {
            cookies: vec![cookie("CAKEPHP", "a")],
            username: Some("a".into()),
            user_id: None,
        };
        let b = Session {
            cookies: vec![cookie("CAKEPHP", "b")],
            username: Some("b".into()),
            user_id: None,
        };
        a.save_to(&p).unwrap();
        b.save_to(&p).unwrap();
        let read: Session = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(read, b);
        // Only session.json is left behind; no temp files.
        let names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("session.json")]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
