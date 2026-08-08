//! Execution Engine Settings V2 — single durable authority for runtime
//! policy (ADR / 02-execution-engine-settings-v2.md).
//!
//! Key: `execution_engine:settings:v2` in natives.db `settings` KV.
//! Migrates the legacy `executor:settings` (six tool switches + maxSelfHeal)
//! once; the legacy reader stays available for one compatibility cycle.
//!
//! Invariants:
//! - Settings only ever TIGHTEN tool surface (`disabledTools` is subtractive).
//! - Codex stays `blocked` until the app-server is real (fail-closed).
//! - External runtime unavailable + `externalUnavailablePolicy=fail` never
//!   silently falls back to Native.
//! - Migration failure keeps old values; never "default success".

use crate::db;
use crate::executor_catalog;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Durable settings key (new authority).
pub const EXECUTION_ENGINE_KEY: &str = "execution_engine:settings:v2";
/// Legacy key kept for one compatibility cycle (rollback / migration source).
pub const EXECUTOR_KEY: &str = "executor:settings";
/// Schema revision bumped on every persisted change (conflict detection).
pub const SCHEMA_VERSION: u32 = 2;

pub const DEFAULT_MAX_STEPS: u32 = 50;
pub const MAX_STEPS_MIN: u32 = 10;
pub const MAX_STEPS_MAX: u32 = 200;

/// Runtime id values (wire names must match the frontend / schema).
pub const RUNTIME_NATIVE: &str = "native";
pub const RUNTIME_CLAUDE_CLI: &str = "claude_cli";
pub const RUNTIME_CODEX_CLI: &str = "codex_cli";

// ── V2 model (mirrors contracts/execution-engine-settings-v2.schema.json) ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionEngineSettingsV2 {
    pub schema_version: u32,
    pub revision: u32,
    #[serde(default = "default_runtime")]
    pub default_runtime: String,
    #[serde(default = "default_external_unavailable_policy")]
    pub external_unavailable_policy: String,
    pub native: NativeSettings,
    pub claude_cli: ExternalCliSettings,
    pub codex_cli: ExternalCliSettings,
    pub diagnostics: DiagnosticsSettings,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compat: Option<CompatSettings>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NativeSettings {
    #[serde(default = "default_max_steps")]
    pub max_steps: u32,
    /// Subtractive only: names of tools the user disabled on top of the
    /// capability/permission surface. Never enables a tool.
    #[serde(default)]
    pub disabled_tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExternalCliSettings {
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsSettings {
    #[serde(default = "default_perf_telemetry")]
    pub performance_telemetry: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CompatSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_max_self_heal: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_enabled_tools: Option<HashMap<String, bool>>,
}

fn default_runtime() -> String {
    RUNTIME_NATIVE.to_string()
}
fn default_external_unavailable_policy() -> String {
    "fail".to_string()
}
fn default_max_steps() -> u32 {
    DEFAULT_MAX_STEPS
}
fn default_perf_telemetry() -> bool {
    true
}

impl Default for ExecutionEngineSettingsV2 {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            revision: 0,
            default_runtime: default_runtime(),
            external_unavailable_policy: default_external_unavailable_policy(),
            native: NativeSettings {
                max_steps: default_max_steps(),
                disabled_tools: Vec::new(),
            },
            claude_cli: ExternalCliSettings { enabled: true },
            codex_cli: ExternalCliSettings { enabled: false },
            diagnostics: DiagnosticsSettings {
                performance_telemetry: default_perf_telemetry(),
            },
            compat: None,
        }
    }
}

impl ExecutionEngineSettingsV2 {
    /// Clamp max_steps to the schema bounds (10..=200, default 50).
    pub fn normalized(mut self) -> Self {
        self.native.max_steps = self.native.max_steps.clamp(MAX_STEPS_MIN, MAX_STEPS_MAX);
        self.native.disabled_tools = self
            .native
            .disabled_tools
            .iter()
            .cloned()
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        self.native.disabled_tools.sort();
        self
    }

    /// Effective tool surface is always the CAPABILITY surface MINUS
    /// disabledTools — Settings can never expand permissions.
    pub fn effective_tool_allowed(&self, tool_name: &str) -> bool {
        !self.native.disabled_tools.iter().any(|d| d == tool_name)
    }
}

// ── Persistence + migration ───────────────────────────────────────────────

