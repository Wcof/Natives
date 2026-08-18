//! Public URL fetch tool with SSRF guards.

use super::ssrf;
use crate::{ToolCallContext, ToolError, ToolHandler, ToolOutput};

/// Fetch a URL with DNS/private IP SSRF guards and redirect re-check.
pub struct WebFetchTool;

impl WebFetchTool {
    async fn fetch(
        client: &reqwest::Client,
        url: &str,
        max_bytes: usize,
    ) -> Result<ToolOutput, ToolError> {
        let resp = client.get(url).send().await.map_err(|e| ToolError {
            code: "network".into(),
            message: e.to_string(),
            retryable: true,
        })?;
        // Re-validate final URL after redirects.
        ssrf::validate_fetch_url(resp.url().as_str())?;
        let status = resp.status().as_u16();
        let mut bytes = Vec::with_capacity(max_bytes);
        let mut truncated = false;
        let mut resp = resp;
        while let Some(chunk) = resp.chunk().await.map_err(|e| ToolError {
            code: "network".into(),
            message: e.to_string(),
            retryable: true,
        })? {
            let remaining = max_bytes.saturating_sub(bytes.len());
            if chunk.len() > remaining {
                bytes.extend_from_slice(&chunk[..remaining]);
                truncated = true;
                break;
            }
            bytes.extend_from_slice(&chunk);
        }
        let body = String::from_utf8_lossy(&bytes).into_owned();
        Ok(ToolOutput {
            result: serde_json::json!({
                "status": status,
                "body": body,
                "truncated": truncated,
                "bytes": bytes.len(),
            }),
            truncated,
            duration_ms: 0,
        })
    }
}

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
        Self::fetch(&client, url, max_bytes).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{net::SocketAddr, time::Duration};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    #[tokio::test]
    async fn streamed_response_stops_after_the_bounded_sentinel() {
        const MAX_BYTES: usize = 64;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (keep_open, wait_open) = tokio::sync::oneshot::channel();
        tokio::spawn(server(
            listener,
            "x".repeat(MAX_BYTES + 1).into_bytes(),
            1_048_576,
            Some(wait_open),
        ));
        let client = public_host_client(address);

        let output = tokio::time::timeout(
            Duration::from_millis(500),
            WebFetchTool::fetch(
                &client,
                &format!("http://example.com:{}/", address.port()),
                MAX_BYTES,
            ),
        )
        .await
        .expect("bounded read must not wait for the unfinished response")
        .expect("public hostname mapped to local test server is allowed");
        drop(keep_open);

        assert_eq!(output.result["body"], "x".repeat(MAX_BYTES));
        assert_eq!(output.result["bytes"], MAX_BYTES);
        assert_eq!(output.result["truncated"], true);
        assert!(output.truncated);
    }

    #[tokio::test]
    async fn streamed_response_at_the_limit_is_not_truncated() {
        const MAX_BYTES: usize = 64;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(server(
            listener,
            "x".repeat(MAX_BYTES).into_bytes(),
            MAX_BYTES,
            None,
        ));

        let output = WebFetchTool::fetch(
            &public_host_client(address),
            &format!("http://example.com:{}/", address.port()),
            MAX_BYTES,
        )
        .await
        .expect("complete response is returned unchanged");

        assert_eq!(output.result["body"], "x".repeat(MAX_BYTES));
        assert_eq!(output.result["bytes"], MAX_BYTES);
        assert_eq!(output.result["truncated"], false);
        assert!(!output.truncated);
    }

    fn public_host_client(address: SocketAddr) -> reqwest::Client {
        reqwest::Client::builder()
            .no_proxy()
            .resolve("example.com", address)
            .build()
            .unwrap()
    }

    async fn server(
        listener: TcpListener,
        body: Vec<u8>,
        content_length: usize,
        hold_open: Option<tokio::sync::oneshot::Receiver<()>>,
    ) {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0; 1024];
        socket.read(&mut request).await.unwrap();
        socket
            .write_all(
                format!("HTTP/1.1 200 OK\r\nContent-Length: {content_length}\r\n\r\n").as_bytes(),
            )
            .await
            .unwrap();
        socket.write_all(&body).await.unwrap();
        if let Some(wait) = hold_open {
            let _ = wait.await;
        }
    }
}
