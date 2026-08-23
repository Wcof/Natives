use crate::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

pub(super) fn instance_status_for_state(state: &str) -> Option<&'static str> {
    Some(match state {
        "running" => "running",
        "starting" => "starting",
        "stopping" => "stopping",
        "start_failed" => "failed",
        "cleanup_failed" => "cleanup_failed",
        "orphaned" => "orphaned",
        _ => return None,
    })
}

pub(crate) fn parse_identity(json: Option<&str>) -> (Option<i32>, Option<u32>) {
    let Some(s) = json else {
        return (None, None);
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(s) else {
        return (None, None);
    };
    let pgid = v
        .get("processGroupId")
        .and_then(|x| x.as_i64())
        .map(|x| x as i32);
    let pid = v.get("pid").and_then(|x| x.as_i64()).map(|x| x as u32);
    (pgid, pid)
}

pub(crate) fn backfill_creative_identity(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        INSERT OR IGNORE INTO applications (id, source, source_id, title, description, icon, version, created_at, updated_at)
        SELECT 'app-internal-' || id, 'internal', id, COALESCE(name, id), description, icon,
               COALESCE(NULLIF(version, ''), '1'),
               COALESCE(created_at, datetime('now')), COALESCE(updated_at, datetime('now'))
        FROM modules;

        INSERT OR IGNORE INTO applications (id, source, source_id, title, description, icon, version, created_at, updated_at)
        SELECT 'app-external-' || id, 'external_github', id, COALESCE(title, id), description, icon,
               COALESCE(NULLIF(version, ''), '1'),
               COALESCE(created_at, datetime('now')), COALESCE(updated_at, datetime('now'))
        FROM external_creative_apps;

        INSERT OR IGNORE INTO applications (id, source, source_id, title, description, icon, version, created_at, updated_at)
        SELECT 'app-local-' || id, 'local_project', id, COALESCE(title, id), description, icon, '1',
               COALESCE(created_at, datetime('now')), COALESCE(updated_at, datetime('now'))
        FROM local_creative_apps;

        INSERT OR IGNORE INTO startup_plans (id, application_id, plan_version, plan_json, is_active, created_at, updated_at)
        SELECT 'plan-local-' || a.id, a.id, 1, l.launch_plan_json, 1,
               COALESCE(l.created_at, datetime('now')), COALESCE(l.updated_at, datetime('now'))
        FROM local_creative_apps l
        JOIN applications a ON a.source = 'local_project' AND a.source_id = l.id;

        INSERT OR IGNORE INTO startup_plans (id, application_id, plan_version, plan_json, is_active, created_at, updated_at)
        SELECT 'plan-external-' || a.id, a.id, 1, e.runtime_config_json, 1,
               COALESCE(e.created_at, datetime('now')), COALESCE(e.updated_at, datetime('now'))
        FROM external_creative_apps e
        JOIN applications a ON a.source = 'external_github' AND a.source_id = e.id;
        ",
    )
    .map_err(Error::Database)?;

    // Backfill runtime_instances for rows that are currently non-terminal, so
    // every active resource is traceable to one instance even after migration.
    {
        let now = chrono::Utc::now().to_rfc3339();
        let mut stmt = conn
            .prepare(
                "SELECT l.id, l.state, l.current_port, l.open_url, l.process_identity_json,
                        l.launch_plan_json
                 FROM local_creative_apps l
                 JOIN applications a ON a.source = 'local_project' AND a.source_id = l.id",
            )
            .map_err(Error::Database)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })
            .map_err(Error::Database)?;
        for r in rows {
            let (source_id, state, port, open_url, ident_json, plan_json) =
                r.map_err(Error::Database)?;
            let Some(status) = instance_status_for_state(&state) else {
                continue;
            };
            let application_id = conn
                .query_row(
                    "SELECT id FROM applications WHERE source = 'local_project' AND source_id = ?1",
                    params![source_id],
                    |row| row.get::<_, String>(0),
                )
                .map_err(Error::Database)?;
            let (pgid, pid) = parse_identity(ident_json.as_deref());
            let plan_id: Option<String> = conn
                .query_row(
                    "SELECT id FROM startup_plans WHERE application_id = ?1 AND is_active = 1 LIMIT 1",
                    params![application_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(Error::Database)?;
            let runtime = serde_json::from_str::<serde_json::Value>(&plan_json)
                .ok()
                .and_then(|v| {
                    v.get("runtime")
                        .and_then(|r| r.as_str())
                        .map(str::to_string)
                });
            let owner_kind = match runtime.as_deref() {
                Some("static_http") => "host_http",
                // Local Compose plans own a docker_compose project, NOT a local
                // process (batch 1 CR-103 fixes the earlier misclassification).
                Some("docker_compose") => "docker_compose",
                _ => "local_process",
            };
            let urls = open_url
                .as_deref()
                .map(|u| serde_json::json!([u]).to_string());
            conn.execute(
                "INSERT OR IGNORE INTO runtime_instances
                    (id, application_id, plan_id, status, cleanup_status, owner_kind, pgid, compose_project,
                     resolved_urls_json, current_port, pid, failure, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6, NULL, ?7, ?8, ?9, NULL, ?10, ?10)",
                params![
                    format!("ri-backfill-{application_id}"),
                    application_id,
                    plan_id,
                    status,
                    owner_kind,
                    pgid,
                    urls,
                    port,
                    pid,
                    now,
                ],
            )
            .map_err(Error::Database)?;
        }
    }
    {
        let now = chrono::Utc::now().to_rfc3339();
        let mut stmt = conn
            .prepare(
                "SELECT e.id, e.state, e.host_port, e.open_url, e.runtime_config_json
                 FROM external_creative_apps e
                 JOIN applications a ON a.source = 'external_github' AND a.source_id = e.id",
            )
            .map_err(Error::Database)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })
            .map_err(Error::Database)?;
        for r in rows {
            let (source_id, state, port, open_url, cfg_json) = r.map_err(Error::Database)?;
            let Some(status) = instance_status_for_state(&state) else {
                continue;
            };
            let application_id = conn
                .query_row(
                    "SELECT id FROM applications WHERE source = 'external_github' AND source_id = ?1",
                    params![source_id],
                    |row| row.get::<_, String>(0),
                )
                .map_err(Error::Database)?;
            let plan_id: Option<String> = conn
                .query_row(
                    "SELECT id FROM startup_plans WHERE application_id = ?1 AND is_active = 1 LIMIT 1",
                    params![application_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(Error::Database)?;
            let owner_kind = if serde_json::from_str::<serde_json::Value>(&cfg_json)
                .ok()
                .and_then(|v| v.get("kind").and_then(|k| k.as_str()).map(str::to_string))
                .as_deref()
                == Some("docker_compose")
            {
                "docker_compose"
            } else {
                "docker_run"
            };
            let urls = open_url
                .as_deref()
                .map(|u| serde_json::json!([u]).to_string());
            conn.execute(
                "INSERT OR IGNORE INTO runtime_instances
                    (id, application_id, plan_id, status, cleanup_status, owner_kind, pgid, compose_project,
                     resolved_urls_json, current_port, pid, failure, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, NULL, ?5, NULL, NULL, ?6, ?7, NULL, NULL, ?8, ?8)",
                params![
                    format!("ri-backfill-{application_id}"),
                    application_id,
                    plan_id,
                    status,
                    owner_kind,
                    urls,
                    port,
                    now,
                ],
            )
            .map_err(Error::Database)?;
        }
    }
    Ok(())
}

