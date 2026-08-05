//! Process supervision seam for terminal / background tasks.
//!
//! Production adapter: [`LocalProcessSupervisor`].
//! Test adapter: [`FakeProcessSupervisor`].

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio::time::{Duration, Instant};

/// Default foreground budget before auto-background (scheme Phase 1).
pub const DEFAULT_FOREGROUND_BUDGET_MS: u64 = 15_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessSpec {
    pub run_id: String,
    pub task_id: String,
    /// Display command as shown to the user (not sandbox wrapper).
    pub display_command: String,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub timeout_ms: u64,
    pub background: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProcessState {
    Running,
    Background,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessSnapshot {
    pub task_id: String,
    pub run_id: String,
    pub state: ProcessState,
    pub display_command: String,
    pub exit_code: Option<i32>,
    pub stdout_tail: String,
    pub stderr_tail: String,
    pub truncated: bool,
    pub background: bool,
}

#[derive(Debug, Clone)]
pub struct OutputChunk {
    pub stream: &'static str, // "stdout" | "stderr"
    pub text: String,
}

#[async_trait]
pub trait ProcessSupervisor: Send + Sync {
    async fn spawn(&self, spec: ProcessSpec) -> Result<ProcessSnapshot, String>;
    async fn poll(&self, task_id: &str) -> Result<ProcessSnapshot, String>;
    async fn wait(&self, task_id: &str, timeout_ms: u64) -> Result<ProcessSnapshot, String>;
    async fn cancel(&self, task_id: &str) -> Result<ProcessSnapshot, String>;
    async fn list_for_run(&self, run_id: &str) -> Vec<ProcessSnapshot>;
    async fn drain_output(&self, task_id: &str) -> Vec<OutputChunk>;
}

struct LiveProcess {
    run_id: String,
    display_command: String,
    child: Option<Child>,
    state: ProcessState,
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
    pending: Vec<OutputChunk>,
    truncated: bool,
    background: bool,
    /// Accumulated bytes counted toward the 1MB delta cap.
    persisted_bytes: usize,
    /// Concurrent pipe reader tasks. Spawned right after the child starts so a
    /// \>64KB producer never blocks on a full OS pipe; drained to EOF and joined
    /// on cancel/finish so the registry stays quiet (J02).
    readers: Vec<JoinHandle<()>>,
}

type ProcessMap = Arc<Mutex<HashMap<String, LiveProcess>>>;

/// Production supervisor: OS children, best-effort kill, 15s foreground budget.
pub struct LocalProcessSupervisor {
    inner: ProcessMap,
    foreground_budget_ms: u64,
}

/// Process-wide supervisor so background tasks survive tool-handler return
/// and can be force-killed by the run cancellation tree (task-03).
pub fn global_process_supervisor() -> &'static LocalProcessSupervisor {
    use std::sync::OnceLock;
    static GLOBAL: OnceLock<LocalProcessSupervisor> = OnceLock::new();
    GLOBAL.get_or_init(LocalProcessSupervisor::new)
}

impl LocalProcessSupervisor {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            foreground_budget_ms: DEFAULT_FOREGROUND_BUDGET_MS,
        }
    }

    pub fn with_budget(ms: u64) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            foreground_budget_ms: ms,
        }
    }

    fn snapshot(task_id: &str, p: &LiveProcess) -> ProcessSnapshot {
        ProcessSnapshot {
            task_id: task_id.to_string(),
            run_id: p.run_id.clone(),
            state: p.state.clone(),
            display_command: p.display_command.clone(),
            exit_code: p.exit_code,
            stdout_tail: tail(&p.stdout, 8_192),
            stderr_tail: tail(&p.stderr, 4_096),
            truncated: p.truncated,
            background: p.background,
        }
    }

    async fn finish_if_exited(proc: &mut LiveProcess) {
        let Some(mut child) = proc.child.take() else {
            return;
        };
        match child.try_wait() {
            Ok(Some(status)) => {
                // Output is drained by the concurrent readers (the pipes were
                // handed to them at spawn), so there is nothing to read here.
                // Synchronous only: this is called while the map is locked and
                // the readers must not be starved waiting on it.
                proc.exit_code = status.code();
                proc.state = if status.success() {
                    ProcessState::Completed
                } else {
                    ProcessState::Failed
                };
            }
            Ok(None) => {
                // Still running — put child back.
                proc.child = Some(child);
            }
            Err(e) => {
                proc.state = ProcessState::Failed;
                append_capped(proc, "stderr", &format!("wait error: {e}"));
            }
        }
    }

    /// Wait for reader tasks to drain and exit, aborting any that outlive the
    /// budget (a descendant holding the pipe open). Keeps the registry quiet
    /// after cancel/timeout.
    async fn join_readers(readers: Vec<JoinHandle<()>>) {
        for mut reader in readers {
            if tokio::time::timeout(Duration::from_millis(500), &mut reader)
                .await
                .is_err()
            {
                reader.abort();
            }
        }
    }
}

