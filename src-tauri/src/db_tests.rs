use super::*;

#[test]
fn repairs_snapshot_table_when_schema_marker_is_already_newer() {
    let conn = Connection::open_in_memory().expect("open in-memory database");
    create_tables(&conn).expect("create base tables");
    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES ('_schema_version', '8')",
        [],
    )
    .expect("set newer schema marker");

    apply_migrations(&conn).expect("repair migrations");

    let table_exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'usage_dashboard_snapshots'",
                [],
                |row| row.get(0),
            )
            .expect("query sqlite schema");
    assert_eq!(table_exists, 1);

    let local_exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'local_creative_apps'",
                [],
                |row| row.get(0),
            )
            .expect("query local_creative_apps");
    assert_eq!(local_exists, 1);
}

/// Batch 1: the three-source backfill into applications / startup_plans /
/// runtime_instances is idempotent and covers active rows.
#[test]
fn creative_identity_backfill_is_idempotent() {
    let conn = Connection::open_in_memory().expect("open in-memory database");
    create_tables(&conn).expect("create base tables");
    apply_migrations(&conn).expect("apply migrations");

    conn.execute(
            "INSERT INTO modules (id, name, version, entry, type, enabled, state, created_at, updated_at)
             VALUES ('mod1', 'M', '1', 'index.html', 'web-module', 1, 'installed', 't', 't')",
            [],
        )
        .expect("insert module");
    conn.execute(
            "INSERT INTO local_creative_apps
                (id, title, canonical_project_root, device_id, device_name, project_kind, launch_mode,
                 launch_plan_json, plan_fingerprint, state, current_port, open_url, process_identity_json,
                 startup_timeout_ms, created_at, updated_at)
             VALUES ('loc1', 'Local', '/tmp/x', 'd', 'n', 'html', 'smart',
                     '{\"schemaVersion\":1,\"runtime\":\"static_http\",\"program\":\"internal\"}', 'fp',
                     'running', 5173, 'http://127.0.0.1:5173/', '{\"pid\":100,\"processGroupId\":100}',
                     60000, 't', 't')",
            [],
        )
        .expect("insert local app");
    conn.execute(
            "INSERT INTO external_creative_apps
                (id, title, version, owner, repo, repository_url, release_tag, runtime, state, host_port,
                 open_url, runtime_config_json, created_at, updated_at)
             VALUES ('ext1', 'Ext', '1', 'o', 'r', 'http://x', 'v1', 'docker_compose', 'running', 8080,
                     'http://127.0.0.1:8080/',
                     '{\"kind\":\"docker_compose\",\"projectName\":\"natives-ext1\",\"composeFile\":\"/tmp/c.yml\",\"service\":\"web\",\"containerPort\":80,\"hostPort\":8080,\"openPath\":\"/\"}',
                     't', 't')",
            [],
        )
        .expect("insert external app");

    backfill_creative_identity(&conn).expect("first backfill");
    backfill_creative_identity(&conn).expect("second backfill (idempotency)");

    let count = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).expect("count") };
    assert_eq!(count("SELECT COUNT(*) FROM applications"), 3);
    assert_eq!(count("SELECT COUNT(*) FROM startup_plans"), 2);
    assert_eq!(count("SELECT COUNT(*) FROM runtime_instances"), 2);

    // The running local app got a real instance with owner_kind/port/pgid.
    let (status, kind, port, pgid): (String, String, Option<i64>, Option<i64>) = conn
        .query_row(
            "SELECT ri.status, ri.owner_kind, ri.current_port, ri.pgid
                 FROM runtime_instances ri
                 JOIN applications a ON a.id = ri.application_id
                 WHERE a.source_id = 'loc1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .expect("local instance");
    assert_eq!(status, "running");
    assert_eq!(kind, "host_http");
    assert_eq!(port, Some(5173));
    assert_eq!(pgid, Some(100));

    // The external running app got a docker_compose instance.
    let (status, kind): (String, String) = conn
        .query_row(
            "SELECT ri.status, ri.owner_kind
                 FROM runtime_instances ri
                 JOIN applications a ON a.id = ri.application_id
                 WHERE a.source_id = 'ext1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("external instance");
    assert_eq!(status, "running");
    assert_eq!(kind, "docker_compose");

    // Re-running again must not create duplicates.
    backfill_creative_identity(&conn).expect("third backfill");
    assert_eq!(count("SELECT COUNT(*) FROM applications"), 3);
    assert_eq!(count("SELECT COUNT(*) FROM runtime_instances"), 2);
}

/// Batch 1 CR-101: a ghost application (no source row, no dependent rows)
/// is deleted and its full row JSON is backed up to the report table.
#[test]
fn repair_ghost_identity_deletes_and_backs_up() {
    let conn = Connection::open_in_memory().expect("open in-memory database");
    create_tables(&conn).expect("create base tables");
    apply_migrations(&conn).expect("apply migrations");

    // A fake local_project identity the old browser show path would fabricate
    // for a GitHub app (no matching source row anywhere).
    conn.execute(
        "INSERT INTO applications (id, source, source_id, title, version, created_at, updated_at)
             VALUES ('ghost-1', 'local_project', 'ext-ghost', 'Ghost', '1', 't', 't')",
        [],
    )
    .expect("insert ghost");

    let deleted = repair_creative_identity_ghosts(&conn).expect("repair");
    assert_eq!(deleted, 1);

    let remaining: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM applications WHERE id = 'ghost-1'",
            [],
            |r| r.get(0),
        )
        .expect("count");
    assert_eq!(remaining, 0);

    // The row JSON was backed up with an audit record.
    let (kind, action, payload): (String, String, String) = conn
            .query_row(
                "SELECT kind, action, payload_json FROM creative_identity_reports WHERE id = 'ghost-ghost-1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .expect("report row");
    assert_eq!(kind, "ghost_application");
    assert_eq!(action, "deleted");
    assert!(payload.contains("ext-ghost"));

    // Idempotent: a second run finds nothing new and adds no duplicate rows.
    let deleted2 = repair_creative_identity_ghosts(&conn).expect("repair again");
    assert_eq!(deleted2, 0);
    let reports: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM creative_identity_reports WHERE kind = 'ghost_application'",
            [],
            |r| r.get(0),
        )
        .expect("count reports");
    assert_eq!(reports, 1);
}

