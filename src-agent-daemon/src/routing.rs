//! Provider routing for one engine stream.
//!
//! The module owns target ordering, pre-delta retry and circuit state. The engine
//! still sees one `EngineProvider`, so routing policy never leaks into agent-core.

use agent_core::{
    EngineError, EngineMessage, EngineProvider, EngineProviderContext, EngineProviderEvent,
    EngineProviderEventStream, ToolSchema,
};
use futures_util::StreamExt;
use provider_adapters::adapter::ProviderAdapter;
use provider_adapters::capabilities::{
    history_message_to_provider, Credential, ProviderRequest, ProviderTool,
};
use provider_adapters::controls::RequestControls;
use provider_adapters::stream::ProviderEvent;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

use crate::natives_db_broker::{NativesDbBroker, Sub2ApiAccountCredential};
use crate::production::RealProvider;

mod routing_errors;
use routing_errors::{error_event, event_to_error, timeout_error};

#[path = "routing_health.rs"]
mod routing_health;
use routing_health::{
    circuit_open, record_failure, record_inflight, record_selected, record_success, route_key,
};

const MAX_ROUTE_TARGETS: usize = 3;

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

impl RoutingPlan {
    fn attempts(&self) -> impl Iterator<Item = &RouteTarget> {
        self.targets.iter().take(MAX_ROUTE_TARGETS)
    }
}

static ACCOUNT_INFLIGHT: OnceLock<Mutex<HashMap<String, u32>>> = OnceLock::new();
static OAUTH_REFRESH_LOCKS: OnceLock<Mutex<HashMap<String, Weak<tokio::sync::Mutex<()>>>>> =
    OnceLock::new();
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

/// Decide the primary route target kind for a run (ADR-0019 P4).
///
/// When the caller did not select an API key and the provider has a usable
/// account pool (OAuth-only provider), the primary target must route through
/// `sub2api_pool` — otherwise `provider_api_keys = 0` + usable OAuth accounts
/// can never start a run. When a key id is given, or no pool is available,
/// the existing API-key behaviour is preserved (fail-closed if no key exists).
fn primary_credential_kind(primary_key: Option<&str>, pool_available: bool) -> &'static str {
    let has_key_selector = primary_key
        .map(str::trim)
        .is_some_and(|key| !key.is_empty() && key != "_primary_");
    if !has_key_selector && pool_available {
        "sub2api_pool"
    } else {
        "api_key"
    }
}

/// Load the route plan via the Host broker lease (T104 / modular remediation
/// W1). The daemon never opens natives.db — `provider_routing_settings` and
/// `provider_route_bindings` are Host-owned and served over the authenticated
/// broker socket. Missing or disabled config falls back to the caller's direct
/// target so an upgrade cannot break existing runs.
pub fn load_plan(
    primary_provider: String,
    primary_key: Option<String>,
    primary_model: String,
) -> RoutingPlan {
    let broker = NativesDbBroker::open_default().ok();
    // OAuth-only fallback probe (ADR-0019 P4): only when the caller did not
    // select a concrete API key, ask the broker whether this provider has a
    // usable account pool. Memory-only lease; the daemon never sees token
    // material. A concrete key_id keeps the pre-existing API-key path without
    // any extra broker round trip.
    let unselected_key = primary_key
        .as_deref()
        .map(str::trim)
        .is_none_or(|key| key.is_empty() || key == "_primary_");
    let pool_available = unselected_key
        && broker.as_ref().is_some_and(|b| {
            b.resolve_sub2api_pool(
                &primary_provider,
                &format!("routing-probe:{}", uuid::Uuid::new_v4()),
            )
            .is_ok_and(|a| !a.is_empty())
        });
    let primary_kind = primary_credential_kind(primary_key.as_deref(), pool_available);
    let primary = RouteTarget {
        provider_id: primary_provider,
        credential_kind: primary_kind.into(),
        credential_id: if primary_kind == "api_key" {
            primary_key
        } else {
            None
        },
        model_id: primary_model,
    };
    match broker.and_then(|b| b.routing_plan("engine").ok()) {
        Some(plan) => configured_plan(
            primary,
            plan.enabled,
            plan.targets
                .into_iter()
                .map(|target| RouteTarget {
                    provider_id: target.provider_id,
                    credential_kind: target.credential_kind,
                    credential_id: target.credential_id,
                    model_id: target.model_id,
                })
                .collect(),
        ),
        None => {
            // Broker unreachable → fail closed to the primary target only.
            configured_plan(primary, false, Vec::new())
        }
    }
}

