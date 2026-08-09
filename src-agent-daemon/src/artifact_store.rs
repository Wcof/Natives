//! Artifact store (Phase 6 minimum) — path-isolated list/open with checksum.
//!
//! Artifacts live under `NATIVES_RUNTIME_DIR/artifacts/{run_id}/`. Paths are
//! canonicalized and must stay inside the run directory (no escape).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactMeta {
    pub id: String,
    pub run_id: String,
    pub name: String,
    pub path: String,
    pub size: u64,
    pub sha256: String,
    pub mime_type: Option<String>,
}

pub struct ArtifactStore {
    root: PathBuf,
    index: Mutex<Vec<ArtifactMeta>>,
}

impl ArtifactStore {
    pub fn open_default() -> Self {
        let root = std::env::var("NATIVES_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                std::env::var_os("HOME")
                    .or_else(|| std::env::var_os("USERPROFILE"))
                    .map(|h| PathBuf::from(h).join(".natives").join("runtime"))
                    .unwrap_or_else(|| std::env::temp_dir().join("natives-runtime"))
            })
            .join("artifacts");
        let _ = std::fs::create_dir_all(&root);
        Self {
            root,
            index: Mutex::new(Vec::new()),
        }
    }

    fn run_dir(&self, run_id: &str) -> Result<PathBuf, String> {
        if run_id.contains("..") || run_id.contains('/') || run_id.contains('\\') {
            return Err("invalid run_id".into());
        }
        let dir = self.root.join(run_id);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        Ok(dir)
    }

    fn resolve_under_run(&self, run_id: &str, name: &str) -> Result<PathBuf, String> {
        let base = self.run_dir(run_id)?;
        let base_canon = base.canonicalize().unwrap_or_else(|_| base.clone());
        let candidate = base.join(name);
        // Prevent path escape before create.
        let candidate_norm = candidate.components().collect::<PathBuf>();
        if candidate_norm
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err("artifact path escape denied".into());
        }
        if let Ok(canon) = candidate.canonicalize() {
            if !canon.starts_with(&base_canon) {
                return Err("artifact path outside run dir".into());
            }
            return Ok(canon);
        }
        Ok(candidate)
    }

    pub fn put(
        &self,
        run_id: &str,
        name: &str,
        bytes: &[u8],
        mime_type: Option<String>,
    ) -> Result<ArtifactMeta, String> {
        if name.trim().is_empty() || name.contains("..") {
            return Err("invalid artifact name".into());
        }
        // Size cap 25 MiB
        if bytes.len() > 25 * 1024 * 1024 {
            return Err("artifact exceeds 25MiB limit".into());
        }
        let path = self.resolve_under_run(run_id, name)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        // R-D5 atomic write: temp sibling → write_all → fsync → rename. The
        // in-memory index below is only touched after this succeeds, so the
        // index never advertises an artifact that is not durably on disk.
        agent_core::fs_util::atomic_write_bytes(&path, bytes).map_err(|e| e.to_string())?;
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let sha256 = hex::encode(hasher.finalize());
        let meta = ArtifactMeta {
            id: format!("{run_id}/{name}"),
            run_id: run_id.to_string(),
            name: name.to_string(),
            path: path.display().to_string(),
            size: bytes.len() as u64,
            sha256,
            mime_type,
        };
        if let Ok(mut idx) = self.index.lock() {
            idx.retain(|a| a.id != meta.id);
            idx.push(meta.clone());
        }
        Ok(meta)
    }

    pub fn list(&self, run_id: Option<&str>) -> Vec<ArtifactMeta> {
        // Merge index + scan disk for run
        let mut out = self.index.lock().map(|g| g.clone()).unwrap_or_default();
        if let Some(rid) = run_id {
            if let Ok(dir) = self.run_dir(rid) {
                if let Ok(rd) = std::fs::read_dir(dir) {
                    for e in rd.flatten() {
                        let path = e.path();
                        if !path.is_file() {
                            continue;
                        }
                        let name = path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("file")
                            .to_string();
                        let id = format!("{rid}/{name}");
                        if out.iter().any(|a| a.id == id) {
                            continue;
                        }
                        let bytes = std::fs::read(&path).unwrap_or_default();
                        let mut hasher = Sha256::new();
                        hasher.update(&bytes);
                        out.push(ArtifactMeta {
                            id,
                            run_id: rid.to_string(),
                            name,
                            path: path.display().to_string(),
                            size: bytes.len() as u64,
                            sha256: hex::encode(hasher.finalize()),
                            mime_type: None,
                        });
                    }
                }
            }
            out.retain(|a| a.run_id == rid);
        }
        out
    }

    pub fn open(&self, id: &str) -> Result<(ArtifactMeta, Vec<u8>), String> {
        let (run_id, name) = id
            .split_once('/')
            .ok_or_else(|| "artifact id must be run_id/name".to_string())?;
        let path = self.resolve_under_run(run_id, name)?;
        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        // Re-check path after read using canonicalize
        let base = self
            .run_dir(run_id)?
            .canonicalize()
            .map_err(|e| e.to_string())?;
        let canon = path.canonicalize().map_err(|e| e.to_string())?;
        if !canon.starts_with(&base) {
            return Err("artifact path outside run dir".into());
        }
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let meta = ArtifactMeta {
            id: id.to_string(),
            run_id: run_id.to_string(),
            name: name.to_string(),
            path: canon.display().to_string(),
            size: bytes.len() as u64,
            sha256: hex::encode(hasher.finalize()),
            mime_type: None,
        };
        Ok((meta, bytes))
    }
}

