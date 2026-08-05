//! Surface, Endpoint, and Window store (batch 5 CR-501).
//!
//! These tables are additive — no existing reader/writer is affected.
//! Backfill happens at migration v19 for existing apps and runtimes.

use super::model::{ApplicationSurface, RuntimeEndpoint, WindowInstance};
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

/// Create a window instance. Returns the new window id.
pub fn create_window(
    conn: &Connection,
    application_id: &str,
    surface_id: &str,
    runtime_instance_id: Option<&str>,
    label: &str,
) -> Result<String> {
    let id = Uuid::new_v4().to_string();
    let t = now();
    conn.execute(
        "INSERT INTO window_instances (id, application_id, surface_id, runtime_instance_id, label, state, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'closed', ?6, ?6)",
        params![id, application_id, surface_id, runtime_instance_id, label, t],
    )
    .map_err(Error::Database)?;
    Ok(id)
}

/// Find a window by its label (unique per Tauri lifecycle).
pub fn find_window_by_label(conn: &Connection, label: &str) -> Result<Option<WindowInstance>> {
    conn.query_row(
        "SELECT id, application_id, surface_id, runtime_instance_id, label, state, bounds_json, created_at, updated_at
         FROM window_instances WHERE label = ?1",
        params![label],
        |row| {
            Ok(WindowInstance {
                id: row.get(0)?,
                application_id: row.get(1)?,
                surface_id: row.get(2)?,
                runtime_instance_id: row.get(3)?,
                label: row.get(4)?,
                state: row.get(5)?,
                bounds_json: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        },
    )
    .optional()
    .map_err(Error::Database)
}

/// List all windows for an application.
pub fn list_windows(conn: &Connection, application_id: &str) -> Result<Vec<WindowInstance>> {
    let mut stmt = conn.prepare(
        "SELECT id, application_id, surface_id, runtime_instance_id, label, state, bounds_json, created_at, updated_at
         FROM window_instances WHERE application_id = ?1 ORDER BY created_at",
    )
    .map_err(Error::Database)?;
    let rows = stmt
        .query_map(params![application_id], |row| {
            Ok(WindowInstance {
                id: row.get(0)?,
                application_id: row.get(1)?,
                surface_id: row.get(2)?,
                runtime_instance_id: row.get(3)?,
                label: row.get(4)?,
                state: row.get(5)?,
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

/// Update window state.
pub fn update_window_state(conn: &Connection, window_id: &str, state: &str) -> Result<()> {
    let t = now();
    conn.execute(
        "UPDATE window_instances SET state = ?1, updated_at = ?2 WHERE id = ?3",
        params![state, t, window_id],
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

        let wid = create_window(&conn, "app-1", &sid, None, "creative-app-test").unwrap();
        assert!(!wid.is_empty());

        // Update state
        update_window_state(&conn, &wid, WindowInstance::STATE_OPEN).unwrap();

        let windows = list_windows(&conn, "app-1").unwrap();
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].state, WindowInstance::STATE_OPEN);
        assert_eq!(windows[0].label, "creative-app-test");
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
        create_window(&conn, "app-1", &sid, None, "creative-app-my-app").unwrap();

        let w = find_window_by_label(&conn, "creative-app-my-app").unwrap();
        assert!(w.is_some());
        assert_eq!(w.unwrap().application_id, "app-1");

        let missing = find_window_by_label(&conn, "creative-app-nonexistent").unwrap();
        assert!(missing.is_none());
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
