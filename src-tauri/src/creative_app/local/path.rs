//! Path normalization and device identity for local creative apps.

use crate::{Error, Result};
use std::path::{Component, Path, PathBuf};

/// Resolve a user-selected project root to a canonical absolute path.
/// Rejects non-directories and paths that cannot be canonicalized.
pub fn canonical_project_root(raw: &str) -> Result<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(Error::InvalidInput("project root is empty".into()));
    }
    let path = PathBuf::from(trimmed);
    if !path.exists() {
        return Err(Error::InvalidInput(format!(
            "project root does not exist: {trimmed}"
        )));
    }
    let canon = path
        .canonicalize()
        .map_err(|e| Error::InvalidInput(format!("cannot canonicalize project root: {e}")))?;
    if !canon.is_dir() {
        return Err(Error::InvalidInput(format!(
            "project root is not a directory: {}",
            canon.display()
        )));
    }
    Ok(canon)
}

/// Ensure `candidate` (relative or absolute) resolves under `base` without escape.
pub fn resolve_under(base: &Path, relative: &str) -> Result<PathBuf> {
    let rel = relative.trim();
    if rel.is_empty() || rel == "." {
        return Ok(base.to_path_buf());
    }
    if Path::new(rel).is_absolute() {
        return Err(Error::InvalidInput(
            "absolute paths are not allowed inside project".into(),
        ));
    }
    // Reject `..` components early (also after join+canonicalize).
    for c in Path::new(rel).components() {
        match c {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir => {
                return Err(Error::InvalidInput("path must not contain '..'".into()));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(Error::InvalidInput(
                    "absolute path components are not allowed".into(),
                ));
            }
        }
    }
    let joined = base.join(rel);
    if !joined.exists() {
        // Allow non-existing for validation of planned entries only when parent is under base.
        let parent = joined.parent().unwrap_or(base);
        let parent_c = if parent.exists() {
            parent
                .canonicalize()
                .map_err(|e| Error::InvalidInput(format!("parent canonicalize: {e}")))?
        } else {
            return Err(Error::InvalidInput(format!("path does not exist: {rel}")));
        };
        let base_c = base
            .canonicalize()
            .map_err(|e| Error::InvalidInput(format!("base canonicalize: {e}")))?;
        if !parent_c.starts_with(&base_c) {
            return Err(Error::InvalidInput("path escapes project root".into()));
        }
        return Ok(joined);
    }
    let cand = joined
        .canonicalize()
        .map_err(|e| Error::InvalidInput(format!("path canonicalize: {e}")))?;
    let base_c = base
        .canonicalize()
        .map_err(|e| Error::InvalidInput(format!("base canonicalize: {e}")))?;
    // Symlink out of tree → reject
    if !cand.starts_with(&base_c) {
        return Err(Error::InvalidInput(
            "path escapes project root (symlink or traversal)".into(),
        ));
    }
    Ok(cand)
}

/// Stable-enough device id for this machine (not a secret).
pub fn device_id() -> String {
    let host = device_name();
    format!("host:{host}")
}

pub fn device_name() -> String {
    let h = whoami::devicename();
    let t = h.trim();
    if !t.is_empty() {
        return t.to_string();
    }
    let host = whoami::fallible::hostname().unwrap_or_default();
    let t = host.trim();
    if !t.is_empty() {
        return t.to_string();
    }
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

/// Stable volume identity for a path (batch 4): the mount point on macOS via
/// statfs, else the longest existing ancestor. Persisted so a volume re-mount
/// can be recognized across restarts; never a secret.
pub fn volume_identity(path: &Path) -> String {
    #[cfg(target_os = "macos")]
    {
        use std::ffi::CString;
        if let Ok(c) = CString::new(path.to_string_lossy().as_bytes()) {
            let mut fs: libc::statfs = unsafe { std::mem::zeroed() };
            if unsafe { libc::statfs(c.as_ptr(), &mut fs) } == 0 {
                let mount = unsafe { std::ffi::CStr::from_ptr(fs.f_mntonname.as_ptr()) }
                    .to_string_lossy()
                    .to_string();
                if !mount.is_empty() {
                    return mount;
                }
            }
        }
    }
    // Fallback: longest existing ancestor (e.g. the /Volumes/... root).
    let mut p = path.to_path_buf();
    while !p.exists() {
        if !p.pop() {
            break;
        }
    }
    p.to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        let mut p = std::env::temp_dir();
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        p.push(format!("natives-local-path-test-{n}"));
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn rejects_empty_root() {
        assert!(canonical_project_root("").is_err());
        assert!(canonical_project_root("   ").is_err());
    }

    #[test]
    fn resolve_under_blocks_parent() {
        let dir = temp_dir();
        assert!(resolve_under(&dir, "../etc/passwd").is_err());
        assert!(resolve_under(&dir, "/etc/passwd").is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_under_allows_nested() {
        let dir = temp_dir();
        fs::create_dir_all(dir.join("a/b")).unwrap();
        fs::write(dir.join("a/b/index.html"), "<html/>").unwrap();
        let p = resolve_under(&dir, "a/b/index.html").unwrap();
        assert!(p.ends_with("index.html"));
        let _ = fs::remove_dir_all(&dir);
    }

    /// Batch 4: the volume identity of an existing path is stable and non-empty
    /// (the mount point), so a volume re-mount can be detected on restart.
    #[test]
    fn volume_identity_of_existing_path_is_stable() {
        let dir = temp_dir();
        let v1 = volume_identity(&dir);
        let v2 = volume_identity(&dir);
        assert!(!v1.is_empty(), "volume identity must never be empty");
        assert_eq!(v1, v2, "volume identity must be stable across calls");
        let _ = fs::remove_dir_all(&dir);
    }
}
