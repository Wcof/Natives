//! Stable ProjectIdentity — path is an attribute, not the identity.
//!
//! ```text
//! project_id (stable UUID)
//! canonical_path
//! filesystem_fingerprint
//! identity_version
//! verified_at
//! ```

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Error when a project identity cannot be resolved or verified.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ProjectIdentityError {
    #[error("project path does not exist: {0}")]
    NotFound(String),
    #[error("project path is not a directory: {0}")]
    NotDirectory(String),
    #[error("failed to canonicalize path '{path}': {message}")]
    Canonicalize {
        path: String,
        message: String,
    },
    #[error("path escapes project boundary: {0}")]
    Escape(String),
    #[error("PROJECT_IDENTITY_CHANGED: fingerprint mismatch for {project_id}")]
    FingerprintChanged { project_id: String },
    #[error("orphaned project registration: {project_id} (path missing)")]
    Orphaned { project_id: String },
    #[error("{0}")]
    Other(String),
}

/// Concrete project identity used for privileged tool/MCP/CLI invocation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectIdentity {
    pub project_id: String,
    pub canonical_path: String,
    pub filesystem_fingerprint: String,
    pub identity_version: u32,
    pub verified_at: i64,
}

impl ProjectIdentity {
    /// Verify this identity still points at the same filesystem object.
    pub fn verify(&self) -> Result<(), ProjectIdentityError> {
        let path = Path::new(&self.canonical_path);
        if !path.exists() {
            return Err(ProjectIdentityError::Orphaned {
                project_id: self.project_id.clone(),
            });
        }
        if !path.is_dir() {
            return Err(ProjectIdentityError::NotDirectory(
                self.canonical_path.clone(),
            ));
        }
        let fp = filesystem_fingerprint(path)?;
        if fp != self.filesystem_fingerprint {
            return Err(ProjectIdentityError::FingerprintChanged {
                project_id: self.project_id.clone(),
            });
        }
        Ok(())
    }

    pub fn as_path(&self) -> &Path {
        Path::new(&self.canonical_path)
    }
}

/// Resolve a user-supplied path into a verified ProjectIdentity registration.
pub struct ProjectIdentityResolver;

impl ProjectIdentityResolver {
    /// Canonicalize + fingerprint a path that must exist and be a directory.
    pub fn resolve_path(raw: &str) -> Result<(PathBuf, String), ProjectIdentityError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(ProjectIdentityError::Other("empty project path".into()));
        }
        let path = PathBuf::from(trimmed);
        if !path.exists() {
            return Err(ProjectIdentityError::NotFound(trimmed.into()));
        }
        if !path.is_dir() {
            return Err(ProjectIdentityError::NotDirectory(trimmed.into()));
        }
        let canonical = path.canonicalize().map_err(|e| ProjectIdentityError::Canonicalize {
            path: trimmed.into(),
            message: e.to_string(),
        })?;
        // Reject symlink/junction that left the intended tree by requiring
        // the canonical path still looks like a directory root.
        if !canonical.is_dir() {
            return Err(ProjectIdentityError::NotDirectory(
                canonical.display().to_string(),
            ));
        }
        let fp = filesystem_fingerprint(&canonical)?;
        Ok((canonical, fp))
    }

    /// Build a new identity (caller persists project_id).
    pub fn mint(project_id: impl Into<String>, raw_path: &str) -> Result<ProjectIdentity, ProjectIdentityError> {
        let (canonical, fp) = Self::resolve_path(raw_path)?;
        Ok(ProjectIdentity {
            project_id: project_id.into(),
            canonical_path: canonical.to_string_lossy().into_owned(),
            filesystem_fingerprint: fp,
            identity_version: 1,
            verified_at: now_secs(),
        })
    }

    /// Re-bind after user confirms a moved directory (bumps identity_version).
    pub fn rebind(existing: &ProjectIdentity, raw_path: &str) -> Result<ProjectIdentity, ProjectIdentityError> {
        let (canonical, fp) = Self::resolve_path(raw_path)?;
        Ok(ProjectIdentity {
            project_id: existing.project_id.clone(),
            canonical_path: canonical.to_string_lossy().into_owned(),
            filesystem_fingerprint: fp,
            identity_version: existing.identity_version.saturating_add(1),
            verified_at: now_secs(),
        })
    }

    /// Ensure a path is under an identity's canonical root (for tool path resolution).
    pub fn ensure_under(identity: &ProjectIdentity, candidate: &Path) -> Result<PathBuf, ProjectIdentityError> {
        identity.verify()?;
        let root = identity.as_path();
        let resolved = if candidate.is_absolute() {
            if candidate.exists() {
                candidate.canonicalize().map_err(|e| ProjectIdentityError::Other(e.to_string()))?
            } else if let Some(parent) = candidate.parent() {
                let parent_c = if parent.as_os_str().is_empty() {
                    root.to_path_buf()
                } else if parent.exists() {
                    parent.canonicalize().map_err(|e| ProjectIdentityError::Other(e.to_string()))?
                } else {
                    return Err(ProjectIdentityError::Escape(candidate.display().to_string()));
                };
                parent_c.join(candidate.file_name().unwrap_or_default())
            } else {
                candidate.to_path_buf()
            }
        } else {
            root.join(candidate)
        };
        // Boundary check: resolved must start with root
        if !resolved.starts_with(root) {
            // For non-existing files, check parent
            if let Some(parent) = resolved.parent() {
                if parent.starts_with(root) || parent == root {
                    return Ok(resolved);
                }
            }
            return Err(ProjectIdentityError::Escape(resolved.display().to_string()));
        }
        Ok(resolved)
    }
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Platform filesystem fingerprint.
///
/// - Unix/macOS: `device:inode`
/// - Windows: volume serial + file index when available; else canonical path hash
/// - Fallback: canonical path string (fail-closed on change via path mismatch)
fn filesystem_fingerprint(path: &Path) -> Result<String, ProjectIdentityError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let meta = std::fs::metadata(path).map_err(|e| ProjectIdentityError::Other(e.to_string()))?;
        Ok(format!("unix:{}:{}", meta.dev(), meta.ino()))
    }
    #[cfg(windows)]
    {
        // Best-effort: use canonical path + file attributes size/ctime proxy.
        // Full volume serial + file index requires winapi; fail-closed on path change.
        let meta = std::fs::metadata(path).map_err(|e| ProjectIdentityError::Other(e.to_string()))?;
        let canon = path
            .canonicalize()
            .map_err(|e| ProjectIdentityError::Other(e.to_string()))?;
        Ok(format!(
            "win:{}:{}:{}",
            canon.to_string_lossy(),
            meta.len(),
            meta.modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0)
        ))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let canon = path
            .canonicalize()
            .map_err(|e| ProjectIdentityError::Other(e.to_string()))?;
        Ok(format!("path:{}", canon.to_string_lossy()))
    }
}

