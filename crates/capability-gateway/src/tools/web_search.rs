//! `web_search` — bring-your-own search backend.
//!
//! # Why there is a backend at all
//!
//! Natives runs on the user's machine and ships no index and no crawler, so
//! there is nothing here that can answer a query on its own. The project rule is
//! that every user-visible field has a real source, which rules out the usual
//! shortcut of returning plausible-looking hits. That leaves exactly two honest
//! options: call a real search API, or do not offer the tool.
//!
//! This module does both, in that order. When a backend is configured the tool
//! is registered and every result is a real response from that API. When none is
//! configured the tool is **not registered at all** (see
//! [`super::builtin_tools`]) — a model that cannot search should not be told it
//! can, and an always-failing tool burns tokens in every request. If a backend
//! disappears between registration and a call, the handler returns a hard error
//! naming the missing configuration. It never returns an empty result set to
//! stand in for a missing backend: "no configuration" and "no hits" are
//! different facts and must never be reported the same way.
//!
//! # Configuration
//!
//! Either install a backend programmatically (the host can resolve a stored
//! credential and call [`install_backend`]), or set:
//!
//! ```text
//! NATIVES_WEB_SEARCH_PROVIDER = brave | tavily
//! NATIVES_WEB_SEARCH_API_KEY  = <key>
//! NATIVES_WEB_SEARCH_ENDPOINT = <optional override>
//! ```
//!
//! # Safety
//!
//! The endpoint goes through the same [`super::ssrf`] guard as `web_fetch`, and
//! so does every URL in the response — search results are attacker-influenced
//! input, and they exist to be handed to `web_fetch` next. Blocked results are
//! dropped and counted, never silently swallowed.

use crate::tools::ssrf;
use crate::{
    PathScope, PermissionClass, SideEffect, Tool, ToolCallContext, ToolError, ToolHandler,
    ToolOutput,
};
use std::sync::{Arc, Mutex, OnceLock};

/// Env var naming the search provider.
pub const ENV_PROVIDER: &str = "NATIVES_WEB_SEARCH_PROVIDER";
/// Env var carrying the API key.
pub const ENV_API_KEY: &str = "NATIVES_WEB_SEARCH_API_KEY";
/// Env var overriding the API endpoint (self-hosted gateways, proxies).
pub const ENV_ENDPOINT: &str = "NATIVES_WEB_SEARCH_ENDPOINT";

const BRAVE_ENDPOINT: &str = "https://api.search.brave.com/res/v1/web/search";
const TAVILY_ENDPOINT: &str = "https://api.tavily.com/search";

const MAX_RESULTS: u64 = 10;
const DEFAULT_RESULTS: u64 = 5;
const MAX_SNIPPET_CHARS: usize = 400;

/// Which real search API to call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchProvider {
    /// Brave Search API — GET, `X-Subscription-Token` header.
    Brave,
    /// Tavily Search API — POST JSON.
    Tavily,
}

impl SearchProvider {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "brave" | "brave_search" => Some(SearchProvider::Brave),
            "tavily" => Some(SearchProvider::Tavily),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            SearchProvider::Brave => "brave",
            SearchProvider::Tavily => "tavily",
        }
    }

    fn default_endpoint(self) -> &'static str {
        match self {
            SearchProvider::Brave => BRAVE_ENDPOINT,
            SearchProvider::Tavily => TAVILY_ENDPOINT,
        }
    }
}

/// A configured, usable search backend.
#[derive(Clone)]
pub struct SearchBackend {
    pub provider: SearchProvider,
    pub api_key: String,
    pub endpoint: String,
}

impl std::fmt::Debug for SearchBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never let a key reach a log line through a stray `{:?}`.
        f.debug_struct("SearchBackend")
            .field("provider", &self.provider.as_str())
            .field("endpoint", &self.endpoint)
            .field("api_key", &"[REDACTED]")
            .finish()
    }
}

impl SearchBackend {
    pub fn new(provider: SearchProvider, api_key: impl Into<String>) -> Result<Self, String> {
        Self::with_endpoint(provider, api_key, provider.default_endpoint())
    }

    pub fn with_endpoint(
        provider: SearchProvider,
        api_key: impl Into<String>,
        endpoint: impl Into<String>,
    ) -> Result<Self, String> {
        let api_key = api_key.into();
        if api_key.trim().is_empty() {
            return Err("search backend api key is empty".into());
        }
        let endpoint = endpoint.into();
        // Refuse a bad endpoint at configuration time, not on the first query.
        ssrf::validate_fetch_url(&endpoint).map_err(|e| e.message)?;
        Ok(SearchBackend {
            provider,
            api_key,
            endpoint,
        })
    }
}

fn installed() -> &'static Mutex<Option<SearchBackend>> {
    static INSTALLED: OnceLock<Mutex<Option<SearchBackend>>> = OnceLock::new();
    INSTALLED.get_or_init(|| Mutex::new(None))
}

/// Install a backend from the host (e.g. a key decrypted by the credential
/// broker). Takes precedence over the environment.
pub fn install_backend(backend: SearchBackend) {
    if let Ok(mut slot) = installed().lock() {
        *slot = Some(backend);
    }
}

/// Remove any installed backend.
pub fn clear_backend() {
    if let Ok(mut slot) = installed().lock() {
        *slot = None;
    }
}

/// Resolve the active backend: installed first, then environment, then nothing.
///
/// A partially configured environment (provider without key, or key without a
/// recognised provider) resolves to `None` rather than to a broken backend, and
/// [`configuration_error`] explains which half is missing.
pub fn active_backend() -> Option<SearchBackend> {
    if let Some(backend) = installed().lock().ok().and_then(|g| g.clone()) {
        return Some(backend);
    }
    let provider = SearchProvider::parse(&std::env::var(ENV_PROVIDER).ok()?)?;
    let api_key = std::env::var(ENV_API_KEY).ok()?;
    let endpoint = std::env::var(ENV_ENDPOINT)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| provider.default_endpoint().to_string());
    SearchBackend::with_endpoint(provider, api_key, endpoint).ok()
}

/// True when `web_search` should be offered to the model.
pub fn is_configured() -> bool {
    active_backend().is_some()
}

/// The error returned when the tool is called with no backend behind it.
pub fn configuration_error() -> ToolError {
    ToolError {
        code: "search_backend_not_configured".into(),
        message: format!(
            "web_search has no backend. Natives ships no search index, so a real \
             search API must be configured first: set {ENV_PROVIDER} to `brave` or \
             `tavily` and {ENV_API_KEY} to that provider's key (optionally \
             {ENV_ENDPOINT} to override the endpoint). No results are returned \
             without one."
        ),
        retryable: false,
    }
}

/// Strip the API key from any text that might be surfaced or logged.
fn redact(text: &str, api_key: &str) -> String {
    if api_key.is_empty() {
        return text.to_string();
    }
    text.replace(api_key, "[REDACTED_KEY]")
}

pub struct WebSearchTool;

#[async_trait::async_trait]
impl ToolHandler for WebSearchTool {
    async fn execute(
        &self,
        input: serde_json::Value,
        context: &ToolCallContext,
    ) -> Result<ToolOutput, ToolError> {
        let backend = active_backend().ok_or_else(configuration_error)?;
        let query = input
            .get("query")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|q| !q.is_empty())
            .ok_or_else(|| ToolError {
                code: "invalid_input".into(),
                message: "web_search requires a non-empty `query`".into(),
                retryable: false,
            })?;
        let count = input
            .get("count")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_RESULTS)
            .clamp(1, MAX_RESULTS);

        if context.cancel.is_cancelled() {
            return Err(ToolError {
                code: "cancelled".into(),
                message: "run cancelled before web_search".into(),
                retryable: false,
            });
        }

        let started = std::time::Instant::now();
        let body = tokio::select! {
            biased;
            _ = context.cancel.cancelled() => {
                return Err(ToolError {
                    code: "cancelled".into(),
                    message: "run cancelled during web_search".into(),
                    retryable: false,
                });
            }
            res = call_backend(&backend, query, count) => res?,
        };

        let raw = match backend.provider {
            SearchProvider::Brave => parse_brave(&body),
            SearchProvider::Tavily => parse_tavily(&body),
        };
        let total = raw.len();
        // Search results are attacker-influenced and are meant to be fed to
        // web_fetch, so they get the same host checks as a direct fetch.
        let mut results = Vec::new();
        for (idx, hit) in raw.into_iter().enumerate() {
            if ssrf::validate_fetch_url(&hit.url).is_err() {
                continue;
            }
            results.push(serde_json::json!({
                "rank": idx + 1,
                "title": hit.title,
                "url": hit.url,
                "snippet": hit.snippet,
            }));
        }
        let filtered = total - results.len();

        Ok(ToolOutput {
            result: serde_json::json!({
                "query": query,
                "backend": backend.provider.as_str(),
                "results": results,
                "count": results.len(),
                "filtered_unsafe": filtered,
            }),
            truncated: false,
            duration_ms: started.elapsed().as_millis() as u64,
        })
    }
}

async fn call_backend(
    backend: &SearchBackend,
    query: &str,
    count: u64,
) -> Result<serde_json::Value, ToolError> {
    // Re-validate: an installed backend could have been set before DNS changed.
    ssrf::validate_fetch_url(&backend.endpoint)?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        // Search endpoints are fixed API roots; a redirect off them is either a
        // misconfiguration or an attack. Neither is worth following.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| ToolError {
            code: "http_client".into(),
            message: e.to_string(),
            retryable: true,
        })?;

    let request = match backend.provider {
        SearchProvider::Brave => client
            .get(&backend.endpoint)
            .query(&[("q", query), ("count", &count.to_string())])
            .header("Accept", "application/json")
            .header("X-Subscription-Token", &backend.api_key),
        SearchProvider::Tavily => client
            .post(&backend.endpoint)
            .header("Accept", "application/json")
            .bearer_auth(&backend.api_key)
            .json(&serde_json::json!({
                // Current Tavily authenticates with the bearer header; the body
                // field is the documented legacy form. Sending both keeps older
                // self-hosted gateways working.
                "api_key": backend.api_key,
                "query": query,
                "max_results": count,
                "search_depth": "basic",
            })),
    };

    let resp = request.send().await.map_err(|e| ToolError {
        code: "network".into(),
        message: redact(&e.to_string(), &backend.api_key),
        retryable: true,
    })?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| ToolError {
        code: "network".into(),
        message: redact(&e.to_string(), &backend.api_key),
        retryable: true,
    })?;
    if !status.is_success() {
        let detail: String = redact(&text, &backend.api_key).chars().take(300).collect();
        return Err(ToolError {
            code: "search_backend_error".into(),
            message: format!(
                "{} search returned HTTP {}: {detail}",
                backend.provider.as_str(),
                status.as_u16()
            ),
            // 429/5xx are worth another turn; 4xx auth/quota problems are not.
            retryable: status.as_u16() == 429 || status.is_server_error(),
        });
    }
    serde_json::from_str(&text).map_err(|e| ToolError {
        code: "search_backend_error".into(),
        message: format!(
            "{} search returned unparseable JSON: {e}",
            backend.provider.as_str()
        ),
        retryable: false,
    })
}

/// One normalized hit. Kept separate from the JSON shape so both backends land
/// on the same contract before SSRF filtering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

fn clip(text: &str) -> String {
    text.trim().chars().take(MAX_SNIPPET_CHARS).collect()
}