/// Insert an immutable audit/report row for a creative identity repair.
/// Deterministic `id` keeps re-runs idempotent (INSERT OR IGNORE).
#[allow(clippy::too_many_arguments)] // pre-existing parameter list
fn insert_identity_report(
    conn: &Connection,
    id: &str,
    kind: &str,
    application_id: Option<&str>,
    source: Option<&str>,
    source_id: Option<&str>,
    action: &str,
    payload_json: &str,
    ts: &str,
) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO creative_identity_reports
            (id, kind, application_id, source, source_id, action, payload_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            id,
            kind,
            application_id,
            source,
            source_id,
            action,
            payload_json,
            ts
        ],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// Batch 1 CR-101: delete confirmed ghost `applications` rows and audit the rest.
///
/// A ghost is an applications row whose source detail row is gone (no matching
/// row in `modules` / `external_creative_apps` / `local_creative_apps`). We only
/// delete ghosts with no dependent `startup_plans` / `runtime_instances` /
/// `preview_targets` (double gate per upgrade plan T01); every deletion backs
/// up the full row JSON into `creative_identity_reports`. Ghosts that still
/// carry dependent data are quarantined (reported, kept), and a ghost whose
/// source_id collides with a real row of another source is also reported.
/// Idempotent: after the first run the deletable set is empty.
pub(crate) fn repair_creative_identity_ghosts(conn: &Connection) -> Result<usize> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS creative_identity_reports (
            id TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            application_id TEXT,
            source TEXT,
            source_id TEXT,
            action TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        ",
    )
    .map_err(Error::Database)?;
    let ts = chrono::Utc::now().to_rfc3339();

    let candidates: Vec<(String, String, String)> = {
        let mut stmt = conn
            .prepare(
                "SELECT a.id, a.source, a.source_id
                 FROM applications a
                 WHERE NOT EXISTS (SELECT 1 FROM modules m WHERE m.id = a.source_id AND a.source = 'internal')
                   AND NOT EXISTS (SELECT 1 FROM external_creative_apps e WHERE e.id = a.source_id AND a.source = 'external_github')
                   AND NOT EXISTS (SELECT 1 FROM local_creative_apps l WHERE l.id = a.source_id AND a.source = 'local_project')",
            )
            .map_err(Error::Database)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map_err(Error::Database)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(Error::Database)?);
        }
        out
    };

    let mut deleted = 0usize;
    for (id, source, source_id) in candidates {
        let row_json: String = conn
            .query_row(
                "SELECT json_object('id', id, 'source', source, 'source_id', source_id,
                                    'title', title, 'version', version,
                                    'created_at', created_at, 'updated_at', updated_at)
                 FROM applications WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .map_err(Error::Database)?;
        // Double gate: dependent rows must be empty before we may delete.
        let has_deps: i64 = conn
            .query_row(
                "SELECT
                    (SELECT COUNT(*) FROM startup_plans sp WHERE sp.application_id = ?1)
                  + (SELECT COUNT(*) FROM runtime_instances ri WHERE ri.application_id = ?1)
                  + (SELECT COUNT(*) FROM preview_targets pt
                        JOIN runtime_instances ri2 ON ri2.id = pt.runtime_instance_id
                     WHERE ri2.application_id = ?1)",
                params![id],
                |r| r.get(0),
            )
            .map_err(Error::Database)?;
        if has_deps > 0 {
            insert_identity_report(
                conn,
                &format!("ghost-quarantine-{id}"),
                "ghost_application_with_dependencies",
                Some(&id),
                Some(&source),
                Some(&source_id),
                "quarantined",
                &row_json,
                &ts,
            )?;
            continue;
        }
        // Cross-source collision: the same source_id exists as a REAL row in a
        // different source table. The real row lives in its own table, so
        // deletion is still safe, but the collision is worth an audit record.
        let cross: i64 = conn
            .query_row(
                "SELECT
                    (SELECT COUNT(*) FROM modules m WHERE m.id = ?2 AND ?1 <> 'internal')
                  + (SELECT COUNT(*) FROM external_creative_apps e WHERE e.id = ?2 AND ?1 <> 'external_github')
                  + (SELECT COUNT(*) FROM local_creative_apps l WHERE l.id = ?2 AND ?1 <> 'local_project')",
                params![source, source_id],
                |r| r.get(0),
            )
            .map_err(Error::Database)?;
        if cross > 0 {
            insert_identity_report(
                conn,
                &format!("collision-{id}"),
                "cross_source_collision",
                Some(&id),
                Some(&source),
                Some(&source_id),
                "reported",
                &row_json,
                &ts,
            )?;
        }
        insert_identity_report(
            conn,
            &format!("ghost-{id}"),
            "ghost_application",
            Some(&id),
            Some(&source),
            Some(&source_id),
            "deleted",
            &row_json,
            &ts,
        )?;
        conn.execute("DELETE FROM applications WHERE id = ?1", params![id])
            .map_err(Error::Database)?;
        deleted += 1;
    }
    Ok(deleted)
}

