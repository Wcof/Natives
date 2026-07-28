//! Harness control plane: persistence, the configuration hierarchy, and the
//! guarantee that an in-flight Run is unaffected by a later publish.
//!
//! These drive the real control plane against a real SQLite file, because the
//! properties worth asserting are storage properties: an immutable version, an
//! optimistic draft, a foreign key that ties a snapshot to its Run. A mocked
//! repository would assert nothing.

use natives_agent_daemon::rpc::harness::repository::{self, DEFAULT_GLOBAL_PROFILE_ID};
use natives_agent_daemon::rpc::harness::{self, control_plane};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Serialize the whole binary.
///
/// Every test in this file shares one SQLite file and, more importantly, one
/// `harness_binding` row per scope — the global binding is a singleton by
/// design. Running two binding tests concurrently would not be a flaky test to
/// paper over; it would be two writers racing for the same row, which is not
/// what any of these tests is about. Parallelism also produces "database is
/// locked" on a shared WAL file with no reader to blame.
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Repoint the global binding for one test, then put it back.
///
/// The global binding is a singleton row shared by the whole binary, so a test
/// that leaves it repointed silently changes what every later test resolves.
/// `Drop` restores it even when the test panics, which keeps a real failure
/// from cascading into unrelated ones.
struct GlobalBinding;

impl GlobalBinding {
    fn pointing_at(profile_id: &str) -> Self {
        call(
            "harness.binding.set",
            json!({ "scope_type": "global", "profile_id": profile_id }),
        );
        Self
    }
}

impl Drop for GlobalBinding {
    fn drop(&mut self) {
        let _ = control_plane::request(
            "harness.binding.set",
            json!({ "scope_type": "global", "profile_id": DEFAULT_GLOBAL_PROFILE_ID }),
        );
    }
}

/// Point every store at a throwaway database before the first call.
fn isolate_env() {
    static INIT: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    INIT.get_or_init(|| {
        let root = std::env::temp_dir().join(format!("natives-harness-{}", uuid::Uuid::new_v4()));
        let runtime = root.join("runtime");
        std::fs::create_dir_all(&runtime).expect("create temp runtime dir");
        let db = root.join("assistant.db");
        std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db);
        std::env::set_var("NATIVES_DB_PATH", &db);
        std::env::set_var("NATIVES_RUNTIME_DIR", &runtime);
        std::env::set_var("NATIVES_DAEMON_FIXTURE", "1");
    });
}

fn call(method: &str, params: Value) -> Value {
    isolate_env();
    control_plane::request(method, params).unwrap_or_else(|e| panic!("{method} failed: {e}"))
}

fn call_err(method: &str, params: Value) -> String {
    isolate_env();
    match control_plane::request(method, params) {
        Ok(value) => panic!("{method} unexpectedly succeeded: {value}"),
        Err(e) => e.code.to_string(),
    }
}

/// A project directory with a real `.claude/hooks.json`, so discovery has
/// something to find rather than the test asserting on an empty list.
struct TempProject {
    root: PathBuf,
}

