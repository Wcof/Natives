//! Unified host file authorization + file browsing / manipulation for the
//! Renderer. Split by responsibility:
//!   - `mod.rs` — wire types, authorization kernel (FileAccessPolicy /
//!     AuthorizedPath), path / name validation
//!   - `detect` — kind / project-badge / metadata / natural-sort helpers
//!   - `listing` — directory listing, entry metadata, recent files
//!   - `ops` — single-file read/write/create/rename/move/copy/duplicate/import
//!   - `trash` — trash + bulk batch operations
//!   - `system` — roots / open-with / clipboard OS integration
//!
//! TS 类型生成落点：`export_to` 相对默认 export 目录 `src-tauri/bindings` 解析，
//! 实际落到仓库根的 `src/types/generated/`。
//! 生成方式：`npm run types:generate`（即 cargo test export_bindings）。

use crate::{Error, Result};

use serde::{Deserialize, Serialize};

use std::path::{Component, Path, PathBuf};

use ts_rs::TS;

const MAX_FULL_READ: u64 = 2 * 1024 * 1024; // 2MB

const MAX_TRUNCATED_READ: u64 = 256 * 1024; // 256KB

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    #[serde(rename = "isDir")]
    pub is_dir: bool,
    // kind 值域：detect_file_kind 的产出 + 目录 "dir"。改这里必须同步改 detect_file_kind。
    #[ts(
        type = "\"text\" | \"image\" | \"video\" | \"audio\" | \"pdf\" | \"archive\" | \"dir\" | \"other\""
    )]
    pub kind: String,
    pub hidden: bool,
    // u64 默认生成 bigint，前端契约是 number（毫秒/字节都在安全整数范围内）
    #[ts(type = "number")]
    pub size: u64,
    pub mtime: f64,
    pub btime: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub symlink: Option<String>,
    /// Shallow project type for directories (node/web/python/rust/go/git).
    /// Mirrors fanbox `projectOf` so grid cards can show badges without N extra round-trips.
    /// 值域由 detect_project_badge 决定，改这里必须同步改该函数。
    #[serde(rename = "projectBadge", skip_serializing_if = "Option::is_none")]
    #[ts(
        optional,
        type = "\"node\" | \"web\" | \"python\" | \"rust\" | \"go\" | \"git\""
    )]
    pub project_badge: Option<String>,
    /// 文件所在目录（「最近修改」视图显示来源目录用；recent_files/stat 填充）
    #[serde(rename = "dirHint", skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub dir_hint: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct ReadFileResult {
    pub content: String,
    pub truncated: bool,
    #[ts(type = "number")]
    pub size: u64,
    pub mtime: f64,
    pub kind: String,
    pub encoding: String,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct WriteResult {
    pub mtime: f64,
    pub conflict: bool,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct ListDirResult {
    pub path: String,
    pub parent: String,
    pub entries: Vec<FileEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub project: Option<String>,
}

/// stat 结果（fs_stat 命令返回）。`found == false` 时仅 `path` 有效，其余字段缺省。
#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
#[serde(rename_all = "camelCase")]
pub struct StatResult {
    pub found: bool,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub is_dir: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(
        optional,
        type = "\"text\" | \"image\" | \"video\" | \"audio\" | \"pdf\" | \"archive\" | \"dir\" | \"other\""
    )]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub mtime: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub btime: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub symlink: Option<String>,
    /// 所在目录（与 FileEntry.dirHint 语义一致）
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub dir_hint: Option<String>,
}

/// Wire format matches the frontend (`sortBy` / `sortDir` / … camelCase).
/// Without rename_all, camelCase options were silently dropped and list always
/// fell back to name/asc — Header sort clicks looked like “no feedback”.
#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
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
/// （archive_ops / image_convert / locate 等兄弟模块复用，保持路径语义一致）
pub(crate) fn expand_tilde(path: &str) -> PathBuf {
    if path.starts_with("~/") || path == "~" {
        if let Some(home) = dirs::home_dir() {
            return home.join(path.strip_prefix("~/").unwrap_or(""));
        }
    }
    PathBuf::from(path)
}

