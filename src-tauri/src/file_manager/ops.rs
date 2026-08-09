//! Single-file read / write / create / rename / move / copy / duplicate /
//! stat / import operations, plus the shared path-dedupe and recursive-copy
//! helpers these build on.

use super::*;
use crate::{Error, Result};
use std::path::{Path, PathBuf};

/// Read file content (utf-8, with truncation for large files).
/// Truncation cuts on a UTF-8 boundary so multi-byte chars aren't corrupted.
pub fn read_file(file_path: &str) -> Result<ReadFileResult> {
    let path = expand_tilde(file_path);
    validate_path(&path)?;

    let meta = std::fs::metadata(&path).map_err(Error::Io)?;
    if meta.is_dir() {
        return Err(Error::InvalidInput("is a directory".into()));
    }

    let size = meta.len();
    let mtime = meta_mtime_ms(&meta);
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let kind = detect_file_kind(name);

    if size > MAX_FULL_READ {
        let read_size = std::cmp::min(size, MAX_TRUNCATED_READ) as usize;
        use std::io::Read;
        let mut file = std::fs::File::open(&path).map_err(Error::Io)?;
        let mut buffer = vec![0u8; read_size];
        let bytes_read = file.read(&mut buffer).map_err(Error::Io)?;
        buffer.truncate(bytes_read);
        // Walk back to a UTF-8 boundary
        let mut end = buffer.len();
        while end > 0 && (buffer[end - 1] & 0xC0) == 0x80 {
            end -= 1;
        }
        if end > 0 && (buffer[end - 1] & 0xC0) == 0xC0 {
            end -= 1;
        }
        let content = String::from_utf8_lossy(&buffer[..end]).to_string();
        Ok(ReadFileResult {
            content,
            truncated: true,
            size,
            mtime,
            kind,
            encoding: "utf-8".to_string(),
        })
    } else {
        let content = std::fs::read_to_string(&path).map_err(Error::Io)?;
        Ok(ReadFileResult {
            content,
            truncated: false,
            size,
            mtime,
            kind,
            encoding: "utf-8".to_string(),
        })
    }
}

/// Atomic file write with mtime conflict detection
pub fn write_file_atomic(
    file_path: &str,
    content: &str,
    expected_mtime: Option<f64>,
) -> Result<WriteResult> {
    let path = expand_tilde(file_path);
    validate_path(&path)?;

    // Mtime conflict detection
    if let Some(expected) = expected_mtime {
        if let Ok(meta) = std::fs::metadata(&path) {
            let actual = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as f64)
                .unwrap_or(0.0);
            if (actual - expected).abs() > 1.0 {
                return Ok(WriteResult {
                    mtime: actual,
                    conflict: true,
                });
            }
        }
    }

    // Atomic write: tmp file -> fsync -> rename
    let parent = path.parent().unwrap_or_else(|| Path::new("/"));
    let basename = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let tmp_name = format!(
        ".tmp-{basename}-{}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        rand_suffix()
    );
    let tmp_path = parent.join(tmp_name);

    // Write to tmp file
    use std::io::Write;
    let mut tmp_file = std::fs::File::create(&tmp_path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        Error::Io(e)
    })?;
    tmp_file.write_all(content.as_bytes()).map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        Error::Io(e)
    })?;
    tmp_file.sync_all().map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        Error::Io(e)
    })?;
    drop(tmp_file);

    // Atomic rename
    std::fs::rename(&tmp_path, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        Error::Io(e)
    })?;

    // Get new mtime
    let new_mtime = std::fs::metadata(&path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0);

    Ok(WriteResult {
        mtime: new_mtime,
        conflict: false,
    })
}

/// Create a file or directory.
/// Accepts both "dir"/"folder" and "file". Parent must already exist (or be creatable).
pub fn create_entry(target_path: &str, entry_type: &str) -> Result<()> {
    let path = expand_tilde(target_path);
    // validate against intended parent (may not exist yet for brand-new leaf)
    if let Some(parent) = path.parent() {
        if parent.exists() {
            validate_path(parent)?;
        } else {
            validate_path(&path)?;
        }
    } else {
        validate_path(&path)?;
    }

    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if !valid_name(name) {
        return Err(Error::InvalidInput("invalid entry name".into()));
    }
    if path.exists() {
        return Err(Error::InvalidInput("entry already exists".into()));
    }

    match entry_type {
        "dir" | "folder" => {
            std::fs::create_dir(&path).map_err(Error::Io)?;
        }
        "file" => {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(Error::Io)?;
            file.write_all(b"").map_err(Error::Io)?;
        }
        _ => {
            return Err(Error::InvalidInput(
                "type must be 'file', 'dir', or 'folder'".into(),
            ))
        }
    }
    Ok(())
}

