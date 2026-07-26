//! fs_watch — File system watcher using notify crate
//!
//! Implements recursive directory watching with:
//! - Noise filtering (atime, metadata-only, self-triggered writes)
//! - Immediate per-event emission to the frontend (no debounce/batching).
//!   The 300ms value below is only the notify poll interval used by the
//!   fallback poll-based backend, not a debounce window.
//! - Tauri Event emission to frontend

use crate::{Error, Result};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use std::collections::HashSet;
use ts_rs::TS;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use tauri::Emitter;

/// Event payload sent to the frontend via Tauri event system.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
#[serde(rename_all = "camelCase")]
pub struct FsWatchEvent {
    pub path: String,
    pub kind: String,
}

/// Thread-safe file system watcher handle.
pub struct FsWatcher {
    inner: Mutex<FsWatcherInner>,
    app: tauri::AppHandle,
}

struct FsWatcherInner {
    watcher: Option<RecommendedWatcher>,
    watched_paths: HashSet<String>,
}

impl FsWatcher {
    pub fn new(app: tauri::AppHandle) -> Self {
        Self {
            inner: Mutex::new(FsWatcherInner {
                watcher: None,
                watched_paths: HashSet::new(),
            }),
            app,
        }
    }

    /// Start watching a directory recursively.
    pub fn start(&self, path: &str) -> Result<()> {
        let mut inner = self.inner.lock().map_err(|e| Error::Internal(e.to_string()))?;
        if inner.watched_paths.contains(path) {
            return Ok(());
        }

        let app = self.app.clone();
        let path_owned = path.to_string();

        if inner.watcher.is_none() {
            let watcher = {
                let app_clone = app.clone();
                RecommendedWatcher::new(
                    move |res: std::result::Result<Event, notify::Error>| {
                        if let Ok(event) = res {
                            handle_fs_event(&app_clone, &event);
                        }
                    },
                    Config::default().with_poll_interval(Duration::from_millis(300)),
                )?
            };
            inner.watcher = Some(watcher);
        }

        if let Some(ref mut watcher) = inner.watcher {
            watcher
                .watch(PathBuf::from(path).as_path(), RecursiveMode::Recursive)
                .map_err(|e| Error::Internal(format!("watch failed: {e}")))?;
        }

        inner.watched_paths.insert(path_owned);
        Ok(())
    }

    /// Stop watching a specific path.
    pub fn stop(&self, path: &str) -> Result<()> {
        let mut inner = self.inner.lock().map_err(|e| Error::Internal(e.to_string()))?;
        if !inner.watched_paths.contains(path) {
            return Ok(());
        }
        if let Some(ref mut watcher) = inner.watcher {
            let _ = watcher.unwatch(PathBuf::from(path).as_path());
        }
        inner.watched_paths.remove(path);
        if inner.watched_paths.is_empty() {
            inner.watcher = None;
        }
        Ok(())
    }

    /// Stop all watchers.
    pub fn stop_all(&self) -> Result<()> {
        let mut inner = self.inner.lock().map_err(|e| Error::Internal(e.to_string()))?;
        let paths: Vec<_> = inner.watched_paths.iter().cloned().collect();
        if let Some(ref mut watcher) = inner.watcher {
            for path in &paths {
                let _ = watcher.unwatch(PathBuf::from(path).as_path());
            }
        }
        inner.watcher = None;
        inner.watched_paths.clear();
        Ok(())
    }

    /// Get list of currently watched paths.
    pub fn watched_paths(&self) -> Result<Vec<String>> {
        let inner = self.inner.lock().map_err(|e| Error::Internal(e.to_string()))?;
        Ok(inner.watched_paths.iter().cloned().collect())
    }
}

/// Metadata-only / access events carry no content change — drop them.
fn is_noise_event_kind(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Modify(notify::event::ModifyKind::Metadata(_)) | EventKind::Access(_)
    )
}

/// Paths from our own atomic-write temp files and Finder metadata are noise.
fn is_noise_path(path_str: &str) -> bool {
    path_str.contains(".n2-tmp-") || path_str.contains(".tmp-") || path_str.ends_with(".DS_Store")
}

/// Map a notify event kind to the wire label sent to the frontend.
fn event_kind_label(kind: &EventKind) -> &'static str {
    match kind {
        EventKind::Create(_) => "create",
        EventKind::Modify(notify::event::ModifyKind::Name(_)) => "rename",
        EventKind::Modify(_) => "modify",
        EventKind::Remove(_) => "remove",
        _ => "modify",
    }
}

/// Handle a raw filesystem event: filter noise and emit to frontend.
fn handle_fs_event(app: &tauri::AppHandle, event: &Event) {
    if is_noise_event_kind(&event.kind) {
        return;
    }

    for path in &event.paths {
        let path_str = path.to_string_lossy();
        if is_noise_path(&path_str) {
            continue;
        }

        let payload = FsWatchEvent {
            path: path_str.to_string(),
            kind: event_kind_label(&event.kind).to_string(),
        };
        let _ = app.emit("fs-watch-change", &payload);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{AccessKind, CreateKind, DataChange, MetadataKind, ModifyKind, RemoveKind, RenameMode};

    #[test]
    fn metadata_and_access_events_are_noise() {
        assert!(is_noise_event_kind(&EventKind::Modify(ModifyKind::Metadata(MetadataKind::Any))));
        assert!(is_noise_event_kind(&EventKind::Access(AccessKind::Any)));
        assert!(!is_noise_event_kind(&EventKind::Create(CreateKind::File)));
        assert!(!is_noise_event_kind(&EventKind::Modify(ModifyKind::Data(DataChange::Any))));
        assert!(!is_noise_event_kind(&EventKind::Remove(RemoveKind::File)));
    }

    #[test]
    fn tmp_and_ds_store_paths_are_noise() {
        assert!(is_noise_path("/p/.tmp-file-123-abc"));
        assert!(is_noise_path("/p/.n2-tmp-xyz"));
        assert!(is_noise_path("/p/sub/.DS_Store"));
        assert!(!is_noise_path("/p/main.rs"));
        assert!(!is_noise_path("/p/tmp-notes.md"));
    }

    #[test]
    fn event_kinds_map_to_wire_labels() {
        assert_eq!(event_kind_label(&EventKind::Create(CreateKind::File)), "create");
        assert_eq!(
            event_kind_label(&EventKind::Modify(ModifyKind::Name(RenameMode::Any))),
            "rename"
        );
        assert_eq!(
            event_kind_label(&EventKind::Modify(ModifyKind::Data(DataChange::Any))),
            "modify"
        );
        assert_eq!(event_kind_label(&EventKind::Remove(RemoveKind::File)), "remove");
    }
}
