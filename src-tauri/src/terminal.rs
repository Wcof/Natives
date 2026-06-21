use crate::{Error, Result};
use crate::terminal_recorder;
use portable_pty::{CommandBuilder, MasterPty, PtySize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use tauri::Emitter;

#[cfg(feature = "ghostty-vt")]
use crate::GhosttyTerminal;

/// Minimum interval (ms) between render-state emits for throttling
#[allow(dead_code)]
const RENDER_STATE_THROTTLE_MS: u64 = 16;

/// Sanitize PTY input by stripping dangerous escape sequences.
///
/// Removes OSC (`ESC ]`) and DCS (`ESC P`) sequences that could be used
/// for injection attacks (e.g., OSC 7 to hijack CWD, DCS to inject commands).
/// Normal keyboard input never contains these; they only appear in paste
/// content or programmatic writes. Bracketed paste mode handles paste
/// correctly, so stripping from raw writes is safe.
fn sanitize_pty_input(data: &str) -> String {
    let bytes = data.as_bytes();
    let mut result = String::with_capacity(data.len());
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() {
            let next = bytes[i + 1];
            if next == b']' {
                // OSC sequence: skip until BEL (0x07) or ST (ESC \)
                i += 2;
                while i < bytes.len() {
                    if bytes[i] == 0x07 { i += 1; break; }
                    if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                        i += 2; break;
                    }
                    i += 1;
                }
                continue;
            } else if next == b'P' {
                // DCS sequence: skip until ST (ESC \)
                i += 2;
                while i < bytes.len() {
                    if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                        i += 2; break;
                    }
                    i += 1;
                }
                continue;
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }

    result
}

/// Session 生命周期状态
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum SessionStatus {
    Running,
    Exiting,
    Exited(i32),
}

/// 收敛的 session 状态结构
pub struct TerminalSession {
    pub cols: u16,
    pub rows: u16,
    pub created_at: std::time::Instant,
    /// 当前 title（来自 OSC 0 / OSC 1 或自动推断）
    pub title: String,
    /// 当前工作目录（OSC 7 追踪优先 → lsof fallback → HOME）
    pub cwd: String,
    /// 前台进程名（用于判断当前 tab 状态）
    pub foreground_process: String,
    /// 生命周期状态
    pub status: SessionStatus,
    /// Stored PtyPair master for actual PTY resize
    master_pty: Option<Arc<Mutex<Box<dyn MasterPty + Send>>>>,
    /// Stored child handle for process termination
    child_killer: Option<Box<dyn portable_pty::ChildKiller + Send>>,
    pub pid: u32,
    /// Ghostty VT 终端引擎实例（feature gate ghostty-vt）
    #[cfg(feature = "ghostty-vt")]
    pub ghostty: Option<Arc<Mutex<GhosttyTerminal>>>,
}

pub struct TerminalManager {
    sessions: Arc<Mutex<HashMap<String, TerminalSession>>>,
    writers: Arc<Mutex<HashMap<String, Box<dyn std::io::Write + Send>>>>,
    recorder: Option<std::sync::Arc<terminal_recorder::Recorder>>,
    /// 递增 session ID 计数器（避免关闭旧 session 后复用 id）
    session_counter: Arc<AtomicU64>,
}

