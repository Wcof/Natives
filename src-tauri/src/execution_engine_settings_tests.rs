use super::*;

fn temp_conn() -> (tempfile::TempDir, rusqlite::Connection) {
    let dir = tempfile::tempdir().expect("tempdir");
    let conn = rusqlite::Connection::open_in_memory().expect("conn");
    conn.execute_batch(
            "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL, updated_at TEXT NOT NULL DEFAULT (datetime('now')));",
        )
        .expect("schema");
    (dir, conn)
}

#[test]
fn defaults_are_safe_and_bounded() {
    let s = ExecutionEngineSettingsV2::default().normalized();
    assert_eq!(s.schema_version, 2);
    assert_eq!(s.default_runtime, RuntimeId::Native);
    assert_eq!(
        s.external_unavailable_policy,
        ExternalUnavailablePolicy::Fail
    );
    assert_eq!(s.native.max_steps, 50);
    assert!(!s.codex_cli.enabled, "codex must default to disabled");
}

#[test]
fn migration_imports_legacy_executor_settings() {
    let (_dir, conn) = temp_conn();
    let legacy = serde_json::json!({
        "enabledTools": { "read_file": true, "run_terminal": false, "write_file": true },
        "maxSelfHeal": 3,
        "maxSteps": 25,
    })
    .to_string();
    db::set_setting(&conn, EXECUTOR_KEY, &legacy).expect("set legacy");
    let v2 = migrate_legacy_executor_settings(&conn).expect("migration must succeed");
    assert_eq!(v2.native.max_steps, 25);
    // Only `false` entries become disabled tools (subtractive).
    assert!(
        v2.native.disabled_tools.iter().any(|t| t == "run_terminal"),
        "legacy false must migrate into disabledTools"
    );
    assert!(
        !v2.native.disabled_tools.iter().any(|t| t == "read_file"),
        "legacy true must NOT expand or disable — subtractive only"
    );
    let compat = v2.compat.expect("compat preserved");
    assert_eq!(compat.legacy_max_self_heal, Some(3));
    // Second read does not re-migrate (V2 now present).
    assert!(db::get_setting(&conn, EXECUTION_ENGINE_KEY)
        .unwrap()
        .is_some());
}

#[test]
fn disabled_tools_cannot_expand_capabilities() {
    let mut s = ExecutionEngineSettingsV2::default().normalized();
    s.native.disabled_tools = vec!["read_file".to_string()];
    assert!(!s.effective_tool_allowed("read_file"));
    assert!(s.effective_tool_allowed("write_file"));
    // Saving never enables codex (pure policy layer, no DB needed).
    let saved = prepare_for_save(s).expect("prepare_for_save succeeds");
    assert!(!saved.codex_cli.enabled, "codex stays blocked");
    assert!(saved.revision >= 1, "revision bumped");
}

#[test]
fn fail_policy_never_silent_fallback() {
    let mut s = ExecutionEngineSettingsV2::default().normalized();
    s.default_runtime = RuntimeId::ClaudeCli;
    s.external_unavailable_policy = ExternalUnavailablePolicy::Fail;
    // Claude not installed → status degraded.
    let rt = build_runtime_descriptors(&s);
    let claude = rt.iter().find(|r| r.id == RUNTIME_CLAUDE_CLI).unwrap();
    // Detection depends on the host: treat any non-ready status as
    // unavailable and assert the FAIL policy resolves to the requested
    // runtime WITHOUT fallback_used.
    let resolved = resolve_default_runtime(&s, &rt);
    assert!(
        !resolved.fallback_used,
        "fail policy must never silently fall back"
    );
    assert_eq!(resolved.runtime_id, RUNTIME_CLAUDE_CLI);
    let _ = claude;

    // Explicit fallback_native allows the switch.
    s.external_unavailable_policy = ExternalUnavailablePolicy::FallbackNative;
    let resolved2 = resolve_default_runtime(&s, &rt);
    if claude.status != "ready" {
        assert!(resolved2.fallback_used);
        assert_eq!(resolved2.runtime_id, RUNTIME_NATIVE);
    }
}