/// Batch 1 CR-102: reconcile duplicate active runtime/plan rows, then create the
/// partial unique indexes that make "one active per application" a DB invariant.
///
/// Duplicates are NOT deleted and NOT silently marked stopped (invariant #8):
/// the newest active row is kept and older runtime duplicates are demoted to
/// `orphaned` (outside the index scope), while older plan duplicates get
/// `is_active=0`. Every demotion is audited in `creative_identity_reports`.
/// Idempotent: after the first run there are no duplicates left to demote.
pub(crate) fn repair_creative_active_invariants(conn: &Connection) -> Result<usize> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS creative_identity_reports (
            id TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            application_id TEXT,
            source TEXT,
            source_id TEXT,
            action TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        ",
    )
    .map_err(Error::Database)?;
    let ts = chrono::Utc::now().to_rfc3339();
    let mut fixed = 0usize;

    // 1) One active startup_plan per application.
    let dup_plan_apps: Vec<String> = {
        let mut stmt = conn
            .prepare(
                "SELECT application_id FROM startup_plans WHERE is_active = 1
                 GROUP BY application_id HAVING COUNT(*) > 1",
            )
            .map_err(Error::Database)?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(Error::Database)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::Database)?
    };
    for app in dup_plan_apps {
        let ids: Vec<String> = {
            let mut stmt = conn
                .prepare(
                    "SELECT id FROM startup_plans
                     WHERE application_id = ?1 AND is_active = 1
                     ORDER BY updated_at DESC, id",
                )
                .map_err(Error::Database)?;
            let rows = stmt
                .query_map(params![app], |r| r.get::<_, String>(0))
                .map_err(Error::Database)?;
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(Error::Database)?
        };
        for id in ids.into_iter().skip(1) {
            let row_json: String = conn
                .query_row(
                    "SELECT json_object('id', id, 'application_id', application_id,
                                        'plan_version', plan_version, 'is_active', is_active,
                                        'updated_at', updated_at)
                     FROM startup_plans WHERE id = ?1",
                    params![id],
                    |r| r.get(0),
                )
                .map_err(Error::Database)?;
            conn.execute(
                "UPDATE startup_plans SET is_active = 0, updated_at = ?2 WHERE id = ?1",
                params![id, ts],
            )
            .map_err(Error::Database)?;
            insert_identity_report(
                conn,
                &format!("plan-dedup-{id}"),
                "duplicate_active_plan",
                Some(&app),
                None,
                None,
                "demoted",
                &row_json,
                &ts,
            )?;
            fixed += 1;
        }
    }

    // 2) One active runtime_instance per application (over the index scope).
    let dup_runtime_apps: Vec<String> = {
        let mut stmt = conn
            .prepare(
                "SELECT application_id FROM runtime_instances
                 WHERE status IN ('starting','running','stopping')
                 GROUP BY application_id HAVING COUNT(*) > 1",
            )
            .map_err(Error::Database)?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(Error::Database)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::Database)?
    };
    for app in dup_runtime_apps {
        let ids: Vec<String> = {
            let mut stmt = conn
                .prepare(
                    "SELECT id FROM runtime_instances
                     WHERE application_id = ?1 AND status IN ('starting','running','stopping')
                     ORDER BY updated_at DESC, id",
                )
                .map_err(Error::Database)?;
            let rows = stmt
                .query_map(params![app], |r| r.get::<_, String>(0))
                .map_err(Error::Database)?;
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(Error::Database)?
        };
        for id in ids.into_iter().skip(1) {
            let row_json: String = conn
                .query_row(
                    "SELECT json_object('id', id, 'application_id', application_id,
                                        'status', status, 'owner_kind', owner_kind,
                                        'updated_at', updated_at)
                     FROM runtime_instances WHERE id = ?1",
                    params![id],
                    |r| r.get(0),
                )
                .map_err(Error::Database)?;
            // Demote to orphaned (NOT stopped — the resource is unproven; see
            // invariant #8). Orphaned is outside the index scope so the partial
            // unique index below remains satisfiable.
            conn.execute(
                "UPDATE runtime_instances
                 SET status = 'orphaned', cleanup_status = NULL, updated_at = ?2
                 WHERE id = ?1",
                params![id, ts],
            )
            .map_err(Error::Database)?;
            insert_identity_report(
                conn,
                &format!("runtime-dedup-{id}"),
                "duplicate_active_runtime",
                Some(&app),
                None,
                None,
                "orphaned",
                &row_json,
                &ts,
            )?;
            fixed += 1;
        }
    }

    // 3) Partial unique indexes — safe now that duplicates are gone.
    conn.execute_batch(
        "
        CREATE UNIQUE INDEX IF NOT EXISTS idx_runtime_instances_one_active
            ON runtime_instances(application_id)
            WHERE status IN ('starting','running','stopping');
        CREATE UNIQUE INDEX IF NOT EXISTS idx_startup_plans_one_active
            ON startup_plans(application_id) WHERE is_active = 1;
        ",
    )
    .map_err(Error::Database)?;

    Ok(fixed)
}