impl TerminalManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            writers: Arc::new(Mutex::new(HashMap::new())),
            recorder: None,
            session_counter: Arc::new(AtomicU64::new(1)),
        }
    }

    /// Set the recorder instance (called from setup)
    pub fn set_recorder(&mut self, recorder: std::sync::Arc<terminal_recorder::Recorder>) {
        self.recorder = Some(recorder);
    }

    /// 从 PTY 输出流中解析 OSC 7 工作目录（file://hostname/path 格式）
    /// 返回 (是否匹配, 提取到的路径)
    fn parse_osc7(data: &str) -> Option<String> {
        // OSC 7: ESC ] 7 ; file://hostname/path BEL 或 ESC ] 7 ; file://hostname/path ESC \
        // 也兼容裸路径: ESC ] 7 ; /path BEL
        for line in data.split('\n') {
            // 查找 ESC ] 7 ;
            if let Some(pos) = line.find("\x1b]7;") {
                let after_prefix = &line[pos + 5..];
                let end = after_prefix.find(|c: char| c == '\x07' || c == '\x1b')
                    .unwrap_or(after_prefix.len());
                let content = &after_prefix[..end];
                // 格式: file://hostname/path 或裸路径
                if content.starts_with("file://") {
                    // file://hostname/path → 跳过多余部分找到 /path
                    let path_start = content.find('/');
                    if let Some(ps) = path_start {
                        // 跳过第一个 /, 找到第二个 / (hostname 后的路径)
                        let rest = &content[ps..];
                        let decoded = url_decode_path(rest);
                        if std::path::Path::new(&decoded).exists() || decoded.starts_with('/') {
                            return Some(decoded);
                        }
                    }
                } else if content.starts_with('/') {
                    // 裸路径格式
                    let decoded = url_decode_path(content);
                    if std::path::Path::new(&decoded).exists() || decoded.starts_with('/') {
                        return Some(decoded);
                    }
                }
            }
        }
        None
    }

    /// 从 PTY 输出流中解析 OSC 0 / OSC 1 / OSC 2 title
    fn parse_osc_title(data: &str) -> Option<String> {
        for line in data.split('\n') {
            // OSC 0: ESC ] 0 ; title BEL
            // OSC 1: ESC ] 1 ; title BEL
            // OSC 2: ESC ] 2 ; title BEL
            for osc_num in ["0", "1", "2"] {
                let pattern = format!("\x1b]{};", osc_num);
                if let Some(pos) = line.find(&pattern) {
                    let after_prefix = &line[pos + pattern.len()..];
                    let end = after_prefix.find(|c: char| c == '\x07' || c == '\x1b')
                        .unwrap_or(after_prefix.len());
                    let title = &after_prefix[..end];
                    if !title.is_empty() {
                        return Some(title.to_string());
                    }
                }
            }
        }
        None
    }

    /// Create a new PTY session. Returns (session_id, pid).
    pub fn create_session(
        &self,
        app_handle: tauri::AppHandle,
        _profile_id: Option<&str>,
        env_overrides: Option<HashMap<String, String>>,
        initial_cols: Option<u16>,
        initial_rows: Option<u16>,
    ) -> Result<(String, u32)> {
        // 使用原子计数器生成唯一 session id，不依赖 sessions.len()
        let counter = self.session_counter.fetch_add(1, Ordering::SeqCst);
        let session_id = format!("session-{counter}");

        // 若调用方提供了真实尺寸则使用，否则 fallback 80x24
        let cols = initial_cols.unwrap_or(80);
        let rows = initial_rows.unwrap_or(24);

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
        let shell = std::env::var("SHELL").unwrap_or_else(|_| {
            if cfg!(windows) { "powershell.exe".to_string() } else { "/bin/zsh".to_string() }
        });
        let mut cmd = CommandBuilder::new(&shell);

        // Login shell (-l): GUI 启动的进程只继承精简 PATH，不读 .zprofile/.zlogin，
        // 用户在那里配的 Homebrew/nvm/npm 全局路径就丢了。login shell 把这些路径带进来。
        // Windows 的 powershell 无此机制，保持空参数。
        if !cfg!(windows) {
            cmd.arg("-l");
        }

        cmd.cwd(dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("/")));

        // 配置 shell 主动发送 OSC 7 (cwd) 和 OSC 0 (title) 序列
        // 让 Natives2 能追踪当前目录和窗口标题，无需轮询 lsof
        let shell_config = if shell.ends_with("zsh") {
            Some((
                "_NATIVES_CFG",
                r#"
# Natives2: 自动发送 OSC 7 (cwd) 和 OSC 0 (title) 序列
if [[ -n "$NATIVES" ]]; then
    # chpwd 在每次 cd 后触发
    function _natives_chpwd() {
        # OSC 7: 当前工作目录
        print -n "\e]7;file://${HOSTNAME}${PWD// /%20}\a"
        # OSC 0: 简洁的 title（当前目录 basename）
        print -n "\e]0;${PWD##*/}\a"
    }
    autoload -Uz add-zsh-hook
    add-zsh-hook chpwd _natives_chpwd
    # 立即发送一次当前状态
    _natives_chpwd
    # preexec: 命令执行时更新 title 为命令名
    function _natives_preexec() {
        local cmd="${1[(w)1]}"
        print -n "\e]0;${cmd}\a"
    }
    add-zsh-hook preexec _natives_preexec
fi
"#.trim()
            ))
        } else if shell.ends_with("bash") {
            Some((
                "_NATIVES_CFG",
                r#"
# Natives2: 自动发送 OSC 7 (cwd) 和 OSC 0 (title) 序列
if [ -n "$NATIVES" ]; then
    _natives_chpwd() {
        printf '\e]7;file://%s%s\a' "$HOSTNAME" "${PWD// /%20}"
        printf '\e]0;%s\a' "${PWD##*/}"
    }
    PROMPT_COMMAND="_natives_chpwd${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
    _natives_chpwd
fi
"#.trim()
            ))
        } else {
            None
        };

        if let Some((var, val)) = shell_config {
            cmd.env(var, val);
        }

        // Sanitize env: strip __NEXT_PRIVATE_* and add overrides + locale
        let mut has_utf8_locale = false;
        for (key, value) in std::env::vars() {
            if key.starts_with("__NEXT_PRIVATE_") { continue; }
            if key == "LC_ALL" || key == "LC_CTYPE" || key == "LANG" {
                if value.contains("UTF-8") || value.contains("utf8") || value.contains("utf-8") {
                    has_utf8_locale = true;
                }
            }
            cmd.env(&key, &value);
        }
        // GUI 启动的 app 不继承 shell 的 locale，zsh 会把中文路径按字节转义成乱码
        // 兜底设 UTF-8 locale
        if !has_utf8_locale {
            cmd.env("LANG", if cfg!(target_os = "macos") { "zh_CN.UTF-8" } else { "en_US.UTF-8" });
        }
        // 标准终端能力声明
        cmd.env("TERM", "xterm-256color");
        cmd.env("NATIVES", "1");
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
        let child_killer: Box<dyn portable_pty::ChildKiller + Send> = child.clone_killer();

        // Get writer and clone reader handle BEFORE moving master into Arc
        let writer = pty_pair
            .master
            .take_writer()
            .map_err(|e| Error::Internal(format!("failed to get PTY writer: {e}")))?;

        // Clone a reader before packaging the master
        let reader = pty_pair
            .master
            .try_clone_reader()
            .map_err(|e| Error::Internal(format!("failed to clone PTY reader: {e}")))?;

        // Wrap master in Arc for shared access (resize + future readers)
        let master_pty = Arc::new(Mutex::new(pty_pair.master));

        // ── Ghostty VT 终端引擎（feature gate ghostty-vt） ──
        #[cfg(feature = "ghostty-vt")]
        let ghostty_vt: Option<Arc<Mutex<GhosttyTerminal>>> =
            match GhosttyTerminal::new(cols, rows, app_handle.clone(), session_id.clone()) {
                Ok(term) => Some(Arc::new(Mutex::new(term))),
                Err(e) => {
                    eprintln!("[ghostty-vt] failed to create terminal: {e}");
                    None
                }
            };
        #[cfg(not(feature = "ghostty-vt"))]
        let _ghostty_vt: Option<Arc<Mutex<()>>> = None;

        let home = dirs::home_dir()
            .and_then(|p| p.to_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "/".to_string());

        let shell_basename = std::path::Path::new(&shell)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("zsh")
            .to_string();

        {
            let mut sessions = self.sessions.lock().map_err(|e| Error::Internal(e.to_string()))?;
            sessions.insert(
                session_id.clone(),
                TerminalSession {
                    cols,
                    rows,
                    created_at: std::time::Instant::now(),
                    title: shell_basename.clone(),
                    cwd: home.clone(),
                    foreground_process: shell_basename.clone(),
                    status: SessionStatus::Running,
                    master_pty: Some(master_pty.clone()),
                    child_killer: Some(child_killer),
                    pid,
                    #[cfg(feature = "ghostty-vt")]
                    ghostty: ghostty_vt.clone(),
                },
            );
        }
        {
            let mut writers = self.writers.lock().map_err(|e| Error::Internal(e.to_string()))?;
            writers.insert(session_id.clone(), writer);
        }

        // Clone ghostty Arc for the reader thread (only if feature enabled)
        #[cfg(feature = "ghostty-vt")]
        let ghostty_reader = ghostty_vt.clone();

        // Clone recorder Arc for reader thread
        let reader_recorder = self.recorder.clone();

        // Clone session tracking for reader thread to update cwd/title in-place
        let reader_sessions = self.sessions.clone();

        // Spawn reader thread — forwards PTY output to Tauri event + Ghostty VT
        let reader_session_id = session_id.clone();
        let app_handle_clone = app_handle.clone();
        thread::spawn(move || {
            let mut reader = reader;
            let mut buf = [0u8; 4096];
            #[cfg(feature = "ghostty-vt")]
            let mut last_render_state = std::time::Instant::now();
            #[cfg(feature = "ghostty-vt")]
            let ghostty = ghostty_reader;
            // 跨多次 read 调用累积 OSC 状态（OSC 序列可能跨越 buffer 边界）
            let mut osc_buffer = String::new();
            let mut in_osc = false;

            loop {
                match std::io::Read::read(&mut reader, &mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let raw = &buf[..n];
                        // 1. Emit raw PTY data to frontend (existing behavior)
                        let data_str = String::from_utf8_lossy(raw).to_string();
                        let _ = app_handle_clone.emit(
                            "terminal:data",
                            serde_json::json!({ "sessionId": &reader_session_id, "data": data_str }),
                        );

                        // 1b. Record to asciinema .cast file (fanbox style, silent skip)
                        if let Some(ref rec) = reader_recorder {
                            let _ = rec.record(&reader_session_id, &data_str);
                        }

                        // 1c. 解析 OSC 7 (cwd) 和 OSC title 序列
                        if let Ok(mut sessions) = reader_sessions.lock() {
                            if let Some(session) = sessions.get_mut(&reader_session_id) {
                                // 解析 OSC 7
                                if let Some(cwd) = Self::parse_osc7(&data_str) {
                                    session.cwd = cwd.clone();
                                    let _ = app_handle_clone.emit(
                                        "terminal:pwd-changed",
                                        serde_json::json!({ "sessionId": &reader_session_id, "pwd": cwd }),
                                    );
                                }
                                // 解析 OSC title
                                if let Some(title) = Self::parse_osc_title(&data_str) {
                                    session.title = title.clone();
                                    let _ = app_handle_clone.emit(
                                        "terminal:title-changed",
                                        serde_json::json!({ "sessionId": &reader_session_id, "title": title }),
                                    );
                                }
                                // 检查输出中的前台进程提示（preexec 已设 title，还需跟踪进程）
                                // 简单启发式：查找常见 TUI 进程名
                            }
                        }

                        // 2. Feed data to Ghostty VT parser (feature gate)
                        #[cfg(feature = "ghostty-vt")]
                        if let Some(ref g) = ghostty {
                            if let Ok(guard) = g.lock() {
                                let _ = guard.vt_write(&buf[..n]);
                                drop(guard); // release lock before emitting

                                // 3. Throttled render-state emit (16ms ≈ 60fps)
                                let now = std::time::Instant::now();
                                if now.duration_since(last_render_state)
                                    >= std::time::Duration::from_millis(RENDER_STATE_THROTTLE_MS)
                                {
                                    if let Ok(guard) = g.lock() {
                                        let _ = app_handle_clone.emit(
                                            "terminal:render-state",
                                            serde_json::json!({
                                                "sessionId": &reader_session_id,
                                                "cursorX": guard.cursor_x(),
                                                "cursorY": guard.cursor_y(),
                                                "title": guard.title(),
                                                "pwd": guard.pwd(),
                                                "cols": guard.cols(),
                                                "rows": guard.rows(),
                                            }),
                                        );
                                    }
                                    last_render_state = now;
                                }
                            }
                        }

                        // 4. 跨 buffer OSC 序列拼接（处理 OSC 被截断的情况）
                        for &byte in raw {
                            if byte == 0x1b {
                                in_osc = true;
                                osc_buffer.clear();
                                osc_buffer.push(byte as char);
                            } else if in_osc {
                                osc_buffer.push(byte as char);
                                if byte == 0x07 || byte == 0x1b {
                                    // OSC 序列结束，尝试解析
                                    if osc_buffer.contains("]7;") || osc_buffer.contains("]0;")
                                        || osc_buffer.contains("]1;") || osc_buffer.contains("]2;")
                                    {
                                        if let Ok(mut sessions) = reader_sessions.lock() {
                                            if let Some(session) = sessions.get_mut(&reader_session_id) {
                                                if let Some(cwd) = Self::parse_osc7(&osc_buffer) {
                                                    session.cwd = cwd.clone();
                                                    let _ = app_handle_clone.emit(
                                                        "terminal:pwd-changed",
                                                        serde_json::json!({ "sessionId": &reader_session_id, "pwd": cwd }),
                                                    );
                                                }
                                                if let Some(title) = Self::parse_osc_title(&osc_buffer) {
                                                    session.title = title.clone();
                                                    let _ = app_handle_clone.emit(
                                                        "terminal:title-changed",
                                                        serde_json::json!({ "sessionId": &reader_session_id, "title": title }),
                                                    );
                                                }
                                            }
                                        }
                                    }
                                    in_osc = false;
                                    osc_buffer.clear();
                                }
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
            // Reader EOF — do NOT emit exit here; the wait thread handles it
        });

        // Spawn wait thread — captures actual exit code and emits exit event
        let wait_session_id = session_id.clone();
        let app_handle_clone2 = app_handle.clone();
        let sessions_clone = self.sessions.clone();
        let writers_clone = self.writers.clone();
        thread::spawn(move || {
            let exit_code = match child.wait() {
                Ok(status) => status.exit_code() as i64,
                Err(_) => -1i64,
            };
            // 更新 session 状态
            if let Ok(mut sessions) = sessions_clone.lock() {
                if let Some(session) = sessions.get_mut(&wait_session_id) {
                    session.status = SessionStatus::Exited(exit_code as i32);
                }
            }
            // 只发送一次 exit 事件（由 wait thread 负责，reader 不触发）
            let _ = app_handle_clone2.emit(
                "terminal:exit",
                serde_json::json!({ "sessionId": &wait_session_id, "exitCode": exit_code }),
            );
            // Clean up session and writer to prevent resource leak
            if let Ok(mut sessions) = sessions_clone.lock() {
                sessions.remove(&wait_session_id);
            }
            if let Ok(mut writers) = writers_clone.lock() {
                writers.remove(&wait_session_id);
            }
        });

        Ok((session_id, pid))
    }

    /// Write data to a PTY session's stdin.
    ///
    /// Security: filters OSC (\x1b]) and DCS (\x1bP) sequences from input
    /// to prevent escape injection attacks (e.g., OSC 7 CWD hijacking,
    /// DCS command injection via malicious paste content).
    pub fn write(&self, session_id: &str, data: &str) -> Result<()> {
        // ── Input sanitization: strip dangerous escape sequences ──
        // Normal keyboard input never contains OSC (ESC ]) or DCS (ESC P).
        // These only appear in paste content or programmatic writes.
        // Bracketed paste mode handles paste correctly, so stripping these
        // from raw writes is safe and prevents injection attacks.
        let sanitized = sanitize_pty_input(data);

        let mut writers = self.writers.lock().map_err(|e| Error::Internal(e.to_string()))?;
        let writer = writers
            .get_mut(session_id)
            .ok_or_else(|| Error::NotFound(format!("session {session_id}")))?;
        use std::io::Write;
        writer
            .write_all(sanitized.as_bytes())
            .map_err(|e| Error::Internal(format!("write failed: {e}")))?;
        writer
            .flush()
            .map_err(|e| Error::Internal(format!("flush failed: {e}")))?;

        // 录制输入事件（asciinema "i" 事件）
        if let Some(ref rec) = self.recorder {
            let _ = rec.record_input(session_id, &sanitized);
        }

        Ok(())
    }

    /// Resize a PTY session — sends the new size to the kernel via the master fd.
    pub fn resize(&self, session_id: &str, cols: u16, rows: u16) -> Result<()> {
        let mut sessions = self.sessions.lock().map_err(|e| Error::Internal(e.to_string()))?;
        if let Some(session) = sessions.get_mut(session_id) {
            // Update in-memory fields
            session.cols = cols;
            session.rows = rows;
            // Actual PTY resize via the master fd
            if let Some(ref master) = session.master_pty {
                if let Ok(m) = master.lock() {
                    if let Err(e) = m.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }) {
                        eprintln!("[Terminal] PTY resize failed for {session_id}: {e}");
                    }
                }
            }
            // 录制 resize 事件（asciinema "r" 事件）
            drop(sessions); // 避免双重锁
            if let Some(ref rec) = self.recorder {
                let _ = rec.record_resize(session_id, cols, rows);
            }
            Ok(())
        } else {
            Err(Error::NotFound(format!("session {session_id}")))
        }
    }

    /// Kill a PTY session — terminates the child process and cleans up.
    pub fn kill(&self, session_id: &str) -> Result<()> {
        let mut sessions = self.sessions.lock().map_err(|e| Error::Internal(e.to_string()))?;
        let mut writers = self.writers.lock().map_err(|e| Error::Internal(e.to_string()))?;

        if let Some(session) = sessions.get_mut(session_id) {
            // ── Unix: send SIGTERM to the entire process group first ──
            // This ensures child processes (npm run dev, database servers, etc.)
            // spawned by the shell are also cleaned up, not orphaned.
            #[cfg(unix)]
            if session.pid > 1 {
                unsafe {
                    let _ = libc::kill(-(session.pid as libc::pid_t), libc::SIGTERM);
                }
            }

            // Kill the child process if we have a handle
            if let Some(ref mut killer) = session.child_killer {
                let _ = killer.kill();
            }
        }

        sessions.remove(session_id);
        writers.remove(session_id);
        Ok(())
    }

    /// Kill all active PTY sessions — called on app exit or Drop.
    pub fn kill_all(&self) {
        let mut sessions = match self.sessions.lock() {
            Ok(s) => s,
            Err(_) => return,
        };
        let _writers = match self.writers.lock() {
            Ok(w) => w,
            Err(_) => return,
        };

        for (_id, session) in sessions.iter_mut() {
            #[cfg(unix)]
            if session.pid > 1 {
                unsafe {
                    let _ = libc::kill(-(session.pid as libc::pid_t), libc::SIGTERM);
                }
            }
            if let Some(ref mut killer) = session.child_killer {
                let _ = killer.kill();
            }
        }
        sessions.clear();
    }

    /// Get session info (used by ghostty-vt render state query).
    #[cfg(feature = "ghostty-vt")]
    pub fn get_session(&self, _session_id: &str) -> Result<std::sync::MutexGuard<'_, std::collections::HashMap<String, TerminalSession>>> {
        self.sessions.lock().map_err(|e| Error::Internal(e.to_string()))
    }

    /// 获取 session 状态快照（用于前端查询）
    pub fn get_session_state(&self, session_id: &str) -> Result<SessionStateSnapshot> {
        let sessions = self.sessions.lock().map_err(|e| Error::Internal(e.to_string()))?;
        let session = sessions
            .get(session_id)
            .ok_or_else(|| Error::NotFound(format!("session {session_id}")))?;
        Ok(SessionStateSnapshot {
            session_id: session_id.to_string(),
            cols: session.cols,
            rows: session.rows,
            title: session.title.clone(),
            cwd: session.cwd.clone(),
            foreground_process: session.foreground_process.clone(),
            pid: session.pid,
            status: session.status.clone(),
        })
    }

    /// 获取 session 列表（用于前端展示 tabs）
    pub fn list_sessions(&self) -> Result<Vec<SessionStateSnapshot>> {
        let sessions = self.sessions.lock().map_err(|e| Error::Internal(e.to_string()))?;
        Ok(sessions.iter().map(|(id, s)| SessionStateSnapshot {
            session_id: id.clone(),
            cols: s.cols,
            rows: s.rows,
            title: s.title.clone(),
            cwd: s.cwd.clone(),
            foreground_process: s.foreground_process.clone(),
            pid: s.pid,
            status: s.status.clone(),
        }).collect())
    }

    /// Get the CWD of a PTY session.
    /// 优先使用 OSC 7 追踪到的 cwd（已存储在 session.cwd），
    /// 失败再走 lsof fallback，最后才回 HOME。
    pub fn cwd(&self, session_id: &str) -> Result<(String, &str)> {
        let sessions = self.sessions.lock().map_err(|e| Error::Internal(e.to_string()))?;
        let session = sessions
            .get(session_id)
            .ok_or_else(|| Error::NotFound(format!("session {session_id}")))?;

        // 1. 如果 OSC 7 追踪到的 cwd 存在且是有效目录，优先返回
        if !session.cwd.is_empty()
            && session.cwd != "/"
            && std::path::Path::new(&session.cwd).exists()
        {
            return Ok((session.cwd.clone(), "osc7"));
        }

        let pid = session.pid;
        drop(sessions); // 释放锁，避免 lsof 阻塞

        // 2. lsof fallback
        if pid != 0 {
            if let Some(lsof_cwd) = Self::lsof_cwd(pid) {
                return Ok((lsof_cwd, "lsof"));
            }
        }

        // 3. HOME fallback
        let home = dirs::home_dir()
            .and_then(|p| p.to_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "/".to_string());
        Ok((home, "home"))
    }

    /// lsof 兜底检测 cwd
    fn lsof_cwd(pid: u32) -> Option<String> {
        let output = std::process::Command::new("lsof")
            .arg("-p")
            .arg(pid.to_string())
            .arg("-Fn")
            .env("LC_ALL", "en_US.UTF-8")
            .output().ok()?;

        if !output.status.success() {
            return None;
        }

        let stdout_str = String::from_utf8_lossy(&output.stdout);
        let mut last_n_line = None;
        for line in stdout_str.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('n') {
                let path_part = &trimmed[1..];
                if std::path::Path::new(path_part).is_dir() {
                    last_n_line = Some(path_part.to_string());
                }
            }
        }

        if let Some(path_str) = last_n_line {
            let decoded = decode_lsof_path(&path_str);
            if std::path::Path::new(&decoded).exists() {
                return Some(decoded);
            }
        }
        None
    }

    /// 获取前台进程信息
    pub fn proc(&self, session_id: &str) -> Result<(String, u32)> {
        let sessions = self.sessions.lock().map_err(|e| Error::Internal(e.to_string()))?;
        let session = sessions
            .get(session_id)
            .ok_or_else(|| Error::NotFound(format!("session {session_id}")))?;

        let pid = session.pid;
        if pid == 0 {
            return Ok(("unknown".to_string(), 0));
        }

        // 获取 PTY shell 的子进程（前台进程）
        // 使用 `ps -o comm=,pid= -p <child>` 递归查找
        drop(sessions);

        let proc_name = Self::get_foreground_process(pid)
            .unwrap_or_else(|| "shell".to_string());

        // 也尝试更新 session 中的 foreground_process
        if let Ok(mut sessions) = self.sessions.lock() {
            if let Some(s) = sessions.get_mut(session_id) {
                s.foreground_process = proc_name.clone();
            }
        }

        Ok((proc_name, pid))
    }

    /// 递归查找前台进程名
    fn get_foreground_process(pid: u32) -> Option<String> {
        // 先试着直接获取进程名
        if let Some(name) = Self::get_process_name(pid) {
            // 如果是 shell，查找其子进程
            let name_lower = name.to_lowercase();
            if name_lower.contains("zsh") || name_lower.contains("bash")
                || name_lower.contains("fish") || name_lower.contains("sh")
                || name_lower.contains("powershell") || name_lower.contains("cmd")
            {
                // 查找子进程
                if let Some(child) = Self::get_child_process(pid) {
                    return Some(child);
                }
            }
            return Some(name);
        }
        None
    }

    /// 通过 ps 获取进程名
    fn get_process_name(pid: u32) -> Option<String> {
        let output = std::process::Command::new("ps")
            .arg("-p")
            .arg(pid.to_string())
            .arg("-o")
            .arg("comm=")
            .output().ok()?;
        if output.status.success() {
            let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !name.is_empty() {
                return Some(name);
            }
        }
        None
    }

    /// 获取指定进程的直接子进程（最近创建的）
    fn get_child_process(parent_pid: u32) -> Option<String> {
        // macOS/Linux: ps -o pid=,comm= --ppid <pid>
        let output = std::process::Command::new("ps")
            .arg("--ppid")
            .arg(parent_pid.to_string())
            .arg("-o")
            .arg("comm=,pid=")
            .output().ok()?;
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // 取最后一行（最新子进程）
            let line = stdout.lines()
                .map(|l| l.trim())
                .filter(|l| !l.is_empty())
                .last()?;
            // 格式: "comm pid"
            let name = line.split_whitespace().next()?;
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
        None
    }
}