/// Load V2 settings; on first read, migrate the legacy `executor:settings`.
pub fn load_execution_engine_settings() -> ExecutionEngineSettingsV2 {
    let defaults = ExecutionEngineSettingsV2::default();
    let Ok(pool_conn) = db::get_main_conn() else {
        return defaults;
    };
    let conn: &rusqlite::Connection = &pool_conn;
    match db::get_setting(conn, EXECUTION_ENGINE_KEY) {
        Ok(Some(json)) => serde_json::from_str::<ExecutionEngineSettingsV2>(&json)
            .map(|s| s.normalized())
            .unwrap_or(defaults),
        Ok(None) => migrate_legacy_executor_settings(conn),
        Err(_) => defaults,
    }
}

/// One-shot migration from `executor:settings` (six tool switches + maxSelfHeal).
///
/// Rules (02 doc §8.1):
/// - `maxSteps` → `native.maxSteps` (bounded).
/// - `enabledTools` entries that are `false` migrate into `native.disabledTools`.
/// - Legacy `true` is HISTORICAL INFORMATION ONLY — it never re-enables a tool
///   the current capability system would deny (no privilege expansion).
/// - Full legacy value is preserved in `compat` for rollback.
pub fn migrate_legacy_executor_settings(conn: &rusqlite::Connection) -> ExecutionEngineSettingsV2 {
    let mut v2 = ExecutionEngineSettingsV2::default();
    let legacy = db::get_setting(conn, EXECUTOR_KEY).ok().flatten();
    if let Some(json) = legacy {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Legacy {
            #[serde(default)]
            enabled_tools: HashMap<String, bool>,
            #[serde(default)]
            max_self_heal: Option<u32>,
            #[serde(default)]
            max_steps: Option<u32>,
        }
        if let Ok(old) = serde_json::from_str::<Legacy>(&json) {
            // Subtract only: `false` in legacy means the user disabled it.
            for (name, enabled) in &old.enabled_tools {
                if !enabled {
                    v2.native.disabled_tools.push(name.clone());
                }
            }
            if let Some(steps) = old.max_steps {
                v2.native.max_steps = steps.clamp(MAX_STEPS_MIN, MAX_STEPS_MAX);
            }
            v2.compat = Some(CompatSettings {
                legacy_max_self_heal: old.max_self_heal,
                legacy_enabled_tools: Some(old.enabled_tools),
            });
        }
    }
    let v2 = v2.normalized();
    // Persist immediately so the migration is exactly-once.
    if let Ok(json) = serde_json::to_string(&v2) {
        let _ = db::set_setting(conn, EXECUTION_ENGINE_KEY, &json);
    }
    v2
}

/// Pure policy application before persistence (testable without a DB pool):
/// schema pin, revision bump, codex fail-closed, bounds.
pub fn prepare_for_save(mut settings: ExecutionEngineSettingsV2) -> ExecutionEngineSettingsV2 {
    settings.schema_version = SCHEMA_VERSION;
    settings.revision = settings.revision.wrapping_add(1);
    // Codex stays fail-closed until the app-server is implemented: force the
    // flag off and never advertise it as enabled regardless of stored value.
    settings.codex_cli.enabled = false;
    settings.normalized()
}

/// Persist V2 settings, bumping revision (conflict detection for multi-window).
pub fn save_execution_engine_settings(
    settings: ExecutionEngineSettingsV2,
) -> Result<ExecutionEngineSettingsV2, String> {
    let settings = prepare_for_save(settings);
    let json = serde_json::to_string(&settings)
        .map_err(|e| format!("serialize execution engine settings: {e}"))?;
    let pool_conn = db::get_main_conn().map_err(|e| e.to_string())?;
    let conn: &rusqlite::Connection = &pool_conn;
    db::set_setting(conn, EXECUTION_ENGINE_KEY, &json).map_err(|e| e.to_string())?;
    Ok(settings)
}

/// Legacy compatibility reader — kept for exactly one release cycle. New
/// producers write V2 only; old readers can still see the old key.
pub fn load_legacy_executor_settings(
) -> Option<crate::commands::executor_settings::ExecutorSettings> {
    let Ok(pool_conn) = db::get_main_conn() else {
        return None;
    };
    let conn: &rusqlite::Connection = &pool_conn;
    db::get_setting(conn, EXECUTOR_KEY)
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str(&s).ok())
}

// ── Runtime detection + snapshot ──────────────────────────────────────────

