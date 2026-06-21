use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const MAX_FULL_READ: u64 = 2 * 1024 * 1024; // 2MB
const MAX_TRUNCATED_READ: u64 = 256 * 1024; // 256KB

#[derive(Debug, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    #[serde(rename = "isDir")]
    pub is_dir: bool,
    pub kind: String,
    pub hidden: bool,
    pub size: u64,
    pub mtime: f64,
    pub btime: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symlink: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ReadFileResult {
    pub content: String,
    pub truncated: bool,
    pub size: u64,
    pub encoding: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WriteResult {
    pub mtime: f64,
    pub conflict: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListDirOptions {
    #[serde(default = "default_sort_by")]
    pub sort_by: String,
    #[serde(default = "default_sort_dir")]
    pub sort_dir: String,
    #[serde(default)]
    pub show_hidden: bool,
}

impl Default for ListDirOptions {
    fn default() -> Self {
        Self {
            sort_by: default_sort_by(),
            sort_dir: default_sort_dir(),
            show_hidden: false,
        }
    }
}

fn default_sort_by() -> String {
    "name".to_string()
}
fn default_sort_dir() -> String {
    "asc".to_string()
}

/// Expand ~ to home directory
fn expand_tilde(path: &str) -> PathBuf {
    if path.starts_with("~/") || path == "~" {
        if let Some(home) = dirs::home_dir() {
            return home.join(path.strip_prefix("~/").unwrap_or(""));
        }
    }
    PathBuf::from(path)
}

/// Validate path security (allowlist: home, /tmp, /private/tmp)
fn validate_path(path: &Path) -> Result<()> {
    let path_str = path.to_string_lossy();
    if path_str.contains('\0') {
        return Err(Error::InvalidInput("path contains null byte".into()));
    }

    let canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));

    // Blocklist: sensitive dotfiles
    let blocked = [".ssh", ".gnupg", ".aws", ".config/gh", ".kube"];
    for b in &blocked {
        if canon.ends_with(b) || canon.to_string_lossy().contains(&format!("/{b}/")) {
            return Err(Error::InvalidInput(format!("access denied: {b}")));
        }
    }

    // Allowlist
    if canon.starts_with(&home) || canon.starts_with("/tmp") || canon.starts_with("/private/tmp") {
        Ok(())
    } else {
        Err(Error::InvalidInput("path not in allowed directories".into()))
    }
}

/// Detect file kind from extension
fn detect_file_kind(name: &str) -> String {
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "txt" | "md" | "mdx" | "json" | "yaml" | "yml" | "toml" | "xml" | "csv" | "log"
        | "ini" | "cfg" | "conf" | "env" | "gitignore" | "dockerignore" | "editorconfig" => {
            "text".to_string()
        }
        "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "py" | "rb" | "rs" | "go" | "java"
        | "c" | "cpp" | "h" | "hpp" | "cs" | "swift" | "kt" | "sh" | "bash" | "zsh"
        | "fish" | "ps1" | "bat" | "cmd" => "text".to_string(),
        "html" | "htm" | "css" | "scss" | "sass" | "less" => "text".to_string(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "ico" | "bmp" | "tiff" | "heic" => {
            "image".to_string()
        }
        "mp4" | "mov" | "avi" | "mkv" | "webm" | "flv" | "wmv" => "video".to_string(),
        "mp3" | "wav" | "ogg" | "flac" | "aac" | "m4a" => "audio".to_string(),
        "pdf" => "pdf".to_string(),
        "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "rar" | "tgz" => "archive".to_string(),
        _ => "other".to_string(),
    }
}

/// List directory contents
pub fn list_dir(dir_path: &str, options: &ListDirOptions) -> Result<Vec<FileEntry>> {
    let path = expand_tilde(dir_path);
    let canon = std::fs::canonicalize(&path).map_err(Error::Io)?;

    let meta = std::fs::metadata(&canon).map_err(Error::Io)?;
    if !meta.is_dir() {
        return Err(Error::InvalidInput("not a directory".into()));
    }

    let mut entries = Vec::new();
    for entry in std::fs::read_dir(&canon).map_err(Error::Io)? {
        let entry = entry.map_err(Error::Io)?;
        let name = entry.file_name().to_string_lossy().to_string();

        // Filter .DS_Store and dotfiles
        if !options.show_hidden && (name == ".DS_Store" || name.starts_with('.')) {
            continue;
        }

        let entry_path = entry.path();
        let entry_meta = match std::fs::symlink_metadata(&entry_path) {
            Ok(m) => m,
            Err(_) => continue,
        };

        let is_symlink = entry_meta.file_type().is_symlink();
        let symlink_target = if is_symlink {
            std::fs::read_link(&entry_path)
                .ok()
                .map(|p| p.to_string_lossy().to_string())
        } else {
            None
        };

        // Resolve target metadata for symlinks
        let target_meta = if is_symlink {
            std::fs::metadata(&entry_path).ok()
        } else {
            None
        };
        let effective_meta = target_meta.as_ref().unwrap_or(&entry_meta);

        let is_dir = effective_meta.is_dir();
        let size = if is_dir {
            4096
        } else {
            effective_meta.len()
        };

        let mtime = effective_meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as f64)
            .unwrap_or(0.0);

        let btime = effective_meta
            .created()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as f64)
            .unwrap_or(0.0);

        entries.push(FileEntry {
            name,
            path: entry_path.to_string_lossy().to_string(),
            is_dir,
            kind: if is_dir {
                "other".to_string()
            } else {
                detect_file_kind(&entry.file_name().to_string_lossy())
            },
            hidden: entry.file_name().to_string_lossy().starts_with('.'),
            size,
            mtime,
            btime,
            symlink: symlink_target,
        });
    }

    // Sort
    let ascending = options.sort_dir == "asc";
    match options.sort_by.as_str() {
        "name" => entries.sort_by(|a, b| {
            let cmp = natural_cmp(&a.name, &b.name);
            if ascending {
                cmp
            } else {
                cmp.reverse()
            }
        }),
        "mtime" => entries.sort_by(|a, b| {
            let cmp = a
                .mtime
                .partial_cmp(&b.mtime)
                .unwrap_or(std::cmp::Ordering::Equal);
            if ascending {
                cmp
            } else {
                cmp.reverse()
            }
        }),
        "size" => entries.sort_by(|a, b| {
            let cmp = a.size.cmp(&b.size);
            if ascending {
                cmp
            } else {
                cmp.reverse()
            }
        }),
        _ => {}
    }

    Ok(entries)
}