// ── Drop: automatically clean up all PTY sessions when TerminalManager is destroyed ──
impl Drop for TerminalManager {
    fn drop(&mut self) {
        self.kill_all();
    }
}

fn decode_lsof_path(path: &str) -> String {
    if !path.contains("\\x") {
        return path.to_string();
    }

    let mut bytes = Vec::new();
    let chars: Vec<char> = path.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if i + 3 < chars.len() && chars[i] == '\\' && chars[i+1] == 'x' {
            let hex_str: String = chars[i+2..i+4].iter().collect();
            if let Ok(byte_val) = u8::from_str_radix(&hex_str, 16) {
                bytes.push(byte_val);
                i += 4;
                continue;
            }
        }
        let mut buf = [0; 4];
        let char_str = chars[i].encode_utf8(&mut buf);
        bytes.extend_from_slice(char_str.as_bytes());
        i += 1;
    }

    String::from_utf8(bytes).unwrap_or_else(|_| path.to_string())
}

/// 简单的 URL 解码（OSC 7 路径可能包含 %20 等编码）
fn url_decode_path(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                result.push(byte as char);
            } else {
                result.push('%');
                result.push_str(&hex);
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Session 状态快照（用于 IPC 传输）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStateSnapshot {
    pub session_id: String,
    pub cols: u16,
    pub rows: u16,
    pub title: String,
    pub cwd: String,
    pub foreground_process: String,
    pub pid: u32,
    pub status: SessionStatus,
}

// ──────────────────────────────────────────────
// Generic external tool detect / launch
// ──────────────────────────────────────────────

/// Detect whether an external binary is installed.
/// Checks `extra_paths` first, then falls back to `which $name`.
pub fn detect_binary(name: &str, extra_paths: &[&str]) -> bool {
    // Check extra paths first (e.g. macOS .app bundles)
    for path in extra_paths {
        if std::path::Path::new(path).exists() {
            return true;
        }
    }
    // Fallback: `which` / `where`
    let output = std::process::Command::new(if cfg!(windows) { "where" } else { "which" })
        .arg(name)
        .output();
    if let Ok(out) = output {
        if out.status.success() && !out.stdout.is_empty() {
            return true;
        }
    }
    false
}

/// Launch an external binary. Searches `extra_paths` first, then `which $name`.
pub fn launch_binary(name: &str, extra_paths: &[&str]) -> Result<()> {
    // Find the actual path
    let mut resolved_path: Option<String> = None;
    for path in extra_paths {
        if std::path::Path::new(path).exists() {
            resolved_path = Some(path.to_string());
            break;
        }
    }
    if resolved_path.is_none() {
        let output = std::process::Command::new(if cfg!(windows) { "where" } else { "which" })
            .arg(name)
            .output();
        if let Ok(out) = output {
            if out.status.success() {
                let stdout = String::from_utf8_lossy(&out.stdout);
                if let Some(first_line) = stdout.lines().next() {
                    resolved_path = Some(first_line.to_string());
                }
            }
        }
    }

    let bin_path = resolved_path
        .ok_or_else(|| Error::NotFound(format!("{name} not found")))?;

    std::process::Command::new(&bin_path)
        .spawn()
        .map_err(|e| Error::Internal(format!("failed to launch {bin_path}: {e}")))?;

    Ok(())
}

// ──────────────────────────────────────────────
// GhosttyManager — dedicated Ghostty lifecycle
// ──────────────────────────────────────────────

pub struct GhosttyManager {
    pids: Arc<Mutex<Vec<u32>>>,
}

impl GhosttyManager {
    pub fn new() -> Self {
        Self { pids: Arc::new(Mutex::new(Vec::new())) }
    }

    /// Launch Ghostty as a standalone terminal emulator.
    ///
    /// On macOS, we use `open -a Ghostty` which correctly handles the .app bundle
    /// (including LSEnvironment and CFBundleExecutable).
    /// If `config_path` is provided, pass `--config-file=<path>` to Ghostty.
    pub fn launch(&self, config_path: Option<&std::path::Path>) -> Result<()> {
        let ghostty_path = Self::find_binary();
        let path = ghostty_path
            .ok_or_else(|| Error::NotFound("Ghostty not found".into()))?;

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

        let mut child = child
            .map_err(|e| Error::Internal(format!("failed to launch Ghostty: {e}")))?;
        let pid = child.id();
        {
            let mut pids = self.pids.lock().map_err(|e| Error::Internal(e.to_string()))?;
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
        if !cfg!(target_os = "macos") { return Ok(()); }
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
                unsafe { libc::kill(pid as i32, libc::SIGTERM); }
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
                    v.push(format!("{h}/Applications/Ghostty.app/Contents/MacOS/ghostty"));
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
