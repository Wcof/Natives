//! Unified Application identity + RuntimeInstance bookkeeping (batch 1).
//!
//! The existing three-source tables (`modules` / `external_creative_apps` /
//! `local_creative_apps`) remain the source detail; this module gives every app
//! one `applications` identity, keeps the active `startup_plans` row, and records
//! `runtime_instances` so each app has at most one active runtime (the CAS batch
//! 2 promotes the instance to the real resource owner). `preview_targets` is
//! reserved for batch 6.

use super::model::*;
use crate::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn source_str(source: CreativeAppSource) -> &'static str {
    match source {
        CreativeAppSource::Internal => "internal",
        CreativeAppSource::ExternalGithub => "external_github",
        CreativeAppSource::LocalProject => "local_project",
    }
}

/// Map a runtime to its owner_kind (what owns the instance's resources).
pub fn owner_kind_for_runtime(runtime: CreativeAppRuntime) -> &'static str {
    match runtime {
        CreativeAppRuntime::WorkshopStatic => "host_http",
        CreativeAppRuntime::LocalStatic => "host_http",
        CreativeAppRuntime::NodeDevServer => "local_process",
        CreativeAppRuntime::DockerCompose => "docker_compose",
        CreativeAppRuntime::DockerRun => "docker_run",
    }
}

/// Resolve the unified application id for a source row; create the identity row
/// on demand for pre-migration / newly registered sources. Idempotent.
pub fn find_or_create_application(
    conn: &Connection,
    source: CreativeAppSource,
    source_id: &str,
) -> Result<String> {
    let src = source_str(source);
    if let Some(id) = lookup_application(conn, src, source_id)? {
        return Ok(id);
    }
    let id = Uuid::new_v4().to_string();
    let t = now();
    conn.execute(
        "INSERT OR IGNORE INTO applications (id, source, source_id, title, version, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, '1', ?5, ?5)",
        params![id, src, source_id, source_id, t],
    )
    .map_err(Error::Database)?;
    lookup_application(conn, src, source_id)?
        .ok_or_else(|| Error::Internal("application identity insert vanished".into()))
}

fn lookup_application(conn: &Connection, src: &str, source_id: &str) -> Result<Option<String>> {
    conn.query_row(
        "SELECT id FROM applications WHERE source = ?1 AND source_id = ?2",
        params![src, source_id],
        |r| r.get(0),
    )
    .optional()
    .map_err(Error::Database)
}

/// Active plan id for an application (startup_plans is_active).
pub fn active_plan_id(conn: &Connection, application_id: &str) -> Result<Option<String>> {
    conn.query_row(
        "SELECT id FROM startup_plans WHERE application_id = ?1 AND is_active = 1 LIMIT 1",
        params![application_id],
        |r| r.get(0),
    )
    .optional()
    .map_err(Error::Database)
}

/// Insert or replace the active startup plan (create / update paths).
pub fn upsert_active_plan(
    conn: &Connection,
    application_id: &str,
    plan_json: &str,
) -> Result<String> {
    let t = now();
    if let Some(id) = active_plan_id(conn, application_id)? {
        conn.execute(
            "UPDATE startup_plans SET plan_json = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, plan_json, t],
        )
        .map_err(Error::Database)?;
        return Ok(id);
    }
    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO startup_plans (id, application_id, plan_version, plan_json, is_active, created_at, updated_at)
         VALUES (?1, ?2, 1, ?3, 1, ?4, ?4)",
        params![id, application_id, plan_json, t],
    )
    .map_err(Error::Database)?;
    Ok(id)
}

/// Id of the active (non-terminal) runtime instance for an app, if any.
pub fn active_instance_id(conn: &Connection, application_id: &str) -> Result<Option<String>> {
    conn.query_row(
        "SELECT id FROM runtime_instances
         WHERE application_id = ?1 AND status IN ('starting','running','stopping')
         ORDER BY created_at DESC LIMIT 1",
        params![application_id],
        |r| r.get(0),
    )
    .optional()
    .map_err(Error::Database)
}

