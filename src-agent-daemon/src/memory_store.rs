//! Workspace / global memory search (Phase 6 minimum).
//!
//! File-backed JSONL under `NATIVES_RUNTIME_DIR/memory/` or `~/.natives/memory/`.
//! No embedding provider yet — keyword scan for CI; API ready for later upgrade.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use uuid::Uuid;

/// Match the daemon's bounded search surfaces.
pub const MAX_MEMORY_SEARCH_LIMIT: usize = 100;
/// Match the skill-body ceiling; one memory record cannot dominate the store.
const MAX_MEMORY_TEXT_BYTES: usize = 64_000;
/// Match the bounded live-event cache budget.
const MAX_MEMORY_BYTES: usize = 1024 * 1024;
const MAX_MEMORY_ENTRIES: usize = 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    pub scope: String, // "workspace" | "global"
    pub project_path: Option<String>,
    pub text: String,
    pub tags: Vec<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryHit {
    pub entry: MemoryEntry,
    pub score: f64,
}

pub struct MemoryStore {
    path: PathBuf,
    cache: Mutex<Vec<MemoryEntry>>,
}

impl MemoryStore {
    pub fn open_default() -> Self {
        let dir = std::env::var("NATIVES_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                std::env::var_os("HOME")
                    .or_else(|| std::env::var_os("USERPROFILE"))
                    .map(|h| PathBuf::from(h).join(".natives").join("runtime"))
                    .unwrap_or_else(|| std::env::temp_dir().join("natives-runtime"))
            })
            .join("memory");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("entries.jsonl");
        let mut store = Self {
            path,
            cache: Mutex::new(Vec::new()),
        };
        store.reload();
        store
    }

    fn reload(&mut self) {
        let Ok(metadata) = std::fs::metadata(&self.path) else {
            return;
        };
        if metadata.len() > MAX_MEMORY_BYTES as u64 {
            return;
        }
        let Ok(raw) = std::fs::read_to_string(&self.path) else {
            return;
        };
        let mut entries = Vec::new();
        for line in raw.lines() {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(e) = serde_json::from_str::<MemoryEntry>(line) {
                if !valid_entry(&e) {
                    continue;
                }
                entries.push(e);
            }
        }
        retain_within_bounds(&mut entries);
        if let Ok(mut c) = self.cache.lock() {
            *c = entries;
        }
    }

    fn persist(&self, entries: &[MemoryEntry]) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let bytes = serialized_bytes(entries)?;
        agent_core::fs_util::atomic_write_bytes(&self.path, &bytes).map_err(|e| e.to_string())
    }

    pub fn add(
        &self,
        scope: &str,
        project_path: Option<String>,
        text: &str,
        tags: Vec<String>,
    ) -> Result<MemoryEntry, String> {
        let (scope, project_path, text, tags) = normalized_input(scope, project_path, text, tags)?;
        // Never store secrets
        let redacted = assistant_protocol::v2::redact_secrets(&text);
        if redacted.len() > MAX_MEMORY_TEXT_BYTES {
            return Err(format!("memory text exceeds {MAX_MEMORY_TEXT_BYTES} bytes"));
        }
        let entry = MemoryEntry {
            id: Uuid::new_v4().to_string(),
            scope,
            project_path,
            text: redacted,
            tags,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        if serialized_line_len(&entry)? > MAX_MEMORY_TEXT_BYTES {
            return Err("memory entry exceeds record limit".into());
        }
        let key = dedup_key(&entry);
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| "memory store unavailable".to_string())?;
        if let Some(existing) = cache.iter().find(|existing| dedup_key(existing) == key) {
            return Ok(existing.clone());
        }
        let mut next = cache.clone();
        next.push(entry.clone());
        retain_within_bounds(&mut next);
        if !next.iter().any(|candidate| candidate.id == entry.id) {
            return Err("memory entry exceeds storage limit".into());
        }
        self.persist(&next)?;
        *cache = next;
        Ok(entry)
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<MemoryHit> {
        let q = query.to_ascii_lowercase();
        let terms: Vec<&str> = q.split_whitespace().filter(|t| !t.is_empty()).collect();
        let cache = self.cache.lock().ok();
        let Some(cache) = cache else {
            return Vec::new();
        };
        let mut hits: Vec<MemoryHit> = cache
            .iter()
            .filter_map(|e| {
                let hay = e.text.to_ascii_lowercase();
                let mut score = 0.0;
                if terms.is_empty() {
                    score = 0.1;
                } else {
                    for t in &terms {
                        if hay.contains(t) {
                            score += 1.0;
                        }
                    }
                }
                if score > 0.0 {
                    Some(MemoryHit {
                        entry: e.clone(),
                        score,
                    })
                } else {
                    None
                }
            })
            .collect();
        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        hits.truncate(limit.clamp(1, MAX_MEMORY_SEARCH_LIMIT));
        hits
    }

    pub fn list(&self, limit: usize) -> Vec<MemoryEntry> {
        self.cache
            .lock()
            .map(|c| {
                c.iter()
                    .rev()
                    .take(limit.clamp(1, MAX_MEMORY_SEARCH_LIMIT))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }
}

fn normalized_input(
    scope: &str,
    project_path: Option<String>,
    text: &str,
    tags: Vec<String>,
) -> Result<(String, Option<String>, String, Vec<String>), String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("memory text required".into());
    }
    if text.len() > MAX_MEMORY_TEXT_BYTES {
        return Err(format!("memory text exceeds {MAX_MEMORY_TEXT_BYTES} bytes"));
    }
    let project_path = project_path.map(|path| path.trim().to_string());
    let project_path = match scope {
        "workspace" => {
            let path = project_path
                .filter(|path| !path.is_empty())
                .ok_or_else(|| "workspace memory requires project_path".to_string())?;
            if path.contains('\0') || !Path::new(&path).is_absolute() {
                return Err("project_path must be an absolute path".into());
            }
            Some(path)
        }
        "global" if project_path.as_deref().map_or(true, str::is_empty) => None,
        "global" => return Err("global memory must not include project_path".into()),
        _ => return Err("memory scope must be workspace or global".into()),
    };
    let mut tags: Vec<String> = tags
        .into_iter()
        .map(|tag| tag.trim().to_string())
        .filter(|tag| !tag.is_empty())
        .collect();
    tags.sort();
    tags.dedup();
    Ok((scope.to_string(), project_path, text.to_string(), tags))
}