/// Batch 1 CR-101: a ghost with dependent rows is quarantined, never deleted.
#[test]
fn repair_quarantines_ghost_with_dependencies() {
    let conn = Connection::open_in_memory().expect("open in-memory database");
    create_tables(&conn).expect("create base tables");
    apply_migrations(&conn).expect("apply migrations");

    conn.execute(
        "INSERT INTO applications (id, source, source_id, title, version, created_at, updated_at)
             VALUES ('ghost-2', 'local_project', 'ext-ghost', 'Ghost', '1', 't', 't')",
        [],
    )
    .expect("insert ghost");
    // Dependent startup_plan keeps the row from being deletable.
    conn.execute(
            "INSERT INTO startup_plans (id, application_id, plan_version, plan_json, is_active, created_at, updated_at)
             VALUES ('plan-ghost', 'ghost-2', 1, '{}', 1, 't', 't')",
            [],
        )
        .expect("insert dependent plan");

    let deleted = repair_creative_identity_ghosts(&conn).expect("repair");
    assert_eq!(deleted, 0, "ghost with dependencies must not be deleted");
    let remaining: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM applications WHERE id = 'ghost-2'",
            [],
            |r| r.get(0),
        )
        .expect("count");
    assert_eq!(remaining, 1);
    let quarantined: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM creative_identity_reports
                 WHERE kind = 'ghost_application_with_dependencies' AND action = 'quarantined'",
            [],
            |r| r.get(0),
        )
        .expect("count reports");
    assert_eq!(quarantined, 1);
}

/// Batch 1 CR-101: a ghost whose source_id collides with a real row of a
/// different source is deleted (the real row lives in its own table) but the
/// collision is recorded for audit.
#[test]
fn repair_reports_cross_source_collision() {
    let conn = Connection::open_in_memory().expect("open in-memory database");
    create_tables(&conn).expect("create base tables");
    apply_migrations(&conn).expect("apply migrations");

    // A real external GitHub app.
    conn.execute(
            "INSERT INTO external_creative_apps
                (id, title, version, owner, repo, repository_url, release_tag, runtime, state, runtime_config_json, created_at, updated_at)
             VALUES ('ext-1', 'Ext', '1', 'o', 'r', 'http://x', 'v1', 'docker_compose', 'installed_stopped', '{}', 't', 't')",
            [],
        )
        .expect("insert external");
    // The ghost local_project identity the old browser path created for it.
    conn.execute(
        "INSERT INTO applications (id, source, source_id, title, version, created_at, updated_at)
             VALUES ('ghost-3', 'local_project', 'ext-1', 'Ghost', '1', 't', 't')",
        [],
    )
    .expect("insert ghost");

    let deleted = repair_creative_identity_ghosts(&conn).expect("repair");
    assert_eq!(deleted, 1);
    let collision: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM creative_identity_reports WHERE kind = 'cross_source_collision'",
            [],
            |r| r.get(0),
        )
        .expect("count reports");
    assert_eq!(collision, 1);
    // The real external app row is untouched.
    let ext: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM external_creative_apps WHERE id = 'ext-1'",
            [],
            |r| r.get(0),
        )
        .expect("count");
    assert_eq!(ext, 1);
}

