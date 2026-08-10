//! GhosttyManager — dedicated Ghostty lifecycle (W3 split from terminal.rs).
//!
//! Launches / focuses / kills the standalone Ghostty terminal emulator. Long
//! child processes are tracked by PID and killed idempotently on app exit.

use crate::{Error, Result};
use std::sync::{Arc, Mutex};

pub struct GhosttyManager {
    pids: Arc<Mutex<Vec<u32>>>,
}

impl GhosttyManager {
    pub fn new() -> Self {
        Self {
            pids: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Launch Ghostty as a standalone terminal emulator.
    ///
    /// On macOS, we use `open -a Ghostty` which correctly handles the .app bundle
    /// (including LSEnvironment and CFBundleExecutable).
    /// If `config_path` is provided, pass `--config-file=<path>` to Ghostty.
    pub fn launch(&self, config_path: Option<&std::path::Path>) -> Result<()> {
        let ghostty_path = Self::find_binary();
        let path = ghostty_path.ok_or_else(|| Error::NotFound("Ghostty not found".into()))?;

        // On macOS, use `open` to launch the .app bundle (handles activation
        // policies, LSEnvironment, and Dock integration correctly).
        // On other platforms, launch the binary directly.
        let child = if cfg!(target_os = "macos") {
            let mut cmd = std::process::Command::new("open");
            cmd.arg("-a").arg("Ghostty");
            if let Some(cfg) = config_path {
                // open passes --args to the launched app
                cmd.arg("--args");
                cmd.arg(format!("--config-file={}", cfg.display()));
            }
            cmd.spawn()
        } else {
            let mut cmd = std::process::Command::new(&path);
            cmd.env("LANG", "en_US.UTF-8");
            cmd.env("TERM", "xterm-256color");
            cmd.env("NATIVES", "1");
            if let Some(cfg) = config_path {
                cmd.arg(format!("--config-file={}", cfg.display()));
            }
            cmd.spawn()
        };

        let mut child =
            child.map_err(|e| Error::Internal(format!("failed to launch Ghostty: {e}")))?;
        let pid = child.id();
        {
            let mut pids = self
                .pids
                .lock()
                .map_err(|e| Error::Internal(e.to_string()))?;
            pids.push(pid);
        }
        // Detach — don't wait for child
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }

    /// Check if any Ghostty process is still running.
    /// Removes stale PIDs from the list to prevent unbounded growth.
    pub fn is_running(&self) -> bool {
        let mut pids = match self.pids.lock() {
            Ok(guard) => guard,
            Err(_) => return false,
        };
        // Filter: keep only pids that are still alive (kill -0 succeeds)
        pids.retain(|&pid| unsafe { libc::kill(pid as i32, 0) == 0 });
        !pids.is_empty()
    }

    /// Focus the Ghostty window (macOS only).
    /// Uses `osascript` to send the activate command to the running app.
    pub fn focus(&self) -> Result<()> {
        if !cfg!(target_os = "macos") {
            return Ok(());
        }
        std::process::Command::new("osascript")
            .arg("-e")
            .arg("tell application \"Ghostty\" to activate")
            .spawn()
            .map_err(|e| Error::Internal(format!("failed to focus Ghostty: {e}")))?;
        Ok(())
    }

    /// Kill all spawned Ghostty processes (called on app exit).
    pub fn kill_all(&self) {
        if let Ok(mut pids) = self.pids.lock() {
            for &pid in pids.iter() {
                unsafe {
                    libc::kill(pid as i32, libc::SIGTERM);
                }
            }
            pids.clear();
        }
    }

    /// Find the Ghostty binary.
    ///
    /// Search order:
    /// 1. /Applications/Ghostty.app/Contents/MacOS/ghostty (macOS)
    /// 2. ~/Applications/Ghostty.app/Contents/MacOS/ghostty (macOS user install)
    /// 3. ~/.local/bin/ghostty (Linux)
    /// 4. which/where ghostty (PATH)
    fn find_binary() -> Option<String> {
        let home = std::env::var("HOME").ok();
        let extra_paths = {
            let mut v = Vec::new();
            if cfg!(target_os = "macos") {
                v.push("/Applications/Ghostty.app/Contents/MacOS/ghostty".to_string());
                if let Some(ref h) = home {
                    v.push(format!(
                        "{h}/Applications/Ghostty.app/Contents/MacOS/ghostty"
                    ));
                }
            }
            if cfg!(target_os = "linux") {
                if let Some(ref h) = home {
                    v.push(format!("{h}/.local/bin/ghostty"));
                }
                v.push("/usr/local/bin/ghostty".to_string());
                v.push("/usr/bin/ghostty".to_string());
            }
            if cfg!(windows) {
                v.push("%LOCALAPPDATA%\\Programs\\ghostty\\ghostty.exe".to_string());
            }
            v
        };
        for path in &extra_paths {
            if std::path::Path::new(path).exists() {
                return Some(path.to_string());
            }
        }
        // Fallback: `which` / `where`
        let output = std::process::Command::new(if cfg!(windows) { "where" } else { "which" })
            .arg("ghostty")
            .output()
            .ok()?;
        if output.status.success() && !output.stdout.is_empty() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            stdout.lines().next().map(|s| s.to_string())
        } else {
            None
        }
    }
}