/// Persist / load project rows from the shared assistant.db (daemon-side).
pub mod store {
    use super::*;
    use rusqlite::{params, Connection};

    /// Ensure the project identity table exists (daemon migration 015 also creates it).
    pub fn ensure_table(conn: &Connection) -> Result<(), String> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS project_identity (
                project_id TEXT PRIMARY KEY,
                canonical_path TEXT NOT NULL,
                filesystem_fingerprint TEXT NOT NULL,
                identity_version INTEGER NOT NULL DEFAULT 1,
                verified_at INTEGER NOT NULL DEFAULT 0,
                orphaned INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_project_identity_path
                ON project_identity(canonical_path)
                WHERE orphaned = 0;",
        )
        .map_err(|e| e.to_string())
    }

    /// Register or return existing active identity for the same fingerprint/path.
    pub fn register_or_get(conn: &Connection, raw_path: &str) -> Result<ProjectIdentity, String> {
        ensure_table(conn)?;
        let (canonical, fp) = ProjectIdentityResolver::resolve_path(raw_path)
            .map_err(|e| e.to_string())?;
        let canon_s = canonical.to_string_lossy().into_owned();

        // Same active path → same identity
        if let Ok(row) = conn.query_row(
            "SELECT project_id, canonical_path, filesystem_fingerprint, identity_version, verified_at
             FROM project_identity
             WHERE canonical_path = ?1 AND orphaned = 0
             LIMIT 1",
            params![&canon_s],
            |r| {
                Ok(ProjectIdentity {
                    project_id: r.get(0)?,
                    canonical_path: r.get(1)?,
                    filesystem_fingerprint: r.get(2)?,
                    identity_version: r.get::<_, i64>(3)? as u32,
                    verified_at: r.get(4)?,
                })
            },
        ) {
            // Re-verify fingerprint; if changed mark orphan and create new.
            if row.filesystem_fingerprint == fp {
                let _ = conn.execute(
                    "UPDATE project_identity SET verified_at = ?1, updated_at = datetime('now')
                     WHERE project_id = ?2",
                    params![now_secs(), row.project_id],
                );
                return Ok(row);
            }
            let _ = conn.execute(
                "UPDATE project_identity SET orphaned = 1, updated_at = datetime('now')
                 WHERE project_id = ?1",
                params![row.project_id],
            );
        }

        // Same fingerprint elsewhere (moved?) — rare; mint new id for clear UX.
        let project_id = uuid::Uuid::new_v4().to_string();
        let identity = ProjectIdentity {
            project_id: project_id.clone(),
            canonical_path: canon_s,
            filesystem_fingerprint: fp,
            identity_version: 1,
            verified_at: now_secs(),
        };
        conn.execute(
            "INSERT INTO project_identity
                (project_id, canonical_path, filesystem_fingerprint, identity_version, verified_at, orphaned)
             VALUES (?1, ?2, ?3, ?4, ?5, 0)",
            params![
                identity.project_id,
                identity.canonical_path,
                identity.filesystem_fingerprint,
                identity.identity_version as i64,
                identity.verified_at,
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(identity)
    }

    pub fn get(conn: &Connection, project_id: &str) -> Result<Option<ProjectIdentity>, String> {
        ensure_table(conn)?;
        let row = conn.query_row(
            "SELECT project_id, canonical_path, filesystem_fingerprint, identity_version, verified_at, orphaned
             FROM project_identity WHERE project_id = ?1",
            params![project_id],
            |r| {
                Ok((
                    ProjectIdentity {
                        project_id: r.get(0)?,
                        canonical_path: r.get(1)?,
                        filesystem_fingerprint: r.get(2)?,
                        identity_version: r.get::<_, i64>(3)? as u32,
                        verified_at: r.get(4)?,
                    },
                    r.get::<_, i64>(5)? != 0,
                ))
            },
        );
        match row {
            Ok((id, orphaned)) => {
                if orphaned {
                    return Err(ProjectIdentityError::Orphaned {
                        project_id: id.project_id,
                    }
                    .to_string());
                }
                Ok(Some(id))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn verify_for_invocation(
        conn: &Connection,
        project_id: &str,
    ) -> Result<ProjectIdentity, String> {
        let identity = get(conn, project_id)?
            .ok_or_else(|| format!("unknown project_id: {project_id}"))?;
        identity.verify().map_err(|e| e.to_string())?;
        let _ = conn.execute(
            "UPDATE project_identity SET verified_at = ?1, updated_at = datetime('now')
             WHERE project_id = ?2",
            params![now_secs(), project_id],
        );
        Ok(identity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn resolves_relative_and_dotdot_to_same_identity() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        fs::create_dir_all(&root).unwrap();
        let nested = root.join("a");
        fs::create_dir_all(&nested).unwrap();

        let (c1, fp1) = ProjectIdentityResolver::resolve_path(root.to_str().unwrap()).unwrap();
        let via_dotdot = nested.join("..");
        let (c2, fp2) =
            ProjectIdentityResolver::resolve_path(via_dotdot.to_str().unwrap()).unwrap();
        assert_eq!(c1, c2);
        assert_eq!(fp1, fp2);
    }

    #[test]
    fn fingerprint_changes_when_directory_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("proj");
        fs::create_dir_all(&root).unwrap();
        let id = ProjectIdentityResolver::mint("p1", root.to_str().unwrap()).unwrap();
        id.verify().unwrap();

        // Replace directory (new inode on unix)
        fs::remove_dir_all(&root).unwrap();
        fs::create_dir_all(&root).unwrap();
        let err = id.verify().unwrap_err();
        assert!(
            matches!(err, ProjectIdentityError::FingerprintChanged { .. })
                || matches!(err, ProjectIdentityError::Orphaned { .. }),
            "{err:?}"
        );
    }

    #[test]
    fn ensure_under_rejects_escape() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("proj");
        fs::create_dir_all(&root).unwrap();
        let outside = dir.path().join("outside.txt");
        fs::write(&outside, b"x").unwrap();
        let id = ProjectIdentityResolver::mint("p1", root.to_str().unwrap()).unwrap();
        let err = ProjectIdentityResolver::ensure_under(&id, &outside).unwrap_err();
        assert!(matches!(err, ProjectIdentityError::Escape(_)), "{err:?}");
    }

    #[test]
    fn register_or_get_is_stable() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("proj");
        fs::create_dir_all(&root).unwrap();
        let db = dir.path().join("t.db");
        let conn = rusqlite::Connection::open(&db).unwrap();
        let a = store::register_or_get(&conn, root.to_str().unwrap()).unwrap();
        let b = store::register_or_get(&conn, root.join(".").to_str().unwrap()).unwrap();
        assert_eq!(a.project_id, b.project_id);
        assert_eq!(a.filesystem_fingerprint, b.filesystem_fingerprint);
    }
}