/// Spawn a concurrent pipe reader that drains chunks into the process state
/// until EOF (or the process leaves the map). Reading starts immediately so a
/// large producer never blocks on a full OS pipe (J02).
fn spawn_pipe_reader<R: AsyncRead + Unpin + Send + 'static>(
    map: ProcessMap,
    task_id: String,
    stream: &'static str,
    pipe: Option<R>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let Some(mut pipe) = pipe else {
            return;
        };
        // Bounded chunk: the append path caps total persisted bytes at 1MB, so
        // each read keeps memory bounded while still draining the pipe fast.
        let mut buf = vec![0u8; 16 * 1024];
        loop {
            let n = match pipe.read(&mut buf).await {
                Ok(0) => break, // EOF
                Ok(n) => n,
                Err(_) => break,
            };
            let text = String::from_utf8_lossy(&buf[..n]);
            let mut map = map.lock().await;
            let Some(proc) = map.get_mut(&task_id) else {
                break; // process removed from registry; stop draining
            };
            append_capped(proc, stream, &text);
        }
    })
}

impl Default for LocalProcessSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ProcessSupervisor for LocalProcessSupervisor {
    async fn spawn(&self, spec: ProcessSpec) -> Result<ProcessSnapshot, String> {
        {
            let map = self.inner.lock().await;
            if map.contains_key(&spec.task_id) {
                return Err(format!("task_id already exists: {}", spec.task_id));
            }
        }

        let mut cmd = Command::new(&spec.program);
        cmd.args(&spec.args)
            .current_dir(&spec.cwd)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
            cmd.creation_flags(CREATE_NEW_PROCESS_GROUP);
        }

        let mut child = cmd.spawn().map_err(|e| format!("spawn failed: {e}"))?;
        // Hand the pipes to concurrent readers so output is drained while the
        // child runs — a >64KB producer must never block on a full OS pipe (J02).
        let stdout_pipe = child.stdout.take();
        let stderr_pipe = child.stderr.take();

        let live = LiveProcess {
            run_id: spec.run_id.clone(),
            display_command: spec.display_command.clone(),
            child: Some(child),
            state: if spec.background {
                ProcessState::Background
            } else {
                ProcessState::Running
            },
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            pending: Vec::new(),
            truncated: false,
            background: spec.background,
            persisted_bytes: 0,
            readers: Vec::new(),
        };

        {
            let mut map = self.inner.lock().await;
            map.insert(spec.task_id.clone(), live);
        }

        // Start readers immediately; they append under the map lock until EOF.
        let stdout_task = spawn_pipe_reader(
            Arc::clone(&self.inner),
            spec.task_id.clone(),
            "stdout",
            stdout_pipe,
        );
        let stderr_task = spawn_pipe_reader(
            Arc::clone(&self.inner),
            spec.task_id.clone(),
            "stderr",
            stderr_pipe,
        );
        {
            let mut map = self.inner.lock().await;
            if let Some(proc) = map.get_mut(&spec.task_id) {
                proc.readers = vec![stdout_task, stderr_task];
            }
        }

