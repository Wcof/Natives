//! Surface, Endpoint, and Window store (batch 5 CR-501).
//!
//! These tables are additive — no existing reader/writer is affected.
//! Backfill happens at migration v19 for existing apps and runtimes.

use super::model::{ApplicationSurface, BrowserBounds, RuntimeEndpoint, WindowInstance};
use crate::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

// ── Application Surfaces ─────────────────────────────────────────────

/// Create a surface for an application. Returns the new surface id.
pub fn create_surface(
    conn: &Connection,
    application_id: &str,
    kind: &str,
    label: &str,
    url: Option<&str>,
) -> Result<String> {
    let id = Uuid::new_v4().to_string();
    let t = now();
    // Ensure no duplicate main surface per application
    if kind == "main" {
        if let Some(existing) = find_main_surface(conn, application_id)? {
            // Update the existing main surface
            conn.execute(
                "UPDATE application_surfaces SET label = ?1, url = ?2, updated_at = ?3 WHERE id = ?4",
                params![label, url, t, existing],
            )
            .map_err(Error::Database)?;
            return Ok(existing);
        }
    }
    conn.execute(
        "INSERT INTO application_surfaces (id, application_id, kind, label, title, url, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6, ?6)",
        params![id, application_id, kind, label, url, t],
    )
    .map_err(Error::Database)?;
    Ok(id)
}

/// Find the main surface for an application.
pub fn find_main_surface(conn: &Connection, application_id: &str) -> Result<Option<String>> {
    conn.query_row(
        "SELECT id FROM application_surfaces WHERE application_id = ?1 AND kind = 'main'",
        params![application_id],
        |row| row.get(0),
    )
    .optional()
    .map_err(Error::Database)
}

/// List all surfaces for an application.
pub fn list_surfaces(conn: &Connection, application_id: &str) -> Result<Vec<ApplicationSurface>> {
    let mut stmt = conn.prepare(
        "SELECT id, application_id, kind, label, title, url, bounds_json, created_at, updated_at
         FROM application_surfaces WHERE application_id = ?1 ORDER BY created_at",
    )
    .map_err(Error::Database)?;
    let rows = stmt
        .query_map(params![application_id], |row| {
            Ok(ApplicationSurface {
                id: row.get(0)?,
                application_id: row.get(1)?,
                kind: row.get(2)?,
                label: row.get(3)?,
                title: row.get(4)?,
                url: row.get(5)?,
                bounds_json: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })
        .map_err(Error::Database)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(Error::Database)?);
    }
    Ok(out)
}

/// Update a surface's URL.
pub fn update_surface_url(conn: &Connection, surface_id: &str, url: &str) -> Result<()> {
    let t = now();
    conn.execute(
        "UPDATE application_surfaces SET url = ?1, updated_at = ?2 WHERE id = ?3",
        params![url, t, surface_id],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// Delete a surface.
pub fn delete_surface(conn: &Connection, surface_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM application_surfaces WHERE id = ?1",
        params![surface_id],
    )
    .map_err(Error::Database)?;
    Ok(())
}

// ── Runtime Endpoints ────────────────────────────────────────────────

/// Create an endpoint for a runtime instance.
pub fn create_endpoint(
    conn: &Connection,
    runtime_instance_id: &str,
    kind: &str,
    url: &str,
    port: Option<u16>,
) -> Result<String> {
    let id = Uuid::new_v4().to_string();
    let t = now();
    conn.execute(
        "INSERT INTO runtime_endpoints (id, runtime_instance_id, kind, url, port, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        params![id, runtime_instance_id, kind, url, port, t],
    )
    .map_err(Error::Database)?;
    Ok(id)
}

/// List all endpoints for a runtime instance.
pub fn list_endpoints(
    conn: &Connection,
    runtime_instance_id: &str,
) -> Result<Vec<RuntimeEndpoint>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, runtime_instance_id, kind, url, port, created_at, updated_at
         FROM runtime_endpoints WHERE runtime_instance_id = ?1 ORDER BY created_at",
        )
        .map_err(Error::Database)?;
    let rows = stmt
        .query_map(params![runtime_instance_id], |row| {
            Ok(RuntimeEndpoint {
                id: row.get(0)?,
                runtime_instance_id: row.get(1)?,
                kind: row.get(2)?,
                url: row.get(3)?,
                port: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })
        .map_err(Error::Database)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(Error::Database)?);
    }
    Ok(out)
}

