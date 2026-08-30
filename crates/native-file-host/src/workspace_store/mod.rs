//! Workspace authoritative store facade (ADR-0024): SQLite at `~/.natives/natives.db`.
//!
//! Submodules:
//! - `types`: Core data structures and `WorkspaceError`
//! - `schema`: DDL, migrations, ID generation, whitelists, size limits
//! - `query`: Read-only queries for session, snapshots, templates, settings
//! - `mutation`: Single-transaction mutations with mandatory expectedRevision
//! - `import`: Tabliss v2/v3 import/export and built-in template payloads

use rusqlite::Connection;
use serde_json::Value;
use std::sync::Mutex;

pub mod import;
pub mod mutation;
pub mod query;
pub mod schema;
pub mod types;

pub use schema::default_db_path;
pub use types::*;

#[derive(Debug)]
pub struct WorkspaceStore {
    conn: Mutex<Connection>,
}

impl WorkspaceStore {
    pub fn open(path: &std::path::Path) -> StoreResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(WorkspaceError::Io)?;
        }
        let conn = Connection::open(path).map_err(WorkspaceError::Sql)?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(WorkspaceError::Sql)?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(WorkspaceError::Sql)?;
        schema::migrate_db(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    #[allow(dead_code)]
    pub fn migrate(&self) -> StoreResult<()> {
        let conn = self.conn.lock().expect("workspace store mutex");
        schema::migrate_db(&conn)
    }

    pub fn session(&self) -> StoreResult<SessionSnapshot> {
        let conn = self.conn.lock().expect("workspace store mutex");
        query::query_session(&conn)
    }

    pub fn snapshot(&self, id: &str) -> StoreResult<WorkspaceSnapshot> {
        let conn = self.conn.lock().expect("workspace store mutex");
        query::query_snapshot(&conn, id)
    }

    pub fn create(&self, name: &str, template: Option<&str>) -> StoreResult<WorkspaceSnapshot> {
        let conn = self.conn.lock().expect("workspace store mutex");
        let tx = conn.unchecked_transaction()?;
        let id = mutation::mutate_create(&tx, name, template)?;
        tx.commit()?;
        query::query_snapshot(&conn, &id)
    }

    pub fn rename(
        &self,
        id: &str,
        name: &str,
        expected_revision: Option<i64>,
    ) -> StoreResult<WorkspaceSnapshot> {
        let conn = self.conn.lock().expect("workspace store mutex");
        let tx = conn.unchecked_transaction()?;
        mutation::mutate_rename(&tx, id, name, expected_revision)?;
        tx.commit()?;
        query::query_snapshot(&conn, id)
    }

    pub fn reorder(
        &self,
        ordered_ids: &[String],
        expected_revision: Option<i64>,
    ) -> StoreResult<SessionSnapshot> {
        let conn = self.conn.lock().expect("workspace store mutex");
        let tx = conn.unchecked_transaction()?;
        mutation::mutate_reorder(&tx, ordered_ids, expected_revision)?;
        tx.commit()?;
        query::query_session(&conn)
    }

    pub fn pin(
        &self,
        id: &str,
        pinned: bool,
        expected_revision: Option<i64>,
    ) -> StoreResult<WorkspaceSnapshot> {
        let conn = self.conn.lock().expect("workspace store mutex");
        let tx = conn.unchecked_transaction()?;
        mutation::mutate_pin(&tx, id, pinned, expected_revision)?;
        tx.commit()?;
        query::query_snapshot(&conn, id)
    }

    pub fn duplicate(
        &self,
        id: &str,
        expected_revision: Option<i64>,
    ) -> StoreResult<WorkspaceSnapshot> {
        let conn = self.conn.lock().expect("workspace store mutex");
        let tx = conn.unchecked_transaction()?;
        let new_id = mutation::mutate_duplicate(&tx, id, expected_revision)?;
        tx.commit()?;
        query::query_snapshot(&conn, &new_id)
    }

    pub fn delete(&self, id: &str, expected_revision: Option<i64>) -> StoreResult<SessionSnapshot> {
        let conn = self.conn.lock().expect("workspace store mutex");
        let tx = conn.unchecked_transaction()?;
        mutation::mutate_delete(&tx, id, expected_revision)?;
        tx.commit()?;
        query::query_session(&conn)
    }

    pub fn open_tab(&self, id: &str) -> StoreResult<SessionSnapshot> {
        let conn = self.conn.lock().expect("workspace store mutex");
        let tx = conn.unchecked_transaction()?;
        mutation::mutate_open_tab(&tx, id)?;
        tx.commit()?;
        query::query_session(&conn)
    }

    pub fn close_tab(&self, id: &str) -> StoreResult<SessionSnapshot> {
        let conn = self.conn.lock().expect("workspace store mutex");
        let tx = conn.unchecked_transaction()?;
        mutation::mutate_close_tab(&tx, id)?;
        tx.commit()?;
        query::query_session(&conn)
    }

    pub fn reorder_tabs(
        &self,
        ordered_ids: &[String],
        expected_revision: Option<i64>,
    ) -> StoreResult<SessionSnapshot> {
        let conn = self.conn.lock().expect("workspace store mutex");
        let tx = conn.unchecked_transaction()?;
        mutation::mutate_reorder_tabs(&tx, ordered_ids, expected_revision)?;
        tx.commit()?;
        query::query_session(&conn)
    }

    pub fn widget_upsert(
        &self,
        workspace_id: &str,
        widget: &WidgetRecord,
        expected_revision: Option<i64>,
    ) -> StoreResult<WorkspaceSnapshot> {
        let conn = self.conn.lock().expect("workspace store mutex");
        let tx = conn.unchecked_transaction()?;
        mutation::mutate_widget_upsert(&tx, workspace_id, widget, expected_revision)?;
        tx.commit()?;
        query::query_snapshot(&conn, workspace_id)
    }

    pub fn widget_remove(
        &self,
        workspace_id: &str,
        widget_id: &str,
        expected_revision: Option<i64>,
    ) -> StoreResult<WorkspaceSnapshot> {
        let conn = self.conn.lock().expect("workspace store mutex");
        let tx = conn.unchecked_transaction()?;
        mutation::mutate_widget_remove(&tx, workspace_id, widget_id, expected_revision)?;
        tx.commit()?;
        query::query_snapshot(&conn, workspace_id)
    }

    pub fn widget_reorder(
        &self,
        workspace_id: &str,
        ordered_ids: &[String],
        expected_revision: Option<i64>,
    ) -> StoreResult<WorkspaceSnapshot> {
        let conn = self.conn.lock().expect("workspace store mutex");
        let tx = conn.unchecked_transaction()?;
        mutation::mutate_widget_reorder(&tx, workspace_id, ordered_ids, expected_revision)?;
        tx.commit()?;
        query::query_snapshot(&conn, workspace_id)
    }

    pub fn background_save(
        &self,
        workspace_id: &str,
        background_json: &Value,
        expected_revision: Option<i64>,
    ) -> StoreResult<WorkspaceSnapshot> {
        let conn = self.conn.lock().expect("workspace store mutex");
        let tx = conn.unchecked_transaction()?;
        mutation::mutate_background_save(&tx, workspace_id, background_json, expected_revision)?;
        tx.commit()?;
        query::query_snapshot(&conn, workspace_id)
    }

    pub fn instantiate_template(
        &self,
        template_id: &str,
        name: &str,
    ) -> StoreResult<WorkspaceSnapshot> {
        let known_builtin = matches!(template_id, "classic" | "blank" | "focus");
        let payload = if known_builtin {
            import::builtin_template_payload(template_id)?
        } else {
            let conn = self.conn.lock().expect("workspace store mutex");
            let payload_json: String = conn.query_row(
                "SELECT payload_json FROM workspace_templates WHERE id = ?1 AND origin = 'personal'",
                [template_id],
                |row| row.get(0),
            ).map_err(|_| WorkspaceError::NotFound)?;
            serde_json::from_str(&payload_json).map_err(|error| {
                WorkspaceError::InvalidInput(format!("template payload: {error}"))
            })?
        };
        let conn = self.conn.lock().expect("workspace store mutex");
        let tx = conn.unchecked_transaction()?;
        let id = mutation::mutate_create_from_payload(&tx, name, &payload, template_id)?;
        tx.commit()?;
        query::query_snapshot(&conn, &id)
    }

    pub fn template_list(&self) -> StoreResult<Vec<TemplateSummary>> {
        let conn = self.conn.lock().expect("workspace store mutex");
        query::query_template_list(&conn)
    }

    pub fn template_save(&self, name: &str, workspace_id: &str) -> StoreResult<String> {
        let conn = self.conn.lock().expect("workspace store mutex");
        let tx = conn.unchecked_transaction()?;
        let id = mutation::mutate_template_save(&tx, name, workspace_id)?;
        tx.commit()?;
        Ok(id)
    }

    pub fn template_delete(&self, template_id: &str) -> StoreResult<()> {
        let conn = self.conn.lock().expect("workspace store mutex");
        let tx = conn.unchecked_transaction()?;
        mutation::mutate_template_delete(&tx, template_id)?;
        tx.commit()?;
        Ok(())
    }

    pub fn tabliss_preview(&self, tabliss_json: &Value) -> StoreResult<TemplatePayload> {
        import::parse_tabliss_config(tabliss_json)
    }

    pub fn export_tabliss(&self, workspace_id: &str) -> StoreResult<Value> {
        let snapshot = self.snapshot(workspace_id)?;
        Ok(import::export_tabliss_config(&snapshot))
    }

    pub fn save_from_tabliss(
        &self,
        workspace_id: &str,
        tabliss_json: &Value,
        expected_revision: Option<i64>,
    ) -> StoreResult<WorkspaceSnapshot> {
        let payload = import::parse_tabliss_config(tabliss_json)?;
        let conn = self.conn.lock().expect("workspace store mutex");
        let tx = conn.unchecked_transaction()?;
        let actual = query::workspace_revision(&tx, workspace_id)?;
        if let Some(expected) = expected_revision {
            if expected != actual {
                return Err(WorkspaceError::RevisionConflict { expected, actual });
            }
        } else {
            return Err(WorkspaceError::InvalidInput(
                "expectedRevision is required".into(),
            ));
        }
        tx.execute(
            "DELETE FROM workspace_widgets WHERE workspace_id = ?1",
            [workspace_id],
        )?;
        mutation::insert_widgets(&tx, workspace_id, &payload.widgets)?;
        tx.execute(
            "UPDATE workspaces SET background_json = ?1, revision = revision + 1, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?2",
            rusqlite::params![payload.background_json.to_string(), workspace_id],
        )?;
        tx.commit()?;
        query::query_snapshot(&conn, workspace_id)
    }

    pub fn reset(
        &self,
        workspace_id: &str,
        template: &str,
        expected_revision: Option<i64>,
    ) -> StoreResult<WorkspaceSnapshot> {
        let payload = import::builtin_template_payload(template)?;
        let conn = self.conn.lock().expect("workspace store mutex");
        let tx = conn.unchecked_transaction()?;
        let actual = query::workspace_revision(&tx, workspace_id)?;
        if let Some(expected) = expected_revision {
            if expected != actual {
                return Err(WorkspaceError::RevisionConflict { expected, actual });
            }
        } else {
            return Err(WorkspaceError::InvalidInput(
                "expectedRevision is required".into(),
            ));
        }
        tx.execute(
            "DELETE FROM workspace_widgets WHERE workspace_id = ?1",
            [workspace_id],
        )?;
        mutation::insert_widgets(&tx, workspace_id, &payload.widgets)?;
        tx.execute(
            "UPDATE workspaces SET background_json = ?1, revision = revision + 1, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?2",
            rusqlite::params![payload.background_json.to_string(), workspace_id],
        )?;
        tx.commit()?;
        query::query_snapshot(&conn, workspace_id)
    }

    pub fn settings_get(&self, keys: &[String]) -> StoreResult<Value> {
        let conn = self.conn.lock().expect("workspace store mutex");
        query::query_settings(&conn, keys)
    }

    pub fn settings_set(&self, entries: &Value) -> StoreResult<Value> {
        let conn = self.conn.lock().expect("workspace store mutex");
        let tx = conn.unchecked_transaction()?;
        mutation::mutate_settings_set(&tx, entries)?;
        tx.commit()?;
        let keys = entries
            .as_object()
            .map(|m| m.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        query::query_settings(&conn, &keys)
    }
}

#[cfg(test)]
mod tests;