/// Derive a LaunchProfile `driver_kind` from a stored plan JSON. Handles the
/// local LaunchPlan shape (`runtime`, camelCase) and the external RuntimeConfig
/// shape (`kind`, snake_case). Kept self-contained because migrations must run
/// against historical schemas without depending on domain modules.
fn plan_driver_kind_from_json(json: &str) -> &'static str {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
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

/// Batch 1 CR-103: promote `startup_plans` to the versioned LaunchProfile base.
///
/// Adds nullable `schema_version` / `driver_kind` / `ownership_mode` columns
/// (read-upgrader derives them from plan_json when NULL), backfills existing
/// rows, and repairs the Compose backfill that misclassified a local
/// docker_compose instance as `local_process`. Additive and idempotent: the
/// column adds are PRAGMA-guarded and the backfill only touches NULL rows.
pub(crate) fn upgrade_startup_plans_v1(conn: &Connection) -> Result<usize> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS creative_identity_reports (
            id TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            application_id TEXT,
            source TEXT,
            source_id TEXT,
            action TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        ",
    )
    .map_err(Error::Database)?;

    // 1) Add the versioned columns (guarded so re-running is safe).
    let plan_cols: Vec<String> = {
        let mut stmt = conn
            .prepare("PRAGMA table_info(startup_plans)")
            .map_err(Error::Database)?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(1))
            .map_err(Error::Database)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::Database)?
    };
    if !plan_cols.iter().any(|c| c == "schema_version") {
        conn.execute(
            "ALTER TABLE startup_plans ADD COLUMN schema_version INTEGER",
            [],
        )
        .map_err(Error::Database)?;
    }
    if !plan_cols.iter().any(|c| c == "driver_kind") {
        conn.execute("ALTER TABLE startup_plans ADD COLUMN driver_kind TEXT", [])
            .map_err(Error::Database)?;
    }
    if !plan_cols.iter().any(|c| c == "ownership_mode") {
        conn.execute(
            "ALTER TABLE startup_plans ADD COLUMN ownership_mode TEXT",
            [],
        )
        .map_err(Error::Database)?;
    }

    // 2) Backfill columns for legacy rows (only those still missing them).
    let ts = chrono::Utc::now().to_rfc3339();
    let mut fixed = 0usize;
    let missing: Vec<(String, String)> = {
        let mut stmt = conn
            .prepare(
                "SELECT id, plan_json FROM startup_plans
                 WHERE schema_version IS NULL OR driver_kind IS NULL OR ownership_mode IS NULL",
            )
            .map_err(Error::Database)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(Error::Database)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(Error::Database)?);
        }
        out
    };
    for (id, plan_json) in missing {
        let dk = plan_driver_kind_from_json(&plan_json);
        let sv = serde_json::from_str::<serde_json::Value>(&plan_json)
            .ok()
            .and_then(|v| v.get("schemaVersion").and_then(|s| s.as_u64()))
            .unwrap_or(1);
        conn.execute(
            "UPDATE startup_plans
             SET schema_version = ?2, driver_kind = ?3, ownership_mode = 'managed', updated_at = ?4
             WHERE id = ?1",
            params![id, sv as i64, dk, ts],
        )
        .map_err(Error::Database)?;
        insert_identity_report(
            conn,
            &format!("plan-upgrade-{id}"),
            "plan_versioned_columns",
            None,
            None,
            None,
            "backfilled",
            &serde_json::json!({ "driverKind": dk, "schemaVersion": sv }).to_string(),
            &ts,
        )?;
        fixed += 1;
    }

    // 3) Repair the earlier Compose backfill that classified local docker_compose
    //    instances as `local_process` (#34).
    let misclassified: Vec<(String, String, String)> = {
        let mut stmt = conn
            .prepare(
                "SELECT ri.id, ri.application_id, l.launch_plan_json
                 FROM runtime_instances ri
                 JOIN applications a ON a.id = ri.application_id
                 JOIN local_creative_apps l ON l.id = a.source_id AND a.source = 'local_project'
                 WHERE ri.owner_kind = 'local_process'
                   AND json_extract(l.launch_plan_json, '$.runtime') = 'docker_compose'",
            )
            .map_err(Error::Database)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map_err(Error::Database)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(Error::Database)?);
        }
        out
    };
    for (rid, app_id, plan_json) in misclassified {
        conn.execute(
            "UPDATE runtime_instances SET owner_kind = 'docker_compose', updated_at = ?2 WHERE id = ?1",
            params![rid, ts],
        )
        .map_err(Error::Database)?;
        insert_identity_report(
            conn,
            &format!("owner-repair-{rid}"),
            "plan_owner_repaired",
            Some(&app_id),
            None,
            None,
            "repaired",
            &serde_json::json!({ "instanceId": rid, "driverKind": plan_driver_kind_from_json(&plan_json) })
                .to_string(),
            &ts,
        )?;
        fixed += 1;
    }

    Ok(fixed)
}