fn valid_entry(entry: &MemoryEntry) -> bool {
    normalized_input(
        &entry.scope,
        entry.project_path.clone(),
        &entry.text,
        entry.tags.clone(),
    )
    .is_ok()
        && serialized_line_len(entry).is_ok_and(|len| len <= MAX_MEMORY_TEXT_BYTES)
}

fn dedup_key(entry: &MemoryEntry) -> String {
    let mut hasher = Sha256::new();
    for value in std::iter::once(entry.scope.as_str())
        .chain(entry.project_path.as_deref())
        .chain(std::iter::once(entry.text.as_str()))
        .chain(entry.tags.iter().map(String::as_str))
    {
        hasher.update(value.as_bytes());
        hasher.update([0]);
    }
    hex::encode(hasher.finalize())
}

fn serialized_line_len(entry: &MemoryEntry) -> Result<usize, String> {
    serde_json::to_vec(entry)
        .map(|line| line.len() + 1)
        .map_err(|e| e.to_string())
}

fn serialized_bytes(entries: &[MemoryEntry]) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    for entry in entries {
        serde_json::to_writer(&mut bytes, entry).map_err(|e| e.to_string())?;
        bytes.push(b'\n');
    }
    if bytes.len() > MAX_MEMORY_BYTES {
        return Err("memory store exceeds storage limit".into());
    }
    Ok(bytes)
}

fn retain_within_bounds(entries: &mut Vec<MemoryEntry>) {
    while entries.len() > MAX_MEMORY_ENTRIES
        || serialized_bytes(entries)
            .map(|bytes| bytes.len() > MAX_MEMORY_BYTES)
            .unwrap_or(true)
    {
        entries.remove(0);
    }
}

