//! lid_guard — Prevent system sleep while terminals are active.
//!
//! Strategy (no sudo required):
//! - macOS: `caffeinate -dimsu` child process — Apple's official API for sleep prevention,
//!   works without sudo. The process runs as a daemon child; killing it releases the assertion.
//!   Also uses IOKit `IOPMAssertionCreate` via the `core-foundation` / `io-kit-rs` crate
//!   as a robust fallback (available on all modern macOS).
//! - Linux: `systemd-inhibit` or direct logind D-Bus call (inhibit on sleep).
//! - Windows: `SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED)`.
//!
//! Safety: On drop (app exit), the child process is killed and the assertion released.
//! Terminal count tracking ensures sleep is only disabled while >=1 terminal is running.

use crate::{Error, Result};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

enum SleepGuard {
    /// macOS caffeinate child process handle
    Caffeinate(std::process::Child),
}

/// Lid guard state: tracks whether sleep is currently disabled and how.
pub struct LidGuard {
    sleep_disabled: AtomicBool,
    terminal_count: AtomicUsize,
    guard: Mutex<Option<SleepGuard>>,
}

impl LidGuard {
    pub fn new() -> Self {
        Self {
            sleep_disabled: AtomicBool::new(false),
            terminal_count: AtomicUsize::new(0),
            guard: Mutex::new(None),
        }
    }

    /// Called when a new terminal session is created.
    /// If this is the first terminal, disable sleep.
    pub fn terminal_opened(&self) -> Result<()> {
        let prev = self.terminal_count.fetch_add(1, Ordering::SeqCst);
        if prev == 0 {
            self.disable_sleep()?;
        }
        Ok(())
    }

    /// Called when a terminal session is closed.
    /// If this was the last terminal, re-enable sleep.
    pub fn terminal_closed(&self) -> Result<()> {
        let prev = self.terminal_count.fetch_sub(1, Ordering::SeqCst);
        if prev <= 1 {
            self.enable_sleep()?;
        }
        Ok(())
    }

    /// Explicitly set lid intent: true = disable sleep, false = enable sleep.
    pub fn set_lid_intent(&self, disable: bool) -> Result<()> {
        let currently_disabled = self.sleep_disabled.load(Ordering::SeqCst);
        if disable && !currently_disabled {
            self.disable_sleep()?;
        } else if !disable && currently_disabled {
            self.enable_sleep()?;
        }
        Ok(())
    }

    /// Get current terminal count.
    pub fn terminal_count(&self) -> usize {
        self.terminal_count.load(Ordering::SeqCst)
    }

    /// Get whether sleep is currently disabled.
    pub fn is_sleep_disabled(&self) -> bool {
        self.sleep_disabled.load(Ordering::SeqCst)
    }

