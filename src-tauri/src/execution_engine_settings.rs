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

/// `externalUnavailablePolicy` wire values.
pub const POLICY_FAIL: &str = "fail";
pub const POLICY_FALLBACK_NATIVE: &str = "fallback_native";

/// Restricted runtime id (SETTINGS-001). Serializes to the wire string; an
/// unknown string from an external/legacy source deserializes into `Unknown`
/// which is **never silently persisted** — the save path rejects it
/// (`validate_enum_fields`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RuntimeId {
    Native,
    ClaudeCli,
    CodexCli,
    /// Unknown string seen on the wire (corrupt/legacy input). Kept so loading
    /// a corrupt value is observable instead of a silent guess; never durable.
    Unknown(String),
}

impl RuntimeId {
    pub fn as_str(&self) -> &str {
        match self {
            RuntimeId::Native => RUNTIME_NATIVE,
            RuntimeId::ClaudeCli => RUNTIME_CLAUDE_CLI,
            RuntimeId::CodexCli => RUNTIME_CODEX_CLI,
            RuntimeId::Unknown(raw) => raw.as_str(),
        }
    }

    /// Known ids only; `Unknown` is never a valid persisted value.
    pub fn is_known(&self) -> bool {
        !matches!(self, RuntimeId::Unknown(_))
    }

    pub fn from_known(value: &str) -> Option<Self> {
        match value {
            RUNTIME_NATIVE => Some(RuntimeId::Native),
            RUNTIME_CLAUDE_CLI => Some(RuntimeId::ClaudeCli),
            RUNTIME_CODEX_CLI => Some(RuntimeId::CodexCli),
            _ => None,
        }
    }
}

impl Serialize for RuntimeId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for RuntimeId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Ok(Self::from_known(&raw).unwrap_or(RuntimeId::Unknown(raw)))
    }
}

/// Restricted unavailable policy (SETTINGS-001). Same contract as `RuntimeId`:
/// unknown strings decode to `Unknown` and are rejected before persistence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExternalUnavailablePolicy {
    Fail,
    FallbackNative,
    Unknown(String),
}

impl ExternalUnavailablePolicy {
    pub fn as_str(&self) -> &str {
        match self {
            ExternalUnavailablePolicy::Fail => POLICY_FAIL,
            ExternalUnavailablePolicy::FallbackNative => POLICY_FALLBACK_NATIVE,
            ExternalUnavailablePolicy::Unknown(raw) => raw.as_str(),
        }
    }

    pub fn is_known(&self) -> bool {
        !matches!(self, ExternalUnavailablePolicy::Unknown(_))
    }

    pub fn from_known(value: &str) -> Option<Self> {
        match value {
            POLICY_FAIL => Some(ExternalUnavailablePolicy::Fail),
            POLICY_FALLBACK_NATIVE => Some(ExternalUnavailablePolicy::FallbackNative),
            _ => None,
        }
    }
}

impl Serialize for ExternalUnavailablePolicy {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ExternalUnavailablePolicy {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Ok(Self::from_known(&raw).unwrap_or(ExternalUnavailablePolicy::Unknown(raw)))
    }
}

// ── V2 model (mirrors contracts/execution-engine-settings-v2.schema.json) ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionEngineSettingsV2 {
    pub schema_version: u32,
    pub revision: u32,
    #[serde(default = "default_runtime")]
    pub default_runtime: RuntimeId,
    #[serde(default = "default_external_unavailable_policy")]
    pub external_unavailable_policy: ExternalUnavailablePolicy,
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

fn default_runtime() -> RuntimeId {
    RuntimeId::Native
}
fn default_external_unavailable_policy() -> ExternalUnavailablePolicy {
    ExternalUnavailablePolicy::Fail
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

    /// Schema-level validation (SETTINGS-001): restricted enum fields must hold
    /// a known value. An `Unknown` value decoded from a corrupt/legacy payload
    /// is rejected here — it is never silently persisted.
    pub fn validate_enum_fields(&self) -> Result<(), String> {
        if !self.default_runtime.is_known() {
            return Err(format!(
                "defaultRuntime '{}' is not a known runtime id; refusing to save",
                self.default_runtime.as_str()
            ));
        }
        if !self.external_unavailable_policy.is_known() {
            return Err(format!(
                "externalUnavailablePolicy '{}' is not a known policy; refusing to save",
                self.external_unavailable_policy.as_str()
            ));
        }
        Ok(())
    }
}