#[test]
fn codex_stays_blocked_without_app_server() {
    let mut s = ExecutionEngineSettingsV2::default().normalized();
    s.codex_cli.enabled = true; // hostile input tries to enable it
    let rt = build_runtime_descriptors(&s);
    let codex = rt.iter().find(|r| r.id == RUNTIME_CODEX_CLI).unwrap();
    assert_eq!(codex.status, "blocked");
    assert_eq!(codex.reason_code, "codex_app_server_not_implemented");
    let saved = prepare_for_save(s).expect("prepare_for_save succeeds");
    assert!(!saved.codex_cli.enabled, "codex force-closed on save");
}

#[test]
fn max_steps_is_bounded() {
    let mut s = ExecutionEngineSettingsV2::default().normalized();
    s.native.max_steps = 9999;
    assert_eq!(s.clone().normalized().native.max_steps, 200);
    s.native.max_steps = 1;
    assert_eq!(s.normalized().native.max_steps, 10);
}

/// §5 exact-name regression: V2 migrates legacy executor settings.
#[test]
fn settings_v2_migrates_legacy_executor_settings() {
    migration_imports_legacy_executor_settings();
}

/// §5 exact-name regression: disabled tools cannot expand capabilities.
#[test]
fn settings_disabled_tools_cannot_expand_capabilities() {
    disabled_tools_cannot_expand_capabilities();
}

/// §5 exact-name regression: fail policy never silently falls back.
#[test]
fn explicit_external_runtime_fail_policy_never_silent_fallback() {
    fail_policy_never_silent_fallback();
}

// ── S3 Execution Policy V1 tests ───────────────────────────────────────

/// A descriptor for a runtime that is NOT installed / not ready, so the
/// availability check is deterministic regardless of the host.
fn degraded_descriptor(id: &str) -> RuntimeDescriptor {
    RuntimeDescriptor {
        id: id.to_string(),
        display_name: id.to_string(),
        status: "degraded".into(),
        version: None,
        authority: "external_bridge".into(),
        reason_code: "not_ready".into(),
        reason: "test".into(),
        capabilities: HashMap::new(),
        controllable: Vec::new(),
    }
}

fn ready_descriptor(id: &str) -> RuntimeDescriptor {
    let mut d = degraded_descriptor(id);
    d.status = "ready".into();
    d
}

#[test]
fn explicit_external_unavailable_never_falls_back() {
    let mut settings = ExecutionEngineSettingsV2::default().normalized();
    // Even with fallback_native configured, an EXPLICIT run override that
    // is unavailable must hard-fail (never silently switch).
    settings.external_unavailable_policy = ExternalUnavailablePolicy::FallbackNative;
    let runtimes = [degraded_descriptor(RUNTIME_CLAUDE_CLI)];
    let err = resolve_execution_policy(
        &settings,
        &runtimes,
        Some(RUNTIME_CLAUDE_CLI), // explicit run override
        None,
        None,
    )
    .expect_err("explicit unavailable runtime must fail, never fall back");
    assert!(
        err.contains(RUNTIME_CLAUDE_CLI),
        "error must name the runtime: {err}"
    );
    assert!(
        err.contains("no silent switch"),
        "error must state the no-fallback invariant: {err}"
    );

    // Same for a conversation override.
    let err2 = resolve_execution_policy(&settings, &runtimes, None, Some(RUNTIME_CLAUDE_CLI), None)
        .expect_err("conversation override unavailable must fail");
    assert!(err2.contains(RUNTIME_CLAUDE_CLI));
}

#[test]
fn application_default_fallback_native_is_explicit() {
    let mut settings = ExecutionEngineSettingsV2::default().normalized();
    settings.default_runtime = RuntimeId::ClaudeCli;
    settings.external_unavailable_policy = ExternalUnavailablePolicy::FallbackNative;
    let runtimes = [degraded_descriptor(RUNTIME_CLAUDE_CLI)];

    let resolved = resolve_execution_policy(&settings, &runtimes, None, None, None)
        .expect("application default may fall back to native");
    assert_eq!(resolved.runtime_id, RUNTIME_NATIVE);
    assert!(
        resolved.fallback_used,
        "fallback must be recorded explicitly"
    );
    assert_eq!(resolved.runtime_source, "safe_default");

    // fail policy → honest error, no silent switch.
    settings.external_unavailable_policy = ExternalUnavailablePolicy::Fail;
    let err = resolve_execution_policy(&settings, &runtimes, None, None, None)
        .expect_err("fail policy must not fall back");
    assert!(err.contains("fail"), "error mentions the policy: {err}");
}