/// Batch 1 CR-102: the v16 migration leaves the two active unique indexes in
/// place so a second active instance / plan is a DB-level violation.
#[test]
fn migration_creates_active_unique_indexes() {
    let conn = Connection::open_in_memory().expect("open in-memory database");
    create_tables(&conn).expect("create base tables");
    apply_migrations(&conn).expect("apply migrations");

    let index = |name: &str| -> i64 {
        conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name=?1",
            rusqlite::params![name],
            |r| r.get(0),
        )
        .expect("index exists")
    };
    assert_eq!(index("idx_runtime_instances_one_active"), 1);
    assert_eq!(index("idx_startup_plans_one_active"), 1);

    conn.execute(
        "INSERT INTO applications (id, source, source_id, title, version, created_at, updated_at)
             VALUES ('app-1', 'local_project', 'loc1', 'L', '1', 't', 't')",
        [],
    )
    .expect("insert app");
    conn.execute(
            "INSERT INTO runtime_instances (id, application_id, status, owner_kind, created_at, updated_at)
             VALUES ('ri-1', 'app-1', 'starting', 'local_process', 't', 't')",
            [],
        )
        .expect("insert active instance");
    // A second active instance violates the partial unique index.
    let err = conn
            .execute(
                "INSERT INTO runtime_instances (id, application_id, status, owner_kind, created_at, updated_at)
                 VALUES ('ri-2', 'app-1', 'starting', 'local_process', 'u', 'u')",
                [],
            )
            .expect_err("second active instance must be rejected by the DB");
    assert!(
        matches!(&err, rusqlite::Error::SqliteFailure(f, _)
                if f.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE),
        "expected UNIQUE constraint violation, got {err:?}"
    );

    // Same for active plans.
    conn.execute(
            "INSERT INTO startup_plans (id, application_id, plan_version, plan_json, is_active, created_at, updated_at)
             VALUES ('plan-1', 'app-1', 1, '{}', 1, 't', 't')",
            [],
        )
        .expect("insert active plan");
    let err = conn
            .execute(
                "INSERT INTO startup_plans (id, application_id, plan_version, plan_json, is_active, created_at, updated_at)
                 VALUES ('plan-2', 'app-1', 1, '{}', 1, 'u', 'u')",
                [],
            )
            .expect_err("second active plan must be rejected by the DB");
    assert!(matches!(&err, rusqlite::Error::SqliteFailure(..)));
}

