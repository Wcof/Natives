//! Per-request HTTP client construction. Proxy credentials are never logged.

use crate::capabilities::{ProviderError, ProviderErrorCategory};

pub fn client(proxy_url: Option<&str>) -> Result<reqwest::Client, ProviderError> {
    let mut builder = reqwest::Client::builder();
    if let Some(proxy_url) = proxy_url.filter(|value| !value.trim().is_empty()) {
        let proxy = reqwest::Proxy::all(proxy_url).map_err(|_| ProviderError {
            code: "invalid_proxy".into(),
            message: "outbound proxy URL is invalid".into(),
            category: ProviderErrorCategory::BadRequest,
            retryable: false,
            retry_after_ms: None,
        })?;
        builder = builder.proxy(proxy);
    }
    builder.build().map_err(|_| ProviderError {
        code: "proxy_client".into(),
        message: "failed to create outbound HTTP client".into(),
        category: ProviderErrorCategory::Network,
        retryable: true,
        retry_after_ms: None,
    })
}
