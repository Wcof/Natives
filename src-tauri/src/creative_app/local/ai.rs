//! AI launch suggestion + fault diagnosis for local creative apps.
//!
//! Reuses existing Provider credentials via provider-adapters.
//! Never executes AI output directly — all suggestions pass LaunchPlan validation.

use super::plan::validate_launch_plan;
use super::scan::inspect_local_project;
use super::store;
use crate::creative_app::model::*;
use crate::env_manager;
use crate::{Error, Result};
use provider_adapters::capabilities::{
    ProviderAdapter, ProviderContentBlock, ProviderMessage, ProviderRequest, ProviderResponseBlock,
};
use provider_adapters::providers::{
    anthropic::AnthropicAdapter, deepseek::DeepSeekAdapter, gemini::GeminiAdapter,
    ollama::OllamaAdapter, openai::OpenAiAdapter, openai_compatible::OpenAiCompatibleAdapter,
};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;

const AI_SETTINGS_KEY: &str = "local_creative_ai_settings_json";
const DEFAULT_TIMEOUT_MS: u64 = 45_000;
const MAX_RETRIES: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalAiSettings {
    pub enabled: bool,
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    /// only_when_uncertain | always
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default)]
    pub user_consented: bool,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
}

fn default_mode() -> String {
    "only_when_uncertain".into()
}
fn default_timeout() -> u64 {
    DEFAULT_TIMEOUT_MS
}

impl Default for LocalAiSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            provider_id: None,
            model: None,
            mode: default_mode(),
            user_consented: false,
            timeout_ms: DEFAULT_TIMEOUT_MS,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiLaunchSuggestion {
    pub schema_version: u32,
    pub project_kind: LocalProjectKind,
    pub program: LaunchProgram,
    #[serde(default)]
    pub script: Option<String>,
    #[serde(default)]
    pub entry_file: Option<String>,
    pub cwd_relative: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub port_mode: LaunchPortMode,
    #[serde(default)]
    pub port: Option<u16>,
    pub open_path: String,
    pub confidence: f32,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiDiagnosisResult {
    pub schema_version: u32,
    pub issue_code: LocalCreativeIssueCode,
    pub summary: String,
    #[serde(default)]
    pub recovery_actions: Vec<String>,
}

pub fn get_ai_settings(conn: &Connection) -> Result<LocalAiSettings> {
    match crate::db::get_setting(conn, AI_SETTINGS_KEY)? {
        Some(s) if !s.trim().is_empty() => {
            let mut st: LocalAiSettings = serde_json::from_str(&s)
                .map_err(|e| Error::InvalidInput(format!("ai settings: {e}")))?;
            normalize_settings(&mut st)?;
            Ok(st)
        }
        _ => Ok(LocalAiSettings::default()),
    }
}

pub fn save_ai_settings(conn: &Connection, settings: &LocalAiSettings) -> Result<LocalAiSettings> {
    let mut st = settings.clone();
    normalize_settings(&mut st)?;
    if st.enabled {
        validate_settings_against_db(conn, &st)?;
    }
    let json = serde_json::to_string(&st).map_err(|e| Error::Internal(e.to_string()))?;
    crate::db::set_setting(conn, AI_SETTINGS_KEY, &json)?;
    Ok(st)
}

fn normalize_settings(st: &mut LocalAiSettings) -> Result<()> {
    if st.mode != "only_when_uncertain" && st.mode != "always" {
        return Err(Error::InvalidInput(
            "mode must be only_when_uncertain or always".into(),
        ));
    }
    st.timeout_ms = st.timeout_ms.clamp(5_000, 120_000);
    Ok(())
}

fn validate_settings_against_db(conn: &Connection, st: &LocalAiSettings) -> Result<()> {
    let provider_id = st
        .provider_id
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| Error::InvalidInput("AI provider not configured".into()))?;
    let exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM user_providers WHERE id = ?1",
            [provider_id],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if exists == 0 {
        return Err(Error::InvalidInput(format!(
            "provider not found: {provider_id}"
        )));
    }
    let keys: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM provider_api_keys WHERE provider_id = ?1 AND is_active = 1",
            [provider_id],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if keys == 0 {
        return Err(Error::InvalidInput(
            "provider has no active API key".into(),
        ));
    }
    if st.model.as_deref().unwrap_or("").trim().is_empty() {
        return Err(Error::InvalidInput("AI model not configured".into()));
    }
    Ok(())
}

