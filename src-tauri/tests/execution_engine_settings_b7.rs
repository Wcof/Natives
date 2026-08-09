//! B7 — Settings Migration (05 §4): V2 single-authority migration semantics.
//!
//! Fixtures:
//! 1. only localStorage runtimePref → frontend one-shot migration (A7); Rust
//!    side leaves V2 default when neither DB key exists.
//! 2. only `executor:settings` → one-shot migration into v2 (also covered by
//!    the unit test `migration_imports_legacy_executor_settings`).
//! 3. both present & conflicting → the persisted V2 is authoritative; the
//!    legacy key is NOT re-applied over it (no re-migration, no overwrite).

use natives_lib::db;
use natives_lib::execution_engine_settings::{
    load_execution_engine_settings, save_execution_engine_settings, ExecutionEngineSettingsV2,
    ExternalUnavailablePolicy, RuntimeId, EXECUTOR_KEY,
};

/// 模块级锁:本文件的两个测试都向全局 MAIN_DB_POOL 注册自己的 temp pool,
/// 同一测试二进制内并行运行会互相覆盖(flaky)。串行化保证每次只注册一个 pool。
static TEST_POOL_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn register_temp_pool() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("natives-b7.db");
    // Clear the global main pool first: other integration tests register their
    // own main pool concurrently, and a stale/foreign pool would make this
    // test's get_main_conn()/set_setting() read a different DB (flaky).
    natives_lib::db::clear_main_pool_for_tests();
    let pool = db::init_db_pool(&db_path).expect("init db pool");
    {
        let conn = pool.get().expect("conn");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS settings (
                 key TEXT PRIMARY KEY,
                 value TEXT NOT NULL,
                 updated_at TEXT NOT NULL DEFAULT (datetime('now'))
             );",
        )
        .expect("settings schema");
    }
    db::register_main_pool(pool);
    dir
}

#[test]
fn b7_conflict_v2_is_authority_legacy_not_reapplied() {
    let _lock = TEST_POOL_LOCK.lock().unwrap();
    let _dir = register_temp_pool();

    // Fixture 3: persisted V2 (max_steps 120) + legacy key (max_steps 25) both
    // exist and conflict.
    let mut v2 = ExecutionEngineSettingsV2::default();
    v2.native.max_steps = 120;
    let saved = save_execution_engine_settings(v2).expect("save v2");
    assert_eq!(saved.native.max_steps, 120);

    let legacy = serde_json::json!({
        "enabledTools": { "read_file": true, "run_terminal": false },
        "maxSelfHeal": 3,
        "maxSteps": 25,
    })
    .to_string();
    {
        let conn = db::get_main_conn().expect("main conn");
        db::set_setting(&conn, EXECUTOR_KEY, &legacy).expect("write legacy");
    }

    // Load MUST return the persisted V2 — the legacy key is not re-migrated
    // over an existing authority, and max_steps stays 120, not 25.
    // P0-13: load is Result-化 — a corrupt/failed read must be an explicit
    // error, never a silent fake-default success.
    let loaded = load_execution_engine_settings().expect("load must not fail");
    assert_eq!(
        loaded.native.max_steps, 120,
        "conflicting legacy must not overwrite the persisted V2 authority"
    );
    assert_eq!(
        loaded.revision, saved.revision,
        "load must not bump revision (no re-migration)"
    );
}

#[test]
fn b7_no_db_keys_yields_safe_defaults() {
    // Fixture 1 (Rust side): neither key present → V2 defaults (native,
    // max_steps 50, fail policy, codex disabled). localStorage runtimePref is a
    // frontend one-shot migration (A7) and never reaches Rust.
    let _lock = TEST_POOL_LOCK.lock().unwrap();
    let _dir = register_temp_pool();
    let loaded = load_execution_engine_settings().expect("load must not fail");
    assert_eq!(loaded.default_runtime, RuntimeId::Native);
    assert_eq!(loaded.native.max_steps, 50);
    assert_eq!(
        loaded.external_unavailable_policy,
        ExternalUnavailablePolicy::Fail
    );
    assert!(!loaded.codex_cli.enabled, "codex stays blocked");
}