/// True when the app already has a non-terminal instance (double-start CAS).
pub fn has_active_instance(conn: &Connection, application_id: &str) -> Result<bool> {
    Ok(active_instance_id(conn, application_id)?.is_some())
}

/// Create a new instance in `starting`. Caller must have checked the CAS.
pub fn create_instance(
    conn: &Connection,
    application_id: &str,
    plan_id: Option<&str>,
    owner_kind: &str,
) -> Result<String> {
    let id = Uuid::new_v4().to_string();
    let t = now();
    conn.execute(
        "INSERT INTO runtime_instances
            (id, application_id, plan_id, status, cleanup_status, owner_kind, pgid, compose_project,
             resolved_urls_json, current_port, pid, failure, created_at, updated_at)
         VALUES (?1, ?2, ?3, 'starting', NULL, ?4, NULL, NULL, NULL, NULL, NULL, NULL, ?5, ?5)",
        params![id, application_id, plan_id, owner_kind, t],
    )
    .map_err(Error::Database)?;
    Ok(id)
}

pub fn mark_running(
    conn: &Connection,
    instance_id: &str,
    urls: &[String],
    port: Option<u16>,
    pgid: Option<i32>,
    pid: Option<u32>,
) -> Result<()> {
    let urls_json = if urls.is_empty() {
        None
    } else {
        Some(serde_json::json!(urls).to_string())
    };
    let ledger = serde_json::json!({
        "pgid": pgid,
        "pid": pid,
        "port": port,
        "urls": urls,
    });
    let ledger_json = serde_json::to_string(&ledger).unwrap_or_default();
    // Guard on `starting`: if a stop already moved the instance to stopping /
    // stopped, the start must not resurrect it (batch 2 start/stop race).
    conn.execute(
        "UPDATE runtime_instances
         SET status = 'running', cleanup_status = NULL, resolved_urls_json = ?2,
             current_port = ?3, pgid = ?4, pid = ?5, owner_pid = ?5,
             last_heartbeat = ?6, resource_ledger_json = ?7, failure = NULL, updated_at = ?6
         WHERE id = ?1 AND status = 'starting'",
        params![instance_id, urls_json, port, pgid, pid, now(), ledger_json],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// Refresh the instance heartbeat while its runtime is alive.
pub fn heartbeat(conn: &Connection, instance_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE runtime_instances SET last_heartbeat = ?2, updated_at = ?2 WHERE id = ?1",
        params![instance_id, now()],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// Record a natural process exit on the instance.
pub fn mark_exited(conn: &Connection, instance_id: &str, exit_code: i32) -> Result<()> {
    conn.execute(
        "UPDATE runtime_instances
         SET status = 'stopped', cleanup_status = 'completed', exit_code = ?2, updated_at = ?3
         WHERE id = ?1 AND status IN ('running','starting')",
        params![instance_id, exit_code, now()],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// Heartbeat the active instance of a source row (called from lifecycle poll).
pub fn heartbeat_for_source(
    conn: &Connection,
    source: CreativeAppSource,
    source_id: &str,
) -> Result<()> {
    if let Some(app_id) = lookup_application(conn, source_str(source), source_id)? {
        if let Some(iid) = active_instance_id(conn, &app_id)? {
            heartbeat(conn, &iid)?;
        }
    }
    Ok(())
}

/// Mark the active instance of a source row exited (natural process exit).
pub fn mark_exited_for_source(
    conn: &Connection,
    source: CreativeAppSource,
    source_id: &str,
    exit_code: i32,
) -> Result<()> {
    if let Some(app_id) = lookup_application(conn, source_str(source), source_id)? {
        if let Some(iid) = active_instance_id(conn, &app_id)? {
            mark_exited(conn, &iid, exit_code)?;
        }
    }
    Ok(())
}

/// Settle the active instance of a source row to a status string during
/// reconcile (crash recovery: orphaned / stopped / failed mirror the app row).
pub fn settle_instance(
    conn: &Connection,
    source: CreativeAppSource,
    source_id: &str,
    status: &str,
) -> Result<()> {
    if let Some(app_id) = lookup_application(conn, source_str(source), source_id)? {
        if let Some(iid) = active_instance_id(conn, &app_id)? {
            conn.execute(
                "UPDATE runtime_instances SET status = ?2, updated_at = ?3 WHERE id = ?1",
                params![iid, status, now()],
            )
            .map_err(Error::Database)?;
        }
    }
    Ok(())
}

pub fn mark_stopping(conn: &Connection, instance_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE runtime_instances SET status = 'stopping', updated_at = ?2 WHERE id = ?1",
        params![instance_id, now()],
    )
    .map_err(Error::Database)?;
    Ok(())
}

pub fn mark_stopped(conn: &Connection, instance_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE runtime_instances
         SET status = 'stopped', cleanup_status = 'completed', failure = NULL, updated_at = ?2
         WHERE id = ?1",
        params![instance_id, now()],
    )
    .map_err(Error::Database)?;
    Ok(())
}

pub fn mark_failed(conn: &Connection, instance_id: &str, failure: &str) -> Result<()> {
    conn.execute(
        "UPDATE runtime_instances
         SET status = 'failed', cleanup_status = 'failed', failure = ?2, updated_at = ?3
         WHERE id = ?1 AND status = 'starting'",
        params![instance_id, failure, now()],
    )
    .map_err(Error::Database)?;
    Ok(())
}

pub fn mark_cleanup_failed(conn: &Connection, instance_id: &str, failure: &str) -> Result<()> {
    conn.execute(
        "UPDATE runtime_instances
         SET status = 'cleanup_failed', cleanup_status = 'failed', failure = ?2, updated_at = ?3
         WHERE id = ?1",
        params![instance_id, failure, now()],
    )
    .map_err(Error::Database)?;
    Ok(())
}

pub fn mark_orphaned(conn: &Connection, instance_id: &str, failure: &str) -> Result<()> {
    conn.execute(
        "UPDATE runtime_instances
         SET status = 'orphaned', cleanup_status = NULL, failure = ?2, updated_at = ?3
         WHERE id = ?1",
        params![instance_id, failure, now()],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// Delete the application identity and its runtime records. Source detail rows
/// are deleted by their own adapters (never delete user project files).
pub fn delete_application(
    conn: &Connection,
    source: CreativeAppSource,
    source_id: &str,
) -> Result<()> {
    let application_id = match lookup_application(conn, source_str(source), source_id)? {
        Some(id) => id,
        None => return Ok(()),
    };
    conn.execute(
        "DELETE FROM preview_targets WHERE runtime_instance_id IN (SELECT id FROM runtime_instances WHERE application_id = ?1)",
        params![application_id],
    )
    .map_err(Error::Database)?;
    conn.execute(
        "DELETE FROM runtime_instances WHERE application_id = ?1",
        params![application_id],
    )
    .map_err(Error::Database)?;
    conn.execute(
        "DELETE FROM startup_plans WHERE application_id = ?1",
        params![application_id],
    )
    .map_err(Error::Database)?;
    conn.execute(
        "DELETE FROM applications WHERE id = ?1",
        params![application_id],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// Fill `application_id` / `runtime_instance_id` on a summary before it crosses
/// to the Renderer. Source detail stays authoritative for state and title.
pub fn attach_identity(
    conn: &Connection,
    mut summary: CreativeAppSummary,
) -> Result<CreativeAppSummary> {
    let application_id = find_or_create_application(conn, summary.source, &summary.id)?;
    summary.application_id = application_id.clone();
    summary.runtime_instance_id = active_instance_id(conn, &application_id)?;
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{apply_migrations, create_tables};

    fn mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        apply_migrations(&conn).unwrap();
        conn
    }

    fn count(conn: &Connection, sql: &str) -> i64 {
        conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn identity_is_idempotent_and_stable() {
        let conn = mem();
        let a1 =
            find_or_create_application(&conn, CreativeAppSource::LocalProject, "loc1").unwrap();
        let a2 =
            find_or_create_application(&conn, CreativeAppSource::LocalProject, "loc1").unwrap();
        assert_eq!(a1, a2, "same source id must map to one application id");
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM applications"), 1);
    }

    #[test]
    fn start_cas_keeps_one_active_instance() {
        let conn = mem();
        let app =
            find_or_create_application(&conn, CreativeAppSource::LocalProject, "loc1").unwrap();
        assert!(!has_active_instance(&conn, &app).unwrap());

        let i1 = create_instance(&conn, &app, None, "local_process").unwrap();
        assert!(has_active_instance(&conn, &app).unwrap());
        assert_eq!(
            active_instance_id(&conn, &app).unwrap().as_deref(),
            Some(i1.as_str())
        );

        // The adapter CAS rejects a second start while an instance is active;
        // the store contract is "at most one active" — active_instance_id points
        // at the running one and has_active_instance stays true.
        assert!(has_active_instance(&conn, &app).unwrap());

        mark_stopping(&conn, &i1).unwrap();
        mark_stopped(&conn, &i1).unwrap();
        assert!(!has_active_instance(&conn, &app).unwrap());
        assert!(active_instance_id(&conn, &app).unwrap().is_none());
    }

    #[test]
    fn restart_creates_new_instance_and_old_is_completed() {
        let conn = mem();
        let app =
            find_or_create_application(&conn, CreativeAppSource::LocalProject, "loc1").unwrap();
        let i1 = create_instance(&conn, &app, None, "local_process").unwrap();
        mark_running(
            &conn,
            &i1,
            &["http://127.0.0.1:5173/".into()],
            Some(5173),
            Some(100),
            Some(42),
        )
        .unwrap();
        // Restart = stop old then start new.
        mark_stopping(&conn, &i1).unwrap();
        mark_stopped(&conn, &i1).unwrap();

        let i2 = create_instance(&conn, &app, None, "local_process").unwrap();
        assert_eq!(
            active_instance_id(&conn, &app).unwrap().as_deref(),
            Some(i2.as_str())
        );

        let old_status: String = conn
            .query_row(
                "SELECT status FROM runtime_instances WHERE id = ?1",
                params![i1],
                |r| r.get(0),
            )
            .unwrap();
        let old_cleanup: Option<String> = conn
            .query_row(
                "SELECT cleanup_status FROM runtime_instances WHERE id = ?1",
                params![i1],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(old_status, "stopped");
        assert_eq!(old_cleanup.as_deref(), Some("completed"));
    }

    #[test]
    fn attach_identity_binds_summary_and_active_instance() {
        let conn = mem();
        let mut summary = CreativeAppSummary {
            id: "loc1".into(),
            application_id: String::new(),
            runtime_instance_id: None,
            source: CreativeAppSource::LocalProject,
            runtime: CreativeAppRuntime::LocalStatic,
            title: "Local".into(),
            description: None,
            icon: None,
            version: "1".into(),
            state: CreativeAppState::InstalledStopped,
            open_url: None,
            repository_url: None,
            last_error: None,
            status_detail: None,
            local_project: None,
            actions: CreativeAppActions::default(),
        };
        let bound = attach_identity(&conn, summary.clone()).unwrap();
        assert_eq!(
            bound.application_id,
            find_or_create_application(&conn, CreativeAppSource::LocalProject, "loc1").unwrap()
        );
        assert!(bound.runtime_instance_id.is_none());

        let app = bound.application_id.clone();
        let iid = create_instance(&conn, &app, None, "host_http").unwrap();
        let bound2 = attach_identity(&conn, summary).unwrap();
        assert_eq!(bound2.runtime_instance_id.as_deref(), Some(iid.as_str()));
    }

    /// Batch 2 race guard: a start must never resurrect an instance that a
    /// concurrent stop already settled to stopped.
    #[test]
    fn mark_running_does_not_resurrect_stopped_instance() {
        let conn = mem();
        let app =
            find_or_create_application(&conn, CreativeAppSource::LocalProject, "loc1").unwrap();
        let iid = create_instance(&conn, &app, None, "local_process").unwrap();
        // Stop wins the race: instance is stopping then stopped.
        mark_stopping(&conn, &iid).unwrap();
        mark_stopped(&conn, &iid).unwrap();

        // Late start health pass must NOT flip it back to running.
        mark_running(
            &conn,
            &iid,
            &["http://127.0.0.1:5173/".into()],
            Some(5173),
            None,
            None,
        )
        .unwrap();
        let status: String = conn
            .query_row(
                "SELECT status FROM runtime_instances WHERE id = ?1",
                params![iid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            status, "stopped",
            "mark_running must be guarded on starting"
        );
    }

    /// Natural process exit settles the instance with its exit code.
    #[test]
    fn mark_exited_records_natural_exit() {
        let conn = mem();
        let app =
            find_or_create_application(&conn, CreativeAppSource::LocalProject, "loc1").unwrap();
        let iid = create_instance(&conn, &app, None, "local_process").unwrap();
        mark_running(&conn, &iid, &[], None, Some(7), Some(42)).unwrap();

        let (pid, owner_pid): (Option<i64>, Option<i64>) = conn
            .query_row(
                "SELECT pid, owner_pid FROM runtime_instances WHERE id = ?1",
                params![iid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(pid, Some(42));
        assert_eq!(owner_pid, Some(42));

        mark_exited(&conn, &iid, 3).unwrap();
        let (status, code): (String, Option<i64>) = conn
            .query_row(
                "SELECT status, exit_code FROM runtime_instances WHERE id = ?1",
                params![iid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "stopped");
        assert_eq!(code, Some(3));
        assert!(active_instance_id(&conn, &app).unwrap().is_none());
    }
}

/// Owner kind for an external app row (docker_compose vs docker_run).
pub fn external_owner_kind(conn: &Connection, source_id: &str) -> Result<&'static str> {
    let cfg_json: String = conn
        .query_row(
            "SELECT runtime_config_json FROM external_creative_apps WHERE id = ?1",
            params![source_id],
            |r| r.get(0),
        )
        .map_err(Error::Database)?;
    let kind = serde_json::from_str::<serde_json::Value>(&cfg_json)
        .ok()
        .and_then(|v| v.get("kind").and_then(|k| k.as_str()).map(str::to_string));
    Ok(if kind.as_deref() == Some("docker_compose") {
        "docker_compose"
    } else {
        "docker_run"
    })
}

/// Owner kind for a local app row (host_http vs local_process).
pub fn local_owner_kind(conn: &Connection, source_id: &str) -> Result<&'static str> {
    let plan_json: String = conn
        .query_row(
            "SELECT launch_plan_json FROM local_creative_apps WHERE id = ?1",
            params![source_id],
            |r| r.get(0),
        )
        .map_err(Error::Database)?;
    let runtime = serde_json::from_str::<serde_json::Value>(&plan_json)
        .ok()
        .and_then(|v| {
            v.get("runtime")
                .and_then(|r| r.as_str())
                .map(str::to_string)
        });
    Ok(match runtime.as_deref() {
        Some("static_http") => "host_http",
        Some("docker_compose") => "docker_compose",
        _ => "local_process",
    })
}

/// Port / pgid / pid hint for a local app row (mirrored onto the instance).
pub fn local_instance_hint(
    conn: &Connection,
    source_id: &str,
) -> Result<(Option<u16>, Option<i32>, Option<u32>)> {
    let row: (Option<i64>, Option<String>) = conn
        .query_row(
            "SELECT current_port, process_identity_json FROM local_creative_apps WHERE id = ?1",
            params![source_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(Error::Database)?;
    let (pgid, pid) = crate::db::parse_identity(row.1.as_deref());
    Ok((row.0.map(|p| p as u16), pgid, pid))
}

/// Port hint for an external app row.
pub fn external_instance_hint(
    conn: &Connection,
    source_id: &str,
) -> Result<(Option<u16>, Option<i32>, Option<u32>)> {
    let port: Option<i64> = conn
        .query_row(
            "SELECT host_port FROM external_creative_apps WHERE id = ?1",
            params![source_id],
            |r| r.get(0),
        )
        .map_err(Error::Database)?;
    Ok((port.map(|p| p as u16), None, None))
}