/// Rename with auto-deduplication. Returns the final path written.
pub fn rename_entry(old_path: &str, new_path: &str) -> Result<String> {
    let old = expand_tilde(old_path);
    let new = expand_tilde(new_path);
    validate_path(&old)?;
    if let Some(parent) = new.parent() {
        if parent.exists() {
            validate_path(parent)?;
        }
    }
    if !old.exists() {
        return Err(Error::NotFound(old_path.to_string()));
    }
    if let Some(name) = new.file_name().and_then(|n| n.to_str()) {
        if !valid_name(name) {
            return Err(Error::InvalidInput("invalid entry name".into()));
        }
    }

    let target = deduplicate_path(&new)?;
    std::fs::rename(&old, &target).map_err(Error::Io)?;
    Ok(target.to_string_lossy().to_string())
}

/// Move entry into a destination path (file path, not just dir).
/// Same-volume: rename. Cross-volume (EXDEV): copy + delete. Auto-dedupe on collision.
pub fn move_entry(from: &str, to: &str) -> Result<String> {
    let src = expand_tilde(from);
    let dst = expand_tilde(to);
    validate_path(&src)?;
    if let Some(parent) = dst.parent() {
        if parent.exists() {
            validate_path(parent)?;
        } else {
            std::fs::create_dir_all(parent).map_err(Error::Io)?;
            validate_path(parent)?;
        }
    }
    if !src.exists() {
        return Err(Error::NotFound(from.to_string()));
    }

    // If `to` is an existing directory, place basename inside it (fanbox movePath shape).
    let dst = if dst.is_dir() {
        dst.join(src.file_name().unwrap_or_default())
    } else {
        dst
    };

    let target = deduplicate_path(&dst)?;

    match std::fs::rename(&src, &target) {
        Ok(()) => Ok(target.to_string_lossy().to_string()),
        Err(e) if is_exdev(&e) => {
            if src.is_dir() {
                copy_dir_recursive(&src, &target)?;
                std::fs::remove_dir_all(&src).map_err(Error::Io)?;
            } else {
                std::fs::copy(&src, &target).map_err(Error::Io)?;
                std::fs::remove_file(&src).map_err(Error::Io)?;
            }
            Ok(target.to_string_lossy().to_string())
        }
        Err(e) => Err(Error::Io(e)),
    }
}

/// Copy entry (file or directory) to a destination path. Auto-dedupe. Does not delete source.
pub fn copy_entry(from: &str, to: &str) -> Result<String> {
    let src = expand_tilde(from);
    let dst = expand_tilde(to);
    validate_path(&src)?;
    if let Some(parent) = dst.parent() {
        if parent.exists() {
            validate_path(parent)?;
        } else {
            std::fs::create_dir_all(parent).map_err(Error::Io)?;
            validate_path(parent)?;
        }
    }
    if !src.exists() {
        return Err(Error::NotFound(from.to_string()));
    }

    let dst = if dst.is_dir() {
        dst.join(src.file_name().unwrap_or_default())
    } else {
        dst
    };
    let target = deduplicate_path(&dst)?;

    if src.is_dir() {
        // Prevent copying a directory into itself / its descendant, which would
        // recurse forever and exhaust the disk. Mirror move_entries' guard.
        let src_canon = std::fs::canonicalize(&src).unwrap_or_else(|_| src.clone());
        let dst_canon = target
            .parent()
            .map(|p| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()))
            .unwrap_or_else(|| target.clone());
        if dst_canon.starts_with(&src_canon) {
            return Err(Error::InvalidInput(
                "cannot copy a folder into itself".into(),
            ));
        }
        copy_dir_recursive(&src, &target)?;
    } else {
        std::fs::copy(&src, &target).map_err(Error::Io)?;
    }
    Ok(target.to_string_lossy().to_string())
}

/// Duplicate an entry next to itself (`foo.txt` → `foo (1).txt`).
pub fn duplicate_entry(file_path: &str) -> Result<String> {
    let src = expand_tilde(file_path);
    validate_path(&src)?;
    if !src.exists() {
        return Err(Error::NotFound(file_path.to_string()));
    }
    let target = deduplicate_path(&src)?;
    if src.is_dir() {
        copy_dir_recursive(&src, &target)?;
    } else {
        std::fs::copy(&src, &target).map_err(Error::Io)?;
    }
    Ok(target.to_string_lossy().to_string())
}

