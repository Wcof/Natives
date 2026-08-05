//! ServiceInstance store (batch 7 CR-701).
//!
//! A runtime instance can expose multiple services (Compose web + db); each
//! gets a ServiceInstance row with typed readiness. Single-service runtimes
//! get a "main" service row.

use super::model::ServiceInstance;
use crate::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Create a service instance for a runtime. Returns the new service id.
pub fn create_service(
    conn: &Connection,
    runtime_instance_id: &str,
    name: &str,
    required: bool,
) -> Result<String> {
    let id = Uuid::new_v4().to_string();
    let t = now();
    conn.execute(
        "INSERT INTO service_instances (id, runtime_instance_id, name, readiness, required, endpoint_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, 'starting', ?4, NULL, ?5, ?5)
         ON CONFLICT(runtime_instance_id, name) DO UPDATE SET updated_at = excluded.updated_at",
        params![id, runtime_instance_id, name, required as i32, t],
    )
    .map_err(Error::Database)?;
    // Return the existing id if this was a conflict
    Ok(conn
        .query_row(
            "SELECT id FROM service_instances WHERE runtime_instance_id = ?1 AND name = ?2",
            params![runtime_instance_id, name],
            |r| r.get(0),
        )
        .optional()
        .map_err(Error::Database)?
        .unwrap_or(id))
}

/// List all services for a runtime instance.
pub fn list_services(
    conn: &Connection,
    runtime_instance_id: &str,
) -> Result<Vec<ServiceInstance>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, runtime_instance_id, name, readiness, required, endpoint_id, created_at, updated_at
             FROM service_instances WHERE runtime_instance_id = ?1 ORDER BY created_at",
        )
        .map_err(Error::Database)?;
    let rows = stmt
        .query_map(params![runtime_instance_id], |row| {
            Ok(ServiceInstance {
                id: row.get(0)?,
                runtime_instance_id: row.get(1)?,
                name: row.get(2)?,
                readiness: row.get(3)?,
                required: row.get::<_, i32>(4)? != 0,
                endpoint_id: row.get(5)?,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        })
        .map_err(Error::Database)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(Error::Database)?);
    }
    Ok(out)
}

/// Update a service's readiness state.
pub fn update_service_readiness(
    conn: &Connection,
    service_id: &str,
    readiness: &str,
) -> Result<()> {
    let t = now();
    conn.execute(
        "UPDATE service_instances SET readiness = ?1, updated_at = ?2 WHERE id = ?3",
        params![readiness, t, service_id],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// Bind a service to its endpoint.
pub fn bind_service_endpoint(
    conn: &Connection,
    service_id: &str,
    endpoint_id: &str,
) -> Result<()> {
    let t = now();
    conn.execute(
        "UPDATE service_instances SET endpoint_id = ?1, updated_at = ?2 WHERE id = ?3",
        params![endpoint_id, t, service_id],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// Backfill a "main" service row for active runtime instances (CR-701).
pub fn backfill_v22(conn: &Connection) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO service_instances (id, runtime_instance_id, name, readiness, required, endpoint_id, created_at, updated_at)
         SELECT 'main-' || ri.id, ri.id, 'main', 'starting', 1, NULL, ri.created_at, ri.updated_at
         FROM runtime_instances ri
         WHERE ri.status IN ('running', 'starting', 'stopping')
         AND NOT EXISTS (SELECT 1 FROM service_instances s WHERE s.runtime_instance_id = ri.id)",
        [],
    )
    .map_err(Error::Database)?;
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
    fn create_and_list_services() {
        let conn = fixture();
        conn.execute(
            "INSERT INTO applications (id, source, source_id, title, version, created_at, updated_at)
             VALUES ('app-1', 'local_project', 'src-1', 'Test', '1', 't', 't')",
            [],
        )
        .unwrap();
        let iid = crate::creative_app::runtime_store::create_instance(&conn, "app-1", None, "host_http")
            .unwrap();
        crate::creative_app::runtime_store::mark_running(&conn, &iid, &[], None, None, None)
            .unwrap();

        let sid = create_service(&conn, &iid, "main", true).unwrap();
        assert!(!sid.is_empty());

        let services = list_services(&conn, &iid).unwrap();
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].name, "main");
        assert!(services[0].required);
        assert_eq!(services[0].readiness, "starting");
    }

    #[test]
    fn multi_service_compose() {
        let conn = fixture();
        conn.execute(
            "INSERT INTO applications (id, source, source_id, title, version, created_at, updated_at)
             VALUES ('app-1', 'local_project', 'src-1', 'Test', '1', 't', 't')",
            [],
        )
        .unwrap();
        let iid = crate::creative_app::runtime_store::create_instance(&conn, "app-1", None, "docker_compose")
            .unwrap();
        crate::creative_app::runtime_store::mark_running(&conn, &iid, &[], None, None, None)
            .unwrap();

        create_service(&conn, &iid, "web", true).unwrap();
        create_service(&conn, &iid, "db", false).unwrap();

        let services = list_services(&conn, &iid).unwrap();
        assert_eq!(services.len(), 2);
        let web = services.iter().find(|s| s.name == "web").unwrap();
        assert!(web.required);
        let db = services.iter().find(|s| s.name == "db").unwrap();
        assert!(!db.required);
    }

    #[test]
    fn update_readiness_and_bind_endpoint() {
        let conn = fixture();
        conn.execute(
            "INSERT INTO applications (id, source, source_id, title, version, created_at, updated_at)
             VALUES ('app-1', 'local_project', 'src-1', 'Test', '1', 't', 't')",
            [],
        )
        .unwrap();
        let iid = crate::creative_app::runtime_store::create_instance(&conn, "app-1", None, "host_http")
            .unwrap();
        crate::creative_app::runtime_store::mark_running(&conn, &iid, &[], None, None, None)
            .unwrap();
        let sid = create_service(&conn, &iid, "main", true).unwrap();

        update_service_readiness(&conn, &sid, ServiceInstance::READY_READY).unwrap();
        bind_service_endpoint(&conn, &sid, "ep-1").unwrap();

        let services = list_services(&conn, &iid).unwrap();
        assert_eq!(services[0].readiness, "ready");
        assert_eq!(services[0].endpoint_id.as_deref(), Some("ep-1"));
    }

    #[test]
    fn backfill_creates_main_service() {
        let conn = fixture();
        conn.execute(
            "INSERT INTO applications (id, source, source_id, title, version, created_at, updated_at)
             VALUES ('app-1', 'local_project', 'src-1', 'Test', '1', 't', 't')",
            [],
        )
        .unwrap();
        let iid = crate::creative_app::runtime_store::create_instance(&conn, "app-1", None, "host_http")
            .unwrap();
        crate::creative_app::runtime_store::mark_running(&conn, &iid, &[], None, None, None)
            .unwrap();

        backfill_v22(&conn).unwrap();
        let services = list_services(&conn, &iid).unwrap();
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].name, "main");

        // Idempotent
        backfill_v22(&conn).unwrap();
        assert_eq!(list_services(&conn, &iid).unwrap().len(), 1);
    }
}