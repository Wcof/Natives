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
use serde::Serialize;
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

/// Application id that owns a runtime instance (or None for unknown/stale ids).
/// Read-only; used to route late resource events to the right instance's owner
/// instead of assuming the current active instance (CR-301 #05/#22).
pub fn instance_application_id(conn: &Connection, instance_id: &str) -> Result<Option<String>> {
    conn.query_row(
        "SELECT application_id FROM runtime_instances WHERE id = ?1",
        params![instance_id],
        |r| r.get(0),
    )
    .optional()
    .map_err(Error::Database)
}

/// Source row id for an application identity (source detail table id). Read-only.
pub fn source_id_for_application(
    conn: &Connection,
    application_id: &str,
) -> Result<Option<String>> {
    conn.query_row(
        "SELECT source_id FROM applications WHERE id = ?1",
        params![application_id],
        |r| r.get(0),
    )
    .optional()
    .map_err(Error::Database)
}

/// Settle a SPECIFIC instance by its id (crash recovery mirror). Unlike
/// `settle_instance` (which targets the current active instance), this is used
/// when a late event from an old run must not touch a newer active instance.
pub fn settle_instance_by_id(conn: &Connection, instance_id: &str, status: &str) -> Result<()> {
    conn.execute(
        "UPDATE runtime_instances SET status = ?2, updated_at = ?3 WHERE id = ?1",
        params![instance_id, status, now()],
    )
    .map_err(Error::Database)?;
    Ok(())
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

/// Derive the LaunchProfile `driver_kind` from a stored plan JSON. Handles the
/// local LaunchPlan shape (`runtime`, camelCase) and the external RuntimeConfig
/// shape (`kind`, snake_case).
pub fn driver_kind_for_plan_json(plan_json: &str) -> &'static str {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(plan_json) else {
        return "unknown";
    };
    if let Some(r) = v.get("runtime").and_then(|r| r.as_str()) {
        return match r {
            "static_http" => "local_static",
            "docker_compose" => "docker_compose",
            _ => "node_dev_server",
        };
    }
    if let Some(k) = v.get("kind").and_then(|k| k.as_str()) {
        return if k == "docker_compose" {
            "docker_compose"
        } else {
            "docker_run"
        };
    }
    "unknown"
}

fn launch_plan_schema_version(plan_json: &str) -> u32 {
    serde_json::from_str::<serde_json::Value>(plan_json)
        .ok()
        .and_then(|v| v.get("schemaVersion").and_then(|s| s.as_u64()))
        .unwrap_or(1) as u32
}

/// Insert or replace the active startup plan (create / update paths).
///
/// Writes the LaunchProfile v1 columns (schema_version / driver_kind /
/// ownership_mode) and preserves the one-active-plan invariant: any other
/// active plan for the application is deactivated first (batch 1 CR-103).
pub fn upsert_active_plan(
    conn: &Connection,
    application_id: &str,
    plan_json: &str,
) -> Result<String> {
    let t = now();
    let driver_kind = driver_kind_for_plan_json(plan_json);
    let schema_version = launch_plan_schema_version(plan_json);
    if let Some(id) = active_plan_id(conn, application_id)? {
        conn.execute(
            "UPDATE startup_plans
             SET plan_json = ?2, is_active = 1, schema_version = ?3, driver_kind = ?4,
                 ownership_mode = 'managed', updated_at = ?5
             WHERE id = ?1",
            params![id, plan_json, schema_version as i64, driver_kind, t],
        )
        .map_err(Error::Database)?;
        // A stray duplicate active plan (pre-dedup DB) must not survive the write.
        conn.execute(
            "UPDATE startup_plans SET is_active = 0, updated_at = ?2
             WHERE application_id = ?1 AND is_active = 1 AND id <> ?3",
            params![application_id, t, id],
        )
        .map_err(Error::Database)?;
        return Ok(id);
    }
    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO startup_plans
            (id, application_id, plan_version, schema_version, driver_kind, ownership_mode,
             plan_json, is_active, created_at, updated_at)
         VALUES (?1, ?2, 1, ?3, ?4, 'managed', ?5, 1, ?6, ?6)",
        params![
            id,
            application_id,
            schema_version as i64,
            driver_kind,
            plan_json,
            t
        ],
    )
    .map_err(Error::Database)?;
    Ok(id)
}