/// Delete all endpoints for a runtime instance.
pub fn clear_endpoints(conn: &Connection, runtime_instance_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM runtime_endpoints WHERE runtime_instance_id = ?1",
        params![runtime_instance_id],
    )
    .map_err(Error::Database)?;
    Ok(())
}

// ── Window Instances ─────────────────────────────────────────────────

/// Row projection shared by every window query.
fn row_to_window(r: &rusqlite::Row<'_>) -> rusqlite::Result<WindowInstance> {
    Ok(WindowInstance {
        id: r.get(0)?,
        application_id: r.get(1)?,
        surface_id: r.get(2)?,
        runtime_instance_id: r.get(3)?,
        label: r.get(4)?,
        state: r.get(5)?,
        bounds_json: r.get(6)?,
        url: r.get(7)?,
        last_error: r.get(8)?,
        reconcile_state: r.get(9)?,
        created_at: r.get(10)?,
        updated_at: r.get(11)?,
    })
}

const WINDOW_COLUMNS: &str =
    "id, application_id, surface_id, runtime_instance_id, label, state, bounds_json, url, last_error, reconcile_state, created_at, updated_at";

/// Create a window instance. Returns the new window id.
///
/// The WebView label is derived from the window id (`creative-window-{id}`) so
/// every window owns a unique label and one app can host multiple child
/// WebViews (T07 window-id label invariant). A caller-supplied label would
/// allow two windows to collide, so the store derives it.
pub fn create_window(
    conn: &Connection,
    application_id: &str,
    surface_id: &str,
    runtime_instance_id: Option<&str>,
) -> Result<String> {
    let id = Uuid::new_v4().to_string();
    let label = crate::creative_app::browser::window_label(&id);
    let t = now();
    conn.execute(
        "INSERT INTO window_instances (id, application_id, surface_id, runtime_instance_id, label, state, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'closed', ?6, ?6)",
        params![id, application_id, surface_id, runtime_instance_id, label, t],
    )
    .map_err(Error::Database)?;
    Ok(id)
}

/// Find a window by its id.
pub fn find_window(conn: &Connection, window_id: &str) -> Result<Option<WindowInstance>> {
    conn.query_row(
        &format!("SELECT {WINDOW_COLUMNS} FROM window_instances WHERE id = ?1"),
        params![window_id],
        row_to_window,
    )
    .optional()
    .map_err(Error::Database)
}

/// Find the most recent window for an app surface (reuse target on reopen).
/// State-agnostic so a reopened surface reuses its last window instead of
/// stacking a new row on every open.
pub fn find_latest_surface_window(
    conn: &Connection,
    application_id: &str,
    surface_id: &str,
) -> Result<Option<WindowInstance>> {
    conn.query_row(
        &format!(
            "SELECT {WINDOW_COLUMNS} FROM window_instances
             WHERE application_id = ?1 AND surface_id = ?2
             ORDER BY created_at DESC LIMIT 1"
        ),
        params![application_id, surface_id],
        row_to_window,
    )
    .optional()
    .map_err(Error::Database)
}

/// Find a window by its label (unique per Tauri lifecycle).
pub fn find_window_by_label(conn: &Connection, label: &str) -> Result<Option<WindowInstance>> {
    conn.query_row(
        &format!("SELECT {WINDOW_COLUMNS} FROM window_instances WHERE label = ?1"),
        params![label],
        row_to_window,
    )
    .optional()
    .map_err(Error::Database)
}

/// List all windows for an application.
pub fn list_windows(conn: &Connection, application_id: &str) -> Result<Vec<WindowInstance>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {WINDOW_COLUMNS} FROM window_instances WHERE application_id = ?1 ORDER BY created_at"
        ))
        .map_err(Error::Database)?;
    let rows = stmt
        .query_map(params![application_id], row_to_window)
        .map_err(Error::Database)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(Error::Database)?);
    }
    Ok(out)
}

