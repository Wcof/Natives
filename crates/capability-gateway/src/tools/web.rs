//! Public URL fetch tool with SSRF guards.

use super::ssrf;
use crate::{ToolCallContext, ToolError, ToolHandler, ToolOutput};

/// Fetch a URL with DNS/private IP SSRF guards and redirect re-check.
pub struct WebFetchTool;
#[async_trait::async_trait]
impl ToolHandler for WebFetchTool {
    async fn execute(
        &self,
        input: serde_json::Value,
        _context: &ToolCallContext,
    ) -> Result<ToolOutput, ToolError> {
        let url = input
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError {
                code: "invalid_input".into(),
                message: "Missing url".into(),
                retryable: false,
            })?;
        ssrf::validate_fetch_url(url)?;
        let max_bytes = input
            .get("max_bytes")
            .and_then(|v| v.as_u64())
            .unwrap_or(64_000)
            .min(512_000) as usize;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                let next = attempt.url().as_str();
                if ssrf::validate_fetch_url(next).is_err() {
                    attempt.error(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "redirect blocked by SSRF policy",
                    ))
                } else if attempt.previous().len() > 5 {
                    attempt.stop()
                } else {
                    attempt.follow()
                }
            }))
            .build()
            .map_err(|e| ToolError {
                code: "http_client".into(),
                message: e.to_string(),
                retryable: true,
            })?;
        let resp = client.get(url).send().await.map_err(|e| ToolError {
            code: "network".into(),
            message: e.to_string(),
            retryable: true,
        })?;
        // Re-validate final URL after redirects.
        ssrf::validate_fetch_url(resp.url().as_str())?;
        let status = resp.status().as_u16();
        let bytes = resp.bytes().await.map_err(|e| ToolError {
            code: "network".into(),
            message: e.to_string(),
            retryable: true,
        })?;
        let truncated = bytes.len() > max_bytes;
        let slice = if truncated {
            &bytes[..max_bytes]
        } else {
            &bytes
        };
        let body = String::from_utf8_lossy(slice).into_owned();
        Ok(ToolOutput {
            result: serde_json::json!({
                "status": status,
                "body": body,
                "truncated": truncated,
                "bytes": slice.len(),
            }),
            truncated,
            duration_ms: 0,
        })
    }
}
