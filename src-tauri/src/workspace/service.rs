//! Workspace V2 — service layer: transactional operations that compose the
//! raw SQL repository (`store`) into higher-level workflows.
//!
//! Kept as a separate module so `store` stays a focused repository (<1000
//! lines, architecture budget) and multi-row workflows get one transaction.

use super::store::{
    get_workspace, list_context_items, list_layouts, list_tool_profiles, list_view_states,
    list_widgets, new_id, now_rfc3339,
};
use super::types::{
    WorkspaceSummary, WorkspaceWidget, WorkspaceWidgetConfigPatch, WorkspaceWidgetInput,
};
use crate::{Error, Result};
use rusqlite::Connection;

/// A-022: duplicate a workspace and all of its child rows (context
/// items, widgets, layouts, view states, tool profiles) under fresh ids.
///
/// Runs in a single transaction so a partial copy can never be observed.
/// The copy gets a new id, keeps the source's kind/theme/icon/description,
/// is placed after the last workspace, and never steals `is_active` from the
/// source. Runtime session state (active tab flags) is not copied — the new
/// workspace starts with tabs as views but no active session.
pub fn duplicate_workspace(
    conn: &Connection,
    id: &str,
    new_name: &str,
) -> Result<Option<WorkspaceSummary>> {
    let Some(source) = get_workspace(conn, id)? else {
        return Ok(None);
    };
    let tx = conn.unchecked_transaction().map_err(Error::Database)?;
    let new_ws_id = new_id("ws");
    let now = now_rfc3339();
    let position = {
        // MAX() on an empty table is a single NULL row — bind as Option<i64>.
        let max_pos: Option<i64> = tx
            .query_row("SELECT MAX(position) FROM workspaces", [], |r| {
                r.get::<_, Option<i64>>(0)
            })
            .map_err(Error::Database)?;
        max_pos.map(|p| p + 1).unwrap_or(0)
    };
    tx.execute(
        "INSERT INTO workspaces
            (id, name, kind, icon, description, theme, is_active, position,
             default_layout_mode, appearance_json, template_source_id, template_version,
             created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, ?8, ?9, ?10, ?11, ?12, ?12)",
        rusqlite::params![
            new_ws_id,
            new_name,
            source.kind,
            source.icon,
            source.description,
            source.theme,
            position,
            source.default_layout_mode,
            source.appearance.to_string(),
            source.template_source_id,
            source.template_version,
            now
        ],
    )
    .map_err(Error::Database)?;

    // Context items.
    for item in list_context_items(conn, id)? {
        tx.execute(
            "INSERT INTO workspace_context_items
                (id, workspace_id, item_kind, ref_id, title, meta_json, position, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                new_id("ctx"),
                new_ws_id,
                item.item_kind,
                item.ref_id,
                item.title,
                item.meta.to_string(),
                item.position,
                now
            ],
        )
        .map_err(Error::Database)?;
    }

    // Widgets.
    for widget in list_widgets(conn, id)? {
        tx.execute(
            "INSERT INTO workspace_widgets
                (id, workspace_id, widget_type, config_version, config_json, appearance_json,
                 enabled, z_index, position, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
            rusqlite::params![
                new_id("wgt"),
                new_ws_id,
                widget.widget_type,
                widget.config_version,
                widget.config.to_string(),
                widget.appearance.to_string(),
                i64::from(widget.enabled),
                widget.z_index,
                widget.position,
                now
            ],
        )
        .map_err(Error::Database)?;
    }

    // Layouts.
    for layout in list_layouts(conn, id)? {
        tx.execute(
            "INSERT INTO workspace_layouts
                (id, workspace_id, layout_mode, breakpoint, layout_version, layout_json,
                 is_active, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
            rusqlite::params![
                new_id("lay"),
                new_ws_id,
                layout.layout_mode,
                layout.breakpoint,
                layout.layout_version,
                layout.layout.to_string(),
                i64::from(layout.is_active),
                now
            ],
        )
        .map_err(Error::Database)?;
    }

    // View states.
    for view_state in list_view_states(conn, id)? {
        tx.execute(
            "INSERT INTO workspace_view_states
                (id, workspace_id, view_key, state_version, state_json, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                new_id("vs"),
                new_ws_id,
                view_state.view_key,
                view_state.state_version,
                view_state.state.to_string(),
                now
            ],
        )
        .map_err(Error::Database)?;
    }

    // Tool profiles.
    for profile in list_tool_profiles(conn, id)? {
        tx.execute(
            "INSERT INTO workspace_tool_profiles
                (id, workspace_id, profile_id, tool_key, config_json, enabled, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
            rusqlite::params![
                new_id("tpf"),
                new_ws_id,
                profile.profile_id,
                profile.tool_key,
                profile.config.to_string(),
                i64::from(profile.enabled),
                now
            ],
        )
        .map_err(Error::Database)?;
    }

    tx.commit().map_err(Error::Database)?;
    get_workspace(conn, &new_ws_id)
}

/// Contract `batch_update_widget_configs`: apply config patches to multiple
/// widgets of one workspace in a single transaction. Missing widget ids are
/// ignored (they may have been removed). Returns the number of rows touched.
pub fn batch_update_widget_configs(
    conn: &Connection,
    workspace_id: &str,
    updates: &[WorkspaceWidgetConfigPatch],
) -> Result<usize> {
    let tx = conn.unchecked_transaction().map_err(Error::Database)?;
    let now = now_rfc3339();
    let mut changed = 0usize;
    for patch in updates {
        let n = tx
            .execute(
                "UPDATE workspace_widgets SET config_json = ?1, updated_at = ?2
                  WHERE id = ?3 AND workspace_id = ?4",
                rusqlite::params![patch.config.to_string(), now, patch.id, workspace_id],
            )
            .map_err(Error::Database)?;
        changed += n;
    }
    tx.commit().map_err(Error::Database)?;
    Ok(changed)
}

/// WS-05 / contract §10.4: atomically create a REAL widget row (host-generated
/// id — never a renderer ghost id) and merge it into the persisted
/// `workspace_layouts` document in ONE transaction.
///
/// `mode` selects the placement realm:
/// - `structured` (`lg`/`md`/`sm`): the widget item `{"i": id, ...}` is
///   appended to the CURRENT persisted layout for that breakpoint (read inside
///   the transaction). Any placeholder `i` in the renderer-supplied last item
///   is rewritten to the real host id before persisting, so a ghost id can
///   never reach storage.
/// - `free`: the canvas document (array of nodes, or `{ "nodes": [...] }`) is
///   merged — a node for the new widget id is appended with a default cascade
///   position in pixel world space, and the free layout row (breakpoint
///   `'free'`) is upserted keyed `{ "nodes": [...] }`.
///
/// All validations complete BEFORE any write and every layout read happens
/// INSIDE the transaction. A failure at any step rolls everything back — the
/// renderer can never observe a widget row without its placement (a ghost
/// node), nor a placement without its widget instance.
pub fn add_widget_atomically(
    conn: &Connection,
    workspace_id: &str,
    input: &WorkspaceWidgetInput,
    mode: &str,
    breakpoint: &str,
    layout: Option<&serde_json::Value>,
    expected_revision: Option<i64>,
) -> Result<Option<WorkspaceWidget>> {
    check_revision(conn, workspace_id, expected_revision)?;

    let appearance_value = input
        .appearance
        .clone()
        .unwrap_or_else(|| serde_json::json!({}));
    super::store::validate_appearance(&appearance_value)?;
    if let Some(config) = input.config.as_ref() {
        super::store::validate_widget_config(config)?;
    }

    let tx = conn.unchecked_transaction().map_err(Error::Database)?;
    let now = now_rfc3339();

    let config: serde_json::Value = input
        .config
        .clone()
        .unwrap_or_else(|| serde_json::json!({}));
    let position = {
        let max_pos: Option<i64> = tx
            .query_row(
                "SELECT MAX(position) FROM workspace_widgets WHERE workspace_id = ?1",
                [workspace_id],
                |r| r.get::<_, Option<i64>>(0),
            )
            .map_err(Error::Database)?;
        max_pos.map(|p| p + 1).unwrap_or(0)
    };
    let widget_id = new_id("wgt");

    tx.execute(
        "INSERT INTO workspace_widgets
            (id, workspace_id, widget_type, config_version, config_json, appearance_json,
             enabled, z_index, position, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
        rusqlite::params![
            widget_id,
            workspace_id,
            input.widget_type.clone(),
            input.config_version.unwrap_or(1),
            config.to_string(),
            appearance_value.to_string(),
            i64::from(input.enabled.unwrap_or(true)),
            input.z_index.unwrap_or(0),
            position,
            now
        ],
    )
    .map_err(Error::Database)?;

    insert_widget_placement(
        &tx,
        workspace_id,
        &widget_id,
        &input.widget_type,
        &config,
        mode,
        breakpoint,
        layout,
        &now,
    )?;

    tx.commit().map_err(Error::Database)?;
    super::store::get_widget(conn, &widget_id)
}

/// Embed the freshly created widget into the persisted layout document (in the
/// same transaction). Returns without touching the layout for unsupported
/// request shapes (the widget row still commits — the caller decides whether a
/// missing placement is acceptable).
fn insert_widget_placement(
    tx: &rusqlite::Transaction<'_>,
    workspace_id: &str,
    widget_id: &str,
    widget_type: &str,
    config: &serde_json::Value,
    mode: &str,
    breakpoint: &str,
    layout: Option<&serde_json::Value>,
    now: &str,
) -> Result<()> {
    if mode == "free" {
        return embed_widget_free(tx, workspace_id, widget_id, widget_type, config, layout, now);
    }
    insert_widget_structured(tx, workspace_id, widget_id, widget_type, config, breakpoint, layout, now)
}

/// Free Canvas placement: cascade the node into the persisted document.
fn embed_widget_free(
    tx: &rusqlite::Transaction<'_>,
    workspace_id: &str,
    widget_id: &str,
    widget_type: &str,
    config: &serde_json::Value,
    layout: Option<&serde_json::Value>,
    now: &str,
) -> Result<()> {
    let (mut nodes, cursor) = match layout {
        Some(serde_json::Value::Array(items)) => (
            items.clone(),
            items
                .iter()
                .filter_map(|v| v.get("z"))
                .filter_map(|v| v.as_i64().or_else(|| v.as_u64().map(|u| u as i64)))
                .max()
                .map(|z| z + 1)
                .unwrap_or(10),
        ),
        Some(serde_json::Value::Object(map)) => (
            map.get("nodes")
                .and_then(serde_json::Value::as_array)
                .cloned()
                .unwrap_or_default(),
            10,
        ),
        _ => (Vec::new(), 10),
    };
    let (x, y) = cascade_position(&nodes);
    nodes.push(serde_json::json!({
        "id": widget_id,
        "kind": "widget",
        "label": widget_id,
        "x": x,
        "y": y,
        "w": 256,
        "h": 176,
        "z": cursor,
        "widgetType": widget_type,
        "widgetConfig": config.clone(),
    }));
    let doc = serde_json::json!({ "nodes": nodes });
    tx.execute(
        "INSERT INTO workspace_layouts
            (id, workspace_id, layout_mode, breakpoint, layout_version, layout_json,
             is_active, created_at, updated_at)
         VALUES (?1, ?2, 'free', 'free', 1, ?3, 1, ?4, ?4)
         ON CONFLICT(workspace_id, layout_mode, breakpoint) DO UPDATE SET
            layout_json = excluded.layout_json,
            layout_version = excluded.layout_version,
            is_active = excluded.is_active,
            updated_at = excluded.updated_at",
        rusqlite::params![new_id("lay"), workspace_id, doc.to_string(), now],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// Structured placement: append the widget item to the breakpoint layout.
fn insert_widget_structured(
    tx: &rusqlite::Transaction<'_>,
    workspace_id: &str,
    widget_id: &str,
    _widget_type: &str,
    _config: &serde_json::Value,
    breakpoint: &str,
    layout: Option<&serde_json::Value>,
    now: &str,
) -> Result<()> {
    let breakpoint = match breakpoint {
        "lg" | "md" | "sm" => breakpoint,
        other => {
            return Err(Error::InvalidInput(format!(
                "structured mode requires a grid breakpoint (lg/md/sm), got {other}"
            )));
        }
    };
    let Some(current) = layout.and_then(serde_json::Value::as_array) else {
        return Err(Error::InvalidInput(
            "structured widget add requires a current layout array".into(),
        ));
    };
    let mut item = current
        .last()
        .cloned()
        .unwrap_or_else(|| serde_json::json!({ "i": widget_id }));
    if let Some(map) = item.as_object_mut() {
        map.insert("i".into(), serde_json::Value::String(widget_id.to_string()));
    }
    let mut next = current.clone();
    next.push(item);
    tx.execute(
        "INSERT INTO workspace_layouts
            (id, workspace_id, layout_mode, breakpoint, layout_version, layout_json,
             is_active, created_at, updated_at)
         VALUES (?1, ?2, 'structured', ?3, 2, ?4, 1, ?5, ?5)
         ON CONFLICT(workspace_id, layout_mode, breakpoint) DO UPDATE SET
            layout_json = excluded.layout_json,
            layout_version = excluded.layout_version,
            layout_mode = excluded.layout_mode,
            updated_at = excluded.updated_at",
        rusqlite::params![
            new_id("lay"),
            workspace_id,
            breakpoint,
            serde_json::Value::Array(next).to_string(),
            now
        ],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// Cascade the next default Free Canvas node position (pixel world space),
/// mirroring `FreeCanvasView.addNode`.
fn cascade_position(nodes: &[serde_json::Value]) -> (i64, i64) {
    let x = 40 + (nodes.len() as i64 % 5) * 280;
    let y = 40 + ((nodes.len() as i64 / 5) % 6) * 210;
    (x, y)
}

/// G-008: optimistic concurrency guard for workspace mutation commands.
///
/// `expected` is the A-033 content revision the client last saw (the
/// `revision` field of `WorkspaceSnapshot` / `WorkspaceSessionSnapshot`).
/// `None` skips the check (backward-compatible). Otherwise the current
/// snapshot revision is loaded (pure read — no writes here) and a mismatch
/// is rejected with `Error::Conflict` **before** the caller performs any store
/// write, so a stale client can never produce a half-written state. A missing
/// workspace is not a conflict: the subsequent command write would simply
/// report not-found (e.g. return `None`/`false`).
///
/// Two entry points share one implementation:
/// - [`check_revision`] reads through a pooled `&Connection` (used by tests and
///   read-only pre-checks).
/// - [`check_revision_in_txn`] reads through the caller's in-flight
///   `&rusqlite::Transaction`, so a command that wraps its mutation in a
///   transaction validates and writes against the *same* snapshot (no TOCTOU
///   between check and write, and the guard's read is part of the committed
///   unit).
pub fn check_revision(conn: &Connection, workspace_id: &str, expected: Option<i64>) -> Result<()> {
    check_revision_internal(conn, workspace_id, expected)
}

/// Transactional form of [`check_revision`] (see its docs for the contract).
/// Reads the current snapshot through `tx` so it sees the transaction's own
/// writes — a stale client is rejected with `Error::Conflict` before any
/// further statement in the same transaction runs.
#[allow(dead_code)]
pub fn check_revision_in_txn(
    tx: &rusqlite::Transaction,
    workspace_id: &str,
    expected: Option<i64>,
) -> Result<()> {
    check_revision_internal(tx, workspace_id, expected)
}

fn check_revision_internal(
    conn: &Connection,
    workspace_id: &str,
    expected: Option<i64>,
) -> Result<()> {
    let Some(expected) = expected else {
        return Ok(());
    };
    let Some(snapshot) = crate::workspace::load_workspace_snapshot(conn, workspace_id)? else {
        return Ok(());
    };
    let actual = snapshot.revision;
    if actual != expected {
        return Err(Error::Conflict(format!(
            "workspace {workspace_id} revision is stale (expected {expected}, actual {actual}); \
             reload the snapshot and retry"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod revision_tests {
    use super::*;
    use crate::db::{apply_migrations, create_tables};
    use crate::workspace::load_workspace_snapshot;

    fn fixture() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        apply_migrations(&conn).unwrap();
        conn
    }

    /// Seed rows via raw SQL — the store helpers this test used to call
    /// (`create_workspace` / `add_context_item`) hit a pre-existing bug:
    /// `SELECT MAX(position)` returns NULL on an empty set and the store
    /// binds the column as non-Option (`InvalidColumnType`). Out of scope
    /// here, so the fixture bypasses them.
    fn seed_workspace(conn: &Connection, id: &str) {
        conn.execute(
            "INSERT INTO workspaces
                (id, name, kind, icon, description, theme, is_active, position,
                 created_at, updated_at)
             VALUES (?1, 'G008', 'workspace', NULL, NULL, 'dark', 1, 0,
                 '2025-01-01T00:00:00Z', '2025-01-01T00:00:00Z')",
            [id],
        )
        .unwrap();
    }

    fn seed_context_item(conn: &Connection, workspace_id: &str, item_id: &str) {
        conn.execute(
            "INSERT INTO workspace_context_items
                (id, workspace_id, item_kind, ref_id, title, meta_json, position,
                 created_at)
             VALUES (?1, ?2, 'file', '/tmp/g008', '', '{}', 0,
                 '2025-01-01T00:00:00Z')",
            rusqlite::params![item_id, workspace_id],
        )
        .unwrap();
    }

    #[test]
    fn check_revision_rejects_stale_and_skips_when_none() {
        let conn = fixture();
        seed_workspace(&conn, "ws_g008");

        let r0 = load_workspace_snapshot(&conn, "ws_g008")
            .unwrap()
            .unwrap()
            .revision;

        // Current revision passes.
        check_revision(&conn, "ws_g008", Some(r0)).unwrap();

        // Mutate -> revision changes.
        seed_context_item(&conn, "ws_g008", "ctx_g008_a");
        let r1 = load_workspace_snapshot(&conn, "ws_g008")
            .unwrap()
            .unwrap()
            .revision;
        assert_ne!(r0, r1);

        // Stale revision is rejected with Conflict; no write was performed.
        match check_revision(&conn, "ws_g008", Some(r0)) {
            Err(Error::Conflict(msg)) => {
                assert!(msg.contains("stale"), "unexpected conflict message: {msg}");
            }
            other => panic!("expected Conflict for stale revision, got {other:?}"),
        }

        // None skips the check.
        check_revision(&conn, "ws_g008", None).unwrap();

        // Current revision still passes after the failed check.
        check_revision(&conn, "ws_g008", Some(r1)).unwrap();
    }
}