/// Build redacted payload summary for UI consent preview (never includes secrets/absolute paths).
pub fn build_ai_payload_preview(scan: &LocalProjectScanResult) -> serde_json::Value {
    serde_json::json!({
        "virtualRoot": "/project",
        "projectKind": scan.project_kind,
        "packageManager": scan.package_manager,
        "scripts": scan.scripts,
        "preferredScript": scan.preferred_script,
        "hasNodeModules": scan.has_node_modules,
        "toolVersions": scan.tool_versions,
        "treeSample": scan.tree_sample.iter().take(80).cloned().collect::<Vec<_>>(),
        "risks": scan.risks,
        "blockers": scan.blockers,
        "rulePlan": scan.rule_plan,
    })
}

/// Step 1: scan + payload preview only (no network).
pub fn preview_local_ai(
    conn: &Connection,
    project_root: &str,
) -> Result<(LocalProjectScanResult, serde_json::Value, LocalAiSettings)> {
    let settings = get_ai_settings(conn)?;
    let scan = inspect_local_project(
        conn,
        &InspectLocalRequest {
            project_root: project_root.to_string(),
        },
    )?;
    let preview = build_ai_payload_preview(&scan);
    Ok((scan, preview, settings))
}

/// Step 2: requires confirmed=true after user reviewed preview.
pub async fn analyze_with_ai(
    conn: &Connection,
    project_root: &str,
    confirmed: bool,
) -> Result<(LocalProjectScanResult, Option<LaunchPlan>, serde_json::Value)> {
    if !confirmed {
        return Err(Error::InvalidInput(
            "AI analysis requires explicit confirmation after reviewing the payload preview".into(),
        ));
    }
    let settings = get_ai_settings(conn)?;
    if !settings.enabled || !settings.user_consented {
        return Err(Error::InvalidInput(
            "local creative AI is disabled or not consented".into(),
        ));
    }
    validate_settings_against_db(conn, &settings)?;

    let scan = inspect_local_project(
        conn,
        &InspectLocalRequest {
            project_root: project_root.to_string(),
        },
    )?;
    let preview = build_ai_payload_preview(&scan);

    let uncertain = scan.rule_plan.is_none()
        || scan.project_kind == LocalProjectKind::Unknown
        || !scan.package_manager_choices.is_empty();
    if settings.mode == "only_when_uncertain" && !uncertain {
        let plan = scan.rule_plan.clone();
        return Ok((scan, plan, preview));
    }

    let suggestion = match call_provider_for_launch(conn, &settings, &preview).await {
        Ok(s) => s,
        Err(e) => {
            let mut scan = scan;
            let plan = scan.rule_plan.clone();
            scan.risks.push(format!("AI analysis failed: {e}"));
            return Ok((scan, plan, preview));
        }
    };

    let root = Path::new(&scan.project_root);
    let plan = suggestion_to_plan(&suggestion);
    match validate_launch_plan(root, plan) {
        Ok(valid) => Ok((scan, Some(valid), preview)),
        Err(e) => {
            let mut scan = scan;
            let plan = scan.rule_plan.clone();
            scan.risks
                .push(format!("AI suggestion failed validation: {e}"));
            Ok((scan, plan, preview))
        }
    }
}

pub async fn diagnose_with_ai(
    conn: &Connection,
    id: &str,
    log_tail: &str,
) -> Result<AiDiagnosisResult> {
    let settings = get_ai_settings(conn)?;
    if !settings.enabled || !settings.user_consented {
        return Err(Error::InvalidInput(
            "local creative AI is disabled or not consented".into(),
        ));
    }
    validate_settings_against_db(conn, &settings)?;
    let rec = store::get_app(conn, id)?.ok_or_else(|| Error::NotFound(id.into()))?;
    let secrets = store::secret_values(conn, id);
    let redacted_logs =
        crate::creative_app::local::logs::sanitize_log_text_with_secrets(log_tail, &secrets);
    let logs = if redacted_logs.chars().count() > 8_000 {
        redacted_logs
            .chars()
            .rev()
            .take(8_000)
            .collect::<String>()
            .chars()
            .rev()
            .collect()
    } else {
        redacted_logs
    };
    let payload = serde_json::json!({
        "virtualRoot": "/project",
        "projectKind": rec.project_kind,
        "state": rec.state.as_str(),
        "lastError": rec.last_error,
        "logTail": logs,
    });

    match call_provider_for_diagnosis(conn, &settings, &payload).await {
        Ok(mut d) => {
            d.recovery_actions.retain(|a| {
                matches!(
                    a.as_str(),
                    "view_logs"
                        | "stop"
                        | "restart"
                        | "install_dependencies"
                        | "edit_plan"
                        | "rescan"
                        | "resolve_orphan_stop"
                        | "resolve_orphan_restart"
                        | "open_terminal"
                        | "open_folder"
                )
            });
            if d.schema_version != 1 {
                d.schema_version = 1;
            }
            Ok(d)
        }
        Err(e) => Ok(AiDiagnosisResult {
            schema_version: 1,
            issue_code: LocalCreativeIssueCode::AiError,
            summary: format!("AI diagnosis unavailable: {e}"),
            recovery_actions: vec!["view_logs".into(), "edit_plan".into()],
        }),
    }
}