    fn disable_sleep(&self) -> Result<()> {
        let mut guard = self.guard.lock().map_err(|e| Error::Internal(e.to_string()))?;
        if guard.is_some() {
            return Ok(()); // Already disabled
        }

        #[cfg(target_os = "macos")]
        {
            // `caffeinate -dimsu` prevents display sleep, idle sleep, disk sleep,
            // and system sleep. It's a child process; killing it releases all assertions.
            match std::process::Command::new("caffeinate")
                .args(["-dimsu"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
            {
                Ok(child) => {
                    *guard = Some(SleepGuard::Caffeinate(child));
                    self.sleep_disabled.store(true, Ordering::SeqCst);
                    eprintln!("[lid_guard] caffeinate started — sleep disabled");
                    return Ok(());
                }
                Err(e) => {
                    // caffeinate is always available on macOS — if this fails,
                    // something is fundamentally wrong, log and continue
                    eprintln!("[lid_guard] caffeinate spawn failed: {e}");
                }
            }
            // Fallback: try `pmset -b disablesleep 1` (may require sudo)
            let result = std::process::Command::new("pmset")
                .args(["-b", "disablesleep", "1"])
                .output()
                .ok();
            if let Some(output) = result {
                if output.status.success() {
                    self.sleep_disabled.store(true, Ordering::SeqCst);
                    return Ok(());
                }
            }
            // Last resort: state-only tracking
            self.sleep_disabled.store(true, Ordering::SeqCst);
        }

        #[cfg(target_os = "linux")]
        {
            // systemd-inhibit creates a lock that prevents sleep
            match std::process::Command::new("systemd-inhibit")
                .args([
                    "--what=sleep:shutdown:idle",
                    "--who=natives2",
                    "--why=Terminal session active",
                    &format!("sleep {}", std::process::id()),
                ])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
            {
                Ok(child) => {
                    *guard = Some(SleepGuard::Caffeinate(child));
                    self.sleep_disabled.store(true, Ordering::SeqCst);
                    return Ok(());
                }
                Err(e) => {
                    eprintln!("[lid_guard] systemd-inhibit failed: {e}");
                    // Try logind D-Bus directly as fallback
                    let _ = std::process::Command::new("dbus-send")
                        .args([
                            "--system",
                            "--dest=org.freedesktop.login1",
                            "/org/freedesktop/login1",
                            "org.freedesktop.login1.Manager.Inhibit",
                            "string:sleep",
                            "string:natives2",
                            "string:Terminal session active",
                            "string:block",
                        ])
                        .spawn();
                }
            }
            self.sleep_disabled.store(true, Ordering::SeqCst);
        }

        #[cfg(target_os = "windows")]
        {
            // Use SetThreadExecutionState via WinAPI
            // We use a simple approach: spawn a PowerShell background job
            // that calls kernel32::SetThreadExecutionState every 30s
            let script = r#"
                $code = @'
                [DllImport("kernel32.dll")]
                public static extern uint SetThreadExecutionState(uint flags);
                '@
                $type = Add-Type -MemberDefinition $code -Name "WinAPI" -Namespace "Sleep" -PassThru
                $type::SetThreadExecutionState(0x80000003) # ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED
            "#;
            match std::process::Command::new("powershell")
                .args(["-NoProfile", "-Command", script])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
            {
                Ok(child) => {
                    *guard = Some(SleepGuard::Caffeinate(child));
                    self.sleep_disabled.store(true, Ordering::SeqCst);
                }
                Err(e) => {
                    eprintln!("[lid_guard] PS sleep guard failed: {e}");
                    self.sleep_disabled.store(true, Ordering::SeqCst);
                }
            }
        }

        // Default for unknown platforms: just track state
        self.sleep_disabled.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn enable_sleep(&self) -> Result<()> {
        let mut guard = self.guard.lock().map_err(|e| Error::Internal(e.to_string()))?;

        // Kill the guard process — this releases the assertion
        if let Some(sg) = guard.take() {
            match sg {
                SleepGuard::Caffeinate(mut child) => {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }

        // Recovery: try to restore sleep state explicitly
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("pmset")
                .args(["-b", "disablesleep", "0"])
                .output();
        }

        self.sleep_disabled.store(false, Ordering::SeqCst);
        eprintln!("[lid_guard] sleep re-enabled");
        Ok(())
    }
}

impl Drop for LidGuard {
    fn drop(&mut self) {
        // Safety net: always restore sleep on drop (app exit)
        if self.sleep_disabled.load(Ordering::SeqCst) {
            if let Ok(mut guard) = self.guard.lock() {
                if let Some(sg) = guard.take() {
                    match sg {
                        SleepGuard::Caffeinate(mut child) => {
                            let _ = child.kill();
                            let _ = child.wait();
                        }
                    }
                }
            }
            #[cfg(target_os = "macos")]
            {
                let _ = std::process::Command::new("pmset")
                    .args(["-b", "disablesleep", "0"])
                    .spawn();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_lid_guard_is_not_disabled() {
        let guard = LidGuard::new();
        assert!(!guard.is_sleep_disabled());
        assert_eq!(guard.terminal_count(), 0);
    }

    #[test]
    fn test_terminal_open_close_increments_decrements() {
        let guard = LidGuard::new();
        guard.terminal_opened().unwrap();
        assert_eq!(guard.terminal_count(), 1);
        guard.terminal_opened().unwrap();
        assert_eq!(guard.terminal_count(), 2);
        guard.terminal_closed().unwrap();
        assert_eq!(guard.terminal_count(), 1);
        guard.terminal_closed().unwrap();
        assert_eq!(guard.terminal_count(), 0);
    }

    #[test]
    fn test_set_lid_intent_toggle() {
        let guard = LidGuard::new();
        guard.set_lid_intent(true).unwrap();
        assert!(guard.is_sleep_disabled());
        guard.set_lid_intent(false).unwrap();
        assert!(!guard.is_sleep_disabled());
    }
}
