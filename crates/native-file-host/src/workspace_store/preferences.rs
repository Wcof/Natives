//! Core-owned display preferences; one atomic record in the existing settings table.
use super::{query, StoreResult, WorkspaceError, WorkspaceStore};
use serde_json::{json, Value};

const KEY: &str = "natives.shared-preferences.v1";

fn valid(key: &str, value: &Value) -> bool {
    if value.is_null() {
        return shared_key(key);
    }
    match key {
        "natives-theme" => matches!(value.as_str(), Some("archive" | "volt")),
        "natives-language" => matches!(value.as_str(), Some("en" | "zh_CN")),
        "natives-view-mode" => matches!(value.as_str(), Some("list" | "grid")),
        "natives-grid-size" => matches!(value.as_str(), Some("small" | "medium" | "large")),
        "natives-sort-by" => matches!(value.as_str(), Some("name" | "mtime" | "size")),
        "natives-sort-direction" => matches!(value.as_str(), Some("asc" | "desc")),
        "natives-preview-width"
        | "natives-preview-height"
        | "natives-sidebar-width"
        | "natives-inspector-width" => value.as_u64().is_some_and(|n| (100..=1200).contains(&n)),
        "natives-show-hidden"
        | "natives-follow-changes"
        | "natives-preview-bottom"
        | "natives-sidebar-collapsed"
        | "natives-recursive-search"
        | "natives-apps-sidebar-collapsed" => value.is_boolean(),
        _ => false,
    }
}

const SHARED_KEYS: &[&str] = &[
    "natives-theme",
    "natives-language",
    "natives-view-mode",
    "natives-grid-size",
    "natives-sort-by",
    "natives-sort-direction",
    "natives-preview-width",
    "natives-preview-height",
    "natives-sidebar-width",
    "natives-inspector-width",
    "natives-show-hidden",
    "natives-follow-changes",
    "natives-preview-bottom",
    "natives-sidebar-collapsed",
    "natives-recursive-search",
    "natives-apps-sidebar-collapsed",
];
fn shared_key(key: &str) -> bool {
    SHARED_KEYS.contains(&key)
}

pub(super) fn validate_legacy_write(entries: &Value) -> StoreResult<()> {
    if entries
        .as_object()
        .is_some_and(|m| m.keys().any(|k| k == KEY || shared_key(k)))
    {
        return Err(WorkspaceError::InvalidInput(
            "use preferences_set with expectedRevision".into(),
        ));
    }
    Ok(())
}

fn snapshot(conn: &rusqlite::Connection) -> StoreResult<Value> {
    let values = query::query_settings(conn, &[KEY.into()])?;
    match values.get(KEY) {
        None => {
            let legacy = query::query_settings(
                conn,
                &SHARED_KEYS
                    .iter()
                    .map(|key| key.to_string())
                    .collect::<Vec<_>>(),
            )?;
            let entries = legacy
                .as_object()
                .map(|map| {
                    map.iter()
                        .filter(|(key, value)| valid(key, value) && !value.is_null())
                        .map(|(key, value)| (key.clone(), value.clone()))
                        .collect::<serde_json::Map<_, _>>()
                })
                .unwrap_or_default();
            let sources = if entries.is_empty() {
                json!([])
            } else {
                json!([{"source":"existing-core-settings","version":1}])
            };
            Ok(json!({"revision":0,"entries":entries,"sources":sources}))
        }
        Some(value)
            if value["revision"].is_i64()
                && value["entries"].is_object()
                && value["sources"].is_array() =>
        {
            Ok(value.clone())
        }
        _ => Err(WorkspaceError::InvalidInput(
            "ERR_PREFERENCES_CORRUPT".into(),
        )),
    }
}

impl WorkspaceStore {
    pub fn preferences_get(&self) -> StoreResult<Value> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| WorkspaceError::InvalidInput("store unavailable".into()))?;
        snapshot(&conn)
    }

    pub fn preferences_write(
        &self,
        entries: &Value,
        expected: i64,
        source: Option<&str>,
    ) -> StoreResult<Value> {
        let entries = entries
            .as_object()
            .filter(|m| !m.is_empty() && m.len() <= 17)
            .ok_or_else(|| WorkspaceError::InvalidInput("ERR_PREFERENCES_INPUT".into()))?;
        if entries.iter().any(|(key, value)| !valid(key, value))
            || source
                .is_some_and(|s| s.is_empty() || s.len() > 120 || s.chars().any(char::is_control))
        {
            return Err(WorkspaceError::InvalidInput("ERR_PREFERENCES_INPUT".into()));
        }
        let conn = self
            .conn
            .lock()
            .map_err(|_| WorkspaceError::InvalidInput("store unavailable".into()))?;
        let tx = conn.unchecked_transaction()?;
        let mut current = snapshot(&tx)?;
        let actual = current["revision"].as_i64().unwrap_or(0);
        if expected != actual {
            return Err(WorkspaceError::RevisionConflict { expected, actual });
        }
        for (key, value) in entries {
            if source.is_some() && current["entries"].get(key).is_some() {
                continue;
            }
            if value.is_null() {
                current["entries"].as_object_mut().unwrap().remove(key);
            } else {
                current["entries"][key] = value.clone();
            }
        }
        if let Some(source) = source {
            let sources = current["sources"].as_array_mut().unwrap();
            if sources.len() >= 16 {
                return Err(WorkspaceError::InvalidInput(
                    "ERR_IMPORT_SOURCE_LIMIT".into(),
                ));
            }
            sources.push(json!({"source":source,"version":1}));
        }
        current["revision"] = json!(actual + 1);
        tx.execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value, updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now')", rusqlite::params![KEY,current.to_string()])?;
        tx.commit()?;
        Ok(current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preferences_are_atomic_versioned_and_import_only_missing_keys() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        super::super::schema::migrate_db(&conn).unwrap();
        let store = WorkspaceStore {
            conn: std::sync::Mutex::new(conn),
        };
        assert_eq!(store.preferences_get().unwrap()["revision"], 0);
        assert!(store
            .preferences_write(&json!({"secret":"no"}), 0, None)
            .is_err());
        store
            .preferences_write(&json!({"natives-theme":"archive"}), 0, None)
            .unwrap();
        assert!(matches!(
            store.preferences_write(&json!({"natives-theme":"volt"}), 0, None),
            Err(WorkspaceError::RevisionConflict { .. })
        ));
        let result = store
            .preferences_write(
                &json!({"natives-theme":"volt","natives-language":"en"}),
                1,
                Some("selected-profile/v1"),
            )
            .unwrap();
        assert_eq!(result["entries"]["natives-theme"], "archive");
        assert_eq!(result["entries"]["natives-language"], "en");
        assert!(store
            .settings_set(&json!({"natives-theme":"volt"}))
            .is_err());
        store
            .preferences_write(&json!({"natives-theme":null}), 2, None)
            .unwrap();
        assert!(store.preferences_get().unwrap()["entries"]
            .get("natives-theme")
            .is_none());
    }
}