/// List every window across all applications (reconcile sweep).
pub fn list_all_windows(conn: &Connection) -> Result<Vec<WindowInstance>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {WINDOW_COLUMNS} FROM window_instances ORDER BY created_at"
        ))
        .map_err(Error::Database)?;
    let rows = stmt.query_map([], row_to_window).map_err(Error::Database)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(Error::Database)?);
    }
    Ok(out)
}

/// Update window state. A 0-row update on an existing id is a typed conflict,
/// never a silent success (T07: DB writes must be observable).
pub fn update_window_state(conn: &Connection, window_id: &str, state: &str) -> Result<()> {
    let t = now();
    let n = conn
        .execute(
            "UPDATE window_instances SET state = ?1, updated_at = ?2 WHERE id = ?3",
            params![state, t, window_id],
        )
        .map_err(Error::Database)?;
    if n == 0 {
        return Err(Error::Conflict(format!(
            "window {window_id} vanished before state update"
        )));
    }
    Ok(())
}

/// Transactionally mark a window OPEN and bind the preview target to its
/// runtime instance. Window row + preview bind commit or roll back together;
/// a vanished row surfaces as a typed conflict.
pub fn commit_window_open(
    conn: &mut Connection,
    window_id: &str,
    runtime_instance_id: &str,
    url: &str,
    bounds: &BrowserBounds,
) -> Result<()> {
    let tx = conn.transaction().map_err(Error::Database)?;
    let bounds_json = serde_json::to_string(bounds).ok();
    let n = tx
        .execute(
            "UPDATE window_instances SET state = 'open', runtime_instance_id = ?5, url = ?2,
                    bounds_json = ?3, reconcile_state = 'ok', last_error = NULL, updated_at = ?4
             WHERE id = ?1",
            params![window_id, url, bounds_json, now(), runtime_instance_id],
        )
        .map_err(Error::Database)?;
    if n == 0 {
        return Err(Error::Conflict(format!(
            "window {window_id} vanished before commit"
        )));
    }
    super::runtime_store::upsert_preview_target(&tx, runtime_instance_id, url, "child_webview")?;
    tx.commit().map_err(Error::Database)
}

/// Transactionally mark a window CLOSED and clear its preview bind. When the
/// real WebView was already gone (`was_missing`), the row records the reconcile
/// gap so the UI can show why the window is closed (honest state).
///
/// The runtime preview bind is dropped ONLY when no other window of the same
/// runtime still shows content — closing one of several windows must not make
/// the surviving windows look offline.
pub fn commit_window_closed(
    conn: &mut Connection,
    window_id: &str,
    runtime_instance_id: Option<&str>,
    was_missing: bool,
) -> Result<()> {
    let tx = conn.transaction().map_err(Error::Database)?;
    let reconcile = if was_missing { "missing" } else { "ok" };
    let last_error = if was_missing {
        Some("webview was already gone; reconciled to closed")
    } else {
        None
    };
    let n = tx
        .execute(
            "UPDATE window_instances SET state = 'closed', url = NULL,
                    reconcile_state = ?2, last_error = ?3, updated_at = ?4
             WHERE id = ?1",
            params![window_id, reconcile, last_error, now()],
        )
        .map_err(Error::Database)?;
    if n == 0 {
        return Err(Error::Conflict(format!(
            "window {window_id} vanished before close commit"
        )));
    }
    if let Some(iid) = runtime_instance_id {
        // Only drop the runtime preview when no other non-closed window still
        // binds to the same runtime instance.
        let other_open: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM window_instances
                 WHERE runtime_instance_id = ?1 AND state != 'closed' AND id != ?2",
                params![iid, window_id],
                |r| r.get(0),
            )
            .map_err(Error::Database)?;
        if other_open == 0 {
            tx.execute(
                "DELETE FROM preview_targets WHERE runtime_instance_id = ?1",
                params![iid],
            )
            .map_err(Error::Database)?;
        }
    }
    tx.commit().map_err(Error::Database)
}

