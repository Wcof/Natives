use super::*;
use crate::db::{apply_migrations, create_tables};

fn empty_db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    create_tables(&conn).unwrap();
    apply_migrations(&conn).unwrap();
    let _ = conn.execute("DELETE FROM workspaces", []);
    conn
}

fn workspace_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM workspaces", [], |row| row.get(0))
        .unwrap()
}

#[test]
fn create_on_empty_db() {
    let conn = empty_db();
    assert_eq!(workspace_count(&conn), 0);
    let a = create_workspace(
        &conn,
        "alpha",
        "workspace",
        None,
        None,
        "dark",
        "structured",
    )
    .unwrap();
    assert_eq!(
        (a.name.as_str(), a.theme.as_str(), a.position),
        ("alpha", "dark", 0)
    );
    assert!(!a.is_active);
    let b = create_workspace(
        &conn,
        "beta",
        "workspace",
        None,
        None,
        "frosted-jasmine",
        "structured",
    )
    .unwrap();
    assert_eq!((b.theme.as_str(), b.position), ("light", 1));
    let c = create_workspace(
        &conn,
        "gamma",
        "workspace",
        None,
        None,
        "light",
        "structured",
    )
    .unwrap();
    assert_eq!(c.position, 2);
}

#[test]
fn list_stable_order() {
    let conn = empty_db();
    let a = create_workspace(
        &conn,
        "alpha",
        "workspace",
        None,
        None,
        "dark",
        "structured",
    )
    .unwrap();
    let b = create_workspace(&conn, "beta", "workspace", None, None, "dark", "structured").unwrap();
    let c = create_workspace(
        &conn,
        "gamma",
        "workspace",
        None,
        None,
        "dark",
        "structured",
    )
    .unwrap();
    for (id, position) in [(a.id.as_str(), 2), (b.id.as_str(), 0), (c.id.as_str(), 1)] {
        conn.execute(
            "UPDATE workspaces SET position = ?1 WHERE id = ?2",
            rusqlite::params![position, id],
        )
        .unwrap();
    }
    let first = list_workspaces(&conn).unwrap();
    let second = list_workspaces(&conn).unwrap();
    assert_eq!(
        first
            .iter()
            .map(|workspace| workspace.name.as_str())
            .collect::<Vec<_>>(),
        vec!["beta", "gamma", "alpha"]
    );
    assert_eq!(first, second);
    conn.execute(
        "UPDATE workspaces SET position = 3 WHERE id IN (?1, ?2)",
        rusqlite::params![a.id, c.id],
    )
    .unwrap();
    let tied = list_workspaces(&conn).unwrap();
    assert_eq!(
        tied.iter()
            .map(|workspace| workspace.name.as_str())
            .collect::<Vec<_>>(),
        vec!["beta", "alpha", "gamma"]
    );
}

#[test]
fn get_workspace_empty_and_existing() {
    let conn = empty_db();
    assert!(get_workspace(&conn, "ws_missing").unwrap().is_none());
    let created = create_workspace(
        &conn,
        "alpha",
        "workspace",
        Some("star"),
        None,
        "dark",
        "structured",
    )
    .unwrap();
    let found = get_workspace(&conn, &created.id).unwrap().unwrap();
    assert_eq!(found.name, "alpha");
    assert_eq!(found.icon.as_deref(), Some("star"));
    assert_eq!(
        (found.kind, found.position),
        (created.kind, created.position)
    );
}

#[test]
fn widget_boundary_rejects_secrets_and_css_appearance() {
    let conn = empty_db();
    let workspace = create_workspace(
        &conn,
        "secure",
        "workspace",
        None,
        None,
        "dark",
        "structured",
    )
    .unwrap();
    let secret = WorkspaceWidgetInput {
        id: None,
        widget_type: "notes".into(),
        config: Some(serde_json::json!({"token":"nope"})),
        config_version: Some(1),
        appearance: None,
        enabled: Some(true),
        z_index: None,
    };
    assert!(matches!(
        upsert_widget(&conn, &workspace.id, &secret),
        Err(Error::InvalidInput(_))
    ));
    let css = WorkspaceWidgetInput {
        config: Some(serde_json::json!({})),
        appearance: Some(serde_json::json!({"header":"#fff"})),
        ..secret
    };
    assert!(matches!(
        upsert_widget(&conn, &workspace.id, &css),
        Err(Error::InvalidInput(_))
    ));
}