fn suggestion_to_plan(s: &AiLaunchSuggestion) -> LaunchPlan {
    let runtime = match s.program {
        LaunchProgram::Internal => LocalLaunchRuntime::StaticHttp,
        _ => LocalLaunchRuntime::NodeDevServer,
    };
    LaunchPlan {
        schema_version: 1,
        source: LaunchPlanSource::Ai,
        project_kind: s.project_kind,
        runtime,
        program: s.program,
        cwd_relative: s.cwd_relative.clone(),
        script: s.script.clone(),
        entry_file: s.entry_file.clone(),
        script_runner: None,
        args: s.args.clone(),
        environment_keys: vec![],
        port: LaunchPort {
            mode: s.port_mode,
            value: s.port,
        },
        open_path: s.open_path.clone(),
        health_path: s.open_path.clone(),
        startup_timeout_ms: 60_000,
        auto_open: true,
        confidence: Some(s.confidence),
        reason: s.reason.clone(),
    }
}

async fn call_provider_for_launch(
    conn: &Connection,
    settings: &LocalAiSettings,
    preview: &serde_json::Value,
) -> Result<AiLaunchSuggestion> {
    let system = r#"You are a local project launch planner for Natives.
Return ONLY a JSON object matching:
{
  "schemaVersion": 1,
  "projectKind": "html|vite|vue|vue_vite|vite_other|unknown",
  "program": "internal|npm|pnpm|yarn|node",
  "script": "optional string",
  "entryFile": "optional relative path",
  "cwdRelative": ".",
  "args": [],
  "portMode": "auto|fixed",
  "port": null,
  "openPath": "/",
  "confidence": 0.0-1.0,
  "reason": "short"
}
Rules: no shell metacharacters, no absolute paths, no secrets, no commands to execute."#;
    let user = format!(
        "Project scan (virtual root /project, no absolute paths):\n{}",
        serde_json::to_string_pretty(preview).unwrap_or_default()
    );
    let text = provider_chat(conn, settings, system, &user).await?;
    parse_json_object::<AiLaunchSuggestion>(&text)
}

async fn call_provider_for_diagnosis(
    conn: &Connection,
    settings: &LocalAiSettings,
    payload: &serde_json::Value,
) -> Result<AiDiagnosisResult> {
    let system = r#"You diagnose local creative app start failures.
Return ONLY JSON:
{
  "schemaVersion": 1,
  "issueCode": "path_missing|environment_missing|dependencies_missing|port_conflict|config_invalid|ai_error|start_unhealthy|orphaned_process",
  "summary": "short",
  "recoveryActions": ["view_logs","stop","restart","install_dependencies","edit_plan","rescan"]
}
Do not return shell commands."#;
    let user = serde_json::to_string_pretty(payload).unwrap_or_default();
    let text = provider_chat(conn, settings, system, &user).await?;
    parse_json_object::<AiDiagnosisResult>(&text)
}