#[test]
fn settings_disabled_tools_hidden_and_denied() {
    let mut settings = ExecutionEngineSettingsV2::default().normalized();
    settings.native.disabled_tools = vec!["run_terminal".to_string(), "write_file".to_string()];
    // Schema gate: subtract-only, never expands capability.
    assert!(!settings.effective_tool_allowed("run_terminal"));
    assert!(settings.effective_tool_allowed("read_file"));
    // Handler gate: the resolved policy carries the deny list and only it;
    // there is no channel to re-enable a tool beyond the capability surface.
    let runtimes = [ready_descriptor(RUNTIME_NATIVE)];
    let policy = resolve_execution_policy(&settings, &runtimes, None, None, None).unwrap();
    assert_eq!(
        policy.disabled_tools,
        vec!["run_terminal".to_string(), "write_file".to_string()]
    );
    assert!(policy.disabled_tools.contains(&"run_terminal".to_string()));
    // Normalizing dedups and sorts, and never invents new entries.
    settings
        .native
        .disabled_tools
        .push("run_terminal".to_string());
    let norm = settings.normalized();
    assert_eq!(norm.native.disabled_tools.len(), 2);
}

/// Serialise the DB-pool tests: they replace the global main pool.
static DB_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn temp_main_pool() -> (tempfile::TempDir, db::DbPool) {
    let dir = tempfile::tempdir().expect("tempdir");
    let pool = db::init_db_pool(&dir.path().join("natives-test.db")).expect("init pool");
    db::register_main_pool(pool.clone());
    (dir, pool)
}

#[test]
fn settings_revision_conflict_is_detected() {
    let _guard = DB_TEST_LOCK.lock().unwrap();
    let (_dir, _pool) = temp_main_pool();
    let first = save_execution_engine_settings(ExecutionEngineSettingsV2::default())
        .expect("first save succeeds (expected revision 0)");
    assert_eq!(first.revision, 1, "prepare_for_save bumps revision");
    // Re-save with a STALE revision → CAS conflict, not silent overwrite.
    let err = save_execution_engine_settings(ExecutionEngineSettingsV2::default())
        .expect_err("stale revision must be rejected");
    assert!(
        err.contains("revision conflict"),
        "conflict error must be explicit: {err}"
    );
    // Correct (fresh) revision saves fine.
    let mut next = first.clone();
    next.native.max_steps = 120;
    let ok = save_execution_engine_settings(next).expect("fresh revision saves");
    assert_eq!(ok.revision, 2);
}

#[test]
fn settings_corrupt_json_is_not_default_success() {
    let _guard = DB_TEST_LOCK.lock().unwrap();
    let (_dir, pool) = temp_main_pool();
    let conn = pool.get().expect("conn");
    db::set_setting(&conn, EXECUTION_ENGINE_KEY, "{ not json !!").expect("seed corrupt");
    let err = load_execution_engine_settings()
        .expect_err("corrupt settings must be a hard error, never default success");
    assert!(err.contains("corrupt"), "explicit corrupt error: {err}");
    // Save on top of corrupt data must also refuse (CAS read fails).
    let err2 = save_execution_engine_settings(ExecutionEngineSettingsV2::default())
        .expect_err("CAS over corrupt data must fail");
    assert!(err2.contains("corrupt"));
}

