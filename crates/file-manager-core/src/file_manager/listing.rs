//! Directory listing, entry metadata, and "recent files" (read side).

use super::*;
use crate::{Error, Result};
use std::path::PathBuf;

#[derive(Debug, serde::Serialize)]
pub struct DiskUsageResult {
    pub path: String,
    pub bytes: u64,
    pub files: u64,
    pub directories: u64,
    pub truncated: bool,
    pub errors: u64,
    pub items: Vec<DiskUsageItem>,
}

#[derive(Debug, serde::Serialize)]
pub struct DiskUsageItem {
    pub name: String,
    pub size: u64,
    #[serde(rename = "isDir")]
    pub is_dir: bool,
}

fn deferred_home_directory_entry(name: &str, path: &std::path::Path) -> Option<FileEntry> {
    is_macos_privacy_protected_home_child(path).then(|| FileEntry {
        name: name.to_string(),
        path: path.to_string_lossy().to_string(),
        is_dir: true,
        kind: "dir".into(),
        hidden: name.starts_with('.'),
        size: 4096,
        mtime: 0.0,
        btime: 0.0,
        symlink: None,
        project_badge: None,
        dir_hint: None,
    })
}

/// On-demand, bounded recursive usage scan. Symlinks are counted as entries,
/// never followed, so a scan cannot escape the authorized tree.
pub fn disk_usage(dir_path: &str) -> Result<DiskUsageResult> {
    let canon = std::fs::canonicalize(expand_tilde(dir_path)).map_err(Error::Io)?;
    validate_path(&canon)?;
    if !std::fs::metadata(&canon).map_err(Error::Io)?.is_dir() {
        return Err(Error::InvalidInput("not a directory".into()));
    }
    const MAX_ENTRIES: u64 = 100_000;
    let mut stack = vec![(canon.clone(), None::<String>)];
    let mut out = DiskUsageResult {
        path: canon.to_string_lossy().into(),
        bytes: 0,
        files: 0,
        directories: 0,
        truncated: false,
        errors: 0,
        items: Vec::new(),
    };
    let mut item_sizes = std::collections::BTreeMap::<String, (u64, bool)>::new();
    while let Some((dir, child)) = stack.pop() {
        out.directories += 1;
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => {
                out.errors += 1;
                continue;
            }
        };
        for entry in entries {
            if out.files + out.directories >= MAX_ENTRIES {
                out.truncated = true;
                break;
            }
            let entry = match entry {
                Ok(v) => v,
                Err(_) => {
                    out.errors += 1;
                    continue;
                }
            };
            let entry_path = entry.path();
            if is_macos_privacy_protected_home_child(&entry_path) {
                out.errors += 1;
                continue;
            }
            let meta = match std::fs::symlink_metadata(&entry_path) {
                Ok(v) => v,
                Err(_) => {
                    out.errors += 1;
                    continue;
                }
            };
            if meta.file_type().is_symlink() {
                out.files += 1;
                out.bytes = out.bytes.saturating_add(meta.len());
                if let Some(name) = &child {
                    let slot = item_sizes.entry(name.clone()).or_insert((0, false));
                    slot.0 = slot.0.saturating_add(meta.len());
                }
                continue;
            }
            if meta.is_dir() {
                let name = child
                    .clone()
                    .unwrap_or_else(|| entry.file_name().to_string_lossy().into());
                item_sizes.entry(name.clone()).or_insert((0, true));
                stack.push((entry_path, Some(name)));
            } else {
                out.files += 1;
                out.bytes = out.bytes.saturating_add(meta.len());
                let name = child
                    .clone()
                    .unwrap_or_else(|| entry.file_name().to_string_lossy().into());
                let slot = item_sizes.entry(name).or_insert((0, false));
                slot.0 = slot.0.saturating_add(meta.len());
            }
        }
        if out.truncated {
            break;
        }
    }
    out.items = item_sizes
        .into_iter()
        .map(|(name, (size, is_dir))| DiskUsageItem { name, size, is_dir })
        .collect();
    out.items.sort_by(|a, b| b.size.cmp(&a.size));
    out.items.truncate(60);
    Ok(out)
}

/// List directory contents (legacy Vec form — kept for callers that only need entries).
pub fn list_dir(dir_path: &str, options: &ListDirOptions) -> Result<Vec<FileEntry>> {
    Ok(list_dir_detailed(dir_path, options)?.entries)
}

