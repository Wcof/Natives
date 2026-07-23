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
    /// Shallow project type for directories (node/web/python/rust/go/git).
    /// Mirrors fanbox `projectOf` so grid cards can show badges without N extra round-trips.
    #[serde(rename = "projectBadge", skip_serializing_if = "Option::is_none")]
    pub project_badge: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ReadFileResult {
    pub content: String,
    pub truncated: bool,
    pub size: u64,
    pub mtime: f64,
    pub kind: String,
    pub encoding: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WriteResult {
    pub mtime: f64,
    pub conflict: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListDirResult {
    pub path: String,
    pub parent: String,
    pub entries: Vec<FileEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
}

/// Wire format matches the frontend (`sortBy` / `sortDir` / … camelCase).
/// Without rename_all, camelCase options were silently dropped and list always
/// fell back to name/asc — Header sort clicks looked like “no feedback”.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListDirOptions {
    #[serde(default = "default_sort_by")]
    pub sort_by: String,
    #[serde(default = "default_sort_dir")]
    pub sort_dir: String,
    #[serde(default)]
    pub show_hidden: bool,
    /// When true, shallow-probe subdirectories (≤80) for project badges.
    #[serde(default = "default_probe_projects")]
    pub probe_projects: bool,
}

fn default_probe_projects() -> bool {
    true
}

impl Default for ListDirOptions {
    fn default() -> Self {
        Self {
            sort_by: default_sort_by(),
            sort_dir: default_sort_dir(),
            show_hidden: false,
            probe_projects: true,
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

/// Validate path security (allowlist: home, /tmp, /private/tmp, macOS per-user temp)
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

    // Allowlist: home + system temp locations (incl. macOS /var/folders/.../T)
    let sys_tmp = std::env::temp_dir();
    let sys_tmp_canon = std::fs::canonicalize(&sys_tmp).unwrap_or(sys_tmp);
    if canon.starts_with(&home)
        || canon.starts_with("/tmp")
        || canon.starts_with("/private/tmp")
        || canon.starts_with(&sys_tmp_canon)
        || canon.starts_with("/var/folders")
    {
        Ok(())
    } else {
        Err(Error::InvalidInput("path not in allowed directories".into()))
    }
}

/// Detect file kind from extension
fn detect_file_kind(name: &str) -> String {
    // Extensionless text files (Dockerfile / Makefile / README …)
    let base = Path::new(name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(name);
    if matches!(
        base,
        "Dockerfile"
            | "Makefile"
            | "Gemfile"
            | "Rakefile"
            | "CHANGELOG"
            | "README"
            | "LICENSE"
            | "VERSION"
            | "Procfile"
            | ".env"
            | ".gitignore"
            | ".dockerignore"
            | ".editorconfig"
    ) {
        return "text".to_string();
    }

    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "txt" | "md" | "mdx" | "markdown" | "json" | "jsonc" | "yaml" | "yml" | "toml" | "xml"
        | "csv" | "log" | "ini" | "cfg" | "conf" | "env" | "gitignore" | "dockerignore"
        | "editorconfig" | "graphql" | "gql" | "sql" | "vue" | "svelte" | "astro" => {
            "text".to_string()
        }
        "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "py" | "pyw" | "rb" | "rs" | "go" | "java"
        | "c" | "cpp" | "h" | "hpp" | "cs" | "swift" | "kt" | "kts" | "sh" | "bash" | "zsh"
        | "fish" | "ps1" | "bat" | "cmd" | "php" | "scala" => "text".to_string(),
        "html" | "htm" | "css" | "scss" | "sass" | "less" => "text".to_string(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "ico" | "bmp" | "tiff" | "tif"
        | "heic" | "avif" => "image".to_string(),
        "mp4" | "mov" | "avi" | "mkv" | "webm" | "flv" | "wmv" | "m4v" => "video".to_string(),
        "mp3" | "wav" | "ogg" | "flac" | "aac" | "m4a" | "opus" | "wma" => "audio".to_string(),
        "pdf" => "pdf".to_string(),
        "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "rar" | "tgz" | "tbz2" => "archive".to_string(),
        _ => "other".to_string(),
    }
}

/// Infer project type from a set of entry names (fanbox `projectOf`).
fn detect_project_badge(names: &std::collections::HashSet<String>) -> Option<String> {
    let lower: std::collections::HashSet<String> =
        names.iter().map(|n| n.to_lowercase()).collect();
    if lower.contains("package.json") {
        return Some("node".into());
    }
    if lower.contains("index.html") {
        return Some("web".into());
    }
    if lower.contains("requirements.txt")
        || lower.contains("setup.py")
        || lower.contains("pyproject.toml")
    {
        return Some("python".into());
    }
    if lower.contains("cargo.toml") {
        return Some("rust".into());
    }
    if lower.contains("go.mod") {
        return Some("go".into());
    }
    if names.contains(".git") || lower.contains(".git") {
        return Some("git".into());
    }
    None
}

fn meta_mtime_ms(meta: &std::fs::Metadata) -> f64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

fn meta_btime_ms(meta: &std::fs::Metadata) -> f64 {
    meta.created()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

/// Validate a bare file/folder name (no path separators).
pub fn valid_name(name: &str) -> bool {
    let n = name.trim();
    !n.is_empty()
        && n.len() <= 255
        && n != "."
        && n != ".."
        && !n.contains('/')
        && !n.contains('\\')
        && !n.contains('\0')
}

/// List directory contents (legacy Vec form — kept for callers that only need entries).
pub fn list_dir(dir_path: &str, options: &ListDirOptions) -> Result<Vec<FileEntry>> {
    Ok(list_dir_detailed(dir_path, options)?.entries)
}

/// List directory with parent/project metadata (fanbox-compatible shape).
pub fn list_dir_detailed(dir_path: &str, options: &ListDirOptions) -> Result<ListDirResult> {
    let path = expand_tilde(dir_path);
    let canon = std::fs::canonicalize(&path).map_err(Error::Io)?;

    let meta = std::fs::metadata(&canon).map_err(Error::Io)?;
    if !meta.is_dir() {
        return Err(Error::InvalidInput("not a directory".into()));
    }

    let mut entries = Vec::new();
    let mut name_set: std::collections::HashSet<String> = std::collections::HashSet::new();

    for entry in std::fs::read_dir(&canon).map_err(Error::Io)? {
        let entry = entry.map_err(Error::Io)?;
        let name = entry.file_name().to_string_lossy().to_string();
        name_set.insert(name.clone());

        // Filter .DS_Store always; hide other dotfiles unless show_hidden
        if name == ".DS_Store" {
            continue;
        }
        if !options.show_hidden && name.starts_with('.') {
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

        entries.push(FileEntry {
            name: name.clone(),
            path: entry_path.to_string_lossy().to_string(),
            is_dir,
            kind: if is_dir {
                "dir".to_string()
            } else {
                detect_file_kind(&name)
            },
            hidden: name.starts_with('.'),
            size,
            mtime: meta_mtime_ms(effective_meta),
            btime: meta_btime_ms(effective_meta),
            symlink: symlink_target,
            project_badge: None,
        });
    }

    let project = detect_project_badge(&name_set);

    // Shallow-probe subdirectories for project badges (fanbox: cap at 80 dirs)
    if options.probe_projects {
        let sub_dirs: Vec<usize> = entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.is_dir && !e.hidden)
            .map(|(i, _)| i)
            .collect();
        if sub_dirs.len() <= 80 {
            for idx in sub_dirs {
                let dir_path = PathBuf::from(&entries[idx].path);
                if let Ok(inner) = std::fs::read_dir(&dir_path) {
                    let mut inner_names = std::collections::HashSet::new();
                    for ent in inner.flatten() {
                        inner_names.insert(ent.file_name().to_string_lossy().to_string());
                    }
                    entries[idx].project_badge = detect_project_badge(&inner_names);
                }
            }
        }
    }

    // Sort: directories first, then by requested key
    let ascending = options.sort_dir == "asc";
    entries.sort_by(|a, b| {
        // Always keep directories before files for name sort (Finder-like)
        if options.sort_by == "name" && a.is_dir != b.is_dir {
            return if a.is_dir {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            };
        }
        let cmp = match options.sort_by.as_str() {
            "mtime" => a
                .mtime
                .partial_cmp(&b.mtime)
                .unwrap_or(std::cmp::Ordering::Equal),
            "size" => a.size.cmp(&b.size),
            _ => natural_cmp(&a.name, &b.name),
        };
        if ascending {
            cmp
        } else {
            cmp.reverse()
        }
    });

    let parent = canon
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| canon.to_string_lossy().to_string());

    Ok(ListDirResult {
        path: canon.to_string_lossy().to_string(),
        parent,
        entries,
        project,
    })
}

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
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");
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

    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
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

    let target = deduplicate_path(&new);
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

    let target = deduplicate_path(&dst);

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
    let target = deduplicate_path(&dst);

    if src.is_dir() {
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
    let target = deduplicate_path(&src);
    if src.is_dir() {
        copy_dir_recursive(&src, &target)?;
    } else {
        std::fs::copy(&src, &target).map_err(Error::Io)?;
    }
    Ok(target.to_string_lossy().to_string())
}

/// Lightweight exists/stat for path resolution (terminal locate, drop validation).
pub fn stat_path(file_path: &str) -> Result<serde_json::Value> {
    let path = expand_tilde(file_path);
    // Allow non-canonical paths: just check existence without allowlist on missing paths
    if !path.exists() {
        return Ok(serde_json::json!({
            "found": false,
            "path": path.to_string_lossy(),
        }));
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
    Ok(serde_json::json!({
        "found": true,
        "path": path.to_string_lossy(),
        "name": name,
        "isDir": is_dir,
        "kind": if is_dir { "dir".to_string() } else { detect_file_kind(&name) },
        "size": if is_dir { 4096 } else { effective.len() },
        "mtime": meta_mtime_ms(effective),
        "btime": meta_btime_ms(effective),
        "symlink": if is_symlink {
            std::fs::read_link(&path).ok().map(|p| p.to_string_lossy().to_string())
        } else {
            None
        },
    }))
}

fn is_exdev(err: &std::io::Error) -> bool {
    // macOS/Linux EXDEV = 18; also accept ErrorKind::CrossesDevices when available
    err.raw_os_error() == Some(18)
        || err.kind() == std::io::ErrorKind::CrossesDevices
}

/// Trash entry (macOS/Linux/Windows via trash crate)
pub fn trash_entry(file_path: &str) -> Result<()> {
    let path = expand_tilde(file_path);
    if !path.exists() {
        return Err(Error::NotFound(file_path.to_string()));
    }

    // Use trash crate
    trash::delete(&path).map_err(|e| Error::Internal(format!("trash failed: {e}")))
}

/// Batch trash. Continues on individual failures and reports them.
pub fn trash_entries(paths: &[String]) -> Result<serde_json::Value> {
    let mut ok: Vec<String> = Vec::new();
    let mut errors: Vec<serde_json::Value> = Vec::new();
    for p in paths {
        match trash_entry(p) {
            Ok(()) => ok.push(p.clone()),
            Err(e) => errors.push(serde_json::json!({ "path": p, "error": e.to_string() })),
        }
    }
    Ok(serde_json::json!({
        "ok": errors.is_empty(),
        "trashed": ok,
        "errors": errors,
        "count": ok.len(),
    }))
}

/// Batch move into a destination directory. Auto-dedupe, cross-volume safe.
pub fn move_entries(paths: &[String], dest_dir: &str) -> Result<serde_json::Value> {
    let dest = expand_tilde(dest_dir);
    if !dest.exists() {
        std::fs::create_dir_all(&dest).map_err(Error::Io)?;
    }
    validate_path(&dest)?;
    if !dest.is_dir() {
        return Err(Error::InvalidInput("destination is not a directory".into()));
    }

    let mut moved: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut errors: Vec<serde_json::Value> = Vec::new();
    let dest_canon = std::fs::canonicalize(&dest).unwrap_or_else(|_| dest.clone());
    for p in paths {
        let src = expand_tilde(p);
        // Skip no-op: already directly inside dest
        if let Some(parent) = src.parent() {
            let parent_canon = std::fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());
            if parent_canon == dest_canon {
                skipped.push(p.clone());
                continue;
            }
        }
        // Prevent moving a directory into itself / its descendant
        if src.is_dir() {
            let src_canon = std::fs::canonicalize(&src).unwrap_or_else(|_| src.clone());
            if dest_canon.starts_with(&src_canon) {
                errors.push(serde_json::json!({ "path": p, "error": "cannot move a folder into itself" }));
                continue;
            }
        }
        match move_entry(p, dest.to_string_lossy().as_ref()) {
            Ok(path) => moved.push(path),
            Err(e) => errors.push(serde_json::json!({ "path": p, "error": e.to_string() })),
        }
    }
    Ok(serde_json::json!({
        "ok": errors.is_empty(),
        "moved": moved,
        "skipped": skipped,
        "errors": errors,
        "count": moved.len(),
    }))
}

/// Batch copy into a destination directory. Auto-dedupe. Source preserved.
pub fn copy_entries(paths: &[String], dest_dir: &str) -> Result<serde_json::Value> {
    let dest = expand_tilde(dest_dir);
    if !dest.exists() {
        std::fs::create_dir_all(&dest).map_err(Error::Io)?;
    }
    validate_path(&dest)?;
    if !dest.is_dir() {
        return Err(Error::InvalidInput("destination is not a directory".into()));
    }

    let mut copied: Vec<String> = Vec::new();
    let mut errors: Vec<serde_json::Value> = Vec::new();
    for p in paths {
        match copy_entry(p, dest.to_string_lossy().as_ref()) {
            Ok(path) => copied.push(path),
            Err(e) => errors.push(serde_json::json!({ "path": p, "error": e.to_string() })),
        }
    }
    Ok(serde_json::json!({
        "ok": errors.is_empty(),
        "copied": copied,
        "errors": errors,
        "count": copied.len(),
    }))
}

/// Default quick-access roots (fanbox `/api/roots` equivalent).
pub fn default_roots() -> Result<Vec<serde_json::Value>> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    let candidates: Vec<(&str, PathBuf)> = vec![
        ("home", home.clone()),
        ("desktop", home.join("Desktop")),
        ("documents", home.join("Documents")),
        ("downloads", home.join("Downloads")),
        ("pictures", home.join("Pictures")),
        ("movies", home.join("Movies")),
        ("music", home.join("Music")),
    ];

    let mut roots = Vec::new();
    for (id, path) in candidates {
        if path.is_dir() {
            roots.push(serde_json::json!({
                "id": id,
                "name": match id {
                    "home" => "Home",
                    "desktop" => "Desktop",
                    "documents" => "Documents",
                    "downloads" => "Downloads",
                    "pictures" => "Pictures",
                    "movies" => "Movies",
                    "music" => "Music",
                    _ => id,
                },
                "path": path.to_string_lossy(),
            }));
        }
    }
    // Always include /tmp if present
    for tmp in ["/tmp", "/private/tmp"] {
        let p = PathBuf::from(tmp);
        if p.is_dir() {
            roots.push(serde_json::json!({
                "id": "tmp",
                "name": "tmp",
                "path": p.to_string_lossy(),
            }));
            break;
        }
    }
    Ok(roots)
}

/// Open path with a preferred app (fanbox `/api/open`).
/// `with`: "default" | "reveal" | "terminal" | "editor"
pub fn open_with(target: &str, with: &str) -> Result<serde_json::Value> {
    let path = expand_tilde(target);
    if !path.exists() {
        return Err(Error::NotFound(target.to_string()));
    }

    match with {
        "reveal" => {
            #[cfg(target_os = "macos")]
            {
                std::process::Command::new("open")
                    .args(["-R", path.to_string_lossy().as_ref()])
                    .spawn()
                    .map_err(|e| Error::Internal(e.to_string()))?;
            }
            #[cfg(target_os = "windows")]
            {
                std::process::Command::new("explorer")
                    .args(["/select,", path.to_string_lossy().as_ref()])
                    .spawn()
                    .map_err(|e| Error::Internal(e.to_string()))?;
            }
            #[cfg(all(unix, not(target_os = "macos")))]
            {
                let parent = path
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| path.clone());
                open::that(parent).map_err(|e| Error::Internal(e.to_string()))?;
            }
            Ok(serde_json::json!({ "ok": true, "with": "reveal" }))
        }
        "terminal" => {
            let dir = if path.is_dir() {
                path.clone()
            } else {
                path.parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| path.clone())
            };
            #[cfg(target_os = "macos")]
            {
                // Fixed argv — no shell.
                std::process::Command::new("open")
                    .args(["-a", "Terminal", dir.to_string_lossy().as_ref()])
                    .spawn()
                    .map_err(|e| Error::Internal(e.to_string()))?;
            }
            #[cfg(target_os = "windows")]
            {
                // Prefer Windows Terminal with independent argv; fall back to Explorer.
                let dir_s = dir.to_string_lossy().to_string();
                let wt = std::process::Command::new("wt.exe")
                    .args(["-d", &dir_s])
                    .spawn();
                if wt.is_err() {
                    std::process::Command::new("explorer")
                        .arg(&dir_s)
                        .spawn()
                        .map_err(|e| Error::Internal(e.to_string()))?;
                }
            }
            #[cfg(all(unix, not(target_os = "macos")))]
            {
                let dir_s = dir.to_string_lossy().to_string();
                // Prefer x-terminal-emulator with working-directory flag when available.
                let tried = std::process::Command::new("x-terminal-emulator")
                    .args(["--working-directory", &dir_s])
                    .spawn();
                if tried.is_err() {
                    let gnome = std::process::Command::new("gnome-terminal")
                        .args([format!("--working-directory={dir_s}")])
                        .spawn();
                    if gnome.is_err() {
                        open::that(&dir).map_err(|e| Error::Internal(e.to_string()))?;
                    }
                }
            }
            Ok(serde_json::json!({ "ok": true, "with": "terminal" }))
        }
        "editor" => {
            // Prefer VS Code CLI, fall back to default opener.
            let path_s = path.to_string_lossy().to_string();
            match std::process::Command::new("code").arg(&path_s).spawn() {
                Ok(_child) => {
                    // Detach: don't wait. If spawn succeeded we're good.
                    Ok(serde_json::json!({ "ok": true, "with": "editor" }))
                }
                Err(_) => {
                    open::that(&path).map_err(|e| Error::Internal(e.to_string()))?;
                    Ok(serde_json::json!({ "ok": true, "with": "default" }))
                }
            }
        }
        _ => {
            open::that(&path).map_err(|e| Error::Internal(e.to_string()))?;
            Ok(serde_json::json!({ "ok": true, "with": "default" }))
        }
    }
}

/// Put file paths on the system pasteboard so Finder/Explorer can paste them.
/// macOS: AppleScript `set the clipboard to … as «class furl»` via osascript.
pub fn clipboard_copy_files(paths: &[String]) -> Result<serde_json::Value> {
    if paths.is_empty() {
        return Err(Error::InvalidInput("no paths".into()));
    }
    // Validate all paths exist & allowed
    let mut abs: Vec<PathBuf> = Vec::new();
    for p in paths {
        let path = expand_tilde(p);
        if !path.exists() {
            return Err(Error::NotFound(p.clone()));
        }
        validate_path(&path)?;
        abs.push(path);
    }

    #[cfg(target_os = "macos")]
    {
        // Build AppleScript list of POSIX files.
        // Use argv-style osascript to avoid quote injection.
        // osascript -e 'on run argv' -e 'set the clipboard to (POSIX file (item 1 of argv) as alias)' ...
        // Multi-file: set the clipboard to {POSIX file a as alias, POSIX file b as alias}
        let mut script = String::from("on run argv\nset fileList to {}\n");
        script.push_str("repeat with a in argv\n");
        script.push_str("set end of fileList to (POSIX file a as alias)\n");
        script.push_str("end repeat\n");
        script.push_str("set the clipboard to fileList\nend run\n");

        let mut cmd = std::process::Command::new("osascript");
        cmd.arg("-e").arg(&script);
        for p in &abs {
            cmd.arg(p.to_string_lossy().as_ref());
        }
        let output = cmd
            .output()
            .map_err(|e| Error::Internal(format!("osascript failed: {e}")))?;
        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            return Err(Error::Internal(format!("clipboard copy files failed: {err}")));
        }
        return Ok(serde_json::json!({ "ok": true, "count": abs.len(), "platform": "macos" }));
    }

    #[cfg(not(target_os = "macos"))]
    {
        // Fallback: put newline-joined paths as text (better than nothing)
        let text = abs
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("\n");
        // Reuse pbcopy/xclip via std process when available
        #[cfg(target_os = "linux")]
        {
            use std::io::Write;
            let mut child = std::process::Command::new("xclip")
                .args(["-selection", "clipboard"])
                .stdin(std::process::Stdio::piped())
                .spawn()
                .or_else(|_| {
                    std::process::Command::new("xsel")
                        .args(["--clipboard", "--input"])
                        .stdin(std::process::Stdio::piped())
                        .spawn()
                })
                .map_err(|e| Error::Internal(format!("clipboard tool failed: {e}")))?;
            if let Some(stdin) = child.stdin.as_mut() {
                let _ = stdin.write_all(text.as_bytes());
            }
            let _ = child.wait();
        }
        #[cfg(target_os = "windows")]
        {
            // PowerShell Set-Clipboard
            let ps = format!("Set-Clipboard -Value @'\n{text}\n'@");
            std::process::Command::new("powershell")
                .args(["-NoProfile", "-Command", &ps])
                .spawn()
                .map_err(|e| Error::Internal(e.to_string()))?;
        }
        Ok(serde_json::json!({
            "ok": true,
            "count": abs.len(),
            "platform": std::env::consts::OS,
            "mode": "text-paths"
        }))
    }
}