static GLOBAL_MEM: std::sync::OnceLock<MemoryStore> = std::sync::OnceLock::new();

pub fn global_memory() -> &'static MemoryStore {
    GLOBAL_MEM.get_or_init(MemoryStore::open_default)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(dir: &std::path::Path) -> MemoryStore {
        MemoryStore {
            path: dir.join("memory").join("entries.jsonl"),
            cache: Mutex::new(Vec::new()),
        }
    }

    fn entry(id: usize, text: String) -> MemoryEntry {
        MemoryEntry {
            id: id.to_string(),
            scope: "workspace".into(),
            project_path: Some("/tmp/p".into()),
            text,
            tags: Vec::new(),
            created_at: id.to_string(),
        }
    }

    #[test]
    fn add_and_search() {
        let dir = std::env::temp_dir().join(format!("natives-mem-{}", Uuid::new_v4()));
        std::env::set_var("NATIVES_RUNTIME_DIR", &dir);
        // Fresh store (not global — construct directly)
        let store = store(&dir);
        store
            .add(
                "workspace",
                Some("/tmp/p".into()),
                "remember the deploy token is rotated",
                vec!["ops".into()],
            )
            .unwrap();
        let hits = store.search("deploy token", 5);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].score >= 1.0);
        let _ = std::fs::remove_dir_all(&dir);
        std::env::remove_var("NATIVES_RUNTIME_DIR");
    }

    #[test]
    fn rejects_invalid_scope_and_project_path() {
        let dir = std::env::temp_dir().join(format!("natives-mem-{}", Uuid::new_v4()));
        let store = store(&dir);

        assert!(store
            .add("other", Some("/tmp/p".into()), "x", vec![])
            .is_err());
        assert!(store.add("workspace", None, "x", vec![]).is_err());
        assert!(store
            .add("workspace", Some("relative".into()), "x", vec![])
            .is_err());
        assert!(store
            .add("global", Some("/tmp/p".into()), "x", vec![])
            .is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn duplicate_does_not_grow_store() {
        let dir = std::env::temp_dir().join(format!("natives-mem-{}", Uuid::new_v4()));
        let store = store(&dir);
        let first = store
            .add(
                "workspace",
                Some("/tmp/p".into()),
                "remember this",
                vec!["ops".into()],
            )
            .unwrap();
        let duplicate = store
            .add(
                "workspace",
                Some("/tmp/p".into()),
                "remember this",
                vec!["ops".into()],
            )
            .unwrap();

        assert_eq!(duplicate.id, first.id);
        assert_eq!(store.list(usize::MAX).len(), 1);
        assert_eq!(
            std::fs::read_to_string(dir.join("memory/entries.jsonl"))
                .unwrap()
                .lines()
                .count(),
            1
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bounds_record_cache_file_and_results() {
        let dir = std::env::temp_dir().join(format!("natives-mem-{}", Uuid::new_v4()));
        let store = store(&dir);
        assert!(store
            .add(
                "workspace",
                Some("/tmp/p".into()),
                &"x".repeat(MAX_MEMORY_TEXT_BYTES + 1),
                vec![],
            )
            .is_err());

        {
            let mut cache = store.cache.lock().unwrap();
            *cache = (0..20)
                .map(|id| entry(id, "x".repeat(MAX_MEMORY_TEXT_BYTES / 2)))
                .collect();
        }
        store
            .add("workspace", Some("/tmp/p".into()), "latest", vec![])
            .unwrap();

        assert!(store.cache.lock().unwrap().len() <= MAX_MEMORY_ENTRIES);
        assert!(
            std::fs::metadata(dir.join("memory/entries.jsonl"))
                .unwrap()
                .len()
                <= MAX_MEMORY_BYTES as u64
        );
        assert!(store.search("x", usize::MAX).len() <= MAX_MEMORY_SEARCH_LIMIT);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