/// External binary detection (existence + version probe, no side effects).
fn detect_external_cli(binary: &str) -> (Option<String>, String) {
    use std::process::Command;
    match Command::new(binary).arg("--version").output() {
        Ok(out) if out.status.success() => {
            let version = String::from_utf8_lossy(&out.stdout).trim().to_string();
            (Some(version), "ready".to_string())
        }
        Ok(_) => (None, "degraded".to_string()),
        Err(_) => (None, "not_installed".to_string()),
    }
}

/// Runtime descriptor for the settings snapshot (backend-derived truth).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeDescriptor {
    pub id: String,
    pub display_name: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub authority: String,
    pub reason_code: String,
    pub reason: String,
    pub capabilities: HashMap<String, String>,
    pub controllable: Vec<String>,
}

/// Resolve the application default runtime (backend authority, not UI guess).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedDefaultRuntime {
    pub runtime_id: String,
    pub source: String,
    pub fallback_used: bool,
    pub reason_code: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionEngineSnapshot {
    pub settings: ExecutionEngineSettingsV2,
    pub runtimes: Vec<RuntimeDescriptor>,
    pub resolved_default: ResolvedDefaultRuntime,
    pub default_provider: Option<serde_json::Value>,
    pub diagnostics_summary: serde_json::Value,
}

/// Build the full snapshot consumed by Settings → 执行引擎 (A7 UI).
pub fn build_execution_engine_snapshot(
    authority_mode: &str,
    protocol_version: &str,
    stream_transport: &str,
    daemon_ready: bool,
) -> ExecutionEngineSnapshot {
    let settings = load_execution_engine_settings();
    let runtimes = build_runtime_descriptors(&settings);
    let resolved_default = resolve_default_runtime(&settings, &runtimes);
    ExecutionEngineSnapshot {
        settings,
        runtimes,
        resolved_default,
        default_provider: None, // provider default resolved at conversation level
        diagnostics_summary: serde_json::json!({
            "authorityMode": authority_mode,
            "protocolVersion": protocol_version,
            "streamTransport": stream_transport,
            "daemonReady": daemon_ready,
        }),
    }
}

/// Native capabilities are the real daemon capability surface; external
/// runtimes never claim Natives' checkpoint/ledger/replay authority.
pub fn build_runtime_descriptors(settings: &ExecutionEngineSettingsV2) -> Vec<RuntimeDescriptor> {
    let mut runtimes = Vec::new();

    // Native — full Natives authority.
    let mut native_caps = HashMap::new();
    for (cap, supported) in [
        ("streaming", "supported"),
        ("tools", "supported"),
        ("mcp", "supported"),
        ("hooks", "supported"),
        ("subagent", "supported"),
        ("checkpoint_resume", "supported"),
        ("side_effect_ledger", "supported"),
        ("provider_routing", "supported"),
    ] {
        native_caps.insert(cap.to_string(), supported.to_string());
    }
    runtimes.push(RuntimeDescriptor {
        id: RUNTIME_NATIVE.to_string(),
        display_name: "Native".to_string(),
        status: "ready".to_string(),
        version: None,
        authority: "native".to_string(),
        reason_code: "native_ready".to_string(),
        reason: "完整 Natives authority（checkpoint / ledger / replay / resume）".to_string(),
        capabilities: native_caps,
        controllable: vec!["maxSteps".to_string(), "disabledTools".to_string()],
    });

    // Claude CLI — external bridge; never claims Natives authority.
    let (claude_version, claude_status) = if settings.claude_cli.enabled {
        detect_external_cli("claude")
    } else {
        (None, "disabled".to_string())
    };
    let mut claude_caps = HashMap::new();
    claude_caps.insert("streaming".to_string(), "supported".to_string());
    claude_caps.insert("tools".to_string(), "external_owned".to_string());
    claude_caps.insert("mcp".to_string(), "external_owned".to_string());
    claude_caps.insert("hooks".to_string(), "external_owned".to_string());
    claude_caps.insert("checkpoint_resume".to_string(), "unsupported".to_string());
    claude_caps.insert("side_effect_ledger".to_string(), "unsupported".to_string());
    runtimes.push(RuntimeDescriptor {
        id: RUNTIME_CLAUDE_CLI.to_string(),
        display_name: "Claude CLI".to_string(),
        status: if claude_status == "ready" {
            "ready"
        } else if claude_status == "disabled" {
            "disabled"
        } else {
            "degraded"
        }
        .to_string(),
        version: claude_version,
        authority: "external_bridge".to_string(),
        reason_code: format!("claude_cli_{claude_status}"),
        reason: "配置与登录由 Claude CLI 自己管理；checkpoint/ledger 不属 CLI 能力".to_string(),
        capabilities: claude_caps,
        controllable: Vec::new(),
    });

    // Codex — fail-closed until the app-server is implemented.
    let mut codex_caps = HashMap::new();
    codex_caps.insert("streaming".to_string(), "unsupported".to_string());
    codex_caps.insert("tools".to_string(), "unsupported".to_string());
    runtimes.push(RuntimeDescriptor {
        id: RUNTIME_CODEX_CLI.to_string(),
        display_name: "Codex".to_string(),
        status: "blocked".to_string(),
        version: None,
        authority: "external_bridge".to_string(),
        reason_code: "codex_app_server_not_implemented".to_string(),
        reason: "Codex app-server 未实现前 fail-closed，即使检测到二进制也不开放".to_string(),
        capabilities: codex_caps,
        controllable: Vec::new(),
    });

    runtimes
}

