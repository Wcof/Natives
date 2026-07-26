//! Provider routing for one engine stream.
//!
//! The module owns target ordering, pre-delta retry and circuit state. The engine
//! still sees one `EngineProvider`, so routing policy never leaks into agent-core.

use agent_core::{
    EngineError, EngineMessage, EngineProvider, EngineProviderEvent, EngineProviderEventStream,
    ToolSchema,
};
use futures_util::StreamExt;
use provider_adapters::capabilities::{
    history_message_to_provider, Credential, ProviderAdapter, ProviderRequest, ProviderTool,
};
use provider_adapters::stream::ProviderEvent;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio_util::sync::CancellationToken;

use crate::natives_db_broker::{NativesDbBroker, Sub2ApiAccountCredential};
use crate::production::RealProvider;

const FAILURE_THRESHOLD: u32 = 3;
const COOLDOWN: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Deserialize)]
pub struct RouteTarget {
    pub provider_id: String,
    pub credential_kind: String,
    pub credential_id: Option<String>,
    pub model_id: String,
}

#[derive(Debug, Clone)]
pub struct RoutingPlan {
    pub enabled: bool,
    pub targets: Vec<RouteTarget>,
}

static CIRCUIT_WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static ACCOUNT_INFLIGHT: OnceLock<Mutex<HashMap<String, u32>>> = OnceLock::new();
static HEALTH_STATE_INITIALIZED: OnceLock<()> = OnceLock::new();
fn inflight() -> &'static Mutex<HashMap<String, u32>> {
    ACCOUNT_INFLIGHT.get_or_init(|| Mutex::new(HashMap::new()))
}

struct AccountLease {
    account_id: String,
    route_key: String,
}

impl Drop for AccountLease {
    fn drop(&mut self) {
        if let Ok(mut map) = inflight().lock() {
            if let Some(value) = map.get_mut(&self.account_id) {
                *value = value.saturating_sub(1);
                record_inflight(&self.route_key, *value);
            }
        }
    }
}

fn acquire_account(
    account: &Sub2ApiAccountCredential,
    target: &RouteTarget,
) -> Option<AccountLease> {
    let mut map = inflight().lock().ok()?;
    let value = map.entry(account.id.clone()).or_default();
    if *value >= account.concurrency {
        return None;
    }
    *value += 1;
    let route_key = route_key(target);
    record_inflight(&route_key, *value);
    Some(AccountLease {
        account_id: account.id.clone(),
        route_key,
    })
}

fn circuit_write_lock() -> &'static Mutex<()> {
    CIRCUIT_WRITE_LOCK.get_or_init(|| Mutex::new(()))
}

