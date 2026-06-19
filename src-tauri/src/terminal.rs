use crate::{Error, Result};
use portable_pty::{CommandBuilder, PtySize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;
use tauri::Emitter;

pub struct TerminalSession {
    #[allow(dead_code)]
    pub id: String,
    #[allow(dead_code)]
    pub pid: u32,
    pub cols: u16,
    pub rows: u16,
    #[allow(dead_code)]
    pub created_at: std::time::Instant,
}

pub struct TerminalManager {
    sessions: Arc<Mutex<HashMap<String, TerminalSession>>>,
    writers: Arc<Mutex<HashMap<String, Box<dyn std::io::Write + Send>>>>,
}

impl TerminalManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            writers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Create a new PTY session. Returns (session_id, pid).
    pub fn create_session(
        &self,
        app_handle: tauri::AppHandle,
        _profile_id: Option<&str>,
        env_overrides: Option<HashMap<String, String>>,
    ) -> Result<(String, u32)> {
        let session_id = {
            let sessions = self.sessions.lock().map_err(|e| Error::Internal(e.to_string()))?;
            let counter = sessions.len() + 1;
            format!("session-{counter}")
        };

        let cols = 80;
        let rows = 24;

        let pty_system = portable_pty::native_pty_system();
        let pty_pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| Error::Internal(format!("failed to open PTY: {e}")))?;

        // Build command
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
        let mut cmd = CommandBuilder::new(&shell);
        cmd.cwd(dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("/")));

        // Sanitize env: strip __NEXT_PRIVATE_* and add overrides
        for (key, value) in std::env::vars() {
            if !key.starts_with("__NEXT_PRIVATE_") {
                cmd.env(&key, &value);
            }
        }
        if let Some(overrides) = &env_overrides {
            for (key, value) in overrides {
                cmd.env(key, value);
            }
        }

        let mut child = pty_pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| Error::Internal(format!("failed to spawn PTY process: {e}")))?;

        let pid = child.process_id().unwrap_or(0);

        // Get writer
        let writer = pty_pair
            .master
            .take_writer()
            .map_err(|e| Error::Internal(format!("failed to get PTY writer: {e}")))?;

        // Store session and writer
        {
            let mut sessions = self.sessions.lock().map_err(|e| Error::Internal(e.to_string()))?;
            sessions.insert(
                session_id.clone(),
                TerminalSession {
                    id: session_id.clone(),
                    pid,
                    cols,
                    rows,
                    created_at: std::time::Instant::now(),
                },
            );
        }
        {
            let mut writers = self.writers.lock().map_err(|e| Error::Internal(e.to_string()))?;
            writers.insert(session_id.clone(), writer);
        }

        // Spawn reader thread — forwards PTY output to Tauri event
        let reader_session_id = session_id.clone();
        let app_handle_clone = app_handle.clone();
        thread::spawn(move || {
            let mut reader = pty_pair.master.try_clone_reader().unwrap();
            let mut buf = [0u8; 4096];
            loop {
                match std::io::Read::read(&mut reader, &mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let data = String::from_utf8_lossy(&buf[..n]).to_string();
                        let _ = app_handle_clone.emit(
                            "terminal:data",
                            serde_json::json!({ "sessionId": &reader_session_id, "data": data }),
                        );
                    }
                    Err(_) => break,
                }
            }
            // Process exited
            let _ = app_handle_clone.emit(
                "terminal:exit",
                serde_json::json!({ "sessionId": &reader_session_id, "exitCode": 0 }),
            );
        });

        // Spawn wait thread — emits exit event when process terminates
        let wait_session_id = session_id.clone();
        let app_handle_clone2 = app_handle.clone();
        let sessions_clone = self.sessions.clone();
        thread::spawn(move || {
            let _ = child.wait();
            let _ = app_handle_clone2.emit(
                "terminal:exit",
                serde_json::json!({ "sessionId": &wait_session_id, "exitCode": 0 }),
            );
            // Clean up session
            if let Ok(mut sessions) = sessions_clone.lock() {
                sessions.remove(&wait_session_id);
            }
        });

        Ok((session_id, pid))
    }

    /// Write data to a PTY session's stdin.
    pub fn write(&self, session_id: &str, data: &str) -> Result<()> {
        let mut writers = self.writers.lock().map_err(|e| Error::Internal(e.to_string()))?;
        let writer = writers
            .get_mut(session_id)
            .ok_or_else(|| Error::NotFound(format!("session {session_id}")))?;
        use std::io::Write;
        writer
            .write_all(data.as_bytes())
            .map_err(|e| Error::Internal(format!("write failed: {e}")))?;
        writer
            .flush()
            .map_err(|e| Error::Internal(format!("flush failed: {e}")))?;
        Ok(())
    }

    /// Resize a PTY session.
    pub fn resize(&self, session_id: &str, cols: u16, rows: u16) -> Result<()> {
        let mut sessions = self.sessions.lock().map_err(|e| Error::Internal(e.to_string()))?;
        if let Some(session) = sessions.get_mut(session_id) {
            session.cols = cols;
            session.rows = rows;
            // Note: PTY resize requires access to the master fd, which we don't store
            // This will be addressed when we store the PtyMaster reference
            Ok(())
        } else {
            Err(Error::NotFound(format!("session {session_id}")))
        }
    }

    /// Kill a PTY session.
    pub fn kill(&self, session_id: &str) -> Result<()> {
        let mut sessions = self.sessions.lock().map_err(|e| Error::Internal(e.to_string()))?;
        let mut writers = self.writers.lock().map_err(|e| Error::Internal(e.to_string()))?;
        sessions.remove(session_id);
        writers.remove(session_id);
        Ok(())
    }

    /// Get the CWD of a PTY session (always returns HOME for now).
    pub fn cwd(&self, session_id: &str) -> Result<String> {
        let sessions = self.sessions.lock().map_err(|e| Error::Internal(e.to_string()))?;
        if sessions.contains_key(session_id) {
            Ok(dirs::home_dir()
                .and_then(|p| p.to_str().map(|s| s.to_string()))
                .unwrap_or_else(|| "/".to_string()))
        } else {
            Err(Error::NotFound(format!("session {session_id}")))
        }
    }
}
