//! Interactive browser login.
//!
//! ModularGrid's login form is protected by reCAPTCHA, so we can't simply POST
//! credentials. Instead we launch a real Chrome/Chromium with a throwaway
//! profile, let the user log in normally, and read the session cookies out of
//! the browser over the Chrome DevTools Protocol.

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use tungstenite::{Message, WebSocket, stream::MaybeTlsStream};

use crate::session::Cookie;

const CANDIDATES: &[&str] = &[
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
    "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
    "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
    "/Applications/Vivaldi.app/Contents/MacOS/Vivaldi",
    "google-chrome",
    "google-chrome-stable",
    "chromium",
    "chromium-browser",
    "brave-browser",
    "microsoft-edge",
    r"C:\Program Files\Google\Chrome\Application\chrome.exe",
    r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
    r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
];

pub fn find_browser(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(p) = explicit {
        return Ok(p.to_path_buf());
    }
    if let Ok(p) = std::env::var("MODULARGRID_BROWSER") {
        return Ok(PathBuf::from(p));
    }
    for c in CANDIDATES {
        let p = Path::new(c);
        if p.is_absolute() {
            if p.exists() {
                return Ok(p.to_path_buf());
            }
        } else if let Some(found) = which(c) {
            return Ok(found);
        }
    }
    bail!(
        "could not find a Chromium-based browser; pass --browser <path> or set MODULARGRID_BROWSER \
         (or use `modulargrid login --cookie <CAKEPHP value>`)"
    )
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

/// On macOS, the `.app` bundle containing `exe`, if any.
fn app_bundle(exe: &Path) -> Option<PathBuf> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    exe.ancestors()
        .find(|p| p.extension().is_some_and(|e| e == "app"))
        .map(Path::to_path_buf)
}

pub struct Browser {
    child: Child,
    ws: WebSocket<MaybeTlsStream<TcpStream>>,
    next_id: u64,
    profile: PathBuf,
}

impl Browser {
    pub fn launch(exe: &Path, url: &str) -> Result<Self> {
        let profile = std::env::temp_dir().join(format!("modulargrid-login-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&profile);
        std::fs::create_dir_all(&profile)?;

        let args = [
            format!("--user-data-dir={}", profile.display()),
            "--remote-debugging-port=0".into(),
            "--no-first-run".into(),
            "--no-default-browser-check".into(),
            "--new-window".into(),
            url.into(),
        ];
        let mut cmd = match app_bundle(exe) {
            // Launched directly, a macOS app started from a terminal/background process opens
            // behind other windows. `open` brings it to the front; `-n` forces a separate
            // instance (so our profile and debugging port are used even when the user's Chrome
            // is already running) and `-W` keeps `open` alive until that instance quits.
            Some(bundle) => {
                let mut c = Command::new("/usr/bin/open");
                c.arg("-n").arg("-W").arg("-a").arg(bundle).arg("--args").args(&args);
                c
            }
            None => {
                let mut c = Command::new(exe);
                c.args(&args);
                c
            }
        };
        let child = cmd
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("failed to launch {}", exe.display()))?;

        // With --remote-debugging-port=0 Chrome picks a free port and writes it,
        // plus the browser websocket path, to DevToolsActivePort.
        let port_file = profile.join("DevToolsActivePort");
        let deadline = Instant::now() + Duration::from_secs(20);
        let contents = loop {
            if let Ok(s) = std::fs::read_to_string(&port_file)
                && s.lines().count() >= 2 {
                    break s;
                }
            if Instant::now() > deadline {
                bail!("browser did not expose a DevTools port (is another instance hijacking the launch?)");
            }
            std::thread::sleep(Duration::from_millis(200));
        };
        let mut lines = contents.lines();
        let port = lines.next().unwrap().trim();
        let path = lines.next().unwrap().trim();
        let ws_url = format!("ws://127.0.0.1:{port}{path}");
        let (ws, _) = tungstenite::connect(&ws_url).context("connecting to DevTools websocket")?;

        Ok(Self { child, ws, next_id: 1, profile })
    }

    fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let msg = json!({"id": id, "method": method, "params": params});
        self.ws.send(Message::text(msg.to_string()))?;
        loop {
            let m = self.ws.read()?;
            let Message::Text(t) = m else { continue };
            let v: Value = serde_json::from_str(&t)?;
            if v.get("id").and_then(Value::as_u64) == Some(id) {
                if let Some(e) = v.get("error") {
                    return Err(anyhow!("CDP {method} failed: {e}"));
                }
                return Ok(v["result"].clone());
            }
        }
    }

    pub fn cookies_for(&mut self, domain_suffix: &str) -> Result<Vec<Cookie>> {
        let r = self.call("Storage.getCookies", json!({}))?;
        let cookies = r["cookies"].as_array().cloned().unwrap_or_default();
        Ok(cookies
            .into_iter()
            .filter(|c| {
                c["domain"]
                    .as_str()
                    .is_some_and(|d| d.trim_start_matches('.').ends_with(domain_suffix))
            })
            .map(|c| Cookie {
                name: c["name"].as_str().unwrap_or_default().to_string(),
                value: c["value"].as_str().unwrap_or_default().to_string(),
            })
            .collect())
    }

    /// Returns false once the browser (or its connection) has gone away.
    pub fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    pub fn close(mut self) {
        let _ = self.call("Browser.close", json!({}));
        std::thread::sleep(Duration::from_millis(300));
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.profile);
    }
}