async fn provider_chat(
    conn: &Connection,
    settings: &LocalAiSettings,
    system: &str,
    user: &str,
) -> Result<String> {
    let provider_id = settings
        .provider_id
        .as_deref()
        .ok_or_else(|| Error::InvalidInput("AI provider not configured".into()))?;
    let model = settings
        .model
        .clone()
        .ok_or_else(|| Error::InvalidInput("AI model not configured".into()))?;

    let (base_url, protocol): (String, String) = conn
        .query_row(
            "SELECT base_url, api_protocol FROM user_providers WHERE id = ?1",
            [provider_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| Error::NotFound(format!("provider {provider_id}")))?;

    let key_id: String = conn
        .query_row(
            "SELECT id FROM provider_api_keys WHERE provider_id = ?1 AND is_primary = 1 AND is_active = 1 LIMIT 1",
            [provider_id],
            |row| row.get(0),
        )
        .or_else(|_| {
            conn.query_row(
                "SELECT id FROM provider_api_keys WHERE provider_id = ?1 AND is_active = 1 LIMIT 1",
                [provider_id],
                |row| row.get(0),
            )
        })
        .map_err(|_| Error::InvalidInput("no active API key for provider".into()))?;

    let api_key = provider_key_plaintext(conn, &key_id)?;
    let adapter = build_adapter(&protocol, &base_url, &api_key)?;
    let request = ProviderRequest {
        model,
        messages: vec![ProviderMessage {
            role: "user".into(),
            content: vec![ProviderContentBlock::Text {
                text: user.to_string(),
            }],
        }],
        system_prompt: Some(system.to_string()),
        tools: None,
        max_tokens: Some(1024),
        temperature: Some(0.1),
        stream: false,
        structured_output: None,
    };

    let mut last_err = String::new();
    for attempt in 0..=MAX_RETRIES {
        match adapter.chat(request.clone()).await {
            Ok(resp) => return Ok(response_text(&resp)),
            Err(e) => {
                last_err = e.message.clone();
                if !e.retryable || attempt == MAX_RETRIES {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(400 * (attempt as u64 + 1))).await;
            }
        }
    }
    Err(Error::Internal(last_err))
}

fn build_adapter(
    protocol: &str,
    base_url: &str,
    api_key: &str,
) -> std::result::Result<Box<dyn ProviderAdapter>, Error> {
    let p = protocol.to_ascii_lowercase();
    let base = base_url.trim().to_string();
    let key = api_key.to_string();
    if p.contains("anthropic") || p.contains("claude") {
        // Anthropic adapter currently has fixed base; key only.
        return Ok(Box::new(AnthropicAdapter::new().with_api_key(key)));
    }
    if p.contains("gemini") || p.contains("google") {
        return Ok(Box::new(
            GeminiAdapter::new().with_api_key(key).with_base_url(base),
        ));
    }
    if p.contains("ollama") {
        return Ok(Box::new(OllamaAdapter::new().with_base_url(base)));
    }
    if p.contains("deepseek") {
        return Ok(Box::new(DeepSeekAdapter::new().with_api_key(key)));
    }
    if p.contains("openai") && !p.contains("compatible") {
        return Ok(Box::new(
            OpenAiAdapter::new().with_api_key(key).with_base_url(base),
        ));
    }
    Ok(Box::new(
        OpenAiCompatibleAdapter::new()
            .with_api_key(key)
            .with_base_url(base),
    ))
}

fn response_text(resp: &provider_adapters::capabilities::ProviderResponse) -> String {
    let mut out = String::new();
    for b in &resp.content {
        if let ProviderResponseBlock::Text(t) = b {
            out.push_str(t);
        }
    }
    out
}

fn provider_key_plaintext(conn: &Connection, key_id: &str) -> Result<String> {
    let (encrypted, dek): (String, Option<String>) = conn
        .query_row(
            "SELECT api_key_encrypted, dek_encrypted FROM provider_api_keys WHERE id = ?1",
            [key_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| Error::Internal(format!("key load: {e}")))?;
    if let Some(dek) = dek {
        if !dek.is_empty() {
            return crate::provider_key_manager::envelope_decrypt(&encrypted, &dek, conn);
        }
    }
    let enc_key = env_manager::get_encryption_key(conn)?;
    env_manager::decrypt(&encrypted, &enc_key)
}

fn parse_json_object<T: for<'de> Deserialize<'de>>(text: &str) -> Result<T> {
    let trimmed = text.trim();
    let json = if let (Some(s), Some(e)) = (trimmed.find('{'), trimmed.rfind('}')) {
        &trimmed[s..=e]
    } else {
        trimmed
    };
    serde_json::from_str(json).map_err(|e| Error::InvalidInput(format!("AI JSON parse: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_suggestion_from_fenced() {
        let raw = r#"```json
{"schemaVersion":1,"projectKind":"html","program":"internal","cwdRelative":".","args":[],"portMode":"auto","openPath":"/","confidence":0.9,"reason":"ok"}
```"#;
        let s: AiLaunchSuggestion = parse_json_object(raw).unwrap();
        assert_eq!(s.program, LaunchProgram::Internal);
    }

    #[test]
    fn mode_normalization() {
        let mut st = LocalAiSettings::default();
        st.mode = "always".into();
        st.timeout_ms = 1;
        normalize_settings(&mut st).unwrap();
        assert_eq!(st.timeout_ms, 5_000);
        st.mode = "nope".into();
        assert!(normalize_settings(&mut st).is_err());
    }
}