/// Lightweight exists/stat for path resolution (terminal locate, drop validation).
pub fn stat_path(file_path: &str) -> Result<StatResult> {
    let path = expand_tilde(file_path);
    // Allow non-canonical paths: just check existence without allowlist on missing paths
    if !path.exists() {
        return Ok(StatResult {
            found: false,
            path: path.to_string_lossy().to_string(),
            name: None,
            is_dir: None,
            kind: None,
            size: None,
            mtime: None,
            btime: None,
            symlink: None,
            dir_hint: None,
        });
    }
    // Existing paths still go through allowlist
    validate_path(&path)?;
    let meta = std::fs::symlink_metadata(&path).map_err(Error::Io)?;
    let is_symlink = meta.file_type().is_symlink();
    let target_meta = if is_symlink {
        std::fs::metadata(&path).ok()
    } else {
        None
    };
    let effective = target_meta.as_ref().unwrap_or(&meta);
    let is_dir = effective.is_dir();
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    Ok(StatResult {
        found: true,
        path: path.to_string_lossy().to_string(),
        kind: Some(if is_dir {
            "dir".to_string()
        } else {
            detect_file_kind(&name)
        }),
        name: Some(name),
        is_dir: Some(is_dir),
        size: Some(if is_dir { 4096 } else { effective.len() }),
        mtime: Some(meta_mtime_ms(effective)),
        btime: Some(meta_btime_ms(effective)),
        symlink: if is_symlink {
            std::fs::read_link(&path)
                .ok()
                .map(|p| p.to_string_lossy().to_string())
        } else {
            None
        },
        // 所在目录：供「最近/定位」视图显示来源目录
        dir_hint: path.parent().map(|p| p.to_string_lossy().to_string()),
    })
}

fn is_exdev(err: &std::io::Error) -> bool {
    // macOS/Linux EXDEV = 18; also accept ErrorKind::CrossesDevices when available
    err.raw_os_error() == Some(18) || err.kind() == std::io::ErrorKind::CrossesDevices
}

/// Import files (copy from external paths)
pub fn import_files(source_paths: &[String], dest_dir: &str) -> Result<Vec<String>> {
    let dest = expand_tilde(dest_dir);
    validate_path(&dest)?;

    let mut result = Vec::new();
    for src_str in source_paths {
        let src = PathBuf::from(src_str);
        if !src.exists() {
            // Frontend contract is Vec<String> of imported paths; keep the shape
            // but surface skipped sources in the log instead of silently dropping.
            eprintln!("import_files: skipping missing source: {src_str}");
            continue;
        }
        let file_name = src.file_name().and_then(|n| n.to_str()).unwrap_or("file");
        let target = deduplicate_path(&dest.join(file_name))?;

        if src.is_dir() {
            copy_dir_recursive(&src, &target)?;
        } else {
            std::fs::copy(&src, &target).map_err(Error::Io)?;
        }
        result.push(target.to_string_lossy().to_string());
    }
    Ok(result)
}

fn rand_suffix() -> u32 {
    rand::random::<u32>()
}

/// 目标已存在时追加「 (1)」「 (2)」… 直到找到空闲名（防覆盖；crate 内共享）
pub(crate) fn deduplicate_path(path: &Path) -> Result<PathBuf> {
    if !path.exists() {
        return Ok(path.to_path_buf());
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("/"));
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{e}"))
        .unwrap_or_default();

    for i in 1..100 {
        let new_name = format!("{stem} ({i}){ext}");
        let new_path = parent.join(new_name);
        if !new_path.exists() {
            return Ok(new_path);
        }
    }
    // Exhausted all counters: return an error rather than the original path,
    // which would silently overwrite the existing file on the ensuing copy/move.
    Err(Error::InvalidInput(
        "too many name collisions; could not find a free filename".into(),
    ))
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest).map_err(Error::Io)?;
    // Resolve dest once so we can skip copying it into itself (fallback guard for
    // callers that reach here directly).
    let dest_canon = std::fs::canonicalize(dest).unwrap_or_else(|_| dest.to_path_buf());
    for entry in std::fs::read_dir(src).map_err(Error::Io)? {
        let entry = entry.map_err(Error::Io)?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());

        // Bottom-out guard: never descend into the destination itself.
        let src_canon = std::fs::canonicalize(&src_path).unwrap_or_else(|_| src_path.clone());
        if src_canon == dest_canon {
            continue;
        }

        // Use file_type() (does NOT follow symlinks). Following links would copy
        // a link target's whole contents, and a link to an ancestor would recurse
        // without bound. Recreate symlinks verbatim instead.
        let file_type = entry.file_type().map_err(Error::Io)?;
        if file_type.is_symlink() {
            #[cfg(unix)]
            {
                let link_target = std::fs::read_link(&src_path).map_err(Error::Io)?;
                std::os::unix::fs::symlink(&link_target, &dest_path).map_err(Error::Io)?;
            }
            #[cfg(not(unix))]
            {
                // Non-unix fallback: copy the link target's file bytes if any.
                if src_path.is_file() {
                    std::fs::copy(&src_path, &dest_path).map_err(Error::Io)?;
                }
            }
        } else if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dest_path)?;
        } else {
            std::fs::copy(&src_path, &dest_path).map_err(Error::Io)?;
        }
    }
    Ok(())
}
