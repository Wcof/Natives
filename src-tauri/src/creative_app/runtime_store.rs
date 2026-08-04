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

/// Read-only application identity lookup for a source row. Never creates a row.
///
/// Identity rows are created by registration / lifecycle write paths and by the
/// migration backfill; catalog reads, open, and close must NOT fabricate an
/// identity (batch 1, #01).
pub fn application_id_for(
    conn: &Connection,
    source: CreativeAppSource,
    source_id: &str,
) -> Result<Option<String>> {
    lookup_application(conn, source_str(source), source_id)
}

/// Active plan id for an application (startup_plans is_active). Deterministic
/// tie-break so migration dedup and writers agree on the newest active plan.
pub fn active_plan_id(conn: &Connection, application_id: &str) -> Result<Option<String>> {
    conn.query_row(
        "SELECT id FROM startup_plans WHERE application_id = ?1 AND is_active = 1
         ORDER BY updated_at DESC, id LIMIT 1",
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
///
/// `cleanup_failed` / `orphaned` are active-like (batch 1 CR-102): they block a
/// new start until the resources are proven released or the user completes
/// external takeover, so they must be visible to the same "has an active
/// instance" guard that `starting`/`running`/`stopping` use.
pub fn active_instance_id(conn: &Connection, application_id: &str) -> Result<Option<String>> {
    conn.query_row(
        "SELECT id FROM runtime_instances
         WHERE application_id = ?1 AND status IN
             ('starting','running','stopping','cleanup_failed','orphaned')
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

/// True when a rusqlite error is a UNIQUE constraint violation. The partial
/// unique indexes added by migration v16 make "one active instance / one active
/// plan per application" a database invariant, so a racing second insert is
/// rejected here instead of depending on a process-level pre-check.
fn is_unique_constraint(e: &rusqlite::Error) -> bool {
    matches!(e, rusqlite::Error::SqliteFailure(err, _)
        if err.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE)
}

/// Fail with `NotFound` when the instance row does not exist, so a 0-row update
/// on a live row can be reported as a conflict instead of silent success (#07).
fn ensure_instance_exists(conn: &Connection, instance_id: &str) -> Result<()> {
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM runtime_instances WHERE id = ?1)",
            params![instance_id],
            |r| r.get(0),
        )
        .map_err(Error::Database)?;
    if !exists {
        return Err(Error::NotFound(format!("runtime instance {instance_id}")));
    }
    Ok(())
}

fn current_status(conn: &Connection, instance_id: &str) -> Result<String> {
    conn.query_row(
        "SELECT status FROM runtime_instances WHERE id = ?1",
        params![instance_id],
        |r| r.get(0),
    )
    .map_err(Error::Database)
}

/// Convert a guarded UPDATE's affected-row count into a typed error: 0 rows on
/// an existing row means the instance is in an unexpected status for that
/// transition (conflict), never a silent success.
fn expect_transition(affected: usize, instance_id: &str, from: &[&str], to: &str) -> Result<()> {
    if affected == 0 {
        return Err(Error::Conflict(format!(
            "runtime instance {instance_id} cannot transition to {to} from current status (expected one of {from:?})"
        )));
    }
    Ok(())
}

/// Create a new instance in `starting`. Caller may pre-check the CAS, but the
/// database partial unique index is the authoritative guard: a second active
/// instance for the same application is rejected with a typed conflict even
/// under concurrent writers (#06).
pub fn create_instance(
    conn: &Connection,
    application_id: &str,
    plan_id: Option<&str>,
    owner_kind: &str,
) -> Result<String> {
    let id = Uuid::new_v4().to_string();
    let t = now();
    let res = conn.execute(
        "INSERT INTO runtime_instances
            (id, application_id, plan_id, status, cleanup_status, owner_kind, pgid, compose_project,
             resolved_urls_json, current_port, pid, failure, created_at, updated_at)
         VALUES (?1, ?2, ?3, 'starting', NULL, ?4, NULL, NULL, NULL, NULL, NULL, NULL, ?5, ?5)",
        params![id, application_id, plan_id, owner_kind, t],
    );
    match res {
        Ok(_) => Ok(id),
        Err(e) if is_unique_constraint(&e) => Err(Error::Conflict(format!(
            "this app already has an active runtime instance; stop it first"
        ))),
        Err(e) => Err(Error::Database(e)),
    }
}

pub fn mark_running(
    conn: &Connection,
    instance_id: &str,
    urls: &[String],
    port: Option<u16>,
    pgid: Option<i32>,
    pid: Option<u32>,
) -> Result<()> {
    ensure_instance_exists(conn, instance_id)?;
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
    // stopped, the start must not resurrect it (batch 2 start/stop race) and a
    // 0-row update is reported as a conflict instead of silent success (#07).
    let n = conn.execute(
        "UPDATE runtime_instances
         SET status = 'running', cleanup_status = NULL, resolved_urls_json = ?2,
             current_port = ?3, pgid = ?4, pid = ?5, owner_pid = ?5,
             last_heartbeat = ?6, resource_ledger_json = ?7, failure = NULL, updated_at = ?6
         WHERE id = ?1 AND status = 'starting'",
        params![instance_id, urls_json, port, pgid, pid, now(), ledger_json],
    )
    .map_err(Error::Database)?;
    expect_transition(n, instance_id, &["starting"], "running")
}

/// Refresh the instance heartbeat while its runtime is alive.
pub fn heartbeat(conn: &Connection, instance_id: &str) -> Result<()> {
    ensure_instance_exists(conn, instance_id)?;
    conn.execute(
        "UPDATE runtime_instances SET last_heartbeat = ?2, updated_at = ?2 WHERE id = ?1",
        params![instance_id, now()],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// Record a natural process exit on the instance.
pub fn mark_exited(conn: &Connection, instance_id: &str, exit_code: i32) -> Result<()> {
    ensure_instance_exists(conn, instance_id)?;
    if current_status(conn, instance_id)? == "stopped" {
        return Ok(());
    }
    let n = conn.execute(
        "UPDATE runtime_instances
         SET status = 'stopped', cleanup_status = 'completed', exit_code = ?2, updated_at = ?3
         WHERE id = ?1 AND status IN ('running','starting')",
        params![instance_id, exit_code, now()],
    )
    .map_err(Error::Database)?;
    expect_transition(n, instance_id, &["running", "starting"], "stopped")
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

/// Bind a preview target to a runtime instance (batch 6). Replaces the previous
/// selected target so a runtime has at most one live preview.
pub fn upsert_preview_target(
    conn: &Connection,
    runtime_instance_id: &str,
    url: &str,
    kind: &str,
) -> Result<String> {
    let id = Uuid::new_v4().to_string();
    let t = now();
    conn.execute(
        "DELETE FROM preview_targets WHERE runtime_instance_id = ?1",
        params![runtime_instance_id],
    )
    .map_err(Error::Database)?;
    conn.execute(
        "INSERT INTO preview_targets (id, runtime_instance_id, url, kind, selected, created_at)
         VALUES (?1, ?2, ?3, ?4, 1, ?5)",
        params![id, runtime_instance_id, url, kind, t],
    )
    .map_err(Error::Database)?;
    Ok(id)
}

/// Active preview target for a source row, if the app has one.
pub fn active_preview_target(
    conn: &Connection,
    source: CreativeAppSource,
    source_id: &str,
) -> Result<Option<(String, String)>> {
    let Some(app_id) = lookup_application(conn, source_str(source), source_id)? else {
        return Ok(None);
    };
    conn.query_row(
        "SELECT pt.id, pt.url FROM preview_targets pt
         JOIN runtime_instances ri ON ri.id = pt.runtime_instance_id
         WHERE ri.application_id = ?1 AND ri.status = 'running' AND pt.selected = 1
         LIMIT 1",
        params![app_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
    .map_err(Error::Database)
}

/// Clear preview targets for a runtime instance (webview closed).
pub fn clear_preview_targets(conn: &Connection, runtime_instance_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM preview_targets WHERE runtime_instance_id = ?1",
        params![runtime_instance_id],
    )
    .map_err(Error::Database)?;
    Ok(())
}

pub fn mark_stopping(conn: &Connection, instance_id: &str) -> Result<()> {
    ensure_instance_exists(conn, instance_id)?;
    if current_status(conn, instance_id)? == "stopping" {
        return Ok(()); // retry of stop is idempotent
    }
    let n = conn.execute(
        "UPDATE runtime_instances SET status = 'stopping', updated_at = ?2
         WHERE id = ?1 AND status IN ('starting','running','cleanup_failed','orphaned')",
        params![instance_id, now()],
    )
    .map_err(Error::Database)?;
    expect_transition(
        n,
        instance_id,
        &["starting", "running", "cleanup_failed", "orphaned"],
        "stopping",
    )
}

pub fn mark_stopped(conn: &Connection, instance_id: &str) -> Result<()> {
    ensure_instance_exists(conn, instance_id)?;
    if current_status(conn, instance_id)? == "stopped" {
        return Ok(()); // idempotent stop
    }
    let n = conn.execute(
        "UPDATE runtime_instances
         SET status = 'stopped', cleanup_status = 'completed', failure = NULL, updated_at = ?2
         WHERE id = ?1 AND status IN
             ('starting','running','stopping','cleanup_failed','orphaned','failed')",
        params![instance_id, now()],
    )
    .map_err(Error::Database)?;
    expect_transition(
        n,
        instance_id,
        &[
            "starting",
            "running",
            "stopping",
            "cleanup_failed",
            "orphaned",
            "failed",
        ],
        "stopped",
    )
}

pub fn mark_failed(conn: &Connection, instance_id: &str, failure: &str) -> Result<()> {
    ensure_instance_exists(conn, instance_id)?;
    if current_status(conn, instance_id)? == "failed" {
        return Ok(()); // idempotent failure settlement
    }
    let n = conn.execute(
        "UPDATE runtime_instances
         SET status = 'failed', cleanup_status = 'failed', failure = ?2, updated_at = ?3
         WHERE id = ?1 AND status = 'starting'",
        params![instance_id, failure, now()],
    )
    .map_err(Error::Database)?;
    expect_transition(n, instance_id, &["starting"], "failed")
}

pub fn mark_cleanup_failed(conn: &Connection, instance_id: &str, failure: &str) -> Result<()> {
    ensure_instance_exists(conn, instance_id)?;
    if current_status(conn, instance_id)? == "cleanup_failed" {
        return Ok(()); // idempotent
    }
    let n = conn.execute(
        "UPDATE runtime_instances
         SET status = 'cleanup_failed', cleanup_status = 'failed', failure = ?2, updated_at = ?3
         WHERE id = ?1 AND status IN ('starting','running','stopping')",
        params![instance_id, failure, now()],
    )
    .map_err(Error::Database)?;
    expect_transition(
        n,
        instance_id,
        &["starting", "running", "stopping"],
        "cleanup_failed",
    )
}

pub fn mark_orphaned(conn: &Connection, instance_id: &str, failure: &str) -> Result<()> {
    ensure_instance_exists(conn, instance_id)?;
    if current_status(conn, instance_id)? == "orphaned" {
        return Ok(()); // idempotent
    }
    let n = conn.execute(
        "UPDATE runtime_instances
         SET status = 'orphaned', cleanup_status = NULL, failure = ?2, updated_at = ?3
         WHERE id = ?1 AND status IN ('starting','running','stopping')",
        params![instance_id, failure, now()],
    )
    .map_err(Error::Database)?;
    expect_transition(
        n,
        instance_id,
        &["starting", "running", "stopping"],
        "orphaned",
    )
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
///
/// This is a READ projection: it looks up the unified identity without creating
/// one. Identity rows are guaranteed by the migration backfill for existing
/// sources and by the registration / lifecycle write paths for new ones; a row
/// without identity is reported as an empty id rather than fabricated (#01).
pub fn attach_identity(
    conn: &Connection,
    mut summary: CreativeAppSummary,
) -> Result<CreativeAppSummary> {
    summary.application_id = lookup_application(conn, source_str(summary.source), &summary.id)?
        .unwrap_or_default();
    if !summary.application_id.is_empty() {
        summary.runtime_instance_id = active_instance_id(conn, &summary.application_id)?;
    }
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
        let summary = CreativeAppSummary {
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
        // Identity is created by the registration write path, then attach_identity
        // (read-only) binds it onto the summary without ever fabricating one.
        let app = find_or_create_application(&conn, CreativeAppSource::LocalProject, "loc1").unwrap();
        let bound = attach_identity(&conn, summary.clone()).unwrap();
        assert_eq!(bound.application_id, app);
        assert!(bound.runtime_instance_id.is_none());

        let iid = create_instance(&conn, &app, None, "host_http").unwrap();
        let bound2 = attach_identity(&conn, summary).unwrap();
        assert_eq!(bound2.runtime_instance_id.as_deref(), Some(iid.as_str()));
    }

    /// Batch 2 race guard: a start must never resurrect an instance that a
    /// concurrent stop already settled to stopped — and a 0-row transition is
    /// surfaced as a typed conflict instead of silent success (#07).
    #[test]
    fn mark_running_does_not_resurrect_stopped_instance() {
        let conn = mem();
        let app =
            find_or_create_application(&conn, CreativeAppSource::LocalProject, "loc1").unwrap();
        let iid = create_instance(&conn, &app, None, "local_process").unwrap();
        // Stop wins the race: instance is stopping then stopped.
        mark_stopping(&conn, &iid).unwrap();
        mark_stopped(&conn, &iid).unwrap();

        // Late start health pass must NOT flip it back to running; the guarded
        // update returns a typed conflict rather than an invisible 0-row Ok.
        let err = mark_running(
            &conn,
            &iid,
            &["http://127.0.0.1:5173/".into()],
            Some(5173),
            None,
            None,
        )
        .unwrap_err();
        assert!(
            matches!(err, Error::Conflict(_)),
            "late mark_running on a stopped instance must conflict, got {err:?}"
        );
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

    /// Batch 1 CR-102: the partial unique index makes the second active instance
    /// a DB-level conflict even when the caller skips the pre-check.
    #[test]
    fn create_instance_cas_rejects_second_active() {
        let conn = mem();
        let app =
            find_or_create_application(&conn, CreativeAppSource::LocalProject, "loc1").unwrap();
        let i1 = create_instance(&conn, &app, None, "local_process").unwrap();
        assert!(active_instance_id(&conn, &app).unwrap().as_deref() == Some(i1.as_str()));

        let err = create_instance(&conn, &app, None, "local_process").unwrap_err();
        assert!(
            matches!(err, Error::Conflict(_)),
            "second active instance must be a typed conflict, got {err:?}"
        );
    }

    /// Batch 1 CR-102 (#07): transitions out of the expected status surface as
    /// typed conflicts; idempotent terminal transitions stay Ok; a missing row
    /// is NotFound — never a silent 0-row success.
    #[test]
    fn transitions_surface_wrong_status_as_conflict() {
        let conn = mem();
        let app =
            find_or_create_application(&conn, CreativeAppSource::LocalProject, "loc1").unwrap();
        let iid = create_instance(&conn, &app, None, "local_process").unwrap();
        // Stop preempts the starting instance directly.
        mark_stopped(&conn, &iid).unwrap();
        assert_eq!(current_status(&conn, &iid).unwrap(), "stopped");

        // mark_failed only applies from 'starting'.
        let err = mark_failed(&conn, &iid, "boom").unwrap_err();
        assert!(
            matches!(err, Error::Conflict(_)),
            "mark_failed on a stopped instance must conflict, got {err:?}"
        );

        // Idempotent terminal transitions remain Ok.
        assert!(mark_stopped(&conn, &iid).is_ok());
        assert!(mark_exited(&conn, &iid, 1).is_ok());

        // A missing instance is NotFound, not a silent success.
        let err = mark_stopped(&conn, "no-such-id").unwrap_err();
        assert!(
            matches!(err, Error::NotFound(_)),
            "missing instance must be NotFound, got {err:?}"
        );
    }

    /// Batch 1 CR-102: two SQLite writers racing to start the same app — only
    /// the first insert wins; the DB partial unique index rejects the second
    /// (concurrent double-start from #06).
    #[test]
    fn two_connections_reject_second_active_instance() {
        let path = std::env::temp_dir().join(format!(
            "natives-cas-two-conn-{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let conn1 = Connection::open(&path).unwrap();
        conn1.busy_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        create_tables(&conn1).unwrap();
        apply_migrations(&conn1).unwrap();
        let conn2 = Connection::open(&path).unwrap();
        conn2.busy_timeout(std::time::Duration::from_secs(5))
            .unwrap();

        let app =
            find_or_create_application(&conn1, CreativeAppSource::LocalProject, "loc1").unwrap();
        let i1 = create_instance(&conn1, &app, None, "local_process").unwrap();
        assert!(active_instance_id(&conn1, &app).unwrap().as_deref() == Some(i1.as_str()));

        // The second writer cannot insert a second active instance for the same app.
        let err = create_instance(&conn2, &app, None, "local_process").unwrap_err();
        assert!(
            matches!(err, Error::Conflict(_)),
            "second writer must hit the DB CAS, got {err:?}"
        );
        let count: i64 = conn1
            .query_row(
                "SELECT COUNT(*) FROM runtime_instances
                 WHERE application_id = ?1 AND status IN ('starting','running','stopping')",
                params![app],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        drop(conn1);
        drop(conn2);
        let _ = std::fs::remove_file(&path);
    }

    /// Batch 1 CR-102 (invariant #3): cleanup_failed is active-like — it is
    /// visible to has_active_instance / active_instance_id so a new start is
    /// blocked until the resources are proven released or the user recovers.
    #[test]
    fn cleanup_failed_is_active_like_and_blocks_new_start() {
        let conn = mem();
        let app =
            find_or_create_application(&conn, CreativeAppSource::LocalProject, "loc1").unwrap();
        let iid = create_instance(&conn, &app, None, "local_process").unwrap();
        mark_running(&conn, &iid, &[], None, None, None).unwrap();
        mark_cleanup_failed(&conn, &iid, "port not released").unwrap();

        assert!(
            has_active_instance(&conn, &app).unwrap(),
            "cleanup_failed must count as active-like"
        );
        assert!(
            active_instance_id(&conn, &app).unwrap().as_deref() == Some(iid.as_str()),
            "cleanup_failed instance must be the active one"
        );

        // The instance can be re-stopped (retry cleanup) and then a new start is
        // allowed again.
        mark_stopping(&conn, &iid).unwrap();
        mark_stopped(&conn, &iid).unwrap();
        assert!(!has_active_instance(&conn, &app).unwrap());
    }

    /// Batch 1 CR-102: orphaned is active-like and blocks new starts until
    /// reconcile proves the identity or the user resolves it.
    #[test]
    fn orphaned_is_active_like_and_blocks_new_start() {
        let conn = mem();
        let app =
            find_or_create_application(&conn, CreativeAppSource::LocalProject, "loc1").unwrap();
        let iid = create_instance(&conn, &app, None, "local_process").unwrap();
        mark_running(&conn, &iid, &[], None, Some(100), Some(42)).unwrap();
        mark_orphaned(&conn, &iid, "host ownership lost").unwrap();

        assert!(has_active_instance(&conn, &app).unwrap());
        assert!(active_instance_id(&conn, &app).unwrap().as_deref() == Some(iid.as_str()));
    }

    /// Batch 6: a preview target binds to the running instance and clears on close.
    #[test]
    fn preview_target_binds_to_running_instance() {
        let conn = mem();
        let app =
            find_or_create_application(&conn, CreativeAppSource::LocalProject, "loc1").unwrap();
        let iid = create_instance(&conn, &app, None, "host_http").unwrap();
        // Not running yet → no preview target is active.
        assert!(
            active_preview_target(&conn, CreativeAppSource::LocalProject, "loc1")
                .unwrap()
                .is_none()
        );

        mark_running(
            &conn,
            &iid,
            &["http://127.0.0.1:5173/".into()],
            Some(5173),
            None,
            None,
        )
        .unwrap();
        let tid =
            upsert_preview_target(&conn, &iid, "http://127.0.0.1:5173/", "child_webview").unwrap();
        let active = active_preview_target(&conn, CreativeAppSource::LocalProject, "loc1")
            .unwrap()
            .expect("running app has a preview");
        assert_eq!(active.0, tid);
        assert_eq!(active.1, "http://127.0.0.1:5173/");

        // A new preview replaces the old one (at most one live preview).
        let tid2 = upsert_preview_target(
            &conn,
            &iid,
            "http://127.0.0.1:5173/#/other",
            "child_webview",
        )
        .unwrap();
        assert_ne!(tid, tid2);
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM preview_targets WHERE runtime_instance_id = ?1",
                params![iid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        clear_preview_targets(&conn, &iid).unwrap();
        assert!(
            active_preview_target(&conn, CreativeAppSource::LocalProject, "loc1")
                .unwrap()
                .is_none()
        );
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