/// Load the route plan from natives.db. Missing tables/config intentionally fall
/// back to the caller's direct target so an upgrade cannot break existing runs.
pub fn load_plan(
    primary_provider: String,
    primary_key: Option<String>,
    primary_model: String,
) -> RoutingPlan {
    let primary = RouteTarget {
        provider_id: primary_provider,
        credential_kind: "api_key".into(),
        credential_id: primary_key,
        model_id: primary_model,
    };
    let path = crate::natives_db_broker::default_natives_db_path();
    let Ok(conn) = Connection::open(path) else {
        return RoutingPlan {
            enabled: false,
            targets: vec![primary],
        };
    };
    let enabled: Option<i64> = conn
        .query_row(
            "SELECT enabled FROM provider_routing_settings WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .ok()
        .flatten();
    if enabled.unwrap_or(0) == 0 {
        return RoutingPlan {
            enabled: false,
            targets: vec![primary],
        };
    }

    let mut targets = vec![primary];
    let Ok(mut stmt) = conn.prepare(
        "SELECT provider_id, credential_kind, credential_id, model_id
         FROM provider_route_bindings WHERE enabled = 1 ORDER BY position ASC, id ASC",
    ) else {
        return RoutingPlan {
            enabled: true,
            targets,
        };
    };
    let rows = stmt.query_map([], |row| {
        Ok(RouteTarget {
            provider_id: row.get(0)?,
            credential_kind: row.get(1)?,
            credential_id: row.get(2)?,
            model_id: row.get(3)?,
        })
    });
    if let Ok(rows) = rows {
        for target in rows.flatten() {
            if target.provider_id.trim().is_empty() || target.model_id.trim().is_empty() {
                continue;
            }
            if targets.iter().any(|existing| {
                existing.provider_id == target.provider_id
                    && existing.credential_kind == target.credential_kind
                    && existing.credential_id == target.credential_id
                    && existing.model_id == target.model_id
            }) {
                continue;
            }
            targets.push(target);
        }
    }
    RoutingPlan {
        enabled: true,
        targets,
    }
}

pub struct RoutedProvider {
    plan: RoutingPlan,
}

impl RoutedProvider {
    pub fn new(plan: RoutingPlan) -> Self {
        Self { plan }
    }
}

#[async_trait::async_trait]
impl EngineProvider for RoutedProvider {
    async fn stream(
        &self,
        model: &str,
        messages: Vec<EngineMessage>,
        tools: &[ToolSchema],
        system_prompt: Option<&str>,
        cancel: CancellationToken,
    ) -> Result<EngineProviderEventStream, EngineError> {
        let targets = self.plan.targets.clone();
        let base_model = model.to_string();
        let messages = messages.to_vec();
        let tools = tools.to_vec();
        let system_prompt = system_prompt.map(str::to_string);
        let output = async_stream::stream! {
            let mut last_error: Option<EngineError> = None;
            for target in targets {
                if cancel.is_cancelled() {
                    return;
                }
                if circuit_open(&target) {
                    continue;
                }
                let route_model = if target.model_id.trim().is_empty() { base_model.clone() } else { target.model_id.clone() };
                let result = tokio::time::timeout(Duration::from_secs(60), async {
                    if target.credential_kind == "sub2api_pool" {
                    Sub2ApiPoolProvider { provider_id: target.provider_id.clone() }.stream(
                        &route_model, messages.clone(), &tools, system_prompt.as_deref(), cancel.clone(),
                    ).await
                } else if target.credential_kind == "api_key" {
                    RealProvider { provider_id: target.provider_id.clone(), key_id: target.credential_id.clone() }.stream(
                        &route_model, messages.clone(), &tools, system_prompt.as_deref(), cancel.clone(),
                    ).await
                } else {
                    Err(EngineError::Message("unsupported routing credential kind".into()))
                }}).await.unwrap_or_else(|_| Err(timeout_error("provider first byte timed out")));
                let mut stream = match result {
                    Ok(stream) => stream,
                    Err(error) => {
                        if error.retryable() {
                            record_failure(&target);
                            last_error = Some(error);
                            continue;
                        }
                        yield error_event(error);
                        return;
                    }
                };
                let mut visible = false;
                let mut retry_target = false;
                let mut completed = false;
                while let Ok(Some(event)) = tokio::time::timeout(Duration::from_secs(120), stream.next()).await {
                    match &event {
                        EngineProviderEvent::TextDelta(_) | EngineProviderEvent::ReasoningDelta(_) | EngineProviderEvent::ToolCallDelta { .. } => visible = true,
                        EngineProviderEvent::Error { retryable: true, .. } if !visible => {
                            record_failure(&target);
                            last_error = Some(event_to_error(&event));
                            retry_target = true;
                            break;
                        }
                        EngineProviderEvent::Error { .. } => {
                            record_failure(&target);
                            yield event;
                            return;
                        }
                        EngineProviderEvent::Completed => { record_success(&target); completed = true; }
                        _ => {}
                    }
                    yield event;
                }
                if !visible && !completed && !retry_target { record_failure(&target); last_error = Some(timeout_error("provider stream idle timed out")); retry_target = true; }
                if !retry_target {
                    return;
                }
            }
            if let Some(error) = last_error {
                yield error_event(error);
            } else {
                yield EngineProviderEvent::Error {
                    message: "no routable provider target is available".into(),
                    code: "route_unavailable".into(),
                    retryable: true,
                    category: "Network".into(),
                    retry_after_ms: Some(60_000),
                };
            }
        };
        Ok(Box::pin(output))
    }
}

/// Provider implementation for one imported Sub2API account pool. It owns
/// account-level retry while the outer `RoutedProvider` owns cross-provider retry.
struct Sub2ApiPoolProvider {
    provider_id: String,
}

#[async_trait::async_trait]
impl EngineProvider for Sub2ApiPoolProvider {
    async fn stream(
        &self,
        model: &str,
        messages: Vec<EngineMessage>,
        tools: &[ToolSchema],
        system_prompt: Option<&str>,
        cancel: CancellationToken,
    ) -> Result<EngineProviderEventStream, EngineError> {
        let broker = NativesDbBroker::open(crate::natives_db_broker::default_natives_db_path())
            .map_err(EngineError::Message)?;
        let accounts = broker
            .resolve_sub2api_pool(&self.provider_id)
            .map_err(EngineError::Message)?;
        if accounts.is_empty() {
            return Err(EngineError::Message("no active Sub2API accounts".into()));
        }
        let model = model.to_string();
        let messages = messages.to_vec();
        let tools = tools.to_vec();
        let system_prompt = system_prompt.map(str::to_string);
        let provider_id = self.provider_id.clone();
        let output = async_stream::stream! {
            let mut last_error: Option<EngineError> = None;
            let mut accounts = accounts;
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            messages.iter().for_each(|message| { message.role.hash(&mut hasher); message.content.hash(&mut hasher); });
            let affinity = hasher.finish();
            accounts.sort_by_key(|account| (account.priority, inflight().lock().ok().and_then(|map| map.get(&account.id).copied()).unwrap_or(0)));
            if let Some(priority) = accounts.first().map(|account| account.priority) {
                let width = accounts.iter().take_while(|account| account.priority == priority).count();
                if width > 1 { accounts[..width].rotate_left((affinity as usize) % width); }
            }
            for account in accounts {
                if cancel.is_cancelled() { return; }
                let account_target = RouteTarget {
                    provider_id: provider_id.clone(), credential_kind: "sub2api_account".into(),
                    credential_id: Some(account.id.clone()), model_id: model.clone(),
                };
                if circuit_open(&account_target) { continue; }
                let Some(_lease) = acquire_account(&account, &account_target) else { continue; };
                record_selected(&account_target);
                let result = tokio::time::timeout(Duration::from_secs(60), account_stream(&account, &model, messages.clone(), &tools, system_prompt.as_deref(), cancel.clone())).await.unwrap_or_else(|_| Err(timeout_error("provider first byte timed out")));
                let mut stream = match result {
                    Ok(stream) => stream,
                    Err(error) if error.retryable() => { record_failure(&account_target); last_error = Some(error); continue; }
                    Err(error) => { yield error_event(error); return; }
                };
                let mut visible = false;
                let mut retry_account = false;
                let mut completed = false;
                while let Ok(Some(event)) = tokio::time::timeout(Duration::from_secs(120), stream.next()).await {
                    match &event {
                        EngineProviderEvent::TextDelta(_) | EngineProviderEvent::ReasoningDelta(_) | EngineProviderEvent::ToolCallDelta { .. } => visible = true,
                        EngineProviderEvent::Error { retryable: true, .. } if !visible => {
                            record_failure(&account_target); last_error = Some(event_to_error(&event)); retry_account = true; break;
                        }
                        EngineProviderEvent::Error { .. } => { record_failure(&account_target); yield event; return; }
                        EngineProviderEvent::Completed => { record_success(&account_target); completed = true; }
                        _ => {}
                    }
                    yield event;
                }
                if !visible && !completed && !retry_account { record_failure(&account_target); last_error = Some(timeout_error("provider stream idle timed out")); retry_account = true; }
                if !retry_account { return; }
            }
            yield error_event(last_error.unwrap_or_else(|| EngineError::Message("all Sub2API accounts are unavailable".into())));
        };
        Ok(Box::pin(output))
    }
}

async fn account_stream(
    account: &Sub2ApiAccountCredential,
    model: &str,
    messages: Vec<EngineMessage>,
    tools: &[ToolSchema],
    system_prompt: Option<&str>,
    cancel: CancellationToken,
) -> Result<EngineProviderEventStream, EngineError> {
    let mut account = account.clone();
    if account.platform == "openai" && account.account_type == "oauth" && oauth_expiring(&account) {
        refresh_codex_account(&mut account).await?;
    }
    let mut request = ProviderRequest {
        model: model.to_string(),
        messages: messages
            .into_iter()
            .map(crate::production::engine_message_to_history)
            .map(history_message_to_provider)
            .collect(),
        system_prompt: system_prompt.map(str::to_string),
        tools: (!tools.is_empty()).then(|| {
            tools
                .iter()
                .map(|tool| ProviderTool {
                    name: tool.name.clone(),
                    description: Some(tool.description.clone()),
                    input_schema: tool.input_schema.clone(),
                })
                .collect()
        }),
        // `None` delegates the output ceiling to the per-model profile in
        // `provider_adapters::model_profile`, which resolves it from the model
        // id. The previous hardcoded 4096 silently truncated every model with a
        // larger output window (Claude 4.x: 64K-128K, Gemini 2.5: 64K). Models
        // absent from the profile table still fall back to the adapter's own
        // 4096, so nothing regresses.
        max_tokens: None,
        temperature: None,
        stream: true,
        structured_output: None,
    };
    crate::request_rectifier::rectify_provider_request(&mut request, rectifier_enabled());
    let stream = if account.platform == "openai" && account.account_type == "oauth" {
        let access_token = string_credential(&account, "access_token")?;
        let credential = provider_adapters::providers::openai_codex::OpenAiCodexCredential {
            access_token,
            chatgpt_account_id: optional_credential(&account, "chatgpt_account_id"),
            fedramp: bool_credential(&account, "chatgpt_account_is_fedramp"),
        };
        let client = provider_adapters::http_client::client(account.proxy_url.as_deref()).map_err(
            |error| EngineError::Provider {
                message: "outbound proxy is unavailable".into(),
                code: error.code,
                retryable: error.retryable,
                category: format!("{:?}", error.category),
                retry_after_ms: error.retry_after_ms,
            },
        )?;
        provider_adapters::providers::openai_codex::stream(&client, request, credential).await
    } else {
        let api_key = string_credential(&account, "api_key")
            .or_else(|_| string_credential(&account, "access_token"))?;
        let credential = Credential {
            api_key,
            base_url: optional_credential(&account, "base_url"),
            proxy_url: account.proxy_url.clone(),
            key_id: Some(account.id.clone()),
            provider_type: Some(protocol_for(&account).into()),
        };
        adapter_for(&account).stream(request, credential).await
    }
    .map_err(|error| EngineError::Provider {
        message: crate::production::provider_error_message(
            &error,
            &account.provider_id,
            &account.platform,
            model,
            Some(&account.id),
            None,
        ),
        code: error.code,
        retryable: error.retryable,
        category: format!("{:?}", error.category),
        retry_after_ms: error.retry_after_ms,
    })?;
    let mapped = futures_util::stream::unfold(
        (stream, cancel),
        |(mut stream, cancel)| async move {
            if cancel.is_cancelled() {
                return None;
            }
            tokio::select! {
                event = stream.next() => event.map(|event| (provider_event_to_engine(event), (stream, cancel))),
                _ = cancel.cancelled() => None,
            }
        },
    );
    Ok(Box::pin(mapped))
}

pub(crate) fn rectifier_enabled() -> bool {
    let Ok(conn) = Connection::open(crate::natives_db_broker::default_natives_db_path()) else {
        return false;
    };
    let raw = conn
        .query_row(
            "SELECT rectifier_json FROM provider_routing_settings WHERE id=1",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .ok()
        .flatten();
    raw.and_then(|value| serde_json::from_str::<Value>(&value).ok())
        .and_then(|value| value.get("enabled").and_then(Value::as_bool))
        .unwrap_or(false)
}

const CODEX_OAUTH_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";

fn oauth_expiring(account: &Sub2ApiAccountCredential) -> bool {
    let expiry = account
        .credentials
        .get("expires_at")
        .and_then(Value::as_str)
        .or(account.expires_at.as_deref());
    let expiry = expiry
        .and_then(parse_expiry_epoch)
        .or_else(|| optional_credential(account, "access_token").and_then(jwt_expiry_epoch));
    // Missing expiry is safe only if refresh is available: refresh proactively.
    expiry
        .map(|at| at <= chrono::Utc::now().timestamp() + 180)
        .unwrap_or(true)
}

async fn refresh_codex_account(account: &mut Sub2ApiAccountCredential) -> Result<(), EngineError> {
    let refresh_token = optional_credential(account, "refresh_token")
        .filter(|token| !token.trim().is_empty())
        .ok_or_else(|| {
            oauth_pool_error("OpenAI OAuth access token expired and has no refresh token")
        })?;
    let client_id = optional_credential(account, "client_id")
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| CODEX_CLIENT_ID.into());
    let client = provider_adapters::http_client::client(account.proxy_url.as_deref())
        .map_err(|_| oauth_pool_error("outbound proxy is unavailable for OAuth refresh"))?;
    let response = client
        .post(CODEX_OAUTH_TOKEN_URL)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token.as_str()),
            ("client_id", client_id.as_str()),
        ])
        .send()
        .await
        .map_err(|_| oauth_pool_error("OpenAI OAuth refresh request failed"))?;
    if !response.status().is_success() {
        return Err(oauth_pool_error("OpenAI OAuth refresh was rejected"));
    }
    let payload: Value = response
        .json()
        .await
        .map_err(|_| oauth_pool_error("OpenAI OAuth refresh returned invalid data"))?;
    let access_token = payload
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| oauth_pool_error("OpenAI OAuth refresh returned no access token"))?;
    let expires_at = payload
        .get("expires_in")
        .and_then(Value::as_i64)
        .map(|seconds| {
            (chrono::Utc::now() + chrono::Duration::seconds(seconds.max(0))).to_rfc3339()
        });
    let credentials = account
        .credentials
        .as_object_mut()
        .ok_or_else(|| oauth_pool_error("OpenAI OAuth credentials are invalid"))?;
    credentials.insert(
        "access_token".into(),
        Value::String(access_token.to_string()),
    );
    if let Some(refresh) = payload
        .get("refresh_token")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        credentials.insert("refresh_token".into(), Value::String(refresh.to_string()));
    }
    if let Some(expiry) = &expires_at {
        credentials.insert("expires_at".into(), Value::String(expiry.clone()));
    }
    let broker = NativesDbBroker::open(crate::natives_db_broker::default_natives_db_path())
        .map_err(EngineError::Message)?;
    broker
        .update_sub2api_credentials(&account.id, &account.credentials, expires_at.as_deref())
        .map_err(EngineError::Message)?;
    account.expires_at = expires_at;
    Ok(())
}