/// Read file content (utf-8, with truncation for large files)
pub fn read_file(file_path: &str) -> Result<ReadFileResult> {
    let path = expand_tilde(file_path);
    validate_path(&path)?;

    let meta = std::fs::metadata(&path).map_err(Error::Io)?;
    if meta.is_dir() {
        return Err(Error::InvalidInput("is a directory".into()));
    }

    let size = meta.len();

    if size > MAX_FULL_READ {
        // Truncated read
        let read_size = std::cmp::min(size, MAX_TRUNCATED_READ);
        use std::io::Read;
        let mut file = std::fs::File::open(&path).map_err(Error::Io)?;
        let mut buffer = vec![0u8; read_size as usize];
        file.read_exact(&mut buffer).map_err(Error::Io)?;
        // Strip null bytes
        buffer.retain(|&b| b != 0);
        let content = String::from_utf8_lossy(&buffer).to_string();
        Ok(ReadFileResult {
            content,
            truncated: true,
            size,
            encoding: "utf-8".to_string(),
        })
    } else {
        // Full read
        let content = std::fs::read_to_string(&path).map_err(Error::Io)?;
        Ok(ReadFileResult {
            content,
            truncated: false,
            size,
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
    let basename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");
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
    let mut tmp_file =
        std::fs::File::create(&tmp_path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp_path);
            Error::Io(e)
        })?;
    tmp_file
        .write_all(content.as_bytes())
        .map_err(|e| {
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

/// Create a file or directory
pub fn create_entry(target_path: &str, entry_type: &str) -> Result<()> {
    let path = expand_tilde(target_path);
    validate_path(&path)?;

    match entry_type {
        "dir" => {
            std::fs::create_dir(&path).map_err(Error::Io)?;
        }
        "file" => {
            // Exclusive create (fails if exists)
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(Error::Io)?;
            file.write_all(b"").map_err(Error::Io)?;
        }
        _ => return Err(Error::InvalidInput("type must be 'file' or 'dir'".into())),
    }
    Ok(())
}

/// Rename with auto-deduplication
pub fn rename_entry(old_path: &str, new_path: &str) -> Result<()> {
    let old = expand_tilde(old_path);
    let new = expand_tilde(new_path);
    validate_path(&old)?;
    validate_path(&new)?;

    if !old.exists() {
        return Err(Error::NotFound(old_path.to_string()));
    }

    // Auto-deduplication
    let target = deduplicate_path(&new);
    std::fs::rename(&old, &target).map_err(Error::Io)?;
    Ok(())
}

/// Move entry (same-volume rename, cross-volume copy+delete)
pub fn move_entry(from: &str, to: &str) -> Result<()> {
    let src = expand_tilde(from);
    let dst = expand_tilde(to);
    validate_path(&src)?;
    validate_path(&dst)?;

    if !src.exists() {
        return Err(Error::NotFound(from.to_string()));
    }

    let target = deduplicate_path(&dst);

    match std::fs::rename(&src, &target) {
        Ok(()) => Ok(()),
        Err(e) if e.raw_os_error() == Some(18) => {
            // EXDEV: cross-volume, copy then delete
            if src.is_dir() {
                copy_dir_recursive(&src, &target)?;
                std::fs::remove_dir_all(&src).map_err(Error::Io)?;
            } else {
                std::fs::copy(&src, &target).map_err(Error::Io)?;
                std::fs::remove_file(&src).map_err(Error::Io)?;
            }
            Ok(())
        }
        Err(e) => Err(Error::Io(e)),
    }
}

/// Trash entry (macOS)
pub fn trash_entry(file_path: &str) -> Result<()> {
    let path = expand_tilde(file_path);
    if !path.exists() {
        return Err(Error::NotFound(file_path.to_string()));
    }

    // Use trash crate
    trash::delete(&path).map_err(|e| Error::Internal(format!("trash failed: {e}")))
}

/// Import files (copy from external paths)
pub fn import_files(source_paths: &[String], dest_dir: &str) -> Result<Vec<String>> {
    let dest = expand_tilde(dest_dir);
    validate_path(&dest)?;

    let mut result = Vec::new();
    for src_str in source_paths {
        let src = PathBuf::from(src_str);
        if !src.exists() {
            continue;
        }
        let file_name = src
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file");
        let target = deduplicate_path(&dest.join(file_name));

        if src.is_dir() {
            copy_dir_recursive(&src, &target)?;
        } else {
            std::fs::copy(&src, &target).map_err(Error::Io)?;
        }
        result.push(target.to_string_lossy().to_string());
    }
    Ok(result)
}

/// Get recently modified files (BFS walk, top 60 by mtime)
pub fn recent_files(root: &str) -> Result<Vec<serde_json::Value>> {
    let path = expand_tilde(root);
    let canon = std::fs::canonicalize(&path).map_err(Error::Io)?;

    let ignore_dirs: std::collections::HashSet<&str> = [
        "node_modules",
        ".git",
        ".next",
        ".cache",
        "dist",
        "out",
        "build",
        ".vscode",
        ".idea",
        ".DS_Store",
    ]
    .iter()
    .cloned()
    .collect();

    let mut files: Vec<(String, f64, u64)> = Vec::new();
    let start = std::time::Instant::now();
    let deadline = std::time::Duration::from_secs_f64(3.5);
    let max_files = 30_000;

    let mut queue = std::collections::VecDeque::new();
    queue.push_back(canon.clone());

    while let Some(dir) = queue.pop_front() {
        if start.elapsed() > deadline || files.len() > max_files {
            break;
        }
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || ignore_dirs.contains(name.as_str()) {
                continue;
            }
            let entry_path = entry.path();
            let meta = match std::fs::metadata(&entry_path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if meta.is_dir() {
                queue.push_back(entry_path);
            } else {
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as f64)
                    .unwrap_or(0.0);
                files.push((
                    entry_path.to_string_lossy().to_string(),
                    mtime,
                    meta.len(),
                ));
            }
        }
    }

    // Sort by mtime descending, take top 60
    files.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    files.truncate(60);

    Ok(files
        .into_iter()
        .map(|(path, mtime, size)| {
            serde_json::json!({
                "path": path,
                "mtime": mtime,
                "size": size,
            })
        })
        .collect())
}

// ── Helpers ──

fn rand_suffix() -> u32 {
    rand::random::<u32>()
}

fn deduplicate_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("/"));
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("file");
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{e}"))
        .unwrap_or_default();

    for i in 1..100 {
        let new_name = format!("{stem} ({i}){ext}");
        let new_path = parent.join(new_name);
        if !new_path.exists() {
            return new_path;
        }
    }
    path.to_path_buf()
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest).map_err(Error::Io)?;
    for entry in std::fs::read_dir(src).map_err(Error::Io)? {
        let entry = entry.map_err(Error::Io)?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dest_path)?;
        } else {
            std::fs::copy(&src_path, &dest_path).map_err(Error::Io)?;
        }
    }
    Ok(())
}