/// Runtime resolution: explicit override > conversation override (caller
/// supplies) > application default > safe default (native). `fallback_used` is
/// only true when the external runtime was unavailable AND the policy allowed
/// `fallback_native`; a `fail` policy never silently switches.
pub fn resolve_default_runtime(
    settings: &ExecutionEngineSettingsV2,
    runtimes: &[RuntimeDescriptor],
) -> ResolvedDefaultRuntime {
    let preferred = settings.default_runtime.clone();
    if preferred == RUNTIME_NATIVE {
        return ResolvedDefaultRuntime {
            runtime_id: RUNTIME_NATIVE.to_string(),
            source: "application_default".to_string(),
            fallback_used: false,
            reason_code: "native_default".to_string(),
            reason: "Native 为应用默认 Runtime".to_string(),
        };
    }
    // External runtime requested. Read its real status.
    let status = runtimes
        .iter()
        .find(|r| r.id == preferred)
        .map(|r| r.status.as_str())
        .unwrap_or("degraded");
    let ready = status == "ready";
    if ready {
        return ResolvedDefaultRuntime {
            runtime_id: preferred.clone(),
            source: "application_default".to_string(),
            fallback_used: false,
            reason_code: format!("{preferred}_ready"),
            reason: "外部 Runtime 可用".to_string(),
        };
    }
    if settings.external_unavailable_policy == "fallback_native" {
        return ResolvedDefaultRuntime {
            runtime_id: RUNTIME_NATIVE.to_string(),
            source: "safe_default".to_string(),
            fallback_used: true,
            reason_code: format!("{preferred}_unavailable_fallback_native"),
            reason: "外部 Runtime 不可用，按用户设置回退 Native".to_string(),
        };
    }
    // fail policy — honest failure, no silent switch.
    ResolvedDefaultRuntime {
        runtime_id: preferred.clone(),
        source: "application_default".to_string(),
        fallback_used: false,
        reason_code: format!("{preferred}_unavailable_fail"),
        reason: "外部 Runtime 不可用且策略为 fail：不暗中切换 Native，需修复或改设置".to_string(),
    }
}

/// Detect which external CLIs exist right now (snapshot refresh action).
pub fn detect_runtimes() -> Vec<RuntimeDescriptor> {
    let settings = load_execution_engine_settings();
    build_runtime_descriptors(&settings)
}

/// Catalog aliases kept for source compatibility (A6 owns executor_catalog.rs).
pub fn default_enabled_tools_legacy() -> HashMap<String, bool> {
    executor_catalog::default_enabled_tools()
}

#[cfg(test)]
mod tests {
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
        assert_eq!(s.default_runtime, RUNTIME_NATIVE);
        assert_eq!(s.external_unavailable_policy, "fail");
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
        let v2 = migrate_legacy_executor_settings(&conn);
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
        let saved = prepare_for_save(s);
        assert!(!saved.codex_cli.enabled, "codex stays blocked");
        assert!(saved.revision >= 1, "revision bumped");
    }

    #[test]
    fn fail_policy_never_silent_fallback() {
        let mut s = ExecutionEngineSettingsV2::default().normalized();
        s.default_runtime = RUNTIME_CLAUDE_CLI.to_string();
        s.external_unavailable_policy = "fail".to_string();
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
        s.external_unavailable_policy = "fallback_native".to_string();
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
        let saved = prepare_for_save(s);
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
}