/// Brave: `{"web": {"results": [{"title", "url", "description"}]}}`.
pub fn parse_brave(body: &serde_json::Value) -> Vec<SearchHit> {
    body.pointer("/web/results")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|hit| {
                    let url = hit.get("url").and_then(|v| v.as_str())?;
                    Some(SearchHit {
                        title: clip(hit.get("title").and_then(|v| v.as_str()).unwrap_or("")),
                        url: url.to_string(),
                        snippet: clip(
                            hit.get("description")
                                .or_else(|| hit.get("snippet"))
                                .and_then(|v| v.as_str())
                                .unwrap_or(""),
                        ),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Tavily: `{"results": [{"title", "url", "content"}]}`.
pub fn parse_tavily(body: &serde_json::Value) -> Vec<SearchHit> {
    body.get("results")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|hit| {
                    let url = hit.get("url").and_then(|v| v.as_str())?;
                    Some(SearchHit {
                        title: clip(hit.get("title").and_then(|v| v.as_str()).unwrap_or("")),
                        url: url.to_string(),
                        snippet: clip(
                            hit.get("content")
                                .or_else(|| hit.get("snippet"))
                                .and_then(|v| v.as_str())
                                .unwrap_or(""),
                        ),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The `web_search` registration. Only reachable through
/// [`super::builtin_tools`], which includes it when a backend is configured.
pub fn web_search_tool() -> Tool {
    Tool {
        name: "web_search",
        description:
            "Search the public web and get back titles, URLs and snippets. Use it to find \
                      sources you do not already have a URL for, then read the promising ones with \
                      web_fetch. Results come from a real search API; there is no offline fallback.",
        schema: serde_json::json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Search query"},
                "count": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": MAX_RESULTS,
                    "description": "How many results to return (default 5)"
                }
            },
            "required": ["query"]
        }),
        side_effect: SideEffect::Network,
        permission_class: PermissionClass::ExternalWrite,
        path_scope: PathScope::None,
        timeout_ms: 25_000,
        output_limit: 64_000,
        cancellable: true,
        parallel_safe: false,
        conflict_key: None,
        idempotency: None,
        per_call_resource: None,
        handler: Arc::new(WebSearchTool),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Public IP literal endpoint so constructing a backend does not require a
    /// real DNS lookup (tests must be hermetic; the SSRF validator still runs).
    const TEST_ENDPOINT: &str = "http://8.8.8.8/search";

    /// Serializes the tests that install/clear the process-global backend so
    /// parallel test threads cannot observe each other's intermediate state.
    fn backend_lock() -> &'static tokio::sync::Mutex<()> {
        static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
        LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
    }

    #[test]
    fn provider_parsing_is_case_insensitive_and_closed() {
        assert_eq!(SearchProvider::parse("Brave"), Some(SearchProvider::Brave));
        assert_eq!(
            SearchProvider::parse(" tavily "),
            Some(SearchProvider::Tavily)
        );
        assert_eq!(SearchProvider::parse("google"), None);
        assert_eq!(SearchProvider::parse(""), None);
    }

    #[test]
    fn backend_rejects_empty_key_and_unsafe_endpoint() {
        assert!(SearchBackend::new(SearchProvider::Brave, "  ").is_err());
        assert!(SearchBackend::with_endpoint(
            SearchProvider::Brave,
            "k",
            "http://127.0.0.1/search"
        )
        .is_err());
        assert!(
            SearchBackend::with_endpoint(SearchProvider::Brave, "k", "file:///etc/passwd").is_err()
        );
        assert!(SearchBackend::with_endpoint(SearchProvider::Brave, "k", TEST_ENDPOINT).is_ok());
    }

    #[test]
    fn debug_never_prints_the_key() {
        let backend =
            SearchBackend::with_endpoint(SearchProvider::Tavily, "tvly-supersecret", TEST_ENDPOINT)
                .unwrap();
        let rendered = format!("{backend:?}");
        assert!(!rendered.contains("supersecret"), "{rendered}");
        assert!(rendered.contains("[REDACTED]"));
    }

    #[test]
    fn redact_removes_the_key_from_error_text() {
        let msg = redact("upstream said key=abc123 is invalid", "abc123");
        assert!(!msg.contains("abc123"));
        assert!(msg.contains("[REDACTED_KEY]"));
    }

    #[test]
    fn brave_response_maps_to_hits() {
        let body = serde_json::json!({
            "web": {"results": [
                {"title": "Rust", "url": "https://www.rust-lang.org/", "description": "A language"},
                {"title": "No URL"}
            ]}
        });
        let hits = parse_brave(&body);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].url, "https://www.rust-lang.org/");
        assert_eq!(hits[0].snippet, "A language");
    }

    #[test]
    fn tavily_response_maps_to_hits() {
        let body = serde_json::json!({
            "results": [
                {"title": "Docs", "url": "https://docs.rs/", "content": "Crate docs"}
            ]
        });
        let hits = parse_tavily(&body);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "Docs");
        assert_eq!(hits[0].snippet, "Crate docs");
    }

    #[test]
    fn an_empty_backend_response_yields_no_hits_not_an_invented_one() {
        assert!(parse_brave(&serde_json::json!({})).is_empty());
        assert!(parse_tavily(&serde_json::json!({"results": []})).is_empty());
    }

    #[tokio::test]
    async fn unconfigured_call_is_a_hard_error_not_an_empty_result() {
        let _lock = backend_lock().lock().await;
        clear_backend();
        // Guard against a developer machine that happens to export the vars.
        if std::env::var(ENV_PROVIDER).is_ok() {
            return;
        }
        let ctx = ToolCallContext::new(
            std::env::temp_dir(),
            "run-search".into(),
            "conv".into(),
            "tc".into(),
            "ask".into(),
        );
        let err = WebSearchTool
            .execute(serde_json::json!({"query": "rust"}), &ctx)
            .await
            .unwrap_err();
        assert_eq!(err.code, "search_backend_not_configured");
        assert!(err.message.contains(ENV_PROVIDER));
    }

    #[tokio::test]
    async fn configured_backend_still_rejects_an_empty_query() {
        let _lock = backend_lock().lock().await;
        install_backend(
            SearchBackend::with_endpoint(SearchProvider::Brave, "k", TEST_ENDPOINT).unwrap(),
        );
        let ctx = ToolCallContext::new(
            std::env::temp_dir(),
            "run-search".into(),
            "conv".into(),
            "tc".into(),
            "ask".into(),
        );
        let err = WebSearchTool
            .execute(serde_json::json!({"query": "   "}), &ctx)
            .await
            .unwrap_err();
        assert_eq!(err.code, "invalid_input");
        clear_backend();
    }

    #[tokio::test]
    async fn registration_follows_configuration() {
        let _lock = backend_lock().lock().await;
        clear_backend();
        let env_configured = std::env::var(ENV_PROVIDER).is_ok();
        assert_eq!(is_configured(), env_configured);
        install_backend(
            SearchBackend::with_endpoint(SearchProvider::Tavily, "k", TEST_ENDPOINT).unwrap(),
        );
        assert!(is_configured());
        clear_backend();
    }
}