/// Natural sort comparison (Natives2 localeCompare with numeric: true).
/// Compares character-by-character without heap allocation.
/// - Text segments: case-insensitive comparison
/// - Numeric segments: numeric comparison (file2 < file10)
fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let mut a_iter = a.chars();
    let mut b_iter = b.chars();

    loop {
        let a_ch = a_iter.next();
        let b_ch = b_iter.next();

        match (a_ch, b_ch) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(ac), Some(bc)) => {
                if ac.is_ascii_digit() && bc.is_ascii_digit() {
                    // Numeric segment: consume all digits and compare as numbers
                    let mut a_num: u64 = (ac as u8 - b'0') as u64;
                    let mut b_num: u64 = (bc as u8 - b'0') as u64;
                    loop {
                        match a_iter.clone().next() {
                            Some(c) if c.is_ascii_digit() => {
                                a_num = a_num * 10 + (c as u8 - b'0') as u64;
                                a_iter.next();
                            }
                            _ => break,
                        }
                    }
                    loop {
                        match b_iter.clone().next() {
                            Some(c) if c.is_ascii_digit() => {
                                b_num = b_num * 10 + (c as u8 - b'0') as u64;
                                b_iter.next();
                            }
                            _ => break,
                        }
                    }
                    match a_num.cmp(&b_num) {
                        std::cmp::Ordering::Equal => continue,
                        other => return other,
                    }
                } else {
                    // Text segment: case-insensitive character comparison
                    let ac_lower = ac.to_lowercase().next().unwrap_or(ac);
                    let bc_lower = bc.to_lowercase().next().unwrap_or(bc);
                    match ac_lower.cmp(&bc_lower) {
                        std::cmp::Ordering::Equal => continue,
                        other => return other,
                    }
                }
            }
        }
    }
}
