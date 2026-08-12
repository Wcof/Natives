//! Harness control plane: persistence, the configuration hierarchy, and the
//! guarantee that an in-flight Run is unaffected by a later publish.
//!
//! These drive the real control plane against a real SQLite file, because the
//! properties worth asserting are storage properties: an immutable version, an
//! optimistic draft, a foreign key that ties a snapshot to its Run. A mocked
//! repository would assert nothing.

use harness_core::resolver::ProfileLayer;
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
#[allow(clippy::await_holding_lock)] // serial() 串行化 guard 跨 await 持有
async fn subscribe_replays_persisted_notices_with_a_cursor() {
    let _serial = serial();
    // 审计收口 #9：profile.create 已退役，fixture 用 SQL 构造已发布 global profile。
    let profile = new_profile("Subscription profile");
    let _global_binding = SqlGlobalBinding::pointing_at(&profile);
    call(
        "harness.draft.save",
        json!({
            "profile_id": profile,
            "revision": 0,
            "document": {
                "schema_version": 1,
                "hook_semantics_version": "legacy_v1",
                "hooks": [{ "hook_id": PROBE_HOOK_ID, "timeout_ms": 5_000 }]
            }
        }),
    );
    call("harness.draft.publish", json!({ "profile_id": profile }));

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
    // 唤醒订阅者：发布第二个 profile 产生新的 published notice。
    let wake = new_profile("Wake subscriber");
    let _wake_binding = SqlGlobalBinding::pointing_at(&wake);
    call(
        "harness.draft.save",
        json!({
            "profile_id": wake,
            "revision": 0,
            "document": {
                "schema_version": 1,
                "hook_semantics_version": "legacy_v1",
                "hooks": [{ "hook_id": PROBE_HOOK_ID, "timeout_ms": 3_000 }]
            }
        }),
    );
    call("harness.draft.publish", json!({ "profile_id": wake }));
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
#[allow(clippy::await_holding_lock)] // serial() 串行化 guard 跨 await 持有
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
    assert!(workspace["prompt_plan"]["blocks"].is_array());
    assert!(workspace["prompt_plan"]["layers"].is_array());
    assert!(workspace["prompt_plan"]["effective_prompt_hash"]
        .as_str()
        .is_some_and(|hash| !hash.is_empty()));
    assert_eq!(workspace["prompt_plan"]["raw_persisted"], false);
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

#[test]
fn run_snapshot_records_the_exact_bounded_tool_plan_without_descriptions() {
    let _serial = serial();
    isolate_env();
    let (conversation, run) = seed_run("tool-plan");
    let schemas = vec![agent_core::ToolSchema {
        name: "mcp__docs__search".into(),
        description: "prompt-like text that must not persist".into(),
        input_schema: serde_json::json!({"type": "object", "properties": {"q": {"type": "string"}}}),
    }];
    control_plane::resolve_run_with_tool_plan(&run, Some(&conversation), None, None, &schemas)
        .expect("resolve");

    let snapshot = call(
        "harness.run.getSnapshot",
        serde_json::json!({"run_id": run}),
    );
    assert_eq!(
        snapshot["snapshot"]["tool_plan"]["tools"][0]["name"],
        "mcp__docs__search"
    );
    assert_eq!(
        snapshot["snapshot"]["tool_plan"]["tools"][0]["source"],
        "mcp:docs"
    );
    assert!(snapshot["snapshot"]["tool_plan"]["tools"][0]
        .get("description")
        .is_none());
    assert!(!snapshot.to_string().contains("prompt-like text"));
}

#[test]
fn run_snapshot_is_persisted_only_after_effective_prompt_is_compiled() {
    let _serial = serial();
    isolate_env();
    let (conversation, run) = seed_run("prompt-evidence");
    let mut plan =
        control_plane::prepare_run_with_tool_plan(&run, Some(&conversation), None, None, &[])
            .expect("prepare");

    let store = repository::store().expect("open store");
    let conn = store.conn().expect("connection");
    let before: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM harness_run_snapshot WHERE run_id = ?1",
            [&run],
            |row| row.get(0),
        )
        .expect("count before bind");
    assert_eq!(before, 0, "preparation must not persist partial evidence");

    let mut builder = harness_core::PromptPlanBuilder::new();
    builder.add_builtin_surface("native", "effective provider prompt");
    let compiled = builder.build();
    control_plane::persist_run_plan(&mut plan, &compiled).expect("persist");

    let snapshot = call(
        "harness.run.getSnapshot",
        serde_json::json!({"run_id": run}),
    );
    assert_eq!(
        snapshot["snapshot"]["prompt_plan"]["effective_prompt_hash"],
        compiled.effective_prompt_hash
    );
    assert_eq!(
        snapshot["snapshot"]["prompt_plan"]["layers"][0]["kind"],
        "builtin_surface"
    );
    assert!(!snapshot.to_string().contains("effective provider prompt"));
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
    let edges = value["edges"]
        .as_array()
        .expect("authoritative topology edges");
    assert_eq!(edges.len(), 12);
    assert!(edges.iter().all(|edge| {
        stages.iter().any(|stage| stage["id"] == edge["from"])
            && stages.iter().any(|stage| stage["id"] == edge["to"])
    }));
    assert!(edges.iter().any(|edge| {
        edge["from"] == "stop" && edge["to"] == "provider" && edge["kind"] == "loop"
    }));

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

/// Create a Harness profile row directly (审计收口 #9：`harness.profile.create`
/// 已退役 fail-closed，测试 fixture 不能再用退役 RPC 构造 profile)。
/// 不插入 version——发布状态由各测试通过 draft.publish 产生，与生产语义一致。
fn new_profile(name: &str) -> String {
    isolate_env();
    let id = format!("profile-{}", uuid::Uuid::new_v4());
    let store = repository::store().expect("open store");
    let conn = store.conn().expect("connection");
    conn.execute(
        "INSERT INTO harness_profile (id, name, kind) VALUES (?1, ?2, 'global_template')",
        rusqlite::params![id, name],
    )
    .expect("insert profile row");
    id
}

/// 审计收口 #9 fixture：直接 SQL 把 global binding 指向 profile（绕过
/// binding.set 的 published 校验——测试需要"未发布但已是 current global"
/// 的状态来验证 draft.publish 产生第一个 version）。Drop 恢复默认 global，
/// 避免污染后续测试。
struct SqlGlobalBinding;

impl SqlGlobalBinding {
    fn pointing_at(profile_id: &str) -> Self {
        let store = repository::store().expect("open store");
        let conn = store.conn().expect("connection");
        conn.execute(
            "INSERT OR REPLACE INTO harness_binding
                (scope_type, scope_id, profile_id, version_id, mode, updated_at)
             VALUES ('global', 'global', ?1, NULL, 'follow_published', datetime('now'))",
            rusqlite::params![profile_id],
        )
        .expect("bind global profile");
        Self
    }
}

impl Drop for SqlGlobalBinding {
    fn drop(&mut self) {
        let store = repository::store().expect("open store");
        let conn = store.conn().expect("connection");
        let _ = conn.execute(
            "INSERT OR REPLACE INTO harness_binding
                (scope_type, scope_id, profile_id, version_id, mode, updated_at)
             VALUES ('global', 'global', ?1, NULL, 'follow_published', datetime('now'))",
            rusqlite::params![DEFAULT_GLOBAL_PROFILE_ID],
        );
    }
}

/// 审计收口 #9 fixture：直接 SQL 把 global binding 指向 profile（绕过
/// binding.set 的 published 校验——测试需要"未发布但已是 current global"
/// 的状态来验证 draft.publish 产生第一个 version）。
fn bind_global_sql(profile: &str) {
    let store = repository::store().expect("open store");
    let conn = store.conn().expect("connection");
    conn.execute(
        "INSERT OR REPLACE INTO harness_binding
            (scope_type, scope_id, profile_id, version_id, mode, updated_at)
         VALUES ('global', 'global', ?1, NULL, 'follow_published', datetime('now'))",
        rusqlite::params![profile],
    )
    .expect("bind global profile");
}

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

// W3 over_1000 split: remaining tests live in the tail submodule.
#[path = "harness_control_plane/tail.rs"]
mod tail;

#[test]
fn audit9_legacy_project_and_session_bindings_do_not_affect_new_runs() {
    // 审计收口 #9：运行时只解析唯一 current global。即使 DB 里残留旧的
    // project/session bindings（迁移前版本写入），新 run 也只加载 global 层，
    // 旧绑定数据保留只读但不参与解析。
    let _serial = serial();
    isolate_env();
    let profile = new_profile("legacy-target");
    let (conversation, run) = seed_run("audit9-legacy-binding");
    let store = repository::store().expect("store");
    let conn = store.conn().expect("connection");
    // 直接插入旧的 project/session bindings（绕过已退役的 binding.set 写入口，
    // 模拟迁移前的历史数据）。
    conn.execute(
        "INSERT OR REPLACE INTO harness_binding
            (scope_type, scope_id, profile_id, version_id, mode, updated_at)
         VALUES ('project', 'proj-legacy', ?1, NULL, 'follow_published', datetime('now')),
                ('session', ?2, ?1, NULL, 'follow_published', datetime('now'))",
        rusqlite::params![profile, conversation],
    )
    .expect("insert legacy project/session bindings");

    // resolve_run 返回 RunHarnessPlan（结构体）；snapshot.layers 是解析证据。
    let plan = control_plane::resolve_run(&run, Some(&conversation), None, None)
        .expect("resolve run with legacy bindings");
    let layers = &plan.snapshot.layers;
    assert_eq!(
        layers.len(),
        1,
        "only the global layer applies (唯一 current global)"
    );
    assert_eq!(layers[0].layer, ProfileLayer::Global);
    assert_ne!(
        layers[0].profile_id, profile,
        "legacy project/session binding must not leak"
    );
}