/// Copy an image file onto the pasteboard as image data (macOS).
pub fn clipboard_copy_image(file_path: &str) -> Result<serde_json::Value> {
    let path = expand_tilde(file_path);
    validate_path(&path)?;
    if !path.exists() {
        return Err(Error::NotFound(file_path.to_string()));
    }
    let kind = detect_file_kind(
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(""),
    );
    if kind != "image" {
        return Err(Error::InvalidInput("not an image file".into()));
    }

    #[cfg(target_os = "macos")]
    {
        // osascript: set the clipboard to (read POSIX file "..." as «class PNGf»/JPEG)
        // Use generic picture data via Finder/System Events is fragile; use `osascript` + `read … as TIFF picture`
        let script = r#"
on run argv
  set p to item 1 of argv
  set the clipboard to (read (POSIX file p) as «class PNGf»)
end run
"#;
        let output = std::process::Command::new("osascript")
            .arg("-e")
            .arg(script)
            .arg(path.to_string_lossy().as_ref())
            .output()
            .map_err(|e| Error::Internal(format!("osascript failed: {e}")))?;
        if !output.status.success() {
            // Fallback: try TIFF
            let script2 = r#"
on run argv
  set p to item 1 of argv
  set the clipboard to (read (POSIX file p) as TIFF picture)
end run
"#;
            let output2 = std::process::Command::new("osascript")
                .arg("-e")
                .arg(script2)
                .arg(path.to_string_lossy().as_ref())
                .output()
                .map_err(|e| Error::Internal(format!("osascript failed: {e}")))?;
            if !output2.status.success() {
                let err = String::from_utf8_lossy(&output2.stderr);
                return Err(Error::Internal(format!("copy image failed: {err}")));
            }
        }
        return Ok(serde_json::json!({ "ok": true, "path": path.to_string_lossy() }));
    }

    #[cfg(not(target_os = "macos"))]
    {
        // Best-effort: copy path text
        let _ = file_path;
        Err(Error::NotImplemented(
            "clipboard image copy not implemented on this platform".into(),
        ))
    }
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

// ── Unit tests (pure helpers; no real filesystem side effects beyond temp dirs) ──

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "natives-fm-{}-{}-{}",
            label,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn natural_cmp_numeric_order() {
        assert_eq!(natural_cmp("file2", "file10"), std::cmp::Ordering::Less);
        assert_eq!(natural_cmp("file10", "file2"), std::cmp::Ordering::Greater);
        assert_eq!(natural_cmp("a", "b"), std::cmp::Ordering::Less);
    }

    #[test]
    fn valid_name_rejects_path_sep_and_dots() {
        assert!(valid_name("readme.md"));
        assert!(!valid_name(""));
        assert!(!valid_name("."));
        assert!(!valid_name(".."));
        assert!(!valid_name("a/b"));
        assert!(!valid_name("a\\b"));
        assert!(!valid_name("a\0b"));
    }

    #[test]
    fn detect_project_badge_priority() {
        let mut names = std::collections::HashSet::new();
        names.insert("package.json".into());
        names.insert("Cargo.toml".into());
        assert_eq!(detect_project_badge(&names).as_deref(), Some("node"));

        names.clear();
        names.insert("Cargo.toml".into());
        assert_eq!(detect_project_badge(&names).as_deref(), Some("rust"));

        names.clear();
        names.insert(".git".into());
        assert_eq!(detect_project_badge(&names).as_deref(), Some("git"));
    }

    #[test]
    fn detect_file_kind_extensionless_and_images() {
        assert_eq!(detect_file_kind("Dockerfile"), "text");
        assert_eq!(detect_file_kind("Makefile"), "text");
        assert_eq!(detect_file_kind("photo.PNG"), "image");
        assert_eq!(detect_file_kind("archive.zip"), "archive");
        assert_eq!(detect_file_kind("notes.md"), "text");
    }

    #[test]
    fn create_rename_copy_duplicate_roundtrip() {
        let dir = tmp_dir("roundtrip");
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
        // Ensure path is under allowlist (home or /tmp)
        let base = if dir.starts_with(&home) || dir.starts_with("/tmp") || dir.starts_with("/private/tmp") {
            dir.clone()
        } else {
            let fallback = std::env::temp_dir().join(format!("natives-fm-home-{}", std::process::id()));
            let _ = std::fs::create_dir_all(&fallback);
            fallback
        };

        let file = base.join("hello.txt");
        create_entry(file.to_str().unwrap(), "file").expect("create file");
        assert!(file.exists());

        let renamed = rename_entry(
            file.to_str().unwrap(),
            base.join("hello-renamed.txt").to_str().unwrap(),
        )
        .expect("rename");
        assert!(Path::new(&renamed).exists());
        assert!(!file.exists());

        let copied = copy_entry(
            &renamed,
            base.join("hello-copy.txt").to_str().unwrap(),
        )
        .expect("copy");
        assert!(Path::new(&copied).exists());
        assert!(Path::new(&renamed).exists());

        let dup = duplicate_entry(&renamed).expect("duplicate");
        assert!(Path::new(&dup).exists());
        assert_ne!(dup, renamed);

        let sub = base.join("sub");
        create_entry(sub.to_str().unwrap(), "folder").expect("create folder");
        assert!(sub.is_dir());

        // folder type alias
        let sub2 = base.join("sub2");
        create_entry(sub2.to_str().unwrap(), "dir").expect("create dir");
        assert!(sub2.is_dir());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn list_dir_detailed_includes_project_and_sorts_dirs_first() {
        let base = tmp_dir("listdetail");
        // Write under /tmp which is allowlisted
        let f = base.join("b.txt");
        let mut file = std::fs::File::create(&f).unwrap();
        file.write_all(b"x").unwrap();
        let d = base.join("a-dir");
        std::fs::create_dir(&d).unwrap();
        // project marker
        std::fs::write(base.join("package.json"), b"{}").unwrap();

        let result = list_dir_detailed(
            base.to_str().unwrap(),
            &ListDirOptions {
                sort_by: "name".into(),
                sort_dir: "asc".into(),
                show_hidden: false,
                probe_projects: false,
            },
        )
        .expect("list");
        assert_eq!(result.project.as_deref(), Some("node"));
        // directories first
        assert!(result.entries[0].is_dir, "first entry should be a dir");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Frontend FileBrowser sends camelCase options via Tauri IPC.
    /// Regression: without rename_all these fields were ignored → default sort only.
    #[test]
    fn list_dir_options_accepts_frontend_camel_case_json() {
        let v = serde_json::json!({
            "sortBy": "size",
            "sortDir": "desc",
            "showHidden": true,
            "probeProjects": false
        });
        let opts: ListDirOptions = serde_json::from_value(v).expect("deserialize camelCase");
        assert_eq!(opts.sort_by, "size");
        assert_eq!(opts.sort_dir, "desc");
        assert!(opts.show_hidden);
        assert!(!opts.probe_projects);
    }

    #[test]
    fn list_dir_sorts_by_size_and_mtime() {
        let base = tmp_dir("sortkeys");
        let small = base.join("small.txt");
        let large = base.join("large.txt");
        std::fs::write(&small, b"a").unwrap();
        std::fs::write(&large, b"aaaaaaaaaa").unwrap();

        // Rewrite small after a brief delay so mtime-desc puts it first.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&small, b"ab").unwrap();

        let by_size = list_dir_detailed(
            base.to_str().unwrap(),
            &ListDirOptions {
                sort_by: "size".into(),
                sort_dir: "desc".into(),
                show_hidden: false,
                probe_projects: false,
            },
        )
        .expect("list size");
        let size_names: Vec<&str> = by_size.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(size_names, vec!["large.txt", "small.txt"], "size desc");

        let by_mtime = list_dir_detailed(
            base.to_str().unwrap(),
            &ListDirOptions {
                sort_by: "mtime".into(),
                sort_dir: "desc".into(),
                show_hidden: false,
                probe_projects: false,
            },
        )
        .expect("list mtime");
        assert_eq!(
            by_mtime.entries[0].name, "small.txt",
            "mtime desc should put rewritten small first"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn read_file_returns_mtime_and_kind() {
        let base = tmp_dir("readmeta");
        let f = base.join("note.md");
        std::fs::write(&f, b"# hi").unwrap();
        let r = read_file(f.to_str().unwrap()).expect("read");
        assert_eq!(r.content, "# hi");
        assert_eq!(r.kind, "text");
        assert!(!r.truncated);
        assert!(r.mtime > 0.0);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn stat_path_missing_and_found() {
        let missing = stat_path("/tmp/natives-definitely-missing-xyz-12345").unwrap();
        assert_eq!(missing["found"], false);

        let base = tmp_dir("stat");
        let f = base.join("x.txt");
        std::fs::write(&f, b"1").unwrap();
        let found = stat_path(f.to_str().unwrap()).unwrap();
        assert_eq!(found["found"], true);
        assert_eq!(found["isDir"], false);
        assert_eq!(found["name"], "x.txt");
        let _ = std::fs::remove_dir_all(&base);
    }


    #[test]
    fn batch_copy_move_trash_and_roots() {
        let base = tmp_dir("batch");
        let a = base.join("a.txt");
        let b = base.join("b.txt");
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        let dest = base.join("out");
        std::fs::create_dir(&dest).unwrap();

        let copied = copy_entries(
            &[a.to_string_lossy().to_string(), b.to_string_lossy().to_string()],
            dest.to_str().unwrap(),
        )
        .unwrap();
        assert_eq!(copied["count"], 2);
        assert!(dest.join("a.txt").exists());

        let dest2 = base.join("out2");
        std::fs::create_dir(&dest2).unwrap();
        let moved = move_entries(
            &[dest.join("a.txt").to_string_lossy().to_string()],
            dest2.to_str().unwrap(),
        )
        .unwrap();
        assert_eq!(moved["count"], 1);
        assert!(dest2.join("a.txt").exists());
        assert!(!dest.join("a.txt").exists());

        let roots = default_roots().unwrap();
        assert!(!roots.is_empty());
        // open_with default on a real file should not panic (may fail headless)
        let _ = open_with(b.to_str().unwrap(), "default");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    #[test]
    fn deduplicate_path_adds_counter() {
        let base = tmp_dir("dedupe");
        let f = base.join("doc.txt");
        std::fs::write(&f, b"a").unwrap();
        let next = deduplicate_path(&f);
        assert_eq!(next.file_name().unwrap().to_str().unwrap(), "doc (1).txt");
        let _ = std::fs::remove_dir_all(&base);
    }
}