/// Reconcile: a DB window whose real WebView is gone is marked CLOSED with the
/// reconcile outcome recorded. Lenient — a vanished row is nothing to fix.
pub fn reconcile_window_missing(conn: &Connection, window_id: &str, reason: &str) -> Result<()> {
    conn.execute(
        "UPDATE window_instances SET state = 'closed', url = NULL,
                reconcile_state = 'missing', last_error = ?2, updated_at = ?3
         WHERE id = ?1 AND state != 'closed'",
        params![window_id, reason, now()],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// Delete a window instance.
pub fn delete_window(conn: &Connection, window_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM window_instances WHERE id = ?1",
        params![window_id],
    )
    .map_err(Error::Database)?;
    Ok(())
}

// ── Backfill (migration v19) ─────────────────────────────────────────

/// Backfill main surfaces for existing applications that don't have one.
/// Also backfill active preview endpoints from runtime_instances.
pub fn backfill_v19(conn: &Connection) -> Result<()> {
    // Backfill main surfaces for every application without one
    let mut stmt = conn
        .prepare(
            "SELECT a.id, a.title FROM applications a
         WHERE a.id NOT IN (SELECT application_id FROM application_surfaces WHERE kind = 'main')",
        )
        .map_err(Error::Database)?;
    let apps: Vec<(String, String)> = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(Error::Database)?
        .filter_map(|r| r.ok())
        .collect();

    for (app_id, title) in &apps {
        create_surface(conn, app_id, "main", title, None)?;
    }

    // Backfill preview endpoints from active runtime_instances
    let mut stmt2 = conn
        .prepare(
            "SELECT ri.id, pt.url
         FROM runtime_instances ri
         JOIN preview_targets pt ON pt.runtime_instance_id = ri.id
         WHERE ri.status IN ('running', 'starting')
         AND NOT EXISTS (
             SELECT 1 FROM runtime_endpoints re WHERE re.runtime_instance_id = ri.id
         )",
        )
        .map_err(Error::Database)?;
    let endpoints: Vec<(String, String)> = stmt2
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(Error::Database)?
        .filter_map(|r| r.ok())
        .collect();

    for (rt_id, url) in &endpoints {
        create_endpoint(conn, rt_id, "preview", url, None)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn fixture() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        db::create_tables(&conn).unwrap();
        db::apply_migrations(&conn).unwrap();
        conn
    }

    #[test]
    fn create_and_list_surfaces() {
        let conn = fixture();
        // Create a test application
        conn.execute(
            "INSERT INTO applications (id, source, source_id, title, version, created_at, updated_at)
             VALUES ('app-1', 'local_project', 'src-1', 'Test App', '1', 't', 't')",
            [],
        )
        .unwrap();

        let sid = create_surface(
            &conn,
            "app-1",
            "main",
            "Main",
            Some("http://localhost:3000/"),
        )
        .unwrap();
        assert!(!sid.is_empty());

        let surfaces = list_surfaces(&conn, "app-1").unwrap();
        assert_eq!(surfaces.len(), 1);
        assert_eq!(surfaces[0].kind, "main");
        assert_eq!(surfaces[0].label, "Main");
        assert_eq!(surfaces[0].url.as_deref(), Some("http://localhost:3000/"));
    }

    #[test]
    fn main_surface_is_unique() {
        let conn = fixture();
        conn.execute(
            "INSERT INTO applications (id, source, source_id, title, version, created_at, updated_at)
             VALUES ('app-1', 'local_project', 'src-1', 'Test', '1', 't', 't')",
            [],
        )
        .unwrap();

        let s1 = create_surface(&conn, "app-1", "main", "Main", None).unwrap();
        // Second call with kind="main" should update, not create another
        let s2 = create_surface(
            &conn,
            "app-1",
            "main",
            "Main Updated",
            Some("http://localhost:8080/"),
        )
        .unwrap();
        assert_eq!(s1, s2, "main surface should be idempotent");

        let surfaces = list_surfaces(&conn, "app-1").unwrap();
        assert_eq!(surfaces.len(), 1);
        assert_eq!(surfaces[0].label, "Main Updated");
        assert_eq!(surfaces[0].url.as_deref(), Some("http://localhost:8080/"));
    }

    #[test]
    fn create_and_list_endpoints() {
        let conn = fixture();
        conn.execute(
            "INSERT INTO applications (id, source, source_id, title, version, created_at, updated_at)
             VALUES ('app-1', 'local_project', 'src-1', 'Test', '1', 't', 't')",
            [],
        )
        .unwrap();
        let app_id = "app-1";
        let iid =
            crate::creative_app::runtime_store::create_instance(&conn, app_id, None, "host_http")
                .unwrap();
        crate::creative_app::runtime_store::mark_running(&conn, &iid, &[], None, None, None)
            .unwrap();

        let eid =
            create_endpoint(&conn, &iid, "preview", "http://127.0.0.1:8080/", Some(8080)).unwrap();
        assert!(!eid.is_empty());

        let endpoints = list_endpoints(&conn, &iid).unwrap();
        assert_eq!(endpoints.len(), 1);
        assert_eq!(endpoints[0].url, "http://127.0.0.1:8080/");
    }

    #[test]
    fn create_and_list_windows() {
        let conn = fixture();
        conn.execute(
            "INSERT INTO applications (id, source, source_id, title, version, created_at, updated_at)
             VALUES ('app-1', 'local_project', 'src-1', 'Test', '1', 't', 't')",
            [],
        )
        .unwrap();
        let sid = create_surface(&conn, "app-1", "main", "Main", None).unwrap();

        let wid = create_window(&conn, "app-1", &sid, None).unwrap();
        assert!(!wid.is_empty());

        // Update state
        update_window_state(&conn, &wid, WindowInstance::STATE_OPEN).unwrap();

        let windows = list_windows(&conn, "app-1").unwrap();
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].state, WindowInstance::STATE_OPEN);
        // T07: the WebView label is derived from the window id — two windows
        // never share a label.
        assert_eq!(
            windows[0].label,
            crate::creative_app::browser::window_label(&windows[0].id)
        );
    }

    #[test]
    fn find_window_by_label_works() {
        let conn = fixture();
        conn.execute(
            "INSERT INTO applications (id, source, source_id, title, version, created_at, updated_at)
             VALUES ('app-1', 'local_project', 'src-1', 'Test', '1', 't', 't')",
            [],
        )
        .unwrap();
        let sid = create_surface(&conn, "app-1", "main", "Main", None).unwrap();
        let wid = create_window(&conn, "app-1", &sid, None).unwrap();
        let label = crate::creative_app::browser::window_label(&wid);

        let w = find_window_by_label(&conn, &label).unwrap();
        assert!(w.is_some());
        assert_eq!(w.unwrap().application_id, "app-1");

        let missing = find_window_by_label(&conn, "creative-window-nonexistent").unwrap();
        assert!(missing.is_none());
    }

    #[test]
    fn update_window_state_on_missing_row_is_typed_error() {
        let conn = fixture();
        // No window row exists; a strict update must not silently succeed.
        let err =
            update_window_state(&conn, "missing-window", WindowInstance::STATE_OPEN).unwrap_err();
        assert!(
            matches!(err, Error::Conflict(_)),
            "missing-row update must be a typed conflict, got {err}"
        );
    }

    #[test]
    fn backfill_creates_main_surfaces() {
        let conn = fixture();
        // Create applications without surfaces
        conn.execute(
            "INSERT INTO applications (id, source, source_id, title, version, created_at, updated_at)
             VALUES ('app-1', 'local_project', 'src-1', 'App One', '1', 't', 't')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO applications (id, source, source_id, title, version, created_at, updated_at)
             VALUES ('app-2', 'internal', 'src-2', 'App Two', '1', 't', 't')",
            [],
        )
        .unwrap();

        backfill_v19(&conn).unwrap();

        let surfaces1 = list_surfaces(&conn, "app-1").unwrap();
        assert_eq!(surfaces1.len(), 1);
        assert_eq!(surfaces1[0].kind, "main");

        let surfaces2 = list_surfaces(&conn, "app-2").unwrap();
        assert_eq!(surfaces2.len(), 1);
        assert_eq!(surfaces2[0].kind, "main");
    }

    #[test]
    fn backfill_is_idempotent() {
        let conn = fixture();
        conn.execute(
            "INSERT INTO applications (id, source, source_id, title, version, created_at, updated_at)
             VALUES ('app-1', 'local_project', 'src-1', 'App', '1', 't', 't')",
            [],
        )
        .unwrap();

        backfill_v19(&conn).unwrap();
        backfill_v19(&conn).unwrap(); // second call

        let surfaces = list_surfaces(&conn, "app-1").unwrap();
        assert_eq!(surfaces.len(), 1, "backfill must be idempotent");
    }
}