/// Validate path security (allowlist: home, /tmp, /private/tmp, macOS per-user temp)
pub(crate) fn validate_path(path: &Path) -> Result<()> {
    FileAccessPolicy::authorize_path_buf(path, OperationPolicy::Read).map(|_| ())
}

/// Operation classes for the unified file authorization kernel. Every
/// caller-controlled path must be classified into one of these before any
/// concrete operation (read / write / reveal / preview / search / …) runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationPolicy {
    Read,
    Write,
    Reveal,
    Preview,
    Search,
}

/// Authorized handle: the canonicalized, allowlisted path an operation may
/// actually touch. Concrete operations take this handle, never the raw caller
/// string, so a caller cannot re-route an operation to an unauthorized target
/// between authorization and use.
#[derive(Debug, Clone)]
pub struct AuthorizedPath {
    path: PathBuf,
}

impl AuthorizedPath {
    pub fn as_path(&self) -> &Path {
        &self.path
    }
}

/// Unified host file authorization (T108): canonicalize → root allow/deny +
/// symlink boundary → operation policy → AuthorizedPath handle. screenshot /
/// search / thumbnail / open / reveal / preview / agent attachment all reuse
/// this kernel; no caller-controlled path may bypass it.
pub struct FileAccessPolicy;

impl FileAccessPolicy {
    pub fn authorize_path(path: &str, _op: OperationPolicy) -> Result<AuthorizedPath> {
        Self::authorize_path_buf(&PathBuf::from(path), _op)
    }

    pub fn authorize_path_buf(path: &Path, _op: OperationPolicy) -> Result<AuthorizedPath> {
        let path_str = path.to_string_lossy();
        if path_str.contains('\0') {
            return Err(Error::InvalidInput("path contains null byte".into()));
        }

        // Reject any `..` component before allowlist checks (same rationale as
        // the old validate_path: canonicalize falls back to the raw path for
        // non-existent targets, and a literal `..` would slip past
        // starts_with). This is the symlink/parent-escape boundary.
        if path.components().any(|c| c == Component::ParentDir) {
            return Err(Error::InvalidInput("path must not contain '..'".into()));
        }

        let canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));

        // Blocklist: sensitive dotfiles (unchanged from validate_path).
        let blocked = [".ssh", ".gnupg", ".aws", ".config/gh", ".kube"];
        for b in &blocked {
            if canon.ends_with(b) || canon.to_string_lossy().contains(&format!("/{b}/")) {
                return Err(Error::InvalidInput(format!("access denied: {b}")));
            }
        }

        // Allowlist: home + system temp locations (incl. macOS /var/folders/.../T).
        let sys_tmp = std::env::temp_dir();
        let sys_tmp_canon = std::fs::canonicalize(&sys_tmp).unwrap_or(sys_tmp);
        if canon.starts_with(&home)
            || canon.starts_with("/tmp")
            || canon.starts_with("/private/tmp")
            || canon.starts_with(&sys_tmp_canon)
            || canon.starts_with("/var/folders")
        {
            Ok(AuthorizedPath { path: canon })
        } else {
            Err(Error::InvalidInput(
                "path not in allowed directories".into(),
            ))
        }
    }
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

mod detect;
mod listing;
mod ops;
mod system;
mod trash;

pub(crate) use detect::*;
pub use listing::{list_dir, list_dir_detailed, recent_files};
pub(crate) use ops::deduplicate_path;
pub use ops::{
    copy_entry, create_entry, duplicate_entry, import_files, move_entry, read_file, rename_entry,
    stat_path, write_file_atomic,
};
pub use system::{clipboard_copy_files, clipboard_copy_image, default_roots, open_with};
pub use trash::{copy_entries, move_entries, trash_entries, trash_entry};

#[cfg(test)]
mod file_manager_tests;
