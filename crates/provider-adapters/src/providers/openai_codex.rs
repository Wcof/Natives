//! ChatGPT Codex Responses transport for imported Sub2API OpenAI OAuth accounts.
//!
//! It deliberately reuses the standard Responses body and SSE parser. Only the
//! OAuth URL and required ChatGPT identity headers live here.

use crate::capabilities::{ProviderError, ProviderErrorCategory, ProviderRequest};
use crate::http_stream::stream_responses_with_headers;
use crate::stream::ProviderEvent;
use reqwest::{
    header::{HeaderMap, HeaderValue, AUTHORIZATION},
    Client,
};

pub const CODEX_RESPONSES_URL: &str = "https://chatgpt.com/backend-api/codex/responses";
pub const CODEX_CLIENT_VERSION: &str = "0.144.1";
pub const CODEX_USER_AGENT: &str = "codex_cli_rs/0.144.1 (Ubuntu 22.4.0; x86_64) xterm-256color";

/// Memory-only OAuth material. Callers must never persist or log this value.
#[derive(Debug, Clone)]
pub struct OpenAiCodexCredential {
    pub access_token: String,
    pub chatgpt_account_id: Option<String>,
    pub fedramp: bool,
}

pub async fn stream(
    client: &Client,
    request: ProviderRequest,
    credential: OpenAiCodexCredential,
) -> Result<std::pin::Pin<Box<dyn futures_util::Stream<Item = ProviderEvent> + Send>>, ProviderError>
{
    let token = credential.access_token.trim();
    if token.is_empty() {
        return Err(auth_error("OpenAI OAuth access token is required"));
    }
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, header_value(&format!("Bearer {token}"))?);
    headers.insert("host", HeaderValue::from_static("chatgpt.com"));
    headers.insert(
        "openai-beta",
        HeaderValue::from_static("responses=experimental"),
    );
    headers.insert("originator", HeaderValue::from_static("codex_cli_rs"));
    headers.insert("version", HeaderValue::from_static(CODEX_CLIENT_VERSION));
    headers.insert("user-agent", HeaderValue::from_static(CODEX_USER_AGENT));
    if let Some(account_id) = credential
        .chatgpt_account_id
        .filter(|value| !value.trim().is_empty())
    {
        headers.insert("chatgpt-account-id", header_value(&account_id)?);
    }
    if credential.fedramp {
        headers.insert("x-openai-fedramp", HeaderValue::from_static("true"));
    }
    stream_responses_with_headers(client, CODEX_RESPONSES_URL, headers, request).await
}

fn header_value(value: &str) -> Result<HeaderValue, ProviderError> {
    HeaderValue::from_str(value)
        .map_err(|_| auth_error("credential contains invalid header characters"))
}

fn auth_error(message: &str) -> ProviderError {
    ProviderError {
        code: "invalid_credential".into(),
        message: message.into(),
        category: ProviderErrorCategory::Auth,
        retryable: false,
        retry_after_ms: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_constants_are_a_complete_identity_pair() {
        assert!(CODEX_RESPONSES_URL.starts_with("https://chatgpt.com/"));
        assert!(CODEX_USER_AGENT.starts_with("codex_cli_rs/"));
        assert!(!CODEX_CLIENT_VERSION.is_empty());
    }
}
