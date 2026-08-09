//! Narrow filesystem utilities shared across the Host and the Agent Daemon.
//!
//! R-D5 (`docs/standards/technical/03-data.md`): overwriting important files
//! MUST use temp file → fsync → rename so a crash or mid-write failure can
//! never leave a truncated/corrupted target. This module is the single shared
//! implementation for both process authorities (`src-tauri` Host and
//! `src-agent-daemon`); new callers reuse it instead of copy-pasting a second
//! atomic-write path (R-B3).

use std::io::Write;
use std::path::{Path, PathBuf};

/// Atomically write `data` to `path`.
///
/// Protocol:
/// 1. Create a unique temp sibling in the same directory (same filesystem as
///    the target, so `rename` is atomic).
/// 2. `write_all` the bytes, then `sync_all` (fsync) to flush the data.
/// 3. `rename` the temp file over the target.
/// 4. Best-effort fsync of the parent directory so the rename itself is
///    durable.
///
/// On any failure before the rename, the temp file is removed and the target
/// is left untouched; if the rename itself fails, the temp file is removed so
/// no partial/temp residue survives.
pub fn atomic_write_bytes(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "atomic write: path has no parent directory",
        )
    })?;
    let file_name = path.file_name().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "atomic write: path has no file name",
        )
    })?;

    let tmp = temp_sibling(dir, file_name);

    let write_result = (|| -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
        Ok(())
    })();
    if let Err(e) = write_result {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }

    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }

    // Best-effort: persist the directory entry of the rename itself. Not every
    // platform supports syncing a directory handle (e.g. Windows), so ignore
    // errors — the temp → rename guarantees still hold.
    if let Ok(dir_handle) = std::fs::File::open(dir) {
        let _ = dir_handle.sync_all();
    }

    Ok(())
}

/// Build a unique temp sibling path: `.<file_name>.<pid>.<nanos>.tmp` in `dir`.
/// The pid + monotonic-nanoseconds suffix keeps concurrent writers from
/// clobbering each other's temp files.
fn temp_sibling(dir: &Path, file_name: &std::ffi::OsStr) -> PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut tmp_name = std::ffi::OsString::from(".");
    tmp_name.push(file_name);
    tmp_name.push(format!(".{}.{}.tmp", std::process::id(), nonce));
    dir.join(tmp_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(tag: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "agent-core-fs-{}-{tag}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn tmp_leftovers(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(".tmp"))
            .collect()
    }

    #[test]
    fn atomic_write_round_trips_bytes() {
        let dir = test_dir("roundtrip");
        let path = dir.join("out.bin");
        atomic_write_bytes(&path, b"hello artifact").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"hello artifact");
        assert!(tmp_leftovers(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn atomic_write_overwrites_existing_file() {
        let dir = test_dir("overwrite");
        let path = dir.join("out.bin");
        std::fs::write(&path, b"old").unwrap();
        atomic_write_bytes(&path, b"new-content").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new-content");
        assert!(tmp_leftovers(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn atomic_write_failure_before_rename_leaves_target_untouched() {
        let dir = test_dir("fail-before-rename");
        // Target whose parent is a regular file → temp creation fails before
        // any write, deterministically (ENOTDIR).
        let blocker = dir.join("blocker");
        std::fs::write(&blocker, b"not a dir").unwrap();
        let bad = blocker.join("out.bin");
        let err = atomic_write_bytes(&bad, b"data").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotADirectory);
        assert!(!bad.exists());
        assert!(tmp_leftovers(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn atomic_write_rename_failure_removes_temp_and_keeps_target() {
        let dir = test_dir("rename-failure");
        let path = dir.join("out.bin");
        std::fs::create_dir(&path).unwrap(); // target is a directory → rename fails
        std::fs::write(path.join("marker"), b"x").unwrap();
        atomic_write_bytes(&path, b"data").unwrap_err();
        // Original target intact, no temp residue.
        assert_eq!(std::fs::read(path.join("marker")).unwrap(), b"x");
        assert!(tmp_leftovers(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
