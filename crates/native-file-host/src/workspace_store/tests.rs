use super::WorkspaceStore;
use crate::workspace_store::types::{WidgetRecord, WorkspaceError};
use rusqlite::Connection;
use std::path::PathBuf;

fn temp_store(tag: &str) -> (WorkspaceStore, PathBuf) {
    let path =
        std::env::temp_dir().join(format!("natives-ws-test-{}-{}.db", tag, std::process::id()));
    let _ = std::fs::remove_file(&path);
    (WorkspaceStore::open(&path).expect("open store"), path)
}

#[test]
fn migrations_are_idempotent() {
    let (store, path) = temp_store("migrate");
    store.migrate().expect("first migrate");
    store.migrate().expect("second migrate");
    let _ = std::fs::remove_file(path);
}

#[test]
fn legacy_workspace_schema_is_upgraded_and_keeps_one_open_workspace() {
    let path =
        std::env::temp_dir().join(format!("natives-ws-test-legacy-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let conn = Connection::open(&path).expect("open legacy db");
    conn.execute_batch(
        r#"
        PRAGMA user_version = 1;
        CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL, updated_at TEXT);
        CREATE TABLE workspaces (
            id TEXT PRIMARY KEY, name TEXT NOT NULL, kind TEXT NOT NULL DEFAULT 'workspace',
            theme TEXT NOT NULL DEFAULT 'dark', is_active INTEGER NOT NULL DEFAULT 0,
            position INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL, deleted_at TEXT
        );
        CREATE TABLE workspace_open_tabs (
            workspace_id TEXT PRIMARY KEY REFERENCES workspaces(id) ON DELETE CASCADE,
            sort_order REAL NOT NULL DEFAULT 0, is_pinned INTEGER NOT NULL DEFAULT 0,
            opened_at TEXT NOT NULL, last_active_at TEXT NOT NULL
        );
        CREATE TABLE workspace_widgets (
            id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
            widget_type TEXT NOT NULL, config_json TEXT NOT NULL DEFAULT '{}',
            hidden INTEGER NOT NULL DEFAULT 0, position INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1, appearance_json TEXT NOT NULL DEFAULT '{}'
        );
        CREATE INDEX idx_widgets_workspace ON workspace_widgets(workspace_id, "order");
        CREATE TABLE workspace_templates (
            id TEXT PRIMARY KEY, name TEXT NOT NULL, origin TEXT NOT NULL DEFAULT 'personal',
            created_at TEXT NOT NULL, updated_at TEXT NOT NULL, deleted_at TEXT
        );
        CREATE TABLE workspace_meta (
            id INTEGER PRIMARY KEY CHECK (id = 1), revision INTEGER NOT NULL DEFAULT 1,
            active_workspace_id TEXT
        );
        INSERT INTO workspace_meta (id, revision) VALUES (1, 1);
        "#,
    )
    .expect("create legacy schema");
    drop(conn);

    let store = WorkspaceStore::open(&path).expect("upgrade legacy store");
    let session = store.session().expect("query upgraded session");
    assert_eq!(session.workspaces.len(), 1, "an empty DB is seeded once");
    assert_eq!(session.opened_tabs.len(), 1, "the seed workspace is open");
    let seed = store
        .snapshot(&session.workspaces[0].id)
        .expect("snapshot seed workspace");
    assert!(seed.widgets.is_empty(), "the seed workspace is blank");

    let created = store
        .create("Legacy compatible", None)
        .expect("create in upgraded legacy schema");
    assert_eq!(created.widgets.len(), 3);
    store
        .template_save("Legacy template", &created.workspace.id)
        .expect("save template in upgraded legacy schema");

    store.migrate().expect("migration remains idempotent");
    assert_eq!(
        store
            .session()
            .expect("session after remigrate")
            .workspaces
            .len(),
        2
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn deleting_or_closing_the_last_workspace_is_rejected() {
    let (store, path) = temp_store("last-workspace");
    let session = store.session().expect("seeded session");
    let only = &session.workspaces[0];
    let delete_error = store
        .delete(&only.id, Some(only.revision))
        .expect_err("last workspace cannot be deleted");
    assert!(matches!(delete_error, WorkspaceError::InvalidInput(_)));
    let close_error = store
        .close_tab(&only.id)
        .expect_err("last open workspace cannot be closed");
    assert!(matches!(close_error, WorkspaceError::InvalidInput(_)));
    let _ = std::fs::remove_file(path);
}

#[test]
fn create_session_snapshot_and_soft_delete() {
    let (store, path) = temp_store("crud");
    let created = store.create("Work", None).expect("create");
    assert_eq!(created.name, "Work");
    assert!(
        !created.widgets.is_empty(),
        "classic template seeds time+greeting"
    );
    let session = store.session().expect("session");
    assert!(session
        .workspaces
        .iter()
        .any(|workspace| workspace.id == created.workspace.id && workspace.is_open));
    let after_delete = store
        .delete(&created.workspace.id, Some(created.revision))
        .expect("delete");
    assert!(!after_delete
        .workspaces
        .iter()
        .any(|workspace| workspace.id == created.workspace.id));
    assert!(
        store.snapshot(&created.workspace.id).is_err(),
        "soft-deleted workspace must not snapshot"
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn revision_conflicts_are_rejected() {
    let (store, path) = temp_store("revision");
    let created = store.create("Rev", None).expect("create");
    let error = store
        .rename(&created.workspace.id, "New", Some(created.revision + 5))
        .expect_err("stale revision must conflict");
    match error {
        WorkspaceError::RevisionConflict { expected, actual } => {
            assert_eq!(expected, created.revision + 5);
            assert_eq!(actual, created.revision);
        }
        other => panic!("unexpected error: {other}"),
    }
    let _ = std::fs::remove_file(path);
}

#[test]
fn duplicate_copies_widgets_in_one_transaction() {
    let (store, path) = temp_store("duplicate");
    let created = store.create("Src", None).expect("create");
    let copy = store
        .duplicate(&created.workspace.id, Some(created.revision))
        .expect("duplicate");
    assert_eq!(copy.widgets.len(), created.widgets.len());
    assert!(copy.name.starts_with("Src (copy)"));
    for widget in &copy.widgets {
        assert_ne!(
            created.widgets.iter().any(|origin| origin.id == widget.id),
            true,
            "widget ids must be fresh"
        );
    }
    let _ = std::fs::remove_file(path);
}

#[test]
fn tabliss_import_is_validated_and_atomic() {
    let (store, path) = temp_store("tabliss");
    let created = store.create("Import", None).expect("create");
    let valid = serde_json::json!({
        "background": { "key": "background/colour", "display": { "colour": "#0b0c0a" } },
        "widget/custom-time": {
            "key": "widget/time", "order": 2,
            "data": { "mode": "digital" },
            "display": { "position": "topCentre", "fontSize": 96 }
        }
    });
    let imported = store
        .save_from_tabliss(&created.workspace.id, &valid, Some(created.revision))
        .expect("valid import");
    assert_eq!(imported.widgets.len(), 1);
    assert_eq!(imported.widgets[0].key, "widget/time");
    let invalid = serde_json::json!({ "background": { "nope": true } });
    let error = store
        .save_from_tabliss(&created.workspace.id, &invalid, Some(imported.revision))
        .expect_err("invalid background must be rejected");
    assert!(matches!(error, WorkspaceError::InvalidInput(_)));
    let after = store
        .snapshot(&created.workspace.id)
        .expect("snapshot after failure");
    assert_eq!(
        after.widgets.len(),
        1,
        "failed import must roll back, keeping prior widgets"
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn tabliss_preview_and_export_roundtrip() {
    let (store, path) = temp_store("export");
    let created = store
        .create("ExportTest", Some("focus"))
        .expect("create focus");
    let exported = store.export_tabliss(&created.workspace.id).expect("export");
    assert!(exported.get("background").is_some());
    assert!(exported.get("widget/time").is_some());
    let preview = store.tabliss_preview(&exported).expect("preview exported");
    assert_eq!(preview.widgets.len(), created.widgets.len());
    let _ = std::fs::remove_file(path);
}

#[test]
fn widget_upsert_reorder_and_background_save() {
    let (store, path) = temp_store("widgets");
    let created = store.create("W", None).expect("create");
    let widget = WidgetRecord {
        id: String::new(),
        workspace_id: created.workspace.id.clone(),
        key: "widget/quote".into(),
        order: 9,
        enabled: true,
        config_json: serde_json::json!({ "category": "inspirational" }),
        display_json: serde_json::json!({ "position": "topLeft" }),
    };
    let after_upsert = store
        .widget_upsert(&created.workspace.id, &widget, Some(created.revision))
        .expect("upsert");
    assert!(after_upsert
        .widgets
        .iter()
        .any(|entry| entry.key == "widget/quote"));
    let ordered: Vec<String> = after_upsert
        .widgets
        .iter()
        .map(|entry| entry.id.clone())
        .rev()
        .collect();
    let after_reorder = store
        .widget_reorder(&created.workspace.id, &ordered, Some(after_upsert.revision))
        .expect("reorder");
    let background =
        serde_json::json!({ "key": "background/colour", "display": { "colour": "#000000" } });
    let after_background = store
        .background_save(
            &created.workspace.id,
            &background,
            Some(after_reorder.revision),
        )
        .expect("background save");
    assert_eq!(after_background.background_json, background);
    let _ = std::fs::remove_file(path);
}

#[test]
fn template_save_list_delete_lifecycle() {
    let (store, path) = temp_store("templates");
    let created = store.create("SourceWs", Some("focus")).expect("create");
    let tpl_id = store
        .template_save("MyFocusTemplate", &created.workspace.id)
        .expect("save template");
    let list = store.template_list().expect("template list");
    assert!(list
        .iter()
        .any(|t| t.id == tpl_id && t.name == "MyFocusTemplate" && t.origin == "personal"));
    let instantiated = store
        .instantiate_template(&tpl_id, "FromPersonal")
        .expect("instantiate personal");
    assert_eq!(instantiated.name, "FromPersonal");
    assert_eq!(instantiated.widgets.len(), created.widgets.len());
    store.template_delete(&tpl_id).expect("delete template");
    let list_after = store.template_list().expect("template list after");
    assert!(!list_after.iter().any(|t| t.id == tpl_id));
    let _ = std::fs::remove_file(path);
}

#[test]
fn settings_roundtrip() {
    let (store, path) = temp_store("settings");
    let saved = store
        .settings_set(&serde_json::json!({ "accent": "#cdf24b", "locale": "zh_CN" }))
        .expect("save settings");
    assert_eq!(saved["accent"], "#cdf24b");
    let loaded = store
        .settings_get(&["accent".into(), "theme".into()])
        .expect("load settings");
    assert_eq!(loaded["accent"], "#cdf24b");
    let _ = std::fs::remove_file(path);
}

#[test]
fn reset_workspace_to_built_in_templates() {
    let (store, path) = temp_store("reset");
    let created = store
        .create("ToReset", Some("focus"))
        .expect("create focus");
    assert_eq!(created.widgets.len(), 3);
    let focus_keys: Vec<&str> = created.widgets.iter().map(|w| w.key.as_str()).collect();
    assert_eq!(
        focus_keys,
        vec!["widget/time", "widget/todo", "widget/notes"]
    );

    // Reset to blank
    let reset_blank = store
        .reset(&created.workspace.id, "blank", Some(created.revision))
        .expect("reset to blank");
    assert_eq!(reset_blank.widgets.len(), 0);
    assert_eq!(reset_blank.revision, created.revision + 1);

    // Reset to classic
    let reset_classic = store
        .reset(&created.workspace.id, "classic", Some(reset_blank.revision))
        .expect("reset to classic");
    assert_eq!(reset_classic.widgets.len(), 3);
    let classic_keys: Vec<&str> = reset_classic
        .widgets
        .iter()
        .map(|w| w.key.as_str())
        .collect();
    assert_eq!(
        classic_keys,
        vec!["widget/time", "widget/greeting", "widget/quote"]
    );

    // Revision conflict on reset
    let conflict = store
        .reset(
            &created.workspace.id,
            "focus",
            Some(reset_classic.revision + 10),
        )
        .expect_err("stale revision conflict");
    assert!(matches!(conflict, WorkspaceError::RevisionConflict { .. }));

    let _ = std::fs::remove_file(path);
}

#[test]
fn migration_purges_retired_widgets_and_cleans_personal_templates() {
    let (store, path) = temp_store("migration-retired");
    let ws = store.create("TestWs", Some("blank")).expect("create blank");

    // Insert retired widgets directly into workspace_widgets
    {
        let conn = store.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO workspace_widgets (id, workspace_id, key, \"order\", enabled, config_json, display_json, config_version)
             VALUES ('w1', ?1, 'widget/joke', 0, 1, '{}', '{}', 1),
                    ('w2', ?1, 'widget/time', 1, 1, '{}', '{}', 1),
                    ('w3', ?1, 'widget/bitcoin', 2, 1, '{}', '{}', 1)",
            [&ws.workspace.id],
        ).unwrap();
        // Insert personal template containing retired widgets
        let payload = serde_json::json!({
            "background_json": {"key": "background/colour"},
            "widgets": [
                {"id": "pw1", "workspaceId": "", "key": "widget/joke", "order": 0, "enabled": true, "configJson": {}, "displayJson": {}},
                {"id": "pw2", "workspaceId": "", "key": "widget/todo", "order": 1, "enabled": true, "configJson": {}, "displayJson": {}}
            ]
        });
        conn.execute(
            "INSERT INTO workspace_templates (id, origin, name, payload_json, created_at, updated_at)
             VALUES ('tpl-retired', 'personal', 'RetiredTpl', ?1, 'now', 'now')",
            [&payload.to_string()],
        ).unwrap();
    }

    let initial_rev = store.snapshot(&ws.workspace.id).unwrap().revision;

    // Run migration
    store.migrate().expect("run migration");

    // Verify workspace widgets were cleaned up
    let snap = store.snapshot(&ws.workspace.id).unwrap();
    assert_eq!(snap.widgets.len(), 1);
    assert_eq!(snap.widgets[0].key, "widget/time");
    assert_eq!(snap.revision, initial_rev + 1);

    // Verify personal template was cleaned up
    let tpl_list = store.template_list().unwrap();
    assert!(tpl_list.iter().any(|t| t.id == "tpl-retired"));
    let instantiated = store
        .instantiate_template("tpl-retired", "CleanedFromTpl")
        .unwrap();
    assert_eq!(instantiated.widgets.len(), 1);
    assert_eq!(instantiated.widgets[0].key, "widget/todo");

    // Run remigration (idempotency)
    let rev_before_remigrate = store.snapshot(&ws.workspace.id).unwrap().revision;
    store.migrate().expect("run remigration");
    let rev_after_remigrate = store.snapshot(&ws.workspace.id).unwrap().revision;
    assert_eq!(
        rev_before_remigrate, rev_after_remigrate,
        "Remigration must be idempotent"
    );

    let _ = std::fs::remove_file(path);
}
