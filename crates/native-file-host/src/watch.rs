use crate::protocol::{respond, Request, Response};
use file_manager_core::file_manager;
use notify::{event::EventKind, Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub(crate) struct WatchRegistration {
    pub(crate) _watcher: RecommendedWatcher,
    pub(crate) cancelled: Arc<AtomicBool>,
}

pub(crate) const WATCH_MODE: RecursiveMode = RecursiveMode::NonRecursive;

#[derive(Default)]
pub(crate) struct WatchManager {
    registrations: HashMap<String, WatchRegistration>,
}

impl WatchManager {
    pub(crate) fn handle(
        &mut self,
        request: &Request,
        writer: &Arc<Mutex<io::Stdout>>,
    ) -> Result<Value, String> {
        let path = request
            .params
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or("");
        if request.method == "watch_stop" {
            let key = std::fs::canonicalize(path)
                .unwrap_or_else(|_| std::path::PathBuf::from(path))
                .to_string_lossy()
                .to_string();
            if let Some(registration) = self.registrations.remove(&key) {
                registration.cancelled.store(true, Ordering::Relaxed);
            }
            return Ok(json!({"watching": false}));
        }

        let path = file_manager::FileAccessPolicy::authorize_path(
            path,
            file_manager::OperationPolicy::Read,
        )
        .map_err(|error| error.to_string())?;
        let key = path.as_path().to_string_lossy().to_string();
        if self.registrations.contains_key(&key) {
            return Ok(json!({"watching": key}));
        }

        let event_writer = Arc::clone(writer);
        let watch_id = request.id.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let callback_cancelled = Arc::clone(&cancelled);
        let recent_events = Arc::new(Mutex::new(HashMap::<String, Instant>::new()));
        let callback_events = Arc::clone(&recent_events);
        let mut watcher = RecommendedWatcher::new(
            move |event: Result<Event, notify::Error>| {
                if callback_cancelled.load(Ordering::Relaxed) {
                    return;
                }
                let Ok(event) = event else { return };
                let Some(kind) = watch_event_kind(&event) else {
                    return;
                };
                for changed in event.paths {
                    if callback_cancelled.load(Ordering::Relaxed) || is_noisy_watch_path(&changed) {
                        continue;
                    }
                    let path = changed.to_string_lossy().to_string();
                    let now = Instant::now();
                    let should_emit = callback_events
                        .lock()
                        .map(|mut recent| {
                            recent.retain(|_, seen| {
                                now.duration_since(*seen) < Duration::from_millis(250)
                            });
                            if recent.contains_key(&path) {
                                false
                            } else {
                                if recent.len() >= 1024 {
                                    recent.clear();
                                }
                                recent.insert(path.clone(), now);
                                true
                            }
                        })
                        .unwrap_or(false);
                    if should_emit {
                        let _ = respond(
                            &event_writer,
                            Response {
                                id: &watch_id,
                                ok: true,
                                result: Some(
                                    json!({"event": "fs_changed", "kind": kind, "path": path}),
                                ),
                                error: None,
                            },
                        );
                    }
                }
            },
            Config::default(),
        )
        .map_err(|error| error.to_string())?;
        watcher
            .watch(path.as_path(), WATCH_MODE)
            .map_err(|error| error.to_string())?;
        self.registrations.insert(
            key.clone(),
            WatchRegistration {
                _watcher: watcher,
                cancelled,
            },
        );
        Ok(json!({"watching": key}))
    }

    pub(crate) fn shutdown(&mut self) {
        for registration in self.registrations.values() {
            registration.cancelled.store(true, Ordering::Relaxed);
        }
        self.registrations.clear();
    }
}

pub(crate) fn watch_event_kind(event: &Event) -> Option<&'static str> {
    match &event.kind {
        EventKind::Create(_) => Some("created"),
        EventKind::Modify(notify::event::ModifyKind::Name(_)) => Some("renamed"),
        EventKind::Modify(_) => Some("modified"),
        EventKind::Remove(_) => Some("removed"),
        _ => None,
    }
}

pub(crate) fn is_noisy_watch_path(path: &Path) -> bool {
    if path.components().any(|component| {
        let name = component.as_os_str().to_string_lossy();
        name.starts_with('.') && name != "."
    }) {
        return true;
    }
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    name.ends_with('~')
        || matches!(
            name.rsplit('.').next(),
            Some("swp" | "swo" | "tmp" | "part" | "lock" | "journal" | "wal" | "shm")
        )
}