#[test]
fn legacy_runtime_pref_migrates_once() {
    let _guard = DB_TEST_LOCK.lock().unwrap();
    let (_dir, _pool) = temp_main_pool();
    // No pref → no-op, pristine default.
    let noop = migrate_legacy_runtime_pref(None).expect("no-op succeeds");
    assert_eq!(noop.revision, 0);
    assert_eq!(noop.default_runtime, RuntimeId::Native);
    // First migration adopts claude_cli and bumps revision (durable).
    let migrated =
        migrate_legacy_runtime_pref(Some("claude_cli".into())).expect("first migration succeeds");
    assert_eq!(migrated.default_runtime, RuntimeId::ClaudeCli);
    assert!(migrated.revision >= 1, "durable exactly-once marker");
    // Re-invocation is a no-op: backend is authoritative, never re-migrates.
    let again =
        migrate_legacy_runtime_pref(Some("claude_cli".into())).expect("second call succeeds");
    assert_eq!(
        again.revision, migrated.revision,
        "no revision bump on re-migrate"
    );
    assert_eq!(again.default_runtime, RuntimeId::ClaudeCli);
    // A different pref after migration must NOT clobber the V2 choice.
    let keep = migrate_legacy_runtime_pref(Some("native".into())).expect("no clobber");
    assert_eq!(keep.default_runtime, RuntimeId::ClaudeCli);
    assert_eq!(keep.revision, migrated.revision);
}

/// MIG-001 negative: a bad old localStorage value fails with an explicit
/// diagnostic — the old value is never silently adopted as the default.
#[test]
fn legacy_runtime_pref_bad_value_fails_explicitly() {
    let _guard = DB_TEST_LOCK.lock().unwrap();
    let (_dir, _pool) = temp_main_pool();
    // Unknown runtime id → explicit error, V2 stays pristine (no fallback).
    let err = migrate_legacy_runtime_pref(Some("garbage_runtime".into()))
        .expect_err("unknown pref must be a hard error");
    assert!(
        err.contains("not a known runtime id"),
        "diagnostic must name the rejection: {err}"
    );
    let after = load_execution_engine_settings().expect("V2 readable");
    assert_eq!(after.revision, 0, "failed migration must not bump revision");
    assert_eq!(after.default_runtime, RuntimeId::Native);
}

/// MIG-001 negative: codex_cli is fail-closed — the migration refuses to
/// adopt it as defaultRuntime (no guarantee of run failures) and stays on
/// the safe default without inventing a fallback value.
#[test]
fn legacy_runtime_pref_codex_fail_closed_not_adopted() {
    let _guard = DB_TEST_LOCK.lock().unwrap();
    let (_dir, _pool) = temp_main_pool();
    let result = migrate_legacy_runtime_pref(Some(RUNTIME_CODEX_CLI.into()))
        .expect("fail-closed refusal is a success no-op");
    assert_eq!(result.default_runtime, RuntimeId::Native);
    assert_eq!(result.revision, 0, "no durable adoption for codex");
}

#[test]
fn existing_run_unchanged_after_settings_edit() {
    let _guard = DB_TEST_LOCK.lock().unwrap();
    let (_dir, _pool) = temp_main_pool();
    // Run created under settings A.
    let mut settings_a = ExecutionEngineSettingsV2::default().normalized();
    settings_a.native.max_steps = 80;
    settings_a.native.disabled_tools = vec!["run_terminal".to_string()];
    let saved_a = save_execution_engine_settings(settings_a).expect("save A");
    let runtimes = [ready_descriptor(RUNTIME_NATIVE)];
    let policy = resolve_execution_policy(&saved_a, &runtimes, None, None, None).unwrap();
    store_policy_snapshot("run-1", &policy).expect("snapshot persisted");

    // Settings edited afterwards.
    let mut settings_b = saved_a.clone();
    settings_b.native.max_steps = 150;
    settings_b.native.disabled_tools = Vec::new();
    settings_b.default_runtime = RuntimeId::ClaudeCli;
    let _saved_b = save_execution_engine_settings(settings_b).expect("save B");

    // The existing Run's frozen snapshot is unchanged.
    let frozen = load_policy_snapshot("run-1")
        .expect("snapshot readable")
        .expect("snapshot present");
    assert_eq!(frozen.max_steps, 80, "existing run maxSteps frozen");
    assert_eq!(frozen.runtime_id, RUNTIME_NATIVE);
    assert_eq!(frozen.disabled_tools, vec!["run_terminal".to_string()]);
    assert_eq!(frozen.settings_revision, saved_a.revision);
    // A NEW run would resolve the new settings instead.
    let fresh = load_execution_engine_settings().expect("reload");
    let fresh_policy = resolve_execution_policy(
        &fresh,
        &[ready_descriptor(RUNTIME_NATIVE)],
        // Explicit native override → always available; the point here is
        // that maxSteps + disabledTools now come from the EDITED settings.
        Some(RUNTIME_NATIVE),
        None,
        None,
    )
    .expect("new run resolves");
    assert_eq!(fresh_policy.max_steps, 150);
    assert!(fresh_policy.disabled_tools.is_empty());
    assert!(fresh_policy.settings_revision > saved_a.revision);
}