/// Supported LaunchPlan schema version this build can execute. Anything newer
/// fails closed — a future plan is never run with this build's assumptions.
pub const SUPPORTED_LAUNCH_PLAN_SCHEMA: u32 = 1;

/// Parse a LaunchPlan JSON, failing closed on an unsupported schema version.
/// Reads stay lenient about unknown/extra fields (serde ignores them by
/// default) so old plans written by earlier builds keep reading (CR-103).
pub fn parse_launch_plan(plan_json: &str) -> Result<LaunchPlan> {
    let plan = LaunchPlan::from_json(plan_json)?;
    if plan.schema_version != SUPPORTED_LAUNCH_PLAN_SCHEMA {
        return Err(Error::InvalidInput(format!(
            "unsupported LaunchPlan schemaVersion: {}",
            plan.schema_version
        )));
    }
    Ok(plan)
}

/// Versioned LaunchProfile projection of a `startup_plans` row (CR-103).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchProfile {
    pub id: String,
    pub plan_version: u32,
    pub schema_version: u32,
    pub driver_kind: String,
    pub ownership_mode: String,
    pub is_active: bool,
    pub plan_json: String,
}

/// Read the active LaunchProfile for an application. Legacy rows saved before
/// migration v17 carry NULL versioned columns; the read-upgrader derives them
/// from `plan_json` so old data stays fully readable without a rewrite.
pub fn get_active_plan(conn: &Connection, application_id: &str) -> Result<Option<LaunchProfile>> {
    let row = conn
        .query_row(
            "SELECT id, plan_version, schema_version, driver_kind, ownership_mode, is_active, plan_json
             FROM startup_plans WHERE application_id = ?1 AND is_active = 1
             ORDER BY updated_at DESC, id LIMIT 1",
            params![application_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, String>(6)?,
                ))
            },
        )
        .optional()
        .map_err(Error::Database)?;
    row.map(
        |(id, plan_version, schema_version, driver_kind, ownership_mode, is_active, plan_json)| {
            Ok(LaunchProfile {
                id,
                plan_version: plan_version as u32,
                schema_version: schema_version
                    .map(|v| v as u32)
                    .unwrap_or_else(|| launch_plan_schema_version(&plan_json)),
                driver_kind: driver_kind
                    .unwrap_or_else(|| driver_kind_for_plan_json(&plan_json).to_string()),
                ownership_mode: ownership_mode.unwrap_or_else(|| "managed".to_string()),
                is_active: is_active != 0,
                plan_json,
            })
        },
    )
    .transpose()
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
        Err(e) if is_unique_constraint(&e) => Err(Error::Conflict(
            "this app already has an active runtime instance; stop it first".to_string(),
        )),
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
    let n = conn
        .execute(
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
    let n = conn
        .execute(
            "UPDATE runtime_instances
         SET status = 'stopped', cleanup_status = 'completed', exit_code = ?2, updated_at = ?3
         WHERE id = ?1 AND status IN ('running','starting')",
            params![instance_id, exit_code, now()],
        )
        .map_err(Error::Database)?;
    expect_transition(n, instance_id, &["running", "starting"], "stopped")
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
    let n = conn
        .execute(
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
    let n = conn
        .execute(
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
    let n = conn
        .execute(
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
    let n = conn
        .execute(
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
    let n = conn
        .execute(
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
    summary.application_id =
        lookup_application(conn, source_str(summary.source), &summary.id)?.unwrap_or_default();
    if !summary.application_id.is_empty() {
        summary.runtime_instance_id = active_instance_id(conn, &summary.application_id)?;
    }
    Ok(summary)
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

#[cfg(test)]
#[path = "runtime_store_tests.rs"]
mod runtime_store_tests;
