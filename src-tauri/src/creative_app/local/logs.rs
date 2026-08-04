//! Rotating file logs + in-memory ring buffer for local creative apps.
//!
//! Layout (CR-301: logs are runtime-instance scoped):
//!   ~/.natives/logs/local-creative/{appId}/current.log            legacy app log (read-only aggregate)
//!   ~/.natives/logs/local-creative/{appId}/runs/{runtimeId}/current.log     per-runtime log
//!   ~/.natives/logs/local-creative/{appId}/runs/{runtimeId}/current.log.1   rotated
//!   ~/.natives/logs/local-creative/{appId}/runs/{runtimeId}/current.log.2   rotated
//!
//! Single file ≤ 5 MiB, keep 3 files. Memory ring keeps last ~1 MiB.
//! The legacy app-level `current.log` is only READ (aggregate view); new writes
//! always target the per-runtime directory so two runs of the same app can never
//! interleave (audit #22).

use crate::creative_app::paths::natives_home;
use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;
pub const MAX_ROTATED: usize = 2; // current.log.1, current.log.2
pub const MEMORY_CAP_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogStream {
    Stdout,
    Stderr,
    System,
}

impl LogStream {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
            Self::System => "system",
        }
    }
}

#[derive(Debug, Clone)]
pub struct LogLine {
    pub seq: u64,
    pub ts_ms: i64,
    pub stream: LogStream,
    pub text: String,
}

struct RingState {
    seq: u64,
    bytes: usize,
    lines: VecDeque<LogLine>,
}

pub struct LocalLogStore {
    app_id: String,
    runtime_id: String,
    dir: PathBuf,
    ring: Mutex<RingState>,
    /// Approximate size of current.log (best-effort).
    current_size: Mutex<u64>,
    /// Concrete env values for this app; redacted from every appended line.
    secrets: Mutex<Vec<String>>,
}

impl LocalLogStore {
    pub fn open(app_id: &str, runtime_id: &str) -> std::io::Result<Self> {
        let dir = log_dir(app_id, runtime_id);
        fs::create_dir_all(&dir)?;
        let size = fs::metadata(dir.join("current.log"))
            .map(|m| m.len())
            .unwrap_or(0);
        Ok(Self {
            app_id: app_id.to_string(),
            runtime_id: runtime_id.to_string(),
            dir,
            ring: Mutex::new(RingState {
                seq: 0,
                bytes: 0,
                lines: VecDeque::new(),
            }),
            current_size: Mutex::new(size),
            secrets: Mutex::new(Vec::new()),
        })
    }

    pub fn app_id(&self) -> &str {
        &self.app_id
    }

    pub fn runtime_id(&self) -> &str {
        &self.runtime_id
    }

    /// Inject this app's concrete env values so live + persisted log lines
    /// redact them by value (not just by pattern). Call before start/install.
    pub fn set_secrets(&self, values: Vec<String>) {
        let filtered: Vec<String> = values.into_iter().filter(|v| v.trim().len() >= 4).collect();
        let mut guard = self.secrets.lock().unwrap_or_else(|e| e.into_inner());
        *guard = filtered;
    }

    pub fn append(&self, stream: LogStream, text: &str) -> LogLine {
        let ts_ms = chrono::Utc::now().timestamp_millis();
        let sanitized = {
            let secrets = self.secrets.lock().unwrap_or_else(|e| e.into_inner());
            sanitize_log_text_with_secrets(text, &secrets)
        };
        let line = {
            let mut ring = self.ring.lock().unwrap_or_else(|e| e.into_inner());
            ring.seq = ring.seq.saturating_add(1);
            let line = LogLine {
                seq: ring.seq,
                ts_ms,
                stream,
                text: sanitized.clone(),
            };
            ring.bytes = ring.bytes.saturating_add(line.text.len() + 32);
            ring.lines.push_back(line.clone());
            while ring.bytes > MEMORY_CAP_BYTES {
                if let Some(old) = ring.lines.pop_front() {
                    ring.bytes = ring.bytes.saturating_sub(old.text.len() + 32);
                } else {
                    break;
                }
            }
            line
        };
        let _ = self.write_file(stream, ts_ms, &sanitized);
        line
    }