fn parse_expiry_epoch(value: &str) -> Option<i64> {
    value.parse::<i64>().ok().or_else(|| {
        chrono::DateTime::parse_from_rfc3339(value)
            .ok()
            .map(|time| time.timestamp())
    })
}

fn jwt_expiry_epoch(token: String) -> Option<i64> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    let payload = token.split('.').nth(1)?;
    let decoded = URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice::<Value>(&decoded)
        .ok()?
        .get("exp")?
        .as_i64()
}

fn oauth_pool_error(message: &str) -> EngineError {
    EngineError::Provider {
        message: message.into(),
        code: "oauth_refresh".into(),
        retryable: true,
        category: "Auth".into(),
        retry_after_ms: None,
    }
}

fn adapter_for(account: &Sub2ApiAccountCredential) -> Box<dyn ProviderAdapter> {
    match account.platform.as_str() {
        "anthropic" => Box::new(provider_adapters::providers::anthropic::AnthropicAdapter::new()),
        "gemini" => Box::new(provider_adapters::providers::gemini::GeminiAdapter::new()),
        "openai" if account.account_type == "upstream" => Box::new(
            provider_adapters::providers::openai_compatible::OpenAiCompatibleAdapter::new(),
        ),
        _ => Box::new(provider_adapters::providers::openai::OpenAiAdapter::new()),
    }
}

