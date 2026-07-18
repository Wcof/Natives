//! Workspace / global memory search (Phase 6 minimum).
//!
//! File-backed JSONL under `NATIVES_RUNTIME_DIR/memory/` or `~/.natives/memory/`.
//! No embedding provider yet — keyword scan for CI; API ready for later upgrade.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use uuid::Uuid;

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
        let Ok(raw) = std::fs::read_to_string(&self.path) else {
            return;
        };
        let mut entries = Vec::new();
        for line in raw.lines() {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(e) = serde_json::from_str::<MemoryEntry>(line) {
                entries.push(e);
            }
        }
        if let Ok(mut c) = self.cache.lock() {
            *c = entries;
        }
    }

    fn persist_append(&self, entry: &MemoryEntry) -> Result<(), String> {
        use std::io::Write;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| e.to_string())?;
        let line = serde_json::to_string(entry).map_err(|e| e.to_string())?;
        writeln!(f, "{line}").map_err(|e| e.to_string())
    }

    pub fn add(
        &self,
        scope: &str,
        project_path: Option<String>,
        text: &str,
        tags: Vec<String>,
    ) -> Result<MemoryEntry, String> {
        if text.trim().is_empty() {
            return Err("memory text required".into());
        }
        // Never store secrets
        let redacted = assistant_protocol::v2::redact_secrets(text);
        let entry = MemoryEntry {
            id: Uuid::new_v4().to_string(),
            scope: scope.to_string(),
            project_path,
            text: redacted,
            tags,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        self.persist_append(&entry)?;
        if let Ok(mut c) = self.cache.lock() {
            c.push(entry.clone());
        }
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
        hits.truncate(limit.max(1));
        hits
    }

    pub fn list(&self, limit: usize) -> Vec<MemoryEntry> {
        self.cache
            .lock()
            .map(|c| c.iter().rev().take(limit.max(1)).cloned().collect())
            .unwrap_or_default()
    }
}

static GLOBAL_MEM: std::sync::OnceLock<MemoryStore> = std::sync::OnceLock::new();

pub fn global_memory() -> &'static MemoryStore {
    GLOBAL_MEM.get_or_init(MemoryStore::open_default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_search() {
        let dir = std::env::temp_dir().join(format!("natives-mem-{}", Uuid::new_v4()));
        std::env::set_var("NATIVES_RUNTIME_DIR", &dir);
        // Fresh store (not global — construct directly)
        let store = MemoryStore {
            path: dir.join("memory").join("entries.jsonl"),
            cache: Mutex::new(Vec::new()),
        };
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
}