    pub fn recent_memory(&self, limit: usize) -> Vec<LogLine> {
        self.recent_memory_after(0, limit)
    }

    /// Lines newer than `cursor` (seq), newest-limited to `limit`. Used by the
    /// `logs(runtime_id, cursor)` contract so a reader can poll incrementally
    /// without re-fetching everything (CR-301).
    pub fn recent_memory_after(&self, cursor: u64, limit: usize) -> Vec<LogLine> {
        let ring = self.ring.lock().unwrap_or_else(|e| e.into_inner());
        let n = limit.min(ring.lines.len());
        ring.lines
            .iter()
            .filter(|l| l.seq > cursor)
            .rev()
            .take(n)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }

    /// Tail persisted log (raw text, already sanitized on write).
    pub fn read_persisted_tail(&self, max_bytes: usize) -> String {
        let path = self.dir.join("current.log");
        read_tail(&path, max_bytes).unwrap_or_default()
    }

    /// Delete log directory for this app (called on creative record delete).
    pub fn purge_files(&self) {
        let _ = fs::remove_dir_all(&self.dir);
    }

    fn write_file(&self, stream: LogStream, ts_ms: i64, text: &str) -> std::io::Result<()> {
        let path = self.dir.join("current.log");
        {
            let size = self.current_size.lock().unwrap_or_else(|e| e.into_inner());
            if *size >= MAX_FILE_BYTES {
                drop(size);
                self.rotate()?;
                let mut size = self.current_size.lock().unwrap_or_else(|e| e.into_inner());
                *size = 0;
            }
        }
        let mut f = OpenOptions::new().create(true).append(true).open(&path)?;
        let record = format!("{ts_ms}\t{}\t{text}\n", stream.as_str());
        f.write_all(record.as_bytes())?;
        let mut size = self.current_size.lock().unwrap_or_else(|e| e.into_inner());
        *size = size.saturating_add(record.len() as u64);
        Ok(())
    }

    fn rotate(&self) -> std::io::Result<()> {
        // shift .2 <- .1 <- current
        let cur = self.dir.join("current.log");
        let one = self.dir.join("current.log.1");
        let two = self.dir.join("current.log.2");
        if two.exists() {
            let _ = fs::remove_file(&two);
        }
        if one.exists() {
            let _ = fs::rename(&one, &two);
        }
        if cur.exists() {
            let _ = fs::rename(&cur, &one);
        }
        // drop oldest beyond MAX_ROTATED is already handled (only .1 .2)
        let _ = MAX_ROTATED;
        Ok(())
    }
}

/// Per-runtime log directory: `.../local-creative/{appId}/runs/{runtimeId}`.
pub fn log_dir(app_id: &str, runtime_id: &str) -> PathBuf {
    app_log_dir(app_id).join("runs").join(safe_segment(runtime_id))
}

/// Legacy / aggregate app-level log directory: `.../local-creative/{appId}`.
/// The old `current.log` here is read-only after CR-301 (dual-read aggregation);
/// per-runtime logs live under its `runs/` subdirectory.
pub fn app_log_dir(app_id: &str) -> PathBuf {
    natives_home()
        .join("logs")
        .join("local-creative")
        .join(safe_segment(app_id))
}