/// Batch 1 CR-102: the repair reconciles pre-existing duplicate active rows
/// (kept newest, demoted rest audited) before the indexes are recreated, and
/// never fabricates a stopped status for an unproven resource.
#[test]
fn repair_active_invariants_dedups_duplicates() {
    let conn = Connection::open_in_memory().expect("open in-memory database");
    create_tables(&conn).expect("create base tables");
    apply_migrations(&conn).expect("apply migrations");

    // Simulate a pre-v16 database: no active unique indexes yet.
    conn.execute("DROP INDEX idx_runtime_instances_one_active", [])
        .expect("drop runtime index");
    conn.execute("DROP INDEX idx_startup_plans_one_active", [])
        .expect("drop plan index");
    conn.execute(
        "INSERT INTO applications (id, source, source_id, title, version, created_at, updated_at)
             VALUES ('app-1', 'local_project', 'loc1', 'L', '1', 't', 't')",
        [],
    )
    .expect("insert app");
    // Two active instances + two active plans for the same app.
    conn.execute(
            "INSERT INTO runtime_instances (id, application_id, status, owner_kind, created_at, updated_at)
             VALUES ('ri-old', 'app-1', 'running', 'local_process', 't', 't')",
            [],
        )
        .expect("insert older running instance");
    conn.execute(
            "INSERT INTO runtime_instances (id, application_id, status, owner_kind, created_at, updated_at)
             VALUES ('ri-new', 'app-1', 'starting', 'local_process', 'u', 'u')",
            [],
        )
        .expect("insert newer starting instance");
    conn.execute(
            "INSERT INTO startup_plans (id, application_id, plan_version, plan_json, is_active, created_at, updated_at)
             VALUES ('plan-old', 'app-1', 1, '{}', 1, 't', 't')",
            [],
        )
        .expect("insert older active plan");
    conn.execute(
            "INSERT INTO startup_plans (id, application_id, plan_version, plan_json, is_active, created_at, updated_at)
             VALUES ('plan-new', 'app-1', 1, '{}', 1, 'u', 'u')",
            [],
        )
        .expect("insert newer active plan");

    let fixed = repair_creative_active_invariants(&conn).expect("repair");
    assert_eq!(fixed, 2, "one runtime + one plan duplicate demoted");

    // The newest active rows were kept; the older ones were demoted (not
    // deleted, not fabricated as stopped).
    let kept_runtime: String = conn
        .query_row(
            "SELECT id FROM runtime_instances WHERE application_id='app-1'
                 AND status IN ('starting','running','stopping')",
            [],
            |r| r.get(0),
        )
        .expect("kept runtime");
    assert_eq!(kept_runtime, "ri-new");
    let old_runtime_status: String = conn
        .query_row(
            "SELECT status FROM runtime_instances WHERE id='ri-old'",
            [],
            |r| r.get(0),
        )
        .expect("old runtime status");
    assert_eq!(
        old_runtime_status, "orphaned",
        "unproven duplicate must be orphaned, never fabricated stopped"
    );
    let kept_plan: String = conn
        .query_row(
            "SELECT id FROM startup_plans WHERE application_id='app-1' AND is_active=1",
            [],
            |r| r.get(0),
        )
        .expect("kept plan");
    assert_eq!(kept_plan, "plan-new");
    let old_plan_active: i64 = conn
        .query_row(
            "SELECT is_active FROM startup_plans WHERE id='plan-old'",
            [],
            |r| r.get(0),
        )
        .expect("old plan active");
    assert_eq!(old_plan_active, 0);

    // The indexes were recreated and now reject a second active instance.
    let err = conn
            .execute(
                "INSERT INTO runtime_instances (id, application_id, status, owner_kind, created_at, updated_at)
                 VALUES ('ri-3', 'app-1', 'starting', 'local_process', 'x', 'x')",
                [],
            )
            .expect_err("index must reject a second active instance");
    assert!(matches!(&err, rusqlite::Error::SqliteFailure(..)));

    // Idempotent re-run: no further demotions, no duplicate report rows.
    let fixed2 = repair_creative_active_invariants(&conn).expect("repair again");
    assert_eq!(fixed2, 0);
    let reports: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM creative_identity_reports WHERE kind IN
                 ('duplicate_active_runtime','duplicate_active_plan')",
            [],
            |r| r.get(0),
        )
        .expect("count reports");
    assert_eq!(reports, 2);
}