        if !spec.background {
            let budget =
                Duration::from_millis(self.foreground_budget_ms.min(spec.timeout_ms.max(1)));
            let deadline = Instant::now() + budget;
            loop {
                {
                    let mut map = self.inner.lock().await;
                    let Some(proc) = map.get_mut(&spec.task_id) else {
                        break;
                    };
                    Self::finish_if_exited(proc).await;
                    if !matches!(proc.state, ProcessState::Running | ProcessState::Background) {
                        break;
                    }
                }
                if Instant::now() >= deadline {
                    let mut map = self.inner.lock().await;
                    if let Some(proc) = map.get_mut(&spec.task_id) {
                        if matches!(proc.state, ProcessState::Running) {
                            proc.state = ProcessState::Background;
                            proc.background = true;
                        }
                    }
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }

        let map = self.inner.lock().await;
        map.get(&spec.task_id)
            .map(|p| Self::snapshot(&spec.task_id, p))
            .ok_or_else(|| "process vanished".into())
    }

    async fn poll(&self, task_id: &str) -> Result<ProcessSnapshot, String> {
        let mut map = self.inner.lock().await;
        let proc = map
            .get_mut(task_id)
            .ok_or_else(|| format!("unknown task {task_id}"))?;
        Self::finish_if_exited(proc).await;
        Ok(Self::snapshot(task_id, proc))
    }

    async fn wait(&self, task_id: &str, timeout_ms: u64) -> Result<ProcessSnapshot, String> {
        let deadline = Instant::now() + Duration::from_millis(timeout_ms.max(1));
        loop {
            let snap = self.poll(task_id).await?;
            if !matches!(snap.state, ProcessState::Running | ProcessState::Background) {
                return Ok(snap);
            }
            if Instant::now() >= deadline {
                return Ok(snap);
            }
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
    }

    async fn cancel(&self, task_id: &str) -> Result<ProcessSnapshot, String> {
        let mut map = self.inner.lock().await;
        let proc = map
            .get_mut(task_id)
            .ok_or_else(|| format!("unknown task {task_id}"))?;
        if let Some(mut child) = proc.child.take() {
            let _ = child.start_kill();
            // Reap the child before touching readers so their pipes hit EOF.
            let _ = child.wait().await;
        }
        proc.state = ProcessState::Cancelled;
        // Drop the lock before joining: readers need it to append final bytes.
        let readers = std::mem::take(&mut proc.readers);
        drop(map);
        Self::join_readers(readers).await;
        let map = self.inner.lock().await;
        let proc = map
            .get(task_id)
            .ok_or_else(|| format!("unknown task {task_id}"))?;
        Ok(Self::snapshot(task_id, proc))
    }

    async fn list_for_run(&self, run_id: &str) -> Vec<ProcessSnapshot> {
        let map = self.inner.lock().await;
        map.iter()
            .filter(|(_, p)| p.run_id == run_id)
            .map(|(id, p)| Self::snapshot(id, p))
            .collect()
    }

    async fn drain_output(&self, task_id: &str) -> Vec<OutputChunk> {
        let mut map = self.inner.lock().await;
        let Some(proc) = map.get_mut(task_id) else {
            return Vec::new();
        };
        std::mem::take(&mut proc.pending)
    }
}

fn append_capped(proc: &mut LiveProcess, stream: &'static str, text: &str) {
    const MAX_PERSIST: usize = 1_048_576;
    let remaining = MAX_PERSIST.saturating_sub(proc.persisted_bytes);
    if remaining == 0 {
        if !text.is_empty() {
            proc.truncated = true;
        }
        return;
    }
    // Never split a UTF-8 character (J02): floor the byte budget to a char
    // boundary. `floor_char_boundary` panics if `i > len`, hence the min().
    let take = text.floor_char_boundary(remaining.min(text.len()));
    if take == 0 {
        if !text.is_empty() {
            proc.truncated = true;
        }
        return;
    }
    let slice = &text[..take];
    if stream == "stdout" {
        proc.stdout.push_str(slice);
    } else {
        proc.stderr.push_str(slice);
    }
    proc.pending.push(OutputChunk {
        stream,
        text: slice.to_string(),
    });
    proc.persisted_bytes += take;
    if take < text.len() {
        proc.truncated = true;
    }
}

fn tail(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        // Never split a UTF-8 character (J02).
        let start = s.floor_char_boundary(s.len() - max);
        s[start..].to_string()
    }
}

/// In-memory fake for tests — no real processes.
pub struct FakeProcessSupervisor {
    inner: Mutex<HashMap<String, ProcessSnapshot>>,
    outputs: Mutex<HashMap<String, Vec<OutputChunk>>>,
}

impl FakeProcessSupervisor {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            outputs: Mutex::new(HashMap::new()),
        }
    }

    pub async fn inject_completed(
        &self,
        task_id: &str,
        run_id: &str,
        exit_code: i32,
        stdout: &str,
    ) {
        self.inner.lock().await.insert(
            task_id.to_string(),
            ProcessSnapshot {
                task_id: task_id.to_string(),
                run_id: run_id.to_string(),
                state: if exit_code == 0 {
                    ProcessState::Completed
                } else {
                    ProcessState::Failed
                },
                display_command: "fake".into(),
                exit_code: Some(exit_code),
                stdout_tail: stdout.to_string(),
                stderr_tail: String::new(),
                truncated: false,
                background: false,
            },
        );
        self.outputs.lock().await.insert(
            task_id.to_string(),
            vec![OutputChunk {
                stream: "stdout",
                text: stdout.to_string(),
            }],
        );
    }
}

