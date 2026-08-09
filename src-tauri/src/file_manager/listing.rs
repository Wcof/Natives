//! Directory listing, entry metadata, and "recent files" (read side).

use super::*;
use crate::{Error, Result};
use std::path::PathBuf;

/// List directory contents (legacy Vec form — kept for callers that only need entries).
pub fn list_dir(dir_path: &str, options: &ListDirOptions) -> Result<Vec<FileEntry>> {
    Ok(list_dir_detailed(dir_path, options)?.entries)
}

/// List directory with parent/project metadata (fanbox-compatible shape).
pub fn list_dir_detailed(dir_path: &str, options: &ListDirOptions) -> Result<ListDirResult> {
    let path = expand_tilde(dir_path);
    let canon = std::fs::canonicalize(&path).map_err(Error::Io)?;
    // Enforce the same allowlist as read/write. `canon` is already resolved, so
    // normal home-subdir browsing is unaffected; only blocklisted/out-of-scope
    // roots are rejected. Keeps the listing boundary consistent with I/O.
    validate_path(&canon)?;

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
        let size = if is_dir { 4096 } else { effective_meta.len() };

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
            dir_hint: None,
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

/// Get recently modified files (BFS walk, top 60 by mtime).
/// 返回完整 FileEntry 契约（name/kind/hidden/dirHint 由后端补齐，
/// 前端「最近修改」视图不再本地计算 kind）。
pub fn recent_files(root: &str) -> Result<Vec<FileEntry>> {
    let path = expand_tilde(root);
    let canon = std::fs::canonicalize(&path).map_err(Error::Io)?;
    // Consistent boundary with list_dir/read: reject blocklisted/out-of-scope
    // roots. `canon` is resolved so legitimate home subdirs still pass.
    validate_path(&canon)?;

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

    let mut files: Vec<FileEntry> = Vec::new();
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
                // dirHint：来源目录，供「最近修改」视图展示（前端不再自行推导）
                let dir_hint = entry_path.parent().map(|p| p.to_string_lossy().to_string());
                files.push(FileEntry {
                    kind: detect_file_kind(&name),
                    hidden: name.starts_with('.'),
                    name,
                    path: entry_path.to_string_lossy().to_string(),
                    is_dir: false,
                    size: meta.len(),
                    mtime: meta_mtime_ms(&meta),
                    btime: meta_btime_ms(&meta),
                    symlink: None,
                    project_badge: None,
                    dir_hint,
                });
            }
        }
    }

    // Sort by mtime descending, take top 60
    files.sort_by(|a, b| {
        b.mtime
            .partial_cmp(&a.mtime)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    files.truncate(60);

    Ok(files)
}