static GLOBAL_ART: std::sync::OnceLock<ArtifactStore> = std::sync::OnceLock::new();

pub fn global_artifacts() -> &'static ArtifactStore {
    GLOBAL_ART.get_or_init(ArtifactStore::open_default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_list_open_and_escape_denied() {
        let dir = std::env::temp_dir().join(format!("art-{}", uuid::Uuid::new_v4()));
        std::env::set_var("NATIVES_RUNTIME_DIR", &dir);
        let store = ArtifactStore {
            root: dir.join("artifacts"),
            index: Mutex::new(Vec::new()),
        };
        let meta = store
            .put(
                "run1",
                "out.txt",
                b"hello artifact",
                Some("text/plain".into()),
            )
            .unwrap();
        assert_eq!(meta.size, 14);
        let list = store.list(Some("run1"));
        assert_eq!(list.len(), 1);
        let (m, bytes) = store.open(&meta.id).unwrap();
        assert_eq!(bytes, b"hello artifact");
        assert_eq!(m.sha256, meta.sha256);
        assert!(store.put("run1", "../escape.txt", b"x", None).is_err());
        let _ = std::fs::remove_dir_all(&dir);
        std::env::remove_var("NATIVES_RUNTIME_DIR");
    }

    #[test]
    fn put_uses_atomic_write_chain_and_updates_index_after_durable_write() {
        let dir = std::env::temp_dir().join(format!("art-atomic-{}", uuid::Uuid::new_v4()));
        let store = ArtifactStore {
            root: dir.join("artifacts"),
            index: Mutex::new(Vec::new()),
        };
        let bytes = b"atomic artifact bytes";
        let meta = store.put("run1", "out.bin", bytes, None).unwrap();
        let path = std::path::PathBuf::from(&meta.path);
        // Success path: file on disk is complete and byte-exact.
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(meta.size, bytes.len() as u64);
        // In-memory index reflects the durably-renamed artifact.
        let list = store.list(Some("run1"));
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, meta.id);
        // No temp residue left in the run directory.
        let run_dir = store.root.join("run1");
        let leftovers: Vec<_> = std::fs::read_dir(&run_dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.contains(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temp files left: {leftovers:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn put_failure_before_rename_leaves_no_partial_file_and_no_index_entry() {
        let dir = std::env::temp_dir().join(format!("art-fail-{}", uuid::Uuid::new_v4()));
        let store = ArtifactStore {
            root: dir.join("artifacts"),
            index: Mutex::new(Vec::new()),
        };
        // A NUL byte in the artifact name makes temp-file creation fail
        // deterministically (std rejects NUL in paths) — a failure strictly
        // before rename, so nothing may touch the target.
        let err = store.put("run1", "bad\0name", b"hello", None).unwrap_err();
        assert!(!err.is_empty());
        let run_dir = store.root.join("run1");
        assert!(run_dir.exists());
        assert!(std::fs::read_dir(&run_dir).unwrap().next().is_none());
        assert!(store.list(Some("run1")).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn put_rename_failure_removes_temp_and_keeps_index_empty() {
        let dir = std::env::temp_dir().join(format!("art-rename-{}", uuid::Uuid::new_v4()));
        let store = ArtifactStore {
            root: dir.join("artifacts"),
            index: Mutex::new(Vec::new()),
        };
        // Target exists as a non-empty directory → rename over it fails; the
        // temp file must be cleaned up and the pre-existing target untouched.
        let run_dir = store.root.join("run1");
        std::fs::create_dir_all(run_dir.join("out.txt")).unwrap();
        std::fs::write(run_dir.join("out.txt").join("marker"), b"x").unwrap();
        assert!(store.put("run1", "out.txt", b"hello", None).is_err());
        assert_eq!(
            std::fs::read(run_dir.join("out.txt").join("marker")).unwrap(),
            b"x"
        );
        let leftovers: Vec<_> = std::fs::read_dir(&run_dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.contains(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temp files left: {leftovers:?}");
        // Index is only updated after a durable rename — a failed write must
        // not advertise the artifact.
        assert!(store.list(Some("run1")).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