impl TempProject {
    fn with_hooks(body: &str) -> Self {
        let root = std::env::temp_dir().join(format!("natives-harness-p-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join(".claude")).expect("create project");
        std::fs::write(root.join(".claude").join("hooks.json"), body).expect("write hooks");
        Self { root }
    }

    fn path(&self) -> String {
        self.root.display().to_string()
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

const PROBE_HOOK: &str = r#"{"hooks":{"PostToolUse":[
    {"matcher":"Edit|Write","hooks":[
        {"type":"http","url":"https://hooks.example/post?token=super-secret","timeout":30}
    ]}
]}}"#;

/// The Hook id discovery produces for the single hook in `PROBE_HOOK`.
const PROBE_HOOK_ID: &str = "project/.claude/hooks.json#PostToolUse[0]/0";

fn tables() -> Vec<String> {
    isolate_env();
    let store = repository::store().expect("open store");
    let conn = store.conn().expect("connection");
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name LIKE 'harness_%' ORDER BY name")
        .expect("prepare");
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query");
    rows.filter_map(Result::ok).collect()
}

// ── migration 022 ───────────────────────────────────────────────────────────

#[test]
fn migration_creates_every_harness_table() {
    let _serial = serial();
    assert_eq!(
        tables(),
        vec![
            "harness_audit",
            "harness_binding",
            "harness_draft",
            "harness_hook_trace",
            "harness_notice",
            "harness_profile",
            "harness_project_identity",
            "harness_run_snapshot",
            "harness_source_manifest",
            "harness_version",
        ]
    );
}

#[tokio::test]
async fn subscribe_replays_persisted_notices_with_a_cursor() {
    let _serial = serial();
    call(
        "harness.profile.create",
        json!({ "name": "Subscription profile", "kind": "global_template" }),
    );

    let first = harness::request("harness.subscribe", json!({ "cursor": 0, "wait_ms": 0 }))
        .await
        .expect("subscribe");
    let notices = first["notices"].as_array().expect("notice array");
    assert!(notices.iter().any(|notice| notice["kind"] == "published"));
    let cursor = first["next_cursor"].as_i64().expect("cursor");

    let caught_up = harness::request(
        "harness.subscribe",
        json!({ "cursor": cursor, "wait_ms": 0 }),
    )
    .await
    .expect("caught-up subscribe");
    assert_eq!(caught_up["notices"], json!([]));
    assert_eq!(caught_up["next_cursor"], cursor);

    let waiter = tokio::spawn(async move {
        harness::request(
            "harness.subscribe",
            json!({ "cursor": cursor, "wait_ms": 2_000 }),
        )
        .await
    });
    tokio::task::yield_now().await;
    call(
        "harness.profile.create",
        json!({ "name": "Wake subscriber", "kind": "global_template" }),
    );
    let pushed = tokio::time::timeout(std::time::Duration::from_secs(1), waiter)
        .await
        .expect("subscriber was not woken")
        .expect("subscriber task")
        .expect("subscribe result");
    assert!(pushed["notices"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
    assert!(pushed["next_cursor"].as_i64().unwrap_or_default() > cursor);
}

#[tokio::test]
async fn hook_run_events_create_replayable_trace_notices() {
    let _serial = serial();
    let before = harness::request(
        "harness.subscribe",
        json!({ "cursor": 0, "wait_ms": 0, "limit": 200 }),
    )
    .await
    .expect("initial cursor");
    let cursor = before["next_cursor"].as_i64().unwrap_or_default();
    let (_, run_id) = seed_run("trace-notice");
    let store = repository::store().expect("store");
    let conn = store.conn().expect("connection");
    conn.execute(
        "INSERT INTO run_event(run_id, sequence, event_type, payload, timestamp, event_id)
         VALUES(?1, 1, 'hook_invocation_started', '{}', datetime('now'), ?2)",
        rusqlite::params![run_id, format!("evt-{}", uuid::Uuid::new_v4())],
    )
    .expect("insert hook event");

    let page = harness::request(
        "harness.subscribe",
        json!({ "cursor": cursor, "wait_ms": 0 }),
    )
    .await
    .expect("trace notices");
    assert!(page["notices"].as_array().is_some_and(|items| items
        .iter()
        .any(|notice| notice["kind"] == "trace_updated" && notice["run_id"] == run_id)));
}

#[test]
fn harness_reuses_run_project_identity_and_rejects_path_mismatch() {
    let _serial = serial();
    let project = TempProject::with_hooks(PROBE_HOOK);
    let other = TempProject::with_hooks("{}");
    let registered = call(
        "project.identity.register",
        json!({ "path": project.path(), "name": "project" }),
    );
    let repeated = call(
        "project.identity.register",
        json!({ "path": project.path(), "name": "renamed" }),
    );
    assert_eq!(registered["project_id"], repeated["project_id"]);

    let other_id = call(
        "project.identity.register",
        json!({ "path": other.path(), "name": "other" }),
    )["project_id"]
        .as_str()
        .unwrap()
        .to_string();
    let (conversation, run) = seed_run("scope-mismatch");
    let error = match control_plane::resolve_run(
        &run,
        Some(&conversation),
        Some(&other_id),
        Some(Path::new(&project.path())),
    ) {
        Ok(_) => panic!("mismatched project id/path must fail closed"),
        Err(error) => error,
    };
    assert_eq!(error.code, "harness_scope_mismatch");
}

#[test]
fn workspace_returns_a_structured_draft_document() {
    let _serial = serial();
    let draft = call(
        "harness.draft.get",
        serde_json::json!({ "profile_id": DEFAULT_GLOBAL_PROFILE_ID }),
    );
    let workspace = call(
        "harness.workspace.get",
        serde_json::json!({ "profile_id": DEFAULT_GLOBAL_PROFILE_ID }),
    );
    assert_eq!(workspace["draft"]["revision"], draft["revision"]);
    assert!(workspace["draft"]["document"].is_object());
    assert!(workspace["draft"].get("document_json").is_none());
}

#[test]
fn the_database_keeps_wal_and_foreign_keys() {
    let _serial = serial();
    isolate_env();
    let store = repository::store().expect("open store");
    let conn = store.conn().expect("connection");
    let journal: String = conn
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .expect("journal mode");
    let foreign_keys: i64 = conn
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
        .expect("foreign keys");
    assert_eq!(journal.to_lowercase(), "wal");
    assert_eq!(foreign_keys, 1, "harness tables rely on ON DELETE CASCADE");
}

/// A snapshot that outlives its Run is evidence for nothing.
#[test]
fn a_run_snapshot_cascades_with_its_run() {
    let _serial = serial();
    isolate_env();
    let (conversation, run) = seed_run("cascade");
    control_plane::resolve_run(&run, Some(&conversation), None, None).expect("resolve");

    let store = repository::store().expect("open store");
    let conn = store.conn().expect("connection");
    let before: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM harness_run_snapshot WHERE run_id = ?1",
            [&run],
            |row| row.get(0),
        )
        .expect("count");
    assert_eq!(before, 1);

    conn.execute("DELETE FROM run WHERE id = ?1", [&run])
        .expect("delete run");
    let after: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM harness_run_snapshot WHERE run_id = ?1",
            [&run],
            |row| row.get(0),
        )
        .expect("count");
    assert_eq!(after, 0, "snapshot must cascade with its run");
}

// ── seeded defaults ─────────────────────────────────────────────────────────

#[test]
fn the_default_global_template_is_seeded_and_empty() {
    let _serial = serial();
    let overview = call("harness.overview", json!({}));
    let layers = overview["layers"].as_array().expect("layers");
    assert_eq!(layers.len(), 1, "only the global layer applies by default");
    assert_eq!(layers[0]["layer"], "global");
    assert_eq!(layers[0]["profile_id"], DEFAULT_GLOBAL_PROFILE_ID);
    assert_eq!(layers[0]["version_number"], 1);

    let profile = call(
        "harness.profile.get",
        json!({ "profile_id": DEFAULT_GLOBAL_PROFILE_ID }),
    );
    assert_eq!(profile["document"]["hooks"], json!([]));
    assert_eq!(profile["document"]["hook_semantics_version"], "legacy_v1");
}

/// Phase 3 owns telemetry. The overview must say so rather than shipping an
/// empty array a caller would read as "nothing ever ran".
#[test]
fn the_overview_declares_telemetry_absent_rather_than_faking_it() {
    let _serial = serial();
    let overview = call("harness.overview", json!({}));
    assert_eq!(overview["telemetry"]["available"], false);
    assert_eq!(overview["telemetry"]["reason"], "phase_3");
}

// ── the two questions the control plane exists to answer ────────────────────

#[test]
fn topology_covers_every_stage_and_every_hook_event() {
    let _serial = serial();
    let value = call("harness.topology", json!({}));
    let stages = value["stages"].as_array().expect("stages");
    assert_eq!(stages.len(), 11);

    let events: Vec<String> = stages
        .iter()
        .flat_map(|s| s["hook_points"].as_array().cloned().unwrap_or_default())
        .map(|p| p["event"].as_str().unwrap_or_default().to_string())
        .collect();
    assert_eq!(events.len(), 16, "all sixteen Hook events must be placed");

    let permission = stages
        .iter()
        .find(|s| s["id"] == "permission")
        .expect("permission stage");
    assert_eq!(permission["safe_points"][0], "after_permission_resolved");
    assert_eq!(
        permission["hook_points"][0]["dispatch_module"], "natives-agent-daemon::production_tools",
        "permission hooks fire from the tool path, not the engine loop"
    );

    let subagent = stages
        .iter()
        .find(|s| s["id"] == "subagent")
        .expect("subagent stage");
    assert_eq!(
        subagent["hook_points"][0]["dispatched"], true,
        "SubagentStart/Stop are dispatched by the `task` tool"
    );
    assert_eq!(
        subagent["hook_points"][0]["dispatch_module"], "natives-agent-daemon::production_tools",
        "the parent's engine hands the spawn off and returns, so the subagent \
         lifecycle cannot come from the engine loop"
    );
}

#[test]
fn the_catalog_answers_which_stage_uses_which_hook_and_where_it_came_from() {
    let _serial = serial();
    let project = TempProject::with_hooks(PROBE_HOOK);
    let value = call(
        "harness.hook.catalog",
        json!({ "project_path": project.path() }),
    );
    let hooks = value["hooks"].as_array().expect("hooks");
    let probe = hooks
        .iter()
        .find(|h| h["id"] == PROBE_HOOK_ID)
        .expect("the project hook must appear in the catalog");

    assert_eq!(probe["event"], "PostToolUse");
    assert_eq!(probe["stage"], "tool_execute");
    assert_eq!(probe["source"]["scope"], "project");
    assert_eq!(probe["source"]["origin"], ".claude/hooks.json");
    assert_eq!(probe["source"]["group_index"], 0);
    assert_eq!(probe["source"]["entry_index"], 0);
    assert_eq!(probe["matcher"], "Edit|Write");
    assert_eq!(probe["timeout_ms"], 30_000);
    assert_eq!(probe["failure_policy"], "fail");
    assert_eq!(probe["enabled"], true);
    assert_eq!(probe["locked"], false);
    assert_eq!(probe["dispatched"], true);

    // The builtin defaults are locked and cannot be configured away.
    let builtin = hooks
        .iter()
        .find(|h| h["id"] == "builtin/allow-all#PreToolUse")
        .expect("builtin default");
    assert_eq!(builtin["locked"], true);
}

#[test]
fn a_hook_url_secret_never_reaches_the_catalog() {
    let _serial = serial();
    let project = TempProject::with_hooks(PROBE_HOOK);
    let value = call(
        "harness.hook.catalog",
        json!({ "project_path": project.path() }),
    );
    let text = value.to_string();
    assert!(!text.contains("super-secret"), "catalog leaked a token");
    assert!(text.contains("https://hooks.example/post"));
}

// ── drafts, publishing, rollback ────────────────────────────────────────────

fn new_profile(name: &str) -> String {
    call(
        "harness.profile.create",
        json!({ "name": name, "kind": "global_template" }),
    )["profile"]["id"]
        .as_str()
        .expect("profile id")
        .to_string()
}

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
fn a_project_overlay_wins_over_the_global_template() {
    let _serial = serial();
    let project = TempProject::with_hooks(PROBE_HOOK);
    let project_id = call(
        "project.identity.register",
        json!({ "path": project.path(), "name": "hierarchy" }),
    )["project_id"]
        .as_str()
        .unwrap()
        .to_string();

    // Global says 5s.
    let global = new_profile("hierarchy-global");
    call(
        "harness.draft.save",
        json!({ "profile_id": global, "revision": 0, "document": overlay_document(PROBE_HOOK_ID, 5_000) }),
    );
    call("harness.draft.publish", json!({ "profile_id": global }));
    let _global_binding = GlobalBinding::pointing_at(&global);

    // Project says 9s.
    let overlay = call(
        "harness.profile.create",
        json!({ "name": "hierarchy-project", "kind": "project_overlay", "project_id": project_id }),
    )["profile"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    call(
        "harness.draft.save",
        json!({ "profile_id": overlay, "revision": 0, "document": overlay_document(PROBE_HOOK_ID, 9_000) }),
    );
    call("harness.draft.publish", json!({ "profile_id": overlay }));
    call(
        "harness.binding.set",
        json!({ "scope_type": "project", "scope_id": project_id, "profile_id": overlay }),
    );

    let catalog = call(
        "harness.hook.catalog",
        json!({ "project_path": project.path(), "project_id": project_id }),
    );
    let probe = catalog["hooks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["id"] == PROBE_HOOK_ID)
        .expect("probe hook");
    assert_eq!(probe["timeout_ms"], 9_000, "project must beat global");
    assert_eq!(probe["overrides"][0]["field"], "timeout_ms");
    assert_eq!(probe["overrides"][0]["layer"], "project");
    assert_eq!(catalog["layers"].as_array().unwrap().len(), 2);

    // Without the project id, only the global layer applies.
    let global_only = call(
        "harness.hook.catalog",
        json!({ "project_path": project.path() }),
    );
    let probe = global_only["hooks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["id"] == PROBE_HOOK_ID)
        .expect("probe hook");
    assert_eq!(probe["timeout_ms"], 5_000);
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
fn seed_run(tag: &str) -> (String, String) {
    isolate_env();
    let conversation = format!("conv-{tag}-{}", uuid::Uuid::new_v4());
    let run = format!("run-{tag}-{}", uuid::Uuid::new_v4());
    let store = repository::store().expect("open store");
    let conn = store.conn().expect("connection");
    conn.execute(
        "INSERT INTO conversation (id, mode, title, provider_id, model_id)
         VALUES (?1, 'agent', 'harness test', 'p', 'm')",
        [&conversation],
    )
    .expect("insert conversation");
    conn.execute(
        "INSERT INTO run (id, conversation_id, status, provider_id, model_id)
         VALUES (?1, ?2, 'queued', 'p', 'm')",
        [&run, &conversation],
    )
    .expect("insert run");
    (conversation, run)
}

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
    call(
        "harness.draft.save",
        json!({ "profile_id": profile, "revision": 0, "document": overlay_document(PROBE_HOOK_ID, 6_000) }),
    );
    call("harness.draft.publish", json!({ "profile_id": profile }));
    let _global_binding = GlobalBinding::pointing_at(&profile);

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