fn protocol_for(account: &Sub2ApiAccountCredential) -> &'static str {
    match account.platform.as_str() {
        "anthropic" => "anthropic_messages",
        "gemini" => "gemini_generate_content",
        "openai" if account.account_type == "upstream" => "openai_chat_completions",
        _ => "openai_responses",
    }
}

fn string_credential(account: &Sub2ApiAccountCredential, key: &str) -> Result<String, EngineError> {
    optional_credential(account, key)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| EngineError::Message(format!("Sub2API account is missing {key}")))
}

fn optional_credential(account: &Sub2ApiAccountCredential, key: &str) -> Option<String> {
    account
        .credentials
        .get(key)
        .and_then(|value| value.as_str())
        .map(str::to_string)
}

fn bool_credential(account: &Sub2ApiAccountCredential, key: &str) -> bool {
    account
        .credentials
        .get(key)
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

fn provider_event_to_engine(event: ProviderEvent) -> EngineProviderEvent {
    match event {
        ProviderEvent::TextDelta(value) => EngineProviderEvent::TextDelta(value),
        ProviderEvent::ReasoningDelta(value) => EngineProviderEvent::ReasoningDelta(value),
        ProviderEvent::ToolCallDelta {
            index,
            id,
            name,
            arguments_delta,
        } => EngineProviderEvent::ToolCallDelta {
            index,
            id,
            name,
            arguments_delta,
        },
        ProviderEvent::Usage(usage) => EngineProviderEvent::Usage {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            reasoning_tokens: usage.reasoning_tokens,
        },
        ProviderEvent::Completed => EngineProviderEvent::Completed,
        ProviderEvent::Error(error) => EngineProviderEvent::Error {
            message: assistant_protocol::v2::redact_secrets(&error.message),
            code: error.code,
            retryable: error.retryable,
            category: format!("{:?}", error.category),
            retry_after_ms: error.retry_after_ms,
        },
    }
}

fn route_key(target: &RouteTarget) -> String {
    format!(
        "{}:{}:{}:{}",
        target.provider_id,
        target.credential_kind,
        target.credential_id.as_deref().unwrap_or(""),
        target.model_id
    )
}

fn route_health_connection() -> Result<Connection, rusqlite::Error> {
    let conn = Connection::open(crate::natives_db_broker::default_assistant_db_path())?;
    conn.execute_batch("CREATE TABLE IF NOT EXISTS provider_route_health (route_key TEXT PRIMARY KEY, consecutive_failures INTEGER NOT NULL DEFAULT 0, open_until_ms INTEGER, half_open_in_flight INTEGER NOT NULL DEFAULT 0, in_flight INTEGER NOT NULL DEFAULT 0, last_selected_at TEXT, last_error TEXT, updated_at TEXT NOT NULL DEFAULT (datetime('now')))")?;
    let columns = conn
        .prepare("PRAGMA table_info(provider_route_health)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    for (name, sql) in [
        ("half_open_in_flight", "ALTER TABLE provider_route_health ADD COLUMN half_open_in_flight INTEGER NOT NULL DEFAULT 0"),
        ("in_flight", "ALTER TABLE provider_route_health ADD COLUMN in_flight INTEGER NOT NULL DEFAULT 0"),
        ("last_selected_at", "ALTER TABLE provider_route_health ADD COLUMN last_selected_at TEXT"),
        ("last_error", "ALTER TABLE provider_route_health ADD COLUMN last_error TEXT"),
    ] {
        if !columns.iter().any(|column| column == name) {
            conn.execute(sql, [])?;
        }
    }
    HEALTH_STATE_INITIALIZED.get_or_init(|| {
        let _ = conn.execute(
            "UPDATE provider_route_health SET in_flight=0, half_open_in_flight=0",
            [],
        );
    });
    Ok(conn)
}

fn epoch_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

fn circuit_open(target: &RouteTarget) -> bool {
    let Ok(conn) = route_health_connection() else {
        return false;
    };
    let open_until = conn
        .query_row(
            "SELECT open_until_ms FROM provider_route_health WHERE route_key=?1",
            [route_key(target)],
            |row| row.get::<_, Option<i64>>(0),
        )
        .optional()
        .ok()
        .flatten()
        .flatten();
    match open_until {
        Some(until) if until > epoch_ms() => true,
        Some(_) => conn.execute("UPDATE provider_route_health SET half_open_in_flight=1, updated_at=datetime('now') WHERE route_key=?1 AND half_open_in_flight=0", [route_key(target)]).map(|changed| changed == 0).unwrap_or(true),
        None => false,
    }
}

fn record_selected(target: &RouteTarget) {
    if let Ok(conn) = route_health_connection() {
        let _ = conn.execute("INSERT INTO provider_route_health(route_key, last_selected_at, updated_at) VALUES(?1,datetime('now'),datetime('now')) ON CONFLICT(route_key) DO UPDATE SET last_selected_at=datetime('now'),updated_at=datetime('now')", [route_key(target)]);
    }
}

fn record_inflight(key: &str, value: u32) {
    if let Ok(conn) = route_health_connection() {
        let _ = conn.execute(
            "INSERT INTO provider_route_health(route_key, in_flight, updated_at)
             VALUES(?1,?2,datetime('now'))
             ON CONFLICT(route_key) DO UPDATE SET in_flight=excluded.in_flight,updated_at=excluded.updated_at",
            params![key, value],
        );
    }
}

fn record_failure(target: &RouteTarget) {
    let Ok(_guard) = circuit_write_lock().lock() else {
        return;
    };
    let Ok(conn) = route_health_connection() else {
        return;
    };
    let key = route_key(target);
    let failures = conn
        .query_row(
            "SELECT consecutive_failures FROM provider_route_health WHERE route_key=?1",
            [&key],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .ok()
        .flatten()
        .unwrap_or(0)
        .saturating_add(1);
    let open_until = (failures >= i64::from(FAILURE_THRESHOLD))
        .then(|| epoch_ms().saturating_add(COOLDOWN.as_millis().min(i64::MAX as u128) as i64));
    let _ = conn.execute("INSERT INTO provider_route_health (route_key, consecutive_failures, open_until_ms, half_open_in_flight, last_error, updated_at) VALUES (?1,?2,?3,0,'request_failed',datetime('now')) ON CONFLICT(route_key) DO UPDATE SET consecutive_failures=excluded.consecutive_failures, open_until_ms=excluded.open_until_ms, half_open_in_flight=0, last_error='request_failed', updated_at=excluded.updated_at", params![key, failures, open_until]);
}

fn record_success(target: &RouteTarget) {
    if let Ok(conn) = route_health_connection() {
        let _ = conn.execute(
            "INSERT INTO provider_route_health(route_key, consecutive_failures, open_until_ms, half_open_in_flight, last_error, updated_at)
             VALUES(?1,0,NULL,0,NULL,datetime('now'))
             ON CONFLICT(route_key) DO UPDATE SET consecutive_failures=0,open_until_ms=NULL,half_open_in_flight=0,last_error=NULL,updated_at=datetime('now')",
            [route_key(target)],
        );
    }
}

fn event_to_error(event: &EngineProviderEvent) -> EngineError {
    match event {
        EngineProviderEvent::Error {
            message,
            code,
            retryable,
            category,
            retry_after_ms,
        } => EngineError::Provider {
            message: message.clone(),
            code: code.clone(),
            retryable: *retryable,
            category: category.clone(),
            retry_after_ms: *retry_after_ms,
        },
        _ => EngineError::Message("routing stream failed".into()),
    }
}

fn error_event(error: EngineError) -> EngineProviderEvent {
    match error {
        EngineError::Provider {
            message,
            code,
            retryable,
            category,
            retry_after_ms,
        } => EngineProviderEvent::Error {
            message,
            code,
            retryable,
            category,
            retry_after_ms,
        },
        other => EngineProviderEvent::Error {
            message: other.to_string(),
            code: other.code().into(),
            retryable: other.retryable(),
            category: "Unknown".into(),
            retry_after_ms: None,
        },
    }
}

fn timeout_error(message: &str) -> EngineError {
    EngineError::Provider {
        message: message.into(),
        code: "timeout".into(),
        retryable: true,
        category: "Network".into(),
        retry_after_ms: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circuit_opens_after_three_failures_and_recovers_after_success() {
        let target = RouteTarget {
            provider_id: "p".into(),
            credential_kind: "api_key".into(),
            credential_id: Some("k".into()),
            model_id: "m".into(),
        };
        record_success(&target);
        record_failure(&target);
        record_failure(&target);
        assert!(!circuit_open(&target));
        record_failure(&target);
        assert!(circuit_open(&target));
        record_success(&target);
        assert!(!circuit_open(&target));
    }
}