impl Default for FakeProcessSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ProcessSupervisor for FakeProcessSupervisor {
    async fn spawn(&self, spec: ProcessSpec) -> Result<ProcessSnapshot, String> {
        let snap = ProcessSnapshot {
            task_id: spec.task_id.clone(),
            run_id: spec.run_id,
            state: if spec.background {
                ProcessState::Background
            } else {
                ProcessState::Running
            },
            display_command: spec.display_command,
            exit_code: None,
            stdout_tail: String::new(),
            stderr_tail: String::new(),
            truncated: false,
            background: spec.background,
        };
        self.inner
            .lock()
            .await
            .insert(spec.task_id.clone(), snap.clone());
        Ok(snap)
    }

    async fn poll(&self, task_id: &str) -> Result<ProcessSnapshot, String> {
        self.inner
            .lock()
            .await
            .get(task_id)
            .cloned()
            .ok_or_else(|| format!("unknown task {task_id}"))
    }

    async fn wait(&self, task_id: &str, _timeout_ms: u64) -> Result<ProcessSnapshot, String> {
        self.poll(task_id).await
    }

    async fn cancel(&self, task_id: &str) -> Result<ProcessSnapshot, String> {
        let mut map = self.inner.lock().await;
        let snap = map
            .get_mut(task_id)
            .ok_or_else(|| format!("unknown task {task_id}"))?;
        snap.state = ProcessState::Cancelled;
        Ok(snap.clone())
    }

    async fn list_for_run(&self, run_id: &str) -> Vec<ProcessSnapshot> {
        self.inner
            .lock()
            .await
            .values()
            .filter(|s| s.run_id == run_id)
            .cloned()
            .collect()
    }

    async fn drain_output(&self, task_id: &str) -> Vec<OutputChunk> {
        self.outputs
            .lock()
            .await
            .remove(task_id)
            .unwrap_or_default()
    }
}

/// Resolve shell program for the platform.
pub fn platform_shell_program() -> (String, Vec<String>) {
    #[cfg(windows)]
    {
        (
            "powershell".into(),
            vec!["-NoProfile".into(), "-Command".into()],
        )
    }
    #[cfg(not(windows))]
    {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
        (shell, vec!["-lc".into()])
    }
}

