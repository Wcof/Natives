//! Remaining harness control-plane tests (W3 over_1000 split).
//! Shares helpers with the parent via super::.

use super::*;

#[test]
fn new_session_overlay_profiles_are_rejected() {
    let _serial = serial();
    assert_eq!(
        call_err(
            "harness.profile.create",
            json!({ "name": "invalid session overlay", "kind": "session_overlay" }),
        ),
        "invalid_input"
    );
}

#[test]
fn acknowledging_tracked_drift_consumes_candidate_and_updates_manifest() {
    let _serial = serial();
    let profile = new_profile("drift-ack");
    // 审计收口 #9：draft/publish 只允许 current global。
    let _global_binding = SqlGlobalBinding::pointing_at(&profile);
    call(
        "harness.draft.publish",
        json!({ "profile_id": profile, "revision": 0 }),
    );
    let store = repository::store().expect("store");
    let conn = store.conn().expect("connection");
    repository::sync_source(&conn, "source-1", "old", "tracked").expect("seed manifest");
    repository::ensure_source_drift_candidate(
        &conn,
        &profile,
        &json!([{
            "source_id": "source-1",
            "published_digest": "old",
            "observed_digest": "new"
        }]),
    )
    .expect("candidate");

    let result = call(
        "harness.source.acknowledgeDrift",
        json!({
            "profile_id": profile,
            "source_id": "source-1",
            "observed_digest": "new",
            "revision": 0
        }),
    );
    assert_eq!(result["revision"], 1);

    let draft = repository::get_draft(&conn, &profile)
        .expect("draft")
        .expect("draft row");
    let candidate: Value =
        serde_json::from_str(draft.source_candidate_json.as_deref().expect("candidate")).unwrap();
    assert_eq!(candidate[0]["acknowledged"], true);
    let before_publish = repository::source_digest(&conn, "source-1")
        .expect("source")
        .expect("manifest");
    assert_eq!(before_publish.0, "old");

    call(
        "harness.draft.publish",
        json!({ "profile_id": profile, "revision": 1 }),
    );
    let after_publish = repository::source_digest(&conn, "source-1")
        .expect("source")
        .expect("manifest");
    assert_eq!(after_publish.0, "new");
    let status: String = conn
        .query_row(
            "SELECT status FROM harness_source_manifest WHERE source_id = 'source-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status, "current");
}

#[test]
fn incomplete_native_adapter_cannot_be_published() {
    let _serial = serial();
    let profile = new_profile("invalid native adapter");
    // 审计收口 #9：draft/publish 只允许 current global。
    let _global_binding = SqlGlobalBinding::pointing_at(&profile);
    call(
        "harness.draft.save",
        json!({
            "profile_id": profile,
            "revision": 0,
            "document": {
                "schema_version": 3,
                "native_hooks": [{
                    "id": "11111111-1111-1111-1111-111111111111",
                    "name": "Broken MCP",
                    "event": "PreToolUse",
                    "matcher": "*",
                    "adapter": {
                        "type": "mcp_tool",
                        "server_id": "",
                        "tool_name": "",
                        "input_template": "{"
                    }
                }]
            }
        }),
    );
    let validation = call("harness.draft.validate", json!({ "profile_id": profile }));
    assert_eq!(validation["publishable"], false);
    assert_eq!(
        call_err("harness.draft.publish", json!({ "profile_id": profile })),
        "harness_validation_failed"
    );
}

fn overlay_document(hook_id: &str, timeout_ms: u64) -> Value {
    json!({
        "schema_version": 1,
        "hook_semantics_version": "legacy_v1",
        "hooks": [{ "hook_id": hook_id, "timeout_ms": timeout_ms }]
    })
}

#[test]
fn saving_a_draft_with_a_stale_revision_is_a_conflict_not_an_overwrite() {
    let _serial = serial();
    let profile = new_profile("conflict");
    // 审计收口 #9：draft/publish 只允许 current global。
    let _global_binding = SqlGlobalBinding::pointing_at(&profile);
    let draft = call("harness.draft.get", json!({ "profile_id": profile }));
    assert_eq!(draft["revision"], 0);

    call(
        "harness.draft.save",
        json!({
            "profile_id": profile,
            "revision": 0,
            "document": overlay_document(PROBE_HOOK_ID, 5_000)
        }),
    );
    let code = call_err(
        "harness.draft.save",
        json!({
            "profile_id": profile,
            "revision": 0,
            "document": overlay_document(PROBE_HOOK_ID, 9_000)
        }),
    );
    assert_eq!(code, "harness_draft_conflict");

    // The first writer's value survived.
    let current = call("harness.draft.get", json!({ "profile_id": profile }));
    assert_eq!(current["document"]["hooks"][0]["timeout_ms"], 5_000);
    assert_eq!(current["revision"], 1);
}

#[test]
fn a_draft_that_overlays_a_locked_hook_cannot_be_published() {
    let _serial = serial();
    let profile = new_profile("locked");
    // 审计收口 #9：draft/publish 只允许 current global。
    let _global_binding = SqlGlobalBinding::pointing_at(&profile);
    call(
        "harness.draft.save",
        json!({
            "profile_id": profile,
            "revision": 0,
            "document": overlay_document("builtin/allow-all#PreToolUse", 5_000)
        }),
    );
    let validation = call("harness.draft.validate", json!({ "profile_id": profile }));
    assert_eq!(validation["publishable"], false);
    assert_eq!(
        validation["findings"][0]["code"],
        "harness.overlay_on_locked_hook"
    );
    assert_eq!(
        call_err("harness.draft.publish", json!({ "profile_id": profile })),
        "harness_validation_failed"
    );
}

#[test]
fn publishing_produces_an_immutable_version_a_diff_and_an_audit_row() {
    let _serial = serial();
    let project = TempProject::with_hooks(PROBE_HOOK);
    let profile = new_profile("publish");
    // 审计收口 #9：draft/publish 只允许 current global。
    let _global_binding = SqlGlobalBinding::pointing_at(&profile);
    call(
        "harness.draft.save",
        json!({
            "profile_id": profile,
            "revision": 0,
            "document": overlay_document(PROBE_HOOK_ID, 7_000)
        }),
    );
    let diff = call("harness.draft.diff", json!({ "profile_id": profile }));
    assert_eq!(diff["changes"][0]["field"], "timeout_ms");
    assert_eq!(diff["changes"][0]["to"], 7_000);

    let published = call(
        "harness.draft.publish",
        json!({ "profile_id": profile, "project_path": project.path() }),
    );
    assert_eq!(published["version"]["version_number"], 1);
    assert!(published["version"]["canonical_hash"]
        .as_str()
        .is_some_and(|h| h.len() == 64));

    // The draft is gone; the published version is now the record.
    let after = call("harness.draft.get", json!({ "profile_id": profile }));
    assert_eq!(after["revision"], 0);
    assert_eq!(after["document"]["hooks"][0]["timeout_ms"], 7_000);

    let audit = call("harness.audit.list", json!({ "limit": 20 }));
    let entries = audit["entries"].as_array().expect("entries");
    assert!(
        entries
            .iter()
            .any(|e| e["action"] == "publish" && e["profile_id"] == profile.as_str()),
        "publish must leave an audit row"
    );
}

#[test]
fn rollback_republishes_forward_instead_of_rewinding_a_pointer() {
    let _serial = serial();
    let profile = new_profile("rollback");
    // 审计收口 #9：draft/publish/rollback 只允许 current global。
    let _global_binding = SqlGlobalBinding::pointing_at(&profile);
    call(
        "harness.draft.save",
        json!({
            "profile_id": profile,
            "revision": 0,
            "document": overlay_document(PROBE_HOOK_ID, 3_000)
        }),
    );
    let v1 = call("harness.draft.publish", json!({ "profile_id": profile }));
    let v1_id = v1["version"]["id"].as_str().unwrap().to_string();

    call(
        "harness.draft.save",
        json!({
            "profile_id": profile,
            "revision": 0,
            "document": overlay_document(PROBE_HOOK_ID, 4_000)
        }),
    );
    let v2 = call("harness.draft.publish", json!({ "profile_id": profile }));
    assert_eq!(v2["version"]["version_number"], 2);
    let v2_id = v2["version"]["id"].as_str().unwrap();

    let v3 = call(
        "harness.version.rollback",
        json!({ "version_id": v1_id, "expected_current_version_id": v2_id }),
    );
    assert_eq!(
        v3["version"]["version_number"], 3,
        "rollback must move forward, so an old snapshot's version_id still \
         resolves to the bytes that run used"
    );
    assert_eq!(
        v3["version"]["canonical_hash"], v1["version"]["canonical_hash"],
        "the restored content must be identical to the version it came from"
    );

    let versions = call("harness.version.list", json!({ "profile_id": profile }));
    assert_eq!(versions["versions"].as_array().unwrap().len(), 3);
}

// ── the configuration hierarchy ─────────────────────────────────────────────

#[test]
fn a_legacy_project_overlay_binding_is_inert_for_new_runs() {
    // 审计收口 #9：运行时只解析唯一 current global——历史 project binding
    // （迁移前写入）不参与 hook.catalog / run 解析。
    let _serial = serial();
    let project = TempProject::with_hooks(PROBE_HOOK);
    let project_id = call(
        "project.identity.register",
        json!({ "path": project.path(), "name": "hierarchy" }),
    )["project_id"]
        .as_str()
        .unwrap()
        .to_string();

    // Global says 5s（唯一 current global）。
    let global = new_profile("hierarchy-global");
    let _global_binding = SqlGlobalBinding::pointing_at(&global);
    call(
        "harness.draft.save",
        json!({ "profile_id": global, "revision": 0, "document": overlay_document(PROBE_HOOK_ID, 5_000) }),
    );
    call("harness.draft.publish", json!({ "profile_id": global }));

    // 残留的历史 project overlay profile + binding（直接 SQL 写入，模拟迁移前数据）。
    let overlay = new_profile("hierarchy-project-legacy");
    let store = repository::store().expect("store");
    let conn = store.conn().expect("connection");
    conn.execute(
        "INSERT OR REPLACE INTO harness_binding
            (scope_type, scope_id, profile_id, version_id, mode, updated_at)
         VALUES ('project', ?1, ?2, NULL, 'follow_published', datetime('now'))",
        rusqlite::params![project_id, overlay],
    )
    .expect("insert legacy project binding");

    let catalog = call(
        "harness.hook.catalog",
        json!({ "project_path": project.path(), "project_id": project_id }),
    );
    let layers = catalog["layers"].as_array().unwrap();
    assert_eq!(
        layers.len(),
        1,
        "only the global layer applies (唯一 current global)"
    );
    assert_eq!(layers[0]["layer"], "global");
    let probe = catalog["hooks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["id"] == PROBE_HOOK_ID)
        .expect("probe hook");
    assert_eq!(
        probe["timeout_ms"], 5_000,
        "legacy project overlay must not override global"
    );
}

#[test]
fn binding_to_an_unpublished_profile_is_refused() {
    let _serial = serial();
    let profile = new_profile("unpublished");
    assert_eq!(
        call_err(
            "harness.binding.set",
            json!({ "scope_type": "global", "profile_id": profile })
        ),
        "invalid_input"
    );
}

#[test]
fn the_seeded_global_template_cannot_be_archived() {
    let _serial = serial();
    assert_eq!(
        call_err(
            "harness.profile.archive",
            json!({ "profile_id": DEFAULT_GLOBAL_PROFILE_ID })
        ),
        "invalid_input"
    );
}

// ── run snapshots ───────────────────────────────────────────────────────────

/// Insert the conversation and run rows a snapshot's foreign key requires.
#[test]
fn an_unresolved_run_reports_absence_rather_than_an_error() {
    let _serial = serial();
    let value = call(
        "harness.run.getSnapshot",
        json!({ "run_id": "run-that-never-started" }),
    );
    assert_eq!(value["resolved"], false);
    assert_eq!(value["snapshot"], Value::Null);
}

#[test]
fn resolve_run_persists_evidence_that_a_later_publish_cannot_change() {
    let _serial = serial();
    let project = TempProject::with_hooks(PROBE_HOOK);
    let (conversation, run) = seed_run("frozen");

    let profile = new_profile("frozen");
    // 审计收口 #9：draft/publish 只允许 current global——必须先绑定再保存。
    let _global_binding = SqlGlobalBinding::pointing_at(&profile);
    call(
        "harness.draft.save",
        json!({ "profile_id": profile, "revision": 0, "document": overlay_document(PROBE_HOOK_ID, 6_000) }),
    );
    call("harness.draft.publish", json!({ "profile_id": profile }));

    let plan = control_plane::resolve_run(
        &run,
        Some(&conversation),
        None,
        Some(Path::new(&project.path())),
    )
    .expect("resolve run");
    let bound_hash = plan.snapshot.canonical_hash();
    assert_eq!(
        plan.resolution
            .hooks
            .iter()
            .find(|h| h.definition.id.as_str() == PROBE_HOOK_ID)
            .expect("probe hook")
            .definition
            .timeout_ms,
        6_000
    );

    // Publish a different value while the Run is "in flight".
    call(
        "harness.draft.save",
        json!({ "profile_id": profile, "revision": 0, "document": overlay_document(PROBE_HOOK_ID, 1_000) }),
    );
    call("harness.draft.publish", json!({ "profile_id": profile }));

    let stored = call("harness.run.getSnapshot", json!({ "run_id": run }));
    assert_eq!(stored["resolved"], true);
    assert_eq!(stored["canonical_hash"], bound_hash);
    let hook = stored["snapshot"]["hooks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["definition"]["id"] == PROBE_HOOK_ID)
        .expect("probe hook in snapshot");
    assert_eq!(
        hook["definition"]["timeout_ms"], 6_000,
        "an active Run must keep the version it bound at start"
    );

    // A new Run picks up the new version.
    let (conversation2, run2) = seed_run("fresh");
    let plan2 = control_plane::resolve_run(
        &run2,
        Some(&conversation2),
        None,
        Some(Path::new(&project.path())),
    )
    .expect("resolve second run");
    assert_ne!(plan2.snapshot.canonical_hash(), bound_hash);
}

#[test]
fn a_persisted_snapshot_carries_no_hook_secret() {
    let _serial = serial();
    let project = TempProject::with_hooks(PROBE_HOOK);
    let (conversation, run) = seed_run("redaction");
    control_plane::resolve_run(
        &run,
        Some(&conversation),
        None,
        Some(Path::new(&project.path())),
    )
    .expect("resolve run");

    isolate_env();
    let store = repository::store().expect("open store");
    let conn = store.conn().expect("connection");
    let raw: String = conn
        .query_row(
            "SELECT snapshot_json FROM harness_run_snapshot WHERE run_id = ?1",
            [&run],
            |row| row.get(0),
        )
        .expect("snapshot row");
    assert!(
        !raw.contains("super-secret"),
        "a credential-shaped hook URL parameter reached the database"
    );
}

/// The plan's compiled registry must reflect the same Hooks the snapshot
/// records — with live, unredacted adapter configuration, because a redacted
/// URL would fire a different request than the user configured.
#[test]
fn the_compiled_registry_matches_the_snapshot_and_keeps_live_values() {
    let _serial = serial();
    let project = TempProject::with_hooks(PROBE_HOOK);
    let (conversation, run) = seed_run("compile");
    let plan = control_plane::resolve_run(
        &run,
        Some(&conversation),
        None,
        Some(Path::new(&project.path())),
    )
    .expect("resolve run");

    let registry = plan.compile(Some(Path::new(&project.path())));
    let described = registry.describe();
    assert_eq!(
        described.len(),
        plan.snapshot.enabled_hooks().count(),
        "compiled registry and snapshot must describe the same hook set"
    );
    assert!(
        registry.fail_closed_security,
        "compiling must not drop the fail-closed posture"
    );

    let live = plan
        .resolution
        .hooks
        .iter()
        .find(|h| h.definition.id.as_str() == PROBE_HOOK_ID)
        .expect("probe hook");
    let live_kind = serde_json::to_string(&live.definition.kind).unwrap();
    assert!(
        live_kind.contains("super-secret"),
        "the executable resolution must keep the live URL; only the snapshot is redacted"
    );
}