fn configured_plan(primary: RouteTarget, enabled: bool, targets: Vec<RouteTarget>) -> RoutingPlan {
    if enabled && !targets.is_empty() {
        RoutingPlan { enabled, targets }
    } else {
        RoutingPlan {
            enabled: false,
            targets: vec![primary],
        }
    }
}

pub struct RoutedProvider {
    plan: RoutingPlan,
    controls: RequestControls,
}

impl RoutedProvider {
    /// Route with provider-default request controls.
    pub fn new(plan: RoutingPlan) -> Self {
        Self {
            plan,
            controls: RequestControls::default(),
        }
    }

    /// Apply per-run request controls (reasoning effort, forced tool choice,
    /// prompt-cache opt-out) to every target this plan falls through to.
    ///
    /// They ride along across a fallback on purpose: a run asked for a given
    /// reasoning depth, and silently dropping it when the primary target fails
    /// would make the retry answer a different question than the first attempt.
    pub fn with_controls(mut self, controls: RequestControls) -> Self {
        self.controls = controls;
        self
    }
}

fn proxy_request_context() -> EngineProviderContext {
    EngineProviderContext {
        run_id: format!("proxy-request:{}", uuid::Uuid::new_v4()),
        attempt: 0,
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
        self.stream_with_context(
            proxy_request_context(),
            model,
            messages,
            tools,
            system_prompt,
            cancel,
        )
        .await
    }

    async fn stream_with_context(
        &self,
        context: EngineProviderContext,
        model: &str,
        messages: Vec<EngineMessage>,
        tools: &[ToolSchema],
        system_prompt: Option<&str>,
        cancel: CancellationToken,
    ) -> Result<EngineProviderEventStream, EngineError> {
        let messages = messages
            .into_iter()
            .map(crate::production::engine_message_to_history)
            .collect();
        self.stream_history_with_context(
            context,
            model.to_string(),
            messages,
            tools,
            system_prompt,
            cancel,
        )
        .await
    }

    async fn stream_turn(
        &self,
        request: agent_core::ProviderTurnRequest,
        cancel: CancellationToken,
    ) -> Result<EngineProviderEventStream, EngineError> {
        let messages = request
            .messages
            .into_iter()
            .map(crate::production::agent_message_to_history)
            .collect();
        self.stream_history_with_context(
            request.context,
            request.model,
            messages,
            &request.tools,
            request.system_prompt.as_deref(),
            cancel,
        )
        .await
    }
}