/// Batch 1 CR-103: migration v17 backfills the LaunchProfile v1 columns from
/// stored plan JSON and repairs the Compose backfill that classified a local
/// docker_compose instance as `local_process` (#34).
#[test]
fn upgrade_startup_plans_v1_backfills_and_repairs_compose_owner() {
    let conn = Connection::open_in_memory().expect("open in-memory database");
    create_tables(&conn).expect("create base tables");
    apply_migrations(&conn).expect("apply migrations");

    // A local compose app whose backfilled instance was misclassified.
    conn.execute(
            "INSERT INTO local_creative_apps
                (id, title, canonical_project_root, device_id, device_name, project_kind, launch_mode,
                 launch_plan_json, plan_fingerprint, state, startup_timeout_ms, created_at, updated_at)
             VALUES ('loc-compose', 'Compose', '/tmp/c', 'd', 'n', 'other', 'smart',
                     '{\"schemaVersion\":1,\"runtime\":\"docker_compose\",\"program\":\"internal\",\"compose\":{\"composeFile\":\"docker-compose.yml\",\"projectSeed\":\"p\"}}',
                     'fp', 'running', 60000, 't', 't')",
            [],
        )
        .expect("insert local compose app");
    conn.execute(
        "INSERT INTO applications (id, source, source_id, title, version, created_at, updated_at)
             VALUES ('app-compose', 'local_project', 'loc-compose', 'Compose', '1', 't', 't')",
        [],
    )
    .expect("insert app");
    conn.execute(
            "INSERT INTO startup_plans (id, application_id, plan_version, plan_json, is_active, created_at, updated_at)
             VALUES ('plan-compose', 'app-compose', 1,
                     '{\"schemaVersion\":1,\"runtime\":\"docker_compose\",\"program\":\"internal\"}',
                     1, 't', 't')",
            [],
        )
        .expect("insert plan with NULL versioned columns");
    conn.execute(
            "INSERT INTO runtime_instances (id, application_id, status, owner_kind, created_at, updated_at)
             VALUES ('ri-compose', 'app-compose', 'running', 'local_process', 't', 't')",
            [],
        )
        .expect("insert misclassified instance");

    let fixed = upgrade_startup_plans_v1(&conn).expect("upgrade v17");
    assert!(
        fixed >= 2,
        "expected plan backfill + owner repair, got {fixed}"
    );

    // Plan columns backfilled from JSON.
    let (dk, sv, om): (String, i64, String) = conn
            .query_row(
                "SELECT driver_kind, schema_version, ownership_mode FROM startup_plans WHERE id = 'plan-compose'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .expect("plan columns");
    assert_eq!(dk, "docker_compose");
    assert_eq!(sv, 1);
    assert_eq!(om, "managed");

    // The misclassified instance now owns a docker_compose project.
    let kind: String = conn
        .query_row(
            "SELECT owner_kind FROM runtime_instances WHERE id = 'ri-compose'",
            [],
            |r| r.get(0),
        )
        .expect("owner kind");
    assert_eq!(kind, "docker_compose");

    // Idempotent re-run finds nothing new.
    let fixed2 = upgrade_startup_plans_v1(&conn).expect("upgrade again");
    assert_eq!(fixed2, 0);
}

/// Batch 2 CR-201: migration v18 creates the `operations` journal table with
/// FK-linked app/instance columns and active-phase indexes, and is idempotent.
#[test]
fn migration_creates_operations_journal() {
    let conn = Connection::open_in_memory().expect("open in-memory database");
    create_tables(&conn).expect("create base tables");
    apply_migrations(&conn).expect("apply migrations");

    let version: String = conn
        .query_row(
            "SELECT value FROM settings WHERE key = '_schema_version'",
            [],
            |r| r.get(0),
        )
        .expect("schema version");
    assert_eq!(
        version, SCHEMA_VERSION,
        "migration must bump the tracked version"
    );

    conn.execute(
        "INSERT INTO operations (kind, phase, actor, started_at, updated_at)
             VALUES ('start', 'pending', 'user', 't', 't')",
        [],
    )
    .expect("operations table accepts a minimal journal row");

    // Application id can be NULL (install before the app identity exists) and
    // the FK is satisfied once an application row exists.
    conn.execute(
        "INSERT INTO applications (id, source, source_id, title, version, created_at, updated_at)
             VALUES ('app-op', 'local_project', 'loc', 'L', '1', 't', 't')",
        [],
    )
    .expect("insert app");
    conn.execute(
        "INSERT INTO operations (application_id, kind, phase, started_at, updated_at)
             VALUES ('app-op', 'stop', 'running', 't', 't')",
        [],
    )
    .expect("operation links to an application");

    // Repeat migration is a no-op (no error, no duplicate table).
    apply_migrations(&conn).expect("re-apply migrations");

    // Deleting the application SET NULLs the operation's application_id so a
    // delete operation journal survives its own app row (audit trail).
    conn.execute(
        "INSERT INTO operations (application_id, kind, phase, started_at, updated_at)
             VALUES ('app-op', 'delete', 'succeeded', 't', 't')",
        [],
    )
    .expect("insert delete operation");
    conn.execute("DELETE FROM applications WHERE id = 'app-op'", [])
        .expect("delete app");
    let orphaned_app: Option<String> = conn
        .query_row(
            "SELECT application_id FROM operations WHERE kind = 'delete'",
            [],
            |r| r.get(0),
        )
        .expect("delete operation row");
    assert_eq!(orphaned_app, None, "FK SET NULL keeps the delete audit row");
}

#[test]
fn migration_v31_creates_ai_and_proxy_tables() {
    let conn = Connection::open_in_memory().expect("open in-memory database");
    create_tables(&conn).expect("create base tables");
    apply_migrations(&conn).expect("apply migrations");

    let version: String = conn
        .query_row(
            "SELECT value FROM settings WHERE key = '_schema_version'",
            [],
            |r| r.get(0),
        )
        .expect("schema version");
    assert_eq!(version, "31", "schema version must be 31");

    for table in [
        "ai_providers",
        "ai_connections",
        "ai_credentials",
        "ai_credential_connections",
        "ai_models",
        "ai_quota_snapshots",
        "ai_quota_windows",
        "proxy_settings",
        "proxy_routes",
        "proxy_route_targets",
        "proxy_usage_records",
    ] {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |r| r.get(0),
            )
            .expect("check table exists");
        assert_eq!(count, 1, "table {table} must exist");
    }

    // Test idempotency
    apply_migrations(&conn).expect("re-apply migration v31 must be idempotent");
}