// ── Persistence + migration ───────────────────────────────────────────────

/// Load V2 settings; on first read, migrate the legacy `executor:settings`.
///
/// P0-13: a DB read failure or corrupt JSON is a hard error (never a silent
/// "default success"). The caller decides whether to surface it or degrade
/// explicitly; the Settings authority never pretends defaults were stored.
pub fn load_execution_engine_settings() -> Result<ExecutionEngineSettingsV2, String> {
    let pool_conn = db::get_main_conn().map_err(|e| format!("open main DB: {e}"))?;
    let conn: &rusqlite::Connection = &pool_conn;
    match db::get_setting(conn, EXECUTION_ENGINE_KEY) {
        Ok(Some(json)) => {
            let settings =
                serde_json::from_str::<ExecutionEngineSettingsV2>(&json).map_err(|e| {
                    format!("execution engine settings JSON corrupt (revision CAS unusable): {e}")
                })?;
            Ok(settings.normalized())
        }
        Ok(None) => migrate_legacy_executor_settings(conn),
        Err(e) => Err(format!("read execution engine settings: {e}")),
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
pub fn migrate_legacy_executor_settings(
    conn: &rusqlite::Connection,
) -> Result<ExecutionEngineSettingsV2, String> {
    let mut v2 = ExecutionEngineSettingsV2::default();
    let legacy = db::get_setting(conn, EXECUTOR_KEY)
        .map_err(|e| format!("read legacy executor settings: {e}"))?;
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
        // P0-13: corrupt legacy JSON is a hard error, not a silent default.
        let old = serde_json::from_str::<Legacy>(&json).map_err(|e| {
            format!("legacy executor settings JSON corrupt, refusing to default: {e}")
        })?;
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
    let v2 = v2.normalized();
    // P0-14: persist immediately so the migration is exactly-once; a failed
    // write is a visible error, never a silent "migration succeeded" lie.
    let json =
        serde_json::to_string(&v2).map_err(|e| format!("serialize migrated settings: {e}"))?;
    db::set_setting(conn, EXECUTION_ENGINE_KEY, &json)
        .map_err(|e| format!("persist migrated execution engine settings: {e}"))?;
    Ok(v2)
}

/// Pure policy application before persistence (testable without a DB pool):
/// schema pin, revision bump, codex fail-closed, bounds. Restricted enums are
/// validated here so an unknown wire value is rejected instead of persisted.
pub fn prepare_for_save(
    mut settings: ExecutionEngineSettingsV2,
) -> Result<ExecutionEngineSettingsV2, String> {
    settings.validate_enum_fields()?;
    settings.schema_version = SCHEMA_VERSION;
    settings.revision = settings.revision.wrapping_add(1);
    // Codex stays fail-closed until the app-server is implemented: force the
    // flag off and never advertise it as enabled regardless of stored value.
    settings.codex_cli.enabled = false;
    Ok(settings.normalized())
}

/// Persist V2 settings with **revision CAS** (conflict detection for
/// multi-window). `settings.revision` is the caller's expected revision; if
/// the durable current value's revision differs, the save is rejected so one
/// window can never silently overwrite another's edit.
pub fn save_execution_engine_settings(
    settings: ExecutionEngineSettingsV2,
) -> Result<ExecutionEngineSettingsV2, String> {
    let expected_revision = settings.revision;
    let pool_conn = db::get_main_conn().map_err(|e| e.to_string())?;
    let conn: &rusqlite::Connection = &pool_conn;
    // CAS read: corrupt existing value is a hard error, not a blind overwrite.
    let current = match db::get_setting(conn, EXECUTION_ENGINE_KEY) {
        Ok(Some(json)) => serde_json::from_str::<ExecutionEngineSettingsV2>(&json)
            .map_err(|e| format!("existing settings JSON corrupt (cannot CAS): {e}"))?,
        Ok(None) => ExecutionEngineSettingsV2::default(),
        Err(e) => return Err(format!("read execution engine settings for CAS: {e}")),
    };
    if current.revision != expected_revision {
        return Err(format!(
            "settings revision conflict: expected {expected_revision}, found {} (another window changed the settings)",
            current.revision
        ));
    }
    let settings = prepare_for_save(settings)?;
    let json = serde_json::to_string(&settings)
        .map_err(|e| format!("serialize execution engine settings: {e}"))?;
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
///
/// P0-13: settings read failure propagates (never a fake "default" snapshot).
/// SETTINGS-002: runtime capability/status comes from real discovery (local
/// binary probes + the daemon capability handshake); the Host only composes
/// what the daemon and the filesystem actually report — no static table.
pub async fn build_execution_engine_snapshot(
    authority_mode: &str,
    protocol_version: &str,
    stream_transport: &str,
    daemon_ready: bool,
) -> Result<ExecutionEngineSnapshot, String> {
    let settings = load_execution_engine_settings()?;
    let mut runtimes = build_runtime_descriptors(&settings);
    project_daemon_capabilities(&mut runtimes, daemon_ready).await;
    let resolved_default = resolve_default_runtime(&settings, &runtimes);
    Ok(ExecutionEngineSnapshot {
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
    })
}

/// Runtime descriptors from **real local discovery** (sync): binary probes,
/// settings flags and fail-closed facts. No capability table is invented
/// here — capabilities are projected separately from the daemon handshake
/// (`project_daemon_capabilities`), so a descriptor never self-announces a
/// static feature surface (SETTINGS-002).
///
/// Used where availability alone matters: policy resolution, the save gate,
/// and as the base for the UI snapshot.
pub fn build_runtime_descriptors(settings: &ExecutionEngineSettingsV2) -> Vec<RuntimeDescriptor> {
    let mut runtimes = Vec::new();

    // Native — local fact: the built-in engine. Its readiness is confirmed by
    // the daemon handshake in the snapshot path (degraded when unreachable);
    // locally it is always selectable so policy resolution never rejects it
    // merely because the sidecar is momentarily down.
    runtimes.push(RuntimeDescriptor {
        id: RUNTIME_NATIVE.to_string(),
        display_name: "Native".to_string(),
        status: "ready".to_string(),
        version: None,
        authority: "native".to_string(),
        reason_code: "native_ready".to_string(),
        reason: "完整 Natives authority（checkpoint / ledger / replay / resume）".to_string(),
        capabilities: HashMap::new(),
        controllable: vec!["maxSteps".to_string(), "disabledTools".to_string()],
    });

    // Claude CLI — real binary probe (existence + --version, no side effects).
    let (claude_version, claude_status) = if settings.claude_cli.enabled {
        detect_external_cli("claude")
    } else {
        (None, "disabled".to_string())
    };
    runtimes.push(RuntimeDescriptor {
        id: RUNTIME_CLAUDE_CLI.to_string(),
        display_name: "Claude CLI".to_string(),
        status: claude_status.clone(),
        version: claude_version,
        authority: "external_bridge".to_string(),
        reason_code: format!("claude_cli_{claude_status}"),
        reason: match claude_status.as_str() {
            "ready" => {
                "配置与登录由 Claude CLI 自己管理；checkpoint/ledger 不属 CLI 能力".to_string()
            }
            "disabled" => "Claude CLI 已在设置中停用".to_string(),
            "not_installed" => "未检测到 claude 二进制".to_string(),
            _ => "claude 二进制存在但 --version 探测失败".to_string(),
        },
        capabilities: HashMap::new(),
        controllable: Vec::new(),
    });

    // Codex — fail-closed until the app-server is implemented.
    runtimes.push(RuntimeDescriptor {
        id: RUNTIME_CODEX_CLI.to_string(),
        display_name: "Codex".to_string(),
        status: "blocked".to_string(),
        version: None,
        authority: "external_bridge".to_string(),
        reason_code: "codex_app_server_not_implemented".to_string(),
        reason: "Codex app-server 未实现前 fail-closed，即使检测到二进制也不开放".to_string(),
        capabilities: HashMap::new(),
        controllable: Vec::new(),
    });

    runtimes
}

/// Pure projection of the daemon's real capability handshake onto descriptors
/// (testable without a live daemon). `daemon_flags` are the daemon-level
/// capability flags and `matrix` the per-runtime feature matrix, both from
/// `daemon.getCapabilities`. Native readiness is the daemon handshake truth;
/// external runtimes keep their locally-detected status and gain only the
/// matrix the daemon actually reports.
fn apply_daemon_projection(
    runtimes: &mut [RuntimeDescriptor],
    daemon_ready: bool,
    daemon_flags: &HashMap<String, String>,
    matrix: &HashMap<String, assistant_protocol::v2::RuntimeFeatureMatrix>,
) {
    for rt in runtimes.iter_mut() {
        if rt.id == RUNTIME_NATIVE {
            rt.capabilities.clear();
            if daemon_ready {
                rt.status = "ready".to_string();
                rt.reason_code = "native_ready".to_string();
                rt.reason =
                    "完整 Natives authority（checkpoint / ledger / replay / resume）".to_string();
                for (flag, value) in daemon_flags {
                    rt.capabilities.insert(flag.clone(), value.clone());
                }
            } else {
                rt.status = "degraded".to_string();
                rt.reason_code = "daemon_unreachable".to_string();
                rt.reason = "Daemon 未就绪，Native 引擎当前不可用".to_string();
            }
            if let Some(row) = matrix.get(RUNTIME_NATIVE) {
                for (cap, value) in matrix_entries(row) {
                    rt.capabilities.insert(cap, value);
                }
            }
        } else if let Some(row) = matrix.get(rt.id.as_str()) {
            rt.capabilities.clear();
            for (cap, value) in matrix_entries(row) {
                rt.capabilities.insert(cap, value);
            }
        }
    }
}

/// Fetch the daemon capability handshake and project it (SETTINGS-002). The
/// Host never self-announces a static feature table; it composes whatever the
/// daemon advertises. Best-effort: a failed handshake leaves external
/// descriptors on their local status and marks native degraded — the honest
/// state, not a fabricated one.
async fn project_daemon_capabilities(runtimes: &mut [RuntimeDescriptor], daemon_ready: bool) {
    use assistant_protocol::v2::RuntimeFeatureMatrix;
    use std::collections::HashMap;

    let handshake = crate::daemon_authority::request(
        assistant_protocol::v2::methods::names::DAEMON_GET_CAPABILITIES,
        serde_json::json!({}),
    )
    .await
    .ok();

    let (daemon_flags, matrix) = match handshake {
        Some(value) => {
            let flags = [
                "tools",
                "hooks",
                "subagents",
                "mcp",
                "extensions",
                "scheduler",
                "event_replay",
                "credential_broker",
            ]
            .iter()
            .map(|flag| {
                let supported = value
                    .get(*flag)
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
                (
                    flag.to_string(),
                    if supported {
                        "supported"
                    } else {
                        "unsupported"
                    }
                    .to_string(),
                )
            })
            .collect::<HashMap<String, String>>();
            let matrix = value
                .get("runtime_capabilities")
                .and_then(|v| {
                    serde_json::from_value::<HashMap<String, RuntimeFeatureMatrix>>(v.clone()).ok()
                })
                .unwrap_or_default();
            (flags, matrix)
        }
        None => (HashMap::new(), HashMap::new()),
    };

    apply_daemon_projection(runtimes, daemon_ready, &daemon_flags, &matrix);
}

/// Project one daemon feature-matrix row into the descriptor's string
/// capability map (`supported` / `unsupported`).
fn matrix_entries(row: &assistant_protocol::v2::RuntimeFeatureMatrix) -> HashMap<String, String> {
    fn bool_str(v: bool) -> String {
        if v {
            "supported".to_string()
        } else {
            "unsupported".to_string()
        }
    }
    let mut entries = HashMap::new();
    entries.insert("expert".to_string(), bool_str(row.expert));
    entries.insert("team".to_string(), bool_str(row.team));
    entries.insert("skills".to_string(), bool_str(row.skills));
    entries.insert("mcp".to_string(), bool_str(row.mcp));
    if let Some(mechanism) = &row.mechanism {
        entries.insert("mechanism".to_string(), mechanism.clone());
    }
    entries
}

/// Runtime resolution: explicit override > conversation override (caller
/// supplies) > application default > safe default (native). `fallback_used` is
/// only true when the external runtime was unavailable AND the policy allowed
/// `fallback_native`; a `fail` policy never silently switches.
pub fn resolve_default_runtime(
    settings: &ExecutionEngineSettingsV2,
    runtimes: &[RuntimeDescriptor],
) -> ResolvedDefaultRuntime {
    if settings.default_runtime == RuntimeId::Native {
        return ResolvedDefaultRuntime {
            runtime_id: RUNTIME_NATIVE.to_string(),
            source: "application_default".to_string(),
            fallback_used: false,
            reason_code: "native_default".to_string(),
            reason: "Native 为应用默认 Runtime".to_string(),
        };
    }
    // External runtime requested. Read its real status.
    let preferred = settings.default_runtime.as_str();
    let status = runtimes
        .iter()
        .find(|r| r.id == preferred)
        .map(|r| r.status.as_str())
        .unwrap_or("degraded");
    let ready = status == "ready";
    if ready {
        return ResolvedDefaultRuntime {
            runtime_id: preferred.to_string(),
            source: "application_default".to_string(),
            fallback_used: false,
            reason_code: format!("{preferred}_ready"),
            reason: "外部 Runtime 可用".to_string(),
        };
    }
    if settings.external_unavailable_policy == ExternalUnavailablePolicy::FallbackNative {
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
        runtime_id: preferred.to_string(),
        source: "application_default".to_string(),
        fallback_used: false,
        reason_code: format!("{preferred}_unavailable_fail"),
        reason: "外部 Runtime 不可用且策略为 fail：不暗中切换 Native，需修复或改设置".to_string(),
    }
}

// ── Execution Policy V1 (S3 Settings Authority → new top-level Run) ────────
//
// Contract: docs/contracts/EXECUTION-POLICY-V1.md. The policy is resolved ONCE
// at Run creation, baked into the create/start request, and persisted as an
// immutable snapshot so later Settings edits never touch an existing Run.

/// Resolved execution policy for a single Run (EXECUTION-POLICY-V1 shape).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedExecutionPolicyV1 {
    pub version: u32,
    pub runtime_id: String,
    /// explicit_run | conversation_override | application_default | safe_default
    pub runtime_source: String,
    /// Settings revision the policy was resolved from (0 = pristine default).
    pub settings_revision: u32,
    pub max_steps: u32,
    /// Subtract-only deny list; never expands the capability surface.
    pub disabled_tools: Vec<String>,
    pub fallback_used: bool,
    pub unavailable_policy: String,
}

impl ResolvedExecutionPolicyV1 {
    pub fn snapshot_key(run_id: &str) -> String {
        format!("execution:policy:snapshot:{run_id}")
    }
}

/// Runtime availability used by policy resolution. Native is always ready;
/// every external runtime is availability-checked from its descriptor
/// (`ready` only). Unknown ids are unavailable.
pub fn runtime_available(runtime_id: &str, runtimes: &[RuntimeDescriptor]) -> bool {
    if runtime_id == RUNTIME_NATIVE {
        return true;
    }
    runtimes
        .iter()
        .find(|r| r.id == runtime_id)
        .map(|r| r.status == "ready")
        .unwrap_or(false)
}

/// SETTINGS-001 Host gate: the application default must point at a runtime
/// whose current status is `ready`. A blocked / degraded / disabled /
/// not_installed runtime is not a valid savable default.
///
/// The gate only fires when the default **actually changes**, so an unavailable
/// default inherited from a legacy migration does not lock out unrelated edits
/// (e.g. adjusting maxSteps). The UI layer disables the matching radios; this
/// is the backend gate that rejects a crafted save trying to persist an
/// invalid default.
pub fn validate_default_runtime_selectable(
    incoming: &ExecutionEngineSettingsV2,
    current: &ExecutionEngineSettingsV2,
    runtimes: &[RuntimeDescriptor],
) -> Result<(), String> {
    if incoming.default_runtime == current.default_runtime {
        return Ok(());
    }
    let id = incoming.default_runtime.as_str();
    match runtimes.iter().find(|r| r.id == id).map(|r| r.status.as_str()) {
        Some("ready") => Ok(()),
        Some(other) => Err(format!(
            "runtime '{id}' is currently {other}; an unavailable runtime cannot be saved as the default (pick a ready runtime)"
        )),
        None => Err(format!(
            "runtime '{id}' is not a recognized runtime; refusing to save it as the default"
        )),
    }
}

/// Resolve the execution policy for a new top-level Run.
///
/// Priority: **explicit run override → conversation override (if any) →
/// application Settings V2 → safe default native**.
///
/// Unavailable semantics (EXECUTION-POLICY-V1 §Invariants):
/// - explicit / conversation external runtime unavailable → hard error, never
///   a silent fallback;
/// - application default external runtime unavailable → falls back to native
///   only when `externalUnavailablePolicy == "fallback_native"` (explicitly
///   recorded as `fallbackUsed: true`, source `safe_default`); a `fail`
///   policy errors instead.
pub fn resolve_execution_policy(
    settings: &ExecutionEngineSettingsV2,
    runtimes: &[RuntimeDescriptor],
    explicit_runtime_id: Option<&str>,
    conversation_runtime_id: Option<&str>,
    explicit_max_steps: Option<u32>,
) -> Result<ResolvedExecutionPolicyV1, String> {
    let (runtime_id, runtime_source) = match explicit_runtime_id {
        Some(rt) if !rt.trim().is_empty() => (rt.trim().to_string(), "explicit_run"),
        _ => match conversation_runtime_id {
            Some(rt) if !rt.trim().is_empty() => (rt.trim().to_string(), "conversation_override"),
            _ => (
                settings.default_runtime.as_str().to_string(),
                "application_default",
            ),
        },
    };

    let available = runtime_available(&runtime_id, runtimes);
    let unavailable_policy = settings.external_unavailable_policy.as_str().to_string();
    let (final_runtime, final_source, fallback_used) = if available {
        (runtime_id, runtime_source, false)
    } else if runtime_source == "application_default"
        && settings.external_unavailable_policy == ExternalUnavailablePolicy::FallbackNative
    {
        // Only the application default may fall back — explicit/conversation
        // selections never silently switch runtime.
        (RUNTIME_NATIVE.to_string(), "safe_default", true)
    } else {
        return Err(format!(
            "runtime '{runtime_id}' is unavailable (source {runtime_source}); unavailable policy '{unavailable_policy}' does not allow fallback — no silent switch to Native",
        ));
    };

    // maxSteps: explicit run override > Settings native.maxSteps, always
    // clamped into the daemon hard bounds (10..=200) per contract.
    let max_steps = explicit_max_steps
        .unwrap_or(settings.native.max_steps)
        .clamp(MAX_STEPS_MIN, MAX_STEPS_MAX);

    Ok(ResolvedExecutionPolicyV1 {
        version: 1,
        runtime_id: final_runtime,
        runtime_source: final_source.to_string(),
        settings_revision: settings.revision,
        max_steps,
        disabled_tools: settings.native.disabled_tools.clone(),
        fallback_used,
        unavailable_policy,
    })
}

/// Persist the immutable policy snapshot for a created Run (固化). Later
/// Settings edits never change an existing Run because the snapshot is read
/// from here, not re-resolved.
pub fn store_policy_snapshot(
    run_id: &str,
    policy: &ResolvedExecutionPolicyV1,
) -> Result<(), String> {
    let pool_conn = db::get_main_conn().map_err(|e| format!("open main DB: {e}"))?;
    let conn: &rusqlite::Connection = &pool_conn;
    let json =
        serde_json::to_string(policy).map_err(|e| format!("serialize policy snapshot: {e}"))?;
    db::set_setting(
        conn,
        &ResolvedExecutionPolicyV1::snapshot_key(run_id),
        &json,
    )
    .map_err(|e| format!("persist policy snapshot for {run_id}: {e}"))
}

/// Read back a persisted policy snapshot (None when absent/corrupt is surfaced
/// as an explicit degraded error, never silent defaults).
pub fn load_policy_snapshot(run_id: &str) -> Result<Option<ResolvedExecutionPolicyV1>, String> {
    let pool_conn = db::get_main_conn().map_err(|e| format!("open main DB: {e}"))?;
    let conn: &rusqlite::Connection = &pool_conn;
    match db::get_setting(conn, &ResolvedExecutionPolicyV1::snapshot_key(run_id)) {
        Ok(Some(json)) => serde_json::from_str::<ResolvedExecutionPolicyV1>(&json)
            .map(Some)
            .map_err(|e| format!("policy snapshot for {run_id} corrupt: {e}")),
        Ok(None) => Ok(None),
        Err(e) => Err(format!("read policy snapshot for {run_id}: {e}")),
    }
}

/// One-shot durable migration of the legacy localStorage runtime pref
/// (`natives.assistant.runtimePref.v1`) into Settings V2 `defaultRuntime`.
///
/// Exactly-once & durable: only applies while the backend still holds the
/// pristine default (revision 0, `defaultRuntime == "native"`). After the
/// first successful CAS save the backend is authoritative, the localStorage
/// key is removed by the caller, and any later invocation is a no-op — the
/// migration can never run twice or clobber an explicit Settings V2 choice.
///
/// Fail-closed (MIG-001): an unknown / un-adoptable runtime id is an explicit
/// error, never a silent fallback to the old value. `codex_cli` is a
/// deliberate no-op (fail-closed until the app-server is real); it is refused
/// as `defaultRuntime` because adopting it would only produce guaranteed run
/// failures — the caller still deletes the obsolete localStorage key.
pub fn migrate_legacy_runtime_pref(
    legacy_runtime_id: Option<String>,
) -> Result<ExecutionEngineSettingsV2, String> {
    let mut settings = load_execution_engine_settings()?;
    if settings.revision > 0 || settings.default_runtime != RuntimeId::Native {
        return Ok(settings); // backend already authoritative → nothing to migrate
    }
    let pref = legacy_runtime_id
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_default();
    if pref.is_empty() || pref == RUNTIME_NATIVE {
        return Ok(settings); // no legacy pref to adopt
    }
    match RuntimeId::from_known(&pref) {
        Some(RuntimeId::ClaudeCli) => {
            settings.default_runtime = RuntimeId::ClaudeCli;
            save_execution_engine_settings(settings)
        }
        // codex_cli is fail-closed until the app-server is real; refusing to
        // adopt it as defaultRuntime is the honest result (no fallback to the
        // old value, no guarantee of run failures).
        Some(RuntimeId::CodexCli) | Some(RuntimeId::Native) | Some(RuntimeId::Unknown(_)) => {
            Ok(settings)
        }
        None => Err(format!(
            "legacy runtime pref '{pref}' is not a known runtime id; refusing to adopt it (no fallback to the old value)"
        )),
    }
}

/// Detect which external CLIs exist right now (snapshot refresh action).
pub fn detect_runtimes() -> Result<Vec<RuntimeDescriptor>, String> {
    let settings = load_execution_engine_settings()?;
    Ok(build_runtime_descriptors(&settings))
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
        let err2 =
            resolve_execution_policy(&settings, &runtimes, None, Some(RUNTIME_CLAUDE_CLI), None)
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
        let migrated = migrate_legacy_runtime_pref(Some("claude_cli".into()))
            .expect("first migration succeeds");
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
        let err =
            save_execution_engine_settings(bad).expect_err("unknown runtime must be rejected");
        assert!(err.contains("not a known runtime id"), "{err}");
        // Incoming carries an unknown unavailable policy → rejected.
        let mut bad2 = seeded.clone();
        bad2.external_unavailable_policy =
            ExternalUnavailablePolicy::Unknown("garbage_policy".into());
        let err2 =
            save_execution_engine_settings(bad2).expect_err("unknown policy must be rejected");
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
}