// ── SETTINGS-001: restricted enums + blocked/degraded default gate ─────

#[test]
fn runtime_id_and_policy_roundtrip_through_serde() {
    // Known values round-trip to the wire string.
    assert_eq!(
        serde_json::to_string(&RuntimeId::Native).unwrap(),
        "\"native\""
    );
    assert_eq!(
        serde_json::to_string(&ExternalUnavailablePolicy::FallbackNative).unwrap(),
        "\"fallback_native\""
    );
    // Unknown strings decode to Unknown (observable, never silently lost).
    let parsed: RuntimeId = serde_json::from_str("\"garbage_runtime\"").unwrap();
    assert_eq!(parsed, RuntimeId::Unknown("garbage_runtime".into()));
    assert!(!parsed.is_known());
    assert_eq!(parsed.as_str(), "garbage_runtime");
    let policy: ExternalUnavailablePolicy = serde_json::from_str("\"garbage_policy\"").unwrap();
    assert_eq!(
        policy,
        ExternalUnavailablePolicy::Unknown("garbage_policy".into())
    );
    assert!(!policy.is_known());
}

#[test]
fn unknown_enum_value_is_rejected_on_save() {
    let _guard = DB_TEST_LOCK.lock().unwrap();
    let (_dir, _pool) = temp_main_pool();
    let seeded =
        save_execution_engine_settings(ExecutionEngineSettingsV2::default()).expect("seed");
    // Incoming carries an unknown defaultRuntime → rejected (never saved).
    let mut bad = seeded.clone();
    bad.default_runtime = RuntimeId::Unknown("garbage_runtime".into());
    let err = save_execution_engine_settings(bad).expect_err("unknown runtime must be rejected");
    assert!(err.contains("not a known runtime id"), "{err}");
    // Incoming carries an unknown unavailable policy → rejected.
    let mut bad2 = seeded.clone();
    bad2.external_unavailable_policy = ExternalUnavailablePolicy::Unknown("garbage_policy".into());
    let err2 = save_execution_engine_settings(bad2).expect_err("unknown policy must be rejected");
    assert!(err2.contains("not a known policy"), "{err2}");
    // The durable value is untouched (revision still the seed's).
    let after = load_execution_engine_settings().expect("readable");
    assert_eq!(after.revision, seeded.revision, "no partial write");
    assert_eq!(after.default_runtime, RuntimeId::Native);
}

#[test]
fn unavailable_default_runtime_is_rejected_by_host_gate() {
    let settings = ExecutionEngineSettingsV2::default().normalized();
    let runtimes = build_runtime_descriptors(&settings);

    // codex_cli is blocked → never a valid savable default.
    let mut codex_default = settings.clone();
    codex_default.default_runtime = RuntimeId::CodexCli;
    let err = validate_default_runtime_selectable(&codex_default, &settings, &runtimes)
        .expect_err("blocked runtime must be rejected as a new default");
    assert!(err.contains("blocked"), "names the status: {err}");

    // An unchanged default passes even when its descriptor is degraded —
    // the gate only fires when the default actually changes.
    let mut degraded_rts = runtimes;
    for r in degraded_rts.iter_mut() {
        if r.id == RUNTIME_NATIVE {
            r.status = "degraded".into();
        }
    }
    assert!(
        validate_default_runtime_selectable(&settings, &settings, &degraded_rts).is_ok(),
        "unchanged default must not lock out unrelated edits"
    );
}

// ── SETTINGS-002: descriptors come from real discovery + daemon matrix ──