impl RoutedProvider {
    async fn stream_history_with_context(
        &self,
        context: EngineProviderContext,
        model: String,
        messages: Vec<provider_adapters::capabilities::HistoryMessage>,
        tools: &[ToolSchema],
        system_prompt: Option<&str>,
        cancel: CancellationToken,
    ) -> Result<EngineProviderEventStream, EngineError> {
        let targets: Vec<_> = self.plan.attempts().cloned().collect();
        let base_model = model;
        let messages = messages.to_vec();
        let tools = tools.to_vec();
        let system_prompt = system_prompt.map(str::to_string);
        let controls = self.controls.clone();
        let context = context.clone();
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
                    Sub2ApiPoolProvider {
                        provider_id: target.provider_id.clone(),
                        controls: controls.clone(),
                        run_id: context.run_id.clone(),
                    }.stream_history(
                        route_model.clone(), messages.clone(), &tools, system_prompt.as_deref(), cancel.clone(),
                    ).await
                } else if target.credential_kind == "api_key" {
                    RealProvider { provider_id: target.provider_id.clone(), key_id: target.credential_id.clone() }.stream_with_history_context_controls(
                        &context, &controls, &route_model, messages.clone(), &tools, system_prompt.as_deref(), cancel.clone(),
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
                        EngineProviderEvent::Completed | EngineProviderEvent::CompletedWithReason { .. } => { record_success(&target); completed = true; }
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
    controls: RequestControls,
    run_id: String,
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
        let messages = messages
            .into_iter()
            .map(crate::production::engine_message_to_history)
            .collect();
        self.stream_history(model.to_string(), messages, tools, system_prompt, cancel)
            .await
    }

    async fn stream_turn(
        &self,
        request: agent_core::ProviderTurnRequest,
        cancel: CancellationToken,
    ) -> Result<EngineProviderEventStream, EngineError> {
        let messages = request
            .messages
            .into_iter()
            .map(crate::production::agent_message_to_history)
            .collect();
        self.stream_history(
            request.model,
            messages,
            &request.tools,
            request.system_prompt.as_deref(),
            cancel,
        )
        .await
    }
}

impl Sub2ApiPoolProvider {
    async fn stream_history(
        &self,
        model: String,
        messages: Vec<provider_adapters::capabilities::HistoryMessage>,
        tools: &[ToolSchema],
        system_prompt: Option<&str>,
        cancel: CancellationToken,
    ) -> Result<EngineProviderEventStream, EngineError> {
        let broker = NativesDbBroker::open_default().map_err(EngineError::Message)?;
        let accounts = broker
            .resolve_sub2api_pool(&self.provider_id, &self.run_id)
            .map_err(EngineError::Message)?;
        if accounts.is_empty() {
            return Err(EngineError::Message("no active Sub2API accounts".into()));
        }
        let messages = messages.to_vec();
        let tools = tools.to_vec();
        let system_prompt = system_prompt.map(str::to_string);
        let provider_id = self.provider_id.clone();
        let controls = self.controls.clone();
        let output = async_stream::stream! {
            let mut last_error: Option<EngineError> = None;
            let mut accounts = accounts;
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            messages.iter().for_each(|message| { message.role.hash(&mut hasher); message.content.hash(&mut hasher); });
            let affinity = hasher.finish();
            order_pool_accounts(&mut accounts, affinity);
            for account in accounts {
                if cancel.is_cancelled() { return; }
                let account_target = RouteTarget {
                    provider_id: provider_id.clone(), credential_kind: "sub2api_account".into(),
                    credential_id: Some(account.id.clone()), model_id: model.clone(),
                };
                if circuit_open(&account_target) { continue; }
                let Some(_lease) = acquire_account(&account, &account_target) else { continue; };
                record_selected(&account_target);
                let result = tokio::time::timeout(Duration::from_secs(60), account_stream_history(&account, &controls, &model, messages.clone(), &tools, system_prompt.as_deref(), cancel.clone())).await.unwrap_or_else(|_| Err(timeout_error("provider first byte timed out")));
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
                        EngineProviderEvent::Completed | EngineProviderEvent::CompletedWithReason { .. } => { record_success(&account_target); completed = true; }
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

/// Order a pool's accounts for selection (P5 routing policy, single executor):
/// 1. ascending `priority` (lower number first), then fewest in-flight;
/// 2. within the top-priority tier, rotate by the message-derived session
///    affinity so equal-priority accounts share load but stick to a session.
///
/// Pure over the account list — the in-flight tie-break reads the global
/// inflight map (bounded), so it stays deterministic for equal-priority pools
/// with no in-flight requests.
fn order_pool_accounts(accounts: &mut [Sub2ApiAccountCredential], affinity: u64) {
    accounts.sort_by_key(|account| {
        let inflight_count = inflight()
            .lock()
            .ok()
            .and_then(|map| map.get(&account.id).copied())
            .unwrap_or(0);
        (account.priority, inflight_count)
    });
    let Some(priority) = accounts.first().map(|account| account.priority) else {
        return;
    };
    let width = accounts
        .iter()
        .take_while(|account| account.priority == priority)
        .count();
    if width > 1 {
        accounts[..width].rotate_left((affinity as usize) % width);
    }
}

async fn account_stream_history(
    account: &Sub2ApiAccountCredential,
    controls: &RequestControls,
    model: &str,
    messages: Vec<provider_adapters::capabilities::HistoryMessage>,
    tools: &[ToolSchema],
    system_prompt: Option<&str>,
    cancel: CancellationToken,
) -> Result<EngineProviderEventStream, EngineError> {
    let mut account = account.clone();
    if account.account_type == "oauth" && oauth_expiring(&account) {
        let refresh_lock = oauth_refresh_lock(&account.id)?;
        let _refresh_guard = refresh_lock.lock().await;
        let broker = NativesDbBroker::open_default().map_err(EngineError::Message)?;
        account = broker
            .resolve_sub2api_pool(&account.provider_id, &account.run_id)
            .map_err(EngineError::Message)?
            .into_iter()
            .find(|candidate| candidate.id == account.id)
            .ok_or_else(|| EngineError::Message("OAuth account is no longer active".into()))?;
        if oauth_expiring(&account) {
            refresh_oauth_account(&mut account).await?;
        }
    }
    let mut request = ProviderRequest {
        model: model.to_string(),
        messages: messages
            .into_iter()
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
        controls: controls.clone(),
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
            project_id: optional_credential(&account, "project_id"),
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

fn oauth_refresh_lock(account_id: &str) -> Result<Arc<tokio::sync::Mutex<()>>, EngineError> {
    let locks = OAUTH_REFRESH_LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut locks = locks
        .lock()
        .map_err(|_| oauth_pool_error("OAuth refresh lock is unavailable"))?;
    locks.retain(|_, lock| lock.strong_count() > 0);
    if let Some(lock) = locks.get(account_id).and_then(Weak::upgrade) {
        return Ok(lock);
    }
    let lock = Arc::new(tokio::sync::Mutex::new(()));
    locks.insert(account_id.to_string(), Arc::downgrade(&lock));
    Ok(lock)
}

pub(crate) fn rectifier_enabled() -> bool {
    // T104 / W1: the rectifier config is Host-owned (natives.db). Read it via
    // the broker lease — `loopback_settings` already returns the rectifier
    // JSON — so the daemon never opens natives.db.
    let Ok(settings) = NativesDbBroker::open_default().and_then(|b| b.loopback_settings()) else {
        return false;
    };
    settings
        .rectifier
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// 问题8：自动协议路由开关。Host 的 `provider_routing_settings.enabled` 经
/// broker 租约读取；不可达/未配置时关闭（沿用供应商显式协议，保证既有配置可运行）。
pub(crate) fn routing_enabled() -> bool {
    let Ok(plan) = NativesDbBroker::open_default().and_then(|b| b.routing_plan("engine")) else {
        return false;
    };
    plan.enabled
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

async fn refresh_oauth_account(account: &mut Sub2ApiAccountCredential) -> Result<(), EngineError> {
    let refresh_token = optional_credential(account, "refresh_token")
        .filter(|token| !token.trim().is_empty())
        .ok_or_else(|| oauth_pool_error("OAuth access token expired and has no refresh token"))?;
    // Generic OAuth accounts carry their own token endpoint + client id (stored
    // by the Host `provider_oauth_start`); legacy Codex accounts fall back to
    // the shared OpenAI OAuth constants.
    let token_url = optional_credential(account, "token_url")
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| CODEX_OAUTH_TOKEN_URL.into());
    let client_id = optional_credential(account, "client_id")
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| CODEX_CLIENT_ID.into());
    let client = provider_adapters::http_client::client(account.proxy_url.as_deref())
        .map_err(|_| oauth_pool_error("outbound proxy is unavailable for OAuth refresh"))?;
    let mut form = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token.as_str()),
        ("client_id", client_id.as_str()),
    ];
    let client_secret =
        optional_credential(account, "client_secret").filter(|value| !value.trim().is_empty());
    if let Some(client_secret) = client_secret.as_deref() {
        form.push(("client_secret", client_secret));
    }
    let response = client
        .post(token_url)
        .form(&form)
        .send()
        .await
        .map_err(|_| oauth_pool_error("OAuth refresh request failed"))?;
    let status = response.status();
    if !status.is_success() {
        let error_code = response
            .bytes()
            .await
            .ok()
            .and_then(|body| oauth_error_code(&body));
        if error_code.as_deref() == Some("invalid_grant") {
            let broker = NativesDbBroker::open_default().map_err(EngineError::Message)?;
            broker
                .update_sub2api_credentials(account, &account.credentials, None, true)
                .map_err(EngineError::Message)?;
            return Err(oauth_reauth_error());
        }
        return Err(oauth_pool_error("OAuth refresh was rejected"));
    }
    let payload: Value = response
        .json()
        .await
        .map_err(|_| oauth_pool_error("OAuth refresh returned invalid data"))?;
    let access_token = payload
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| oauth_pool_error("OAuth refresh returned no access token"))?;
    let expires_at = payload
        .get("expires_in")
        .and_then(Value::as_i64)
        .map(|seconds| {
            (chrono::Utc::now() + chrono::Duration::seconds(seconds.max(0))).to_rfc3339()
        });
    let credentials = account
        .credentials
        .as_object_mut()
        .ok_or_else(|| oauth_pool_error("OAuth credentials are invalid"))?;
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
    let broker = NativesDbBroker::open_default().map_err(EngineError::Message)?;
    broker
        .update_sub2api_credentials(account, &account.credentials, expires_at.as_deref(), false)
        .map_err(EngineError::Message)?;
    account.expires_at = expires_at;
    Ok(())
}

fn oauth_error_code(body: &[u8]) -> Option<String> {
    const MAX_OAUTH_ERROR_BODY: usize = 64 * 1024;
    let body = body.get(..body.len().min(MAX_OAUTH_ERROR_BODY))?;
    let value: Value = serde_json::from_slice(body).ok()?;
    value
        .get("error")
        .and_then(|error| match error {
            Value::String(code) => Some(code.as_str()),
            Value::Object(object) => object.get("code").and_then(Value::as_str),
            _ => None,
        })
        .map(str::to_string)
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

fn oauth_reauth_error() -> EngineError {
    EngineError::Provider {
        message: "OAuth authorization is no longer valid; reconnect the account".into(),
        code: "oauth_reauth_required".into(),
        retryable: false,
        category: "Auth".into(),
        retry_after_ms: None,
    }
}

fn adapter_for(account: &Sub2ApiAccountCredential) -> Box<dyn ProviderAdapter> {
    match account_adapter_kind(account) {
        AccountAdapterKind::Antigravity => {
            Box::new(provider_adapters::providers::antigravity::AntigravityAdapter::new())
        }
        AccountAdapterKind::Anthropic => {
            Box::new(provider_adapters::providers::anthropic::AnthropicAdapter::new())
        }
        AccountAdapterKind::Gemini => {
            Box::new(provider_adapters::providers::gemini::GeminiAdapter::new())
        }
        AccountAdapterKind::OpenAiCompatible => Box::new(
            provider_adapters::providers::openai_compatible::OpenAiCompatibleAdapter::new(),
        ),
        AccountAdapterKind::OpenAi => {
            Box::new(provider_adapters::providers::openai::OpenAiAdapter::new())
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AccountAdapterKind {
    Antigravity,
    Anthropic,
    Gemini,
    OpenAiCompatible,
    OpenAi,
}

fn account_adapter_kind(account: &Sub2ApiAccountCredential) -> AccountAdapterKind {
    if account.provider_id.eq_ignore_ascii_case("antigravity") {
        AccountAdapterKind::Antigravity
    } else {
        match account.platform.as_str() {
            "anthropic" => AccountAdapterKind::Anthropic,
            "gemini" => AccountAdapterKind::Gemini,
            "openai" if account.account_type == "upstream" => AccountAdapterKind::OpenAiCompatible,
            _ => AccountAdapterKind::OpenAi,
        }
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
            cache_creation_tokens: usage.cache_creation_tokens,
            cache_read_tokens: usage.cache_read_tokens,
        },
        ProviderEvent::Completed { reason } => EngineProviderEvent::CompletedWithReason {
            reason: match reason {
                provider_adapters::stream::ProviderStopReason::Stop => {
                    agent_core::ProviderStopReason::Stop
                }
                provider_adapters::stream::ProviderStopReason::ToolUse => {
                    agent_core::ProviderStopReason::ToolUse
                }
                provider_adapters::stream::ProviderStopReason::Length => {
                    agent_core::ProviderStopReason::Length
                }
                provider_adapters::stream::ProviderStopReason::Cancelled => {
                    agent_core::ProviderStopReason::Cancelled
                }
                provider_adapters::stream::ProviderStopReason::Error => {
                    agent_core::ProviderStopReason::Error
                }
                provider_adapters::stream::ProviderStopReason::Unknown(value) => {
                    agent_core::ProviderStopReason::Unknown(value)
                }
            },
        },
        ProviderEvent::Error(error) => EngineProviderEvent::Error {
            message: assistant_protocol::v2::redact_secrets(&error.message),
            code: error.code,
            retryable: error.retryable,
            category: format!("{:?}", error.category),
            retry_after_ms: error.retry_after_ms,
        },
    }
}

#[cfg(test)]
#[path = "routing_tests.rs"]
mod tests;