/// Ensure cwd is under project root (canonical when possible).
pub fn resolve_cwd(project_root: &Path, cwd_rel: Option<&str>) -> Result<PathBuf, String> {
    let root = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());
    let candidate = match cwd_rel {
        None | Some("") => root.clone(),
        Some(rel) => {
            if rel.contains("..") {
                return Err("cwd path traversal rejected".into());
            }
            root.join(rel)
        }
    };
    let abs = candidate.canonicalize().unwrap_or(candidate);
    if !abs.starts_with(&root) {
        return Err("cwd must be within project root".into());
    }
    Ok(abs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fake_supervisor_run_isolation() {
        let sup = FakeProcessSupervisor::new();
        let a = sup
            .spawn(ProcessSpec {
                run_id: "run-a".into(),
                task_id: "t1".into(),
                display_command: "echo a".into(),
                program: "echo".into(),
                args: vec![],
                cwd: PathBuf::from("."),
                timeout_ms: 1000,
                background: false,
            })
            .await
            .unwrap();
        assert_eq!(a.run_id, "run-a");
        let list_b = sup.list_for_run("run-b").await;
        assert!(list_b.is_empty());
        let list_a = sup.list_for_run("run-a").await;
        assert_eq!(list_a.len(), 1);
    }

    #[tokio::test]
    async fn fake_cancel() {
        let sup = FakeProcessSupervisor::new();
        sup.spawn(ProcessSpec {
            run_id: "r".into(),
            task_id: "t".into(),
            display_command: "x".into(),
            program: "x".into(),
            args: vec![],
            cwd: PathBuf::from("."),
            timeout_ms: 1000,
            background: true,
        })
        .await
        .unwrap();
        let snap = sup.cancel("t").await.unwrap();
        assert_eq!(snap.state, ProcessState::Cancelled);
    }

    #[test]
    fn cwd_must_stay_in_project() {
        let tmp = std::env::temp_dir().join(format!("natives-cwd-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(tmp.join("sub")).unwrap();
        let ok = resolve_cwd(&tmp, Some("sub")).unwrap();
        let root = tmp.canonicalize().unwrap();
        assert!(ok.starts_with(&root));
        assert!(resolve_cwd(&tmp, Some("../x")).is_err());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // -----------------------------------------------------------------------
    // TASK-002 (J02): bounded concurrent drain, Unicode-safe truncation, and
    // cancel/timeout reaping. These fail against the pre-fix supervisor
    // (post-exit-only drain deadlocks on >64KB output; byte-slicing panics on
    // multi-byte characters).
    // -----------------------------------------------------------------------

    fn empty_proc() -> LiveProcess {
        LiveProcess {
            run_id: "run-shell".into(),
            display_command: "test".into(),
            child: None,
            state: ProcessState::Running,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            pending: Vec::new(),
            truncated: false,
            background: false,
            persisted_bytes: 0,
            readers: Vec::new(),
        }
    }

    fn sh_spec(task_id: &str, command: &str, background: bool) -> ProcessSpec {
        ProcessSpec {
            run_id: "run-shell".into(),
            task_id: task_id.into(),
            display_command: command.into(),
            program: "/bin/sh".into(),
            args: vec!["-lc".into(), command.into()],
            cwd: std::env::temp_dir(),
            timeout_ms: 60_000,
            background,
        }
    }

    #[tokio::test]
    async fn shell_10mb_stdout_completes_without_deadlock() {
        let sup = LocalProcessSupervisor::with_budget(2_000);
        let snap = sup
            .spawn(sh_spec("t-10mb", "head -c 10485760 /dev/zero; true", false))
            .await
            .unwrap();
        assert!(
            matches!(snap.state, ProcessState::Completed | ProcessState::Failed),
            "10MB stdout must not deadlock the child, got {:?}",
            snap.state
        );
        assert_eq!(snap.exit_code, Some(0));
    }

    #[tokio::test]
    async fn shell_10mb_both_streams_interleaved_completes() {
        let sup = LocalProcessSupervisor::with_budget(3_000);
        // 10MB on stdout and 10MB on stderr, produced concurrently.
        let cmd = "(head -c 10485760 /dev/zero) & (head -c 10485760 /dev/zero >&2) & wait";
        let snap = sup.spawn(sh_spec("t-2x10mb", cmd, false)).await.unwrap();
        assert!(
            matches!(snap.state, ProcessState::Completed | ProcessState::Failed),
            "interleaved 10MB streams must not deadlock, got {:?}",
            snap.state
        );
        assert_eq!(snap.exit_code, Some(0));
    }

    #[test]
    fn shell_unicode_tail_never_panics() {
        let s = "你好世界🌍🎉日本語テキスト";
        for max in 0..=s.len() {
            let _ = tail(s, max);
        }
        // A cap smaller than a multi-byte char must not slice mid-character.
        let _ = tail("你", 1);
    }

    #[test]
    fn shell_unicode_append_capped_never_panics() {
        let text = "你好世界🌍🎉日本語テキスト";
        // Remaining budgets of 1..4 bytes must never split a 3/4-byte char.
        for rem in 1..=4usize {
            let mut proc = empty_proc();
            proc.persisted_bytes = 1_048_576 - rem;
            append_capped(&mut proc, "stdout", text);
            assert!(proc.stdout.len() <= 1_048_576, "persist cap exceeded");
        }
    }

    #[tokio::test]
    async fn shell_cancel_kills_waits_and_reaps() {
        let sup = LocalProcessSupervisor::new();
        sup.spawn(sh_spec("t-cancel", "sleep 30", false))
            .await
            .unwrap();
        let snap = sup.cancel("t-cancel").await.unwrap();
        assert_eq!(snap.state, ProcessState::Cancelled);
        let after = sup.poll("t-cancel").await.unwrap();
        assert_eq!(after.state, ProcessState::Cancelled, "child must be reaped");
    }

    #[tokio::test]
    async fn shell_cancel_unknown_task_errors_gracefully() {
        let sup = LocalProcessSupervisor::new();
        let err = sup.cancel("no-such-task").await.unwrap_err();
        assert!(err.contains("unknown task"), "got: {err}");
    }

    #[tokio::test]
    async fn shell_background_output_captured_after_completion() {
        let sup = LocalProcessSupervisor::new();
        sup.spawn(sh_spec("t-bg", "printf 'bg-out-你好'", true))
            .await
            .unwrap();
        let snap = sup.wait("t-bg", 5_000).await.unwrap();
        assert!(matches!(
            snap.state,
            ProcessState::Completed | ProcessState::Failed
        ));
        assert!(
            snap.stdout_tail.contains("bg-out-你好"),
            "background output must be captured, got {:?}",
            snap.stdout_tail
        );
    }
}