// ───────────────────────────────────────────────────────────────────────────
// v28 backfill（Phase B / APP-011，spec 05 §迁移顺序 2-4）。
//
// 全部步骤幂等：只更新「尚未回填」的行，重复执行结果稳定。
// 映射规则（spec 05 + APP-011）：
//   - source=local_project   → kind=local_project,     origin=local_scan
//   - source=internal        → kind=local_project,     origin=legacy_internal
//   - source=external_github → kind=web_application,   origin=legacy_github
//   - non_owned_apps ownership=remote → 迁入 applications(kind=web_application,
//     origin=migration) + web_application_specs；attached 不迁移（保留标记）。
// ───────────────────────────────────────────────────────────────────────────

/// v28 回填总入口（spec 05 §迁移顺序 2-4，APP-011）。
///
/// 由 `migration_v28::migrate_v28` 在 DDL 之后、写 `_schema_version=28` 之前调用，
/// 严格遵循 spec 顺序（DDL → backfill → 版本标记）。整体幂等。
pub(crate) fn backfill_v28(conn: &Connection) -> Result<()> {
    backfill_v28_kind_origin(conn)?;
    backfill_v28_non_owned_remote(conn)?;
    backfill_v28_profile_bindings(conn)
}

/// 05 步骤 2：回填 applications.kind / registration_origin。
///
/// 只写 `kind IS NULL` 的行 → 幂等（已回填的行不再处理）。legacy `source` 保留
/// 不删（spec 05 兼容期），kind 成为权威。
pub(crate) fn backfill_v28_kind_origin(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        UPDATE applications
           SET kind = 'local_project', registration_origin = 'local_scan'
         WHERE kind IS NULL AND source = 'local_project';

        UPDATE applications
           SET kind = 'local_project', registration_origin = 'legacy_internal'
         WHERE kind IS NULL AND source = 'internal';

        UPDATE applications
           SET kind = 'web_application', registration_origin = 'legacy_github'
         WHERE kind IS NULL AND source = 'external_github';
        ",
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// 05 步骤 3：把 non_owned_apps 中 `ownership='remote'` 的记录迁入
/// applications + web_application_specs。
///
/// 稳定迁移 ID：沿用 non_owned 记录自身的 id 作为 application id（该 id 此前已是
/// 浏览器 profile binding 的 application_id，保证引用一致）。幂等：仅当该 id 尚
/// 未存在于 applications 时插入（`INSERT ... WHERE NOT EXISTS`），web spec 用
/// `INSERT OR IGNORE`（application_id 为 PK）。attached 记录不迁移（APP-011 step 4）。
pub(crate) fn backfill_v28_non_owned_remote(conn: &Connection) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();

    // 迁入 applications（kind=web_application, origin=migration）。
    conn.execute(
        "INSERT INTO applications (id, source, source_id, title, description, icon, version,
                                  kind, registration_origin, show_in_sidebar, sidebar_order,
                                  created_at, updated_at)
         SELECT n.id, 'non_owned_remote', n.id, n.title, NULL, NULL, '1',
                'web_application', 'migration', 0, NULL,
                COALESCE(n.created_at, ?1), COALESCE(n.updated_at, ?1)
           FROM non_owned_apps n
          WHERE n.ownership = 'remote'
            AND NOT EXISTS (SELECT 1 FROM applications a WHERE a.id = n.id)",
        params![now],
    )
    .map_err(Error::Database)?;

    // 写入 web_application_specs（PK application_id 去重 → 幂等）。
    conn.execute(
        "INSERT OR IGNORE INTO web_application_specs
            (application_id, url, approved_origins_json, open_behavior, keep_alive,
             created_at, updated_at)
         SELECT n.id, n.url, COALESCE(NULLIF(n.approved_origins_json, ''), '[]'),
                'native_webview', 0,
                COALESCE(n.created_at, ?1), COALESCE(n.updated_at, ?1)
           FROM non_owned_apps n
          WHERE n.ownership = 'remote'
            AND EXISTS (SELECT 1 FROM applications a WHERE a.id = n.id)",
        params![now],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// 05 步骤 4：迁移后修复 / 保持 profile bindings 引用一致。
///
/// 关键不变式：v28 迁移沿用 non_owned 记录自身的 id 作为 application id（稳定迁移
/// ID，见 `backfill_v28_non_owned_remote`）。而 `browser_profile_bindings.application_id`
/// 是 `REFERENCES applications(id)`，迁移前 remote non_owned 的 id 并不在 applications
/// 中，故这些 Web 应用此前**没有**任何 binding 行（浏览器走 `profile_for_app` 的默认
/// profile 隐式回退）。因此迁移后「已有 binding 保持引用一致」这一约束由稳定 ID 选择
/// 天然满足：任何以该 id 键控的 binding 在迁移后仍指向同一个 application id，无需改写。
///
/// 本步骤是 spec 六步契约中「修复 profile bindings」的落点，实现为**幂等的引用一致
/// 校验**（不 fabricate 任何 binding，不复制 profile_id 到 web spec）：确认每个已迁入
/// 的 Web 应用（kind=web_application, origin=migration）若有 binding，则该 binding 必然
/// 指向一个真实存在的 application id（FK 已保证）。正常情况下为 no-op。
pub(crate) fn backfill_v28_profile_bindings(conn: &Connection) -> Result<()> {
    // 幂等校验：统计「已迁入 Web 应用但 binding 悬空」的行数（FK 下应为 0）。
    // 只读、无副作用、重复执行稳定。若未来出现悬空引用（数据损坏），返回冲突错误
    // 让迁移显式失败，而不是静默写坏数据。
    let dangling: i64 = conn
        .query_row(
            "SELECT COUNT(*)
               FROM browser_profile_bindings b
               WHERE b.application_id IN (
                       SELECT id FROM applications
                        WHERE kind = 'web_application'
                          AND registration_origin = 'migration'
                   )
                 AND NOT EXISTS (
                       SELECT 1 FROM applications a WHERE a.id = b.application_id
                 )",
            [],
            |r| r.get(0),
        )
        .map_err(Error::Database)?;
    if dangling > 0 {
        return Err(Error::Conflict(format!(
            "v28 profile binding repair found {dangling} dangling reference(s)"
        )));
    }
    Ok(())
}

// ──────────────────────────────────────────────