/// Sanitize an id for use as a path segment.
fn safe_segment(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Delete the whole app log directory (legacy aggregate + all per-runtime runs).
pub fn purge_app_logs(app_id: &str) {
    let _ = fs::remove_dir_all(app_log_dir(app_id));
}

/// Best-effort redaction of secrets in log lines.
pub fn sanitize_log_text(text: &str) -> String {
    sanitize_log_text_with_secrets(text, &[])
}

/// Redact known patterns plus concrete env values for this app.
pub fn sanitize_log_text_with_secrets(text: &str, secret_values: &[String]) -> String {
    let mut out = crate::log_sanitizer::sanitize(&text.replace('\0', ""));
    // UTF-8 safe truncation
    const MAX_CHARS: usize = 16 * 1024;
    if out.chars().count() > MAX_CHARS {
        out = out.chars().take(MAX_CHARS).collect::<String>() + "…[truncated]";
    }
    for secret in secret_values {
        let s = secret.trim();
        if s.len() < 4 {
            continue;
        }
        if out.contains(s) {
            out = out.replace(s, "***");
        }
    }
    // Also redact common assignment shapes after global sanitizer.
    for key in [
        "token",
        "password",
        "secret",
        "api_key",
        "apikey",
        "authorization",
    ] {
        if out.to_ascii_lowercase().contains(key) {
            out = redact_key_values(&out, key);
        }
    }
    out
}

/// One-shot redacted append against explicit secrets, independent of the
/// store's injected secret set. Prefer `store.set_secrets()` + `append()` for
/// the live process path; this remains for callers that hold values ad hoc.
pub fn append_with_secrets(
    store: &LocalLogStore,
    stream: LogStream,
    text: &str,
    secret_values: &[String],
) -> LogLine {
    let sanitized = sanitize_log_text_with_secrets(text, secret_values);
    store.append(stream, &sanitized)
}

fn redact_key_values(input: &str, key: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let key_l = key.to_ascii_lowercase();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    let bytes = input.as_bytes();
    let lower_b = lower.as_bytes();
    while i < bytes.len() {
        if i + key_l.len() <= lower_b.len() && &lower_b[i..i + key_l.len()] == key_l.as_bytes() {
            out.push_str(&input[i..i + key_l.len()]);
            i += key_l.len();
            // skip whitespace
            while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
                out.push(bytes[i] as char);
                i += 1;
            }
            if i < bytes.len() && (bytes[i] == b'=' || bytes[i] == b':') {
                out.push(bytes[i] as char);
                i += 1;
                while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
                    out.push(bytes[i] as char);
                    i += 1;
                }
                // redact until whitespace or end
                out.push_str("***");
                while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                continue;
            }
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn read_tail(path: &Path, max_bytes: usize) -> std::io::Result<String> {
    let mut f = File::open(path)?;
    let len = f.metadata()?.len() as usize;
    if len == 0 {
        return Ok(String::new());
    }
    let start = len.saturating_sub(max_bytes);
    if start > 0 {
        use std::io::Seek;
        f.seek(std::io::SeekFrom::Start(start as u64))?;
    }
    let mut buf = String::new();
    f.read_to_string(&mut buf)?;
    if start > 0 {
        // drop partial first line
        if let Some(pos) = buf.find('\n') {
            buf = buf[pos + 1..].to_string();
        }
    }
    Ok(buf)
}

/// Process-global registry of open log stores (lazy), keyed by runtime id so
/// two runs of the same app never share a store (CR-301 #22).
#[derive(Clone, Default)]
pub struct LogRegistry {
    inner: Arc<Mutex<std::collections::HashMap<String, Arc<LocalLogStore>>>>,
}

impl LogRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Open (or reuse) the per-runtime log store for `(app_id, runtime_id)`.
    /// The store owns its app_id so events and purge know the owning app.
    pub fn get_or_open(&self, app_id: &str, runtime_id: &str) -> Arc<LocalLogStore> {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(s) = map.get(runtime_id) {
            return s.clone();
        }
        let store = Arc::new(LocalLogStore::open(app_id, runtime_id).unwrap_or_else(|_| {
            // fallback: still construct with the intended dir semantics
            LocalLogStore {
                app_id: app_id.to_string(),
                runtime_id: runtime_id.to_string(),
                dir: log_dir(app_id, runtime_id),
                ring: Mutex::new(RingState {
                    seq: 0,
                    bytes: 0,
                    lines: VecDeque::new(),
                }),
                current_size: Mutex::new(0),
                secrets: Mutex::new(Vec::new()),
            }
        }));
        map.insert(runtime_id.to_string(), store.clone());
        store
    }

    /// Drop a runtime's live store (no purge — the user may still read its logs).
    pub fn remove(&self, runtime_id: &str) {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        map.remove(runtime_id);
    }

    /// Drop every store of an app and purge its whole log directory.
    pub fn remove_app(&self, app_id: &str) {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        map.retain(|_rt, store| store.app_id != app_id);
        drop(map);
        purge_app_logs(app_id);
    }

    /// App-level aggregate tail: the legacy app log plus every per-runtime run,
    /// newest run last. Read-only — never writes to the legacy file (dual-read,
    /// no long-term dual-write, CR-301).
    pub fn app_aggregate_tail(&self, app_id: &str, max_bytes: usize) -> String {
        let mut parts: Vec<String> = Vec::new();
        let mut run_dirs: Vec<PathBuf> = Vec::new();
        let app_dir = app_log_dir(app_id);
        if let Ok(rd) = fs::read_dir(app_dir.join("runs")) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    run_dirs.push(p);
                }
            }
        }
        run_dirs.sort_by_key(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string()
        });
        // Per-runtime tails in run order.
        for dir in run_dirs {
            let tail = read_tail(&dir.join("current.log"), max_bytes).unwrap_or_default();
            if !tail.is_empty() {
                parts.push(tail);
            }
        }
        // Legacy app-level log (pre-CR-301) last so it reads oldest-first.
        let legacy = read_tail(&app_dir.join("current.log"), max_bytes).unwrap_or_default();
        if !legacy.is_empty() {
            parts.push(legacy);
        }
        if parts.is_empty() {
            return String::new();
        }
        // Join in chronological order (runs are chronological; legacy is oldest).
        let mut out = parts.join("\n");
        let cap = max_bytes;
        if out.len() > cap {
            out = out[out.len() - cap..].to_string();
        }
        out
    }

    pub fn clone_registry(&self) -> LogRegistry {
        self.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_token_assignment() {
        let s = sanitize_log_text("Authorization: Bearer abcdefghijklmnop secret=xyzsecret");
        assert!(s.contains("***"));
        assert!(!s.contains("abcdefghijklmnop"));
        assert!(!s.contains("xyzsecret"));
    }

    #[test]
    fn truncates_utf8_safely() {
        let big = "中".repeat(20_000);
        let s = sanitize_log_text(&big);
        assert!(s.ends_with("…[truncated]") || s.chars().count() <= 16 * 1024 + 20);
        // must remain valid UTF-8 (String guarantees)
        assert!(std::str::from_utf8(s.as_bytes()).is_ok());
    }

    #[test]
    fn redacts_explicit_secret_values() {
        let s = sanitize_log_text_with_secrets(
            "connected with mypass-12345 ok",
            &["mypass-12345".into()],
        );
        assert!(!s.contains("mypass-12345"));
        assert!(s.contains("***"));
    }

    #[test]
    fn injected_secrets_redact_appended_lines() {
        let dir = std::env::temp_dir().join(format!(
            "natives-log-secret-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::create_dir_all(&dir);
        let store = LocalLogStore {
            app_id: "s".into(),
            runtime_id: "s-run".into(),
            dir: dir.clone(),
            ring: Mutex::new(RingState {
                seq: 0,
                bytes: 0,
                lines: VecDeque::new(),
            }),
            current_size: Mutex::new(0),
            secrets: Mutex::new(Vec::new()),
        };
        store.set_secrets(vec!["super-secret-token".into(), "ab".into()]);
        let line = store.append(LogStream::Stdout, "using super-secret-token now");
        assert!(!line.text.contains("super-secret-token"));
        assert!(line.text.contains("***"));
        // Too-short values are ignored (not redacted to avoid noise).
        let line2 = store.append(LogStream::Stdout, "value ab here");
        assert!(line2.text.contains("ab"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn ring_caps_memory() {
        let dir = std::env::temp_dir().join(format!(
            "natives-log-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::create_dir_all(&dir);
        let store = LocalLogStore {
            app_id: "t".into(),
            runtime_id: "t-run".into(),
            dir: dir.clone(),
            ring: Mutex::new(RingState {
                seq: 0,
                bytes: 0,
                lines: VecDeque::new(),
            }),
            current_size: Mutex::new(0),
            secrets: Mutex::new(Vec::new()),
        };
        for i in 0..200 {
            store.append(LogStream::Stdout, &format!("line {i} {}", "x".repeat(8000)));
        }
        let ring = store.ring.lock().unwrap();
        assert!(ring.bytes <= MEMORY_CAP_BYTES + 20_000);
        let _ = fs::remove_dir_all(&dir);
    }

    fn temp_store(app_id: &str, runtime_id: &str) -> (tempfile::TempDir, LocalLogStore) {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = LocalLogStore {
            app_id: app_id.into(),
            runtime_id: runtime_id.into(),
            dir: dir.path().to_path_buf(),
            ring: Mutex::new(RingState {
                seq: 0,
                bytes: 0,
                lines: VecDeque::new(),
            }),
            current_size: Mutex::new(0),
            secrets: Mutex::new(Vec::new()),
        };
        (dir, store)
    }

    /// CR-301 (#22): two runs of the same app must never share one ring/seq —
    /// each runtime gets its own store, so late events from run 1 can't land in
    /// run 2's log.
    #[test]
    fn per_runtime_stores_are_isolated() {
        let (_d1, run1) = temp_store("app-a", "run-1");
        let (_d2, run2) = temp_store("app-a", "run-2");
        run1.append(LogStream::System, "first run");
        run2.append(LogStream::System, "second run");

        assert_eq!(run1.recent_memory(10).len(), 1);
        assert_eq!(run2.recent_memory(10).len(), 1);
        assert_eq!(run1.recent_memory(10)[0].text, "first run");
        assert_eq!(run2.recent_memory(10)[0].text, "second run");
        // Independent rings: each run's line is invisible to the other.
        assert!(
            !run1.recent_memory(10)[0]
                .text
                .contains("second"),
            "run 1 must not see run 2's lines"
        );
        assert!(
            !run2.recent_memory(10)[0].text.contains("first"),
            "run 2 must not see run 1's lines"
        );
    }

    /// CR-301: `logs(runtime_id, cursor)` returns only lines newer than cursor.
    #[test]
    fn recent_memory_after_filters_by_cursor() {
        let (_d, store) = temp_store("app-a", "run-1");
        for i in 0..5 {
            store.append(LogStream::Stdout, &format!("line {i}"));
        }
        // 5 lines → seq 1..=5. cursor 2 → seq 3,4,5.
        let after2 = store.recent_memory_after(2, 100);
        assert_eq!(after2.len(), 3, "only lines with seq > 2 are newer");
        assert_eq!(after2[0].text, "line 2");
        assert_eq!(after2[1].text, "line 3");
        assert_eq!(after2[2].text, "line 4");
        let all = store.recent_memory_after(0, 100);
        assert_eq!(all.len(), 5);
        let none = store.recent_memory_after(99, 100);
        assert!(none.is_empty());
    }

    /// CR-301: runtime log paths are instance-scoped under the app dir.
    #[test]
    fn runtime_log_dir_shape() {
        let p = log_dir("app-a", "run-1");
        assert_eq!(p, app_log_dir("app-a").join("runs").join("run-1"));
        // Unsafe id segments are sanitized so paths never escape the app dir.
        let safe = log_dir("app/../..", "x/y");
        assert!(safe.to_string_lossy().contains("local-creative"));
        assert!(!safe.to_string_lossy().contains(".."));
    }
}