#[test]
fn descriptors_reflect_real_detection_not_static_claims() {
    let settings = ExecutionEngineSettingsV2::default().normalized();
    let runtimes = build_runtime_descriptors(&settings);

    // codex_cli stays blocked (fail-closed real fact).
    let codex = runtimes.iter().find(|r| r.id == RUNTIME_CODEX_CLI).unwrap();
    assert_eq!(codex.status, "blocked");
    assert_eq!(codex.reason_code, "codex_app_server_not_implemented");

    // claude_cli status is the real probe output — never a hardcoded claim.
    let claude = runtimes
        .iter()
        .find(|r| r.id == RUNTIME_CLAUDE_CLI)
        .unwrap();
    assert!(
        ["ready", "degraded", "not_installed", "disabled"].contains(&claude.status.as_str()),
        "claude status must come from real detection, got: {}",
        claude.status
    );

    // Native is locally ready; the sync descriptors carry NO capability
    // table — capabilities are projected from the daemon handshake only.
    let native = runtimes.iter().find(|r| r.id == RUNTIME_NATIVE).unwrap();
    assert_eq!(native.status, "ready");
    assert!(
        native.capabilities.is_empty(),
        "sync descriptors must not self-announce a static capability table"
    );
}

#[test]
fn daemon_projection_applies_real_matrix() {
    use assistant_protocol::v2::RuntimeFeatureMatrix;

    let settings = ExecutionEngineSettingsV2::default().normalized();
    let mut runtimes = build_runtime_descriptors(&settings);

    let mut matrix = HashMap::new();
    matrix.insert(
        RUNTIME_NATIVE.to_string(),
        RuntimeFeatureMatrix {
            expert: true,
            team: true,
            skills: true,
            mcp: true,
            mechanism: Some("native_gateway".into()),
            ..Default::default()
        },
    );
    matrix.insert(
        RUNTIME_CLAUDE_CLI.to_string(),
        RuntimeFeatureMatrix {
            expert: true,
            team: true,
            skills: true,
            mcp: true,
            mechanism: Some("cli_flags".into()),
            execution_backend: Some("claude_cli_harness".into()),
            note: Some("injected via CLI flags".into()),
            ..Default::default()
        },
    );
    matrix.insert(
        RUNTIME_CODEX_CLI.to_string(),
        RuntimeFeatureMatrix::default(),
    );
    let mut flags = HashMap::new();
    flags.insert("tools".to_string(), "supported".to_string());
    flags.insert("hooks".to_string(), "supported".to_string());

    apply_daemon_projection(&mut runtimes, true, &flags, &matrix);

    let native = runtimes.iter().find(|r| r.id == RUNTIME_NATIVE).unwrap();
    assert_eq!(native.status, "ready");
    assert_eq!(
        native.capabilities.get("expert").map(String::as_str),
        Some("supported")
    );
    assert_eq!(
        native.capabilities.get("tools").map(String::as_str),
        Some("supported")
    );
    assert_eq!(
        native.capabilities.get("mechanism").map(String::as_str),
        Some("native_gateway")
    );

    let claude = runtimes
        .iter()
        .find(|r| r.id == RUNTIME_CLAUDE_CLI)
        .unwrap();
    assert_eq!(
        claude.capabilities.get("expert").map(String::as_str),
        Some("supported")
    );
    // claude keeps its locally detected status; only the matrix is projected.
    assert!(
        claude.status == "ready"
            || claude.status == "degraded"
            || claude.status == "not_installed"
            || claude.status == "disabled"
    );

    let codex = runtimes.iter().find(|r| r.id == RUNTIME_CODEX_CLI).unwrap();
    assert_eq!(
        codex.capabilities.get("expert").map(String::as_str),
        Some("unsupported")
    );
    assert_eq!(codex.status, "blocked", "projection never un-blocks codex");

    // Daemon unreachable → native is honestly degraded, no fabricated ready.
    apply_daemon_projection(&mut runtimes, false, &HashMap::new(), &HashMap::new());
    let native = runtimes.iter().find(|r| r.id == RUNTIME_NATIVE).unwrap();
    assert_eq!(native.status, "degraded");
    assert_eq!(native.reason_code, "daemon_unreachable");
    assert!(native.capabilities.is_empty());
}