/// Bounded page for Native Messaging: retain only the requested window plus
/// one sentinel entry while scanning the directory stream.
pub fn list_dir_page(dir_path: &str, offset: usize, limit: usize) -> Result<(ListDirResult, bool)> {
    list_dir_page_with_options(dir_path, offset, limit, &ListDirOptions::default())
}

/// Bounded, option-aware directory page. The scan retains at most the page
/// window plus one sentinel, so a large directory never becomes an unbounded
/// response or allocation in the Native Messaging process.
pub fn list_dir_page_with_options(
    dir_path: &str,
    offset: usize,
    limit: usize,
    options: &ListDirOptions,
) -> Result<(ListDirResult, bool)> {
    let path = expand_tilde(dir_path);
    let canon = std::fs::canonicalize(&path).map_err(Error::Io)?;
    validate_path(&canon)?;
    if !std::fs::metadata(&canon).map_err(Error::Io)?.is_dir() {
        return Err(Error::InvalidInput("not a directory".into()));
    }
    const MAX_PAGE_WINDOW: usize = 100_001;
    let cap = offset
        .saturating_add(limit)
        .saturating_add(1)
        .min(MAX_PAGE_WINDOW);
    let mut selected = Vec::with_capacity(cap);
    let mut scan_truncated = false;
    for entry in std::fs::read_dir(&canon).map_err(Error::Io)? {
        let entry = entry.map_err(Error::Io)?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name == ".DS_Store" || (!options.show_hidden && name.starts_with('.')) {
            continue;
        }
        let path = entry.path();
        let item = if let Some(item) = deferred_home_directory_entry(&name, &path) {
            item
        } else {
            let link_meta = match std::fs::symlink_metadata(&path) {
                Ok(meta) => meta,
                Err(_) => continue,
            };
            let is_link = link_meta.file_type().is_symlink();
            let target_meta = is_link.then(|| std::fs::metadata(&path).ok()).flatten();
            let meta = target_meta.as_ref().unwrap_or(&link_meta);
            let is_dir = meta.is_dir();
            FileEntry {
                name: name.clone(),
                path: path.to_string_lossy().to_string(),
                is_dir,
                kind: if is_dir {
                    "dir".into()
                } else {
                    detect_file_kind(&name)
                },
                hidden: name.starts_with('.'),
                size: if is_dir { 4096 } else { meta.len() },
                mtime: meta_mtime_ms(meta),
                btime: meta_btime_ms(meta),
                symlink: is_link
                    .then(|| std::fs::read_link(&path).ok())
                    .flatten()
                    .map(|p| p.to_string_lossy().to_string()),
                project_badge: None,
                dir_hint: None,
            }
        };
        selected.push(item);
        if selected.len() > cap {
            scan_truncated = true;
            // Keep a small bounded slack window; sorting every overflow entry
            // makes large directories needlessly expensive.
            if selected.len() >= cap.saturating_add(64) {
                sort_entries(&mut selected, options);
                selected.truncate(cap);
            }
        }
    }
    sort_entries(&mut selected, options);
    let mut entries = selected;
    let has_more = scan_truncated || entries.len() > offset.saturating_add(limit);
    if offset < entries.len() {
        entries = entries.into_iter().skip(offset).take(limit).collect();
    } else {
        entries.clear();
    }
    let parent = canon
        .parent()
        .unwrap_or(&canon)
        .to_string_lossy()
        .to_string();
    Ok((
        ListDirResult {
            path: canon.to_string_lossy().to_string(),
            parent,
            entries,
            project: None,
        },
        has_more,
    ))
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
        if let Some(item) = deferred_home_directory_entry(&name, &entry_path) {
            entries.push(item);
            continue;
        }

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
            .filter(|(_, e)| {
                e.is_dir
                    && !e.hidden
                    && !is_macos_privacy_protected_home_child(std::path::Path::new(&e.path))
            })
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

    sort_entries(&mut entries, options);

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

fn sort_entries(entries: &mut [FileEntry], options: &ListDirOptions) {
    let ascending = options.sort_dir != "desc";
    entries.sort_by(|a, b| {
        // Finder-like folder grouping is invariant under the selected key.
        if a.is_dir != b.is_dir {
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
        let cmp = if ascending { cmp } else { cmp.reverse() };
        if cmp == std::cmp::Ordering::Equal {
            natural_cmp(&a.name, &b.name)
        } else {
            cmp
        }
    });
}
