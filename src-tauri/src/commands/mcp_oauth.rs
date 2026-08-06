//! MCP OAuth browser flow, Host side (ADR-0016 decision 7).
//!
//! `mcp_oauth_start` runs the full authorization-code + PKCE (S256) dance:
//! 1. bind a one-shot loopback listener on 127.0.0.1;
//! 2. open the authorize URL in the system browser (`open::that`, project norm);
//! 3. receive the redirect on /callback, verify `state`;
//! 4. exchange code + verifier at the token endpoint (reqwest);
//! 5. inject the access token into the daemon via RPC `mcp.auth.set`;
//! 6. persist the refresh token (if any) encrypted in `capability_secrets`.
//!
//! Security invariants: tokens, codes and verifiers never appear in logs or in
//! any returned value/error string. The listener answers exactly one callback
//! and shuts down; total flow timeout is 180 seconds.

use crate::{AppState, Error, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};
use tauri::State;

const FLOW_TIMEOUT: Duration = Duration::from_secs(180);
const TOKEN_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

// ── Data types ──

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpOauthStartInput {
    pub server_id: String,
    pub authorize_url: String,
    pub token_url: String,
    pub client_id: String,
    pub scopes: Option<Vec<String>>,
    pub redirect_port: Option<u16>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpOauthStartResult {
    pub ok: bool,
    pub has_refresh: bool,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
}

// ── Command ──

#[tauri::command]
pub async fn mcp_oauth_start(
    input: McpOauthStartInput,
    state: State<'_, AppState>,
) -> Result<McpOauthStartResult> {
    let server_id = input.server_id.trim().to_string();
    if server_id.is_empty() {
        return Err(Error::Internal("serverId is required".into()));
    }
    let client_id = input.client_id.trim().to_string();
    if client_id.is_empty() {
        return Err(Error::Internal("clientId is required".into()));
    }
    validate_endpoint_url(&input.authorize_url, "authorizeUrl")?;
    validate_endpoint_url(&input.token_url, "tokenUrl")?;

    // PKCE S256 + CSRF state.
    let code_verifier = random_urlsafe_token();
    let code_challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(code_verifier.as_bytes()));
    let oauth_state = random_urlsafe_token();

    // One-shot loopback listener; bind before opening the browser.
    let listener = TcpListener::bind(("127.0.0.1", input.redirect_port.unwrap_or(0)))
        .map_err(|e| Error::Internal(format!("failed to bind loopback listener: {e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| Error::Internal(format!("failed to read loopback port: {e}")))?
        .port();
    listener
        .set_nonblocking(true)
        .map_err(|e| Error::Internal(format!("failed to configure loopback listener: {e}")))?;
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");

    let mut authorize = String::from(input.authorize_url.trim());
    authorize.push(if authorize.contains('?') { '&' } else { '?' });
    authorize.push_str(&format!(
        "response_type=code&client_id={}&redirect_uri={}&state={}&code_challenge={}&code_challenge_method=S256",
        percent_encode(&client_id),
        percent_encode(&redirect_uri),
        percent_encode(&oauth_state),
        percent_encode(&code_challenge),
    ));
    let scope = input
        .scopes
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if !scope.is_empty() {
        authorize.push_str("&scope=");
        authorize.push_str(&percent_encode(&scope));
    }

    open::that(&authorize)
        .map_err(|e| Error::Internal(format!("failed to open system browser: {e}")))?;

    // Wait for the browser redirect (blocking I/O off the async runtime).
    let expected_state = oauth_state.clone();
    let code = tauri::async_runtime::spawn_blocking(move || {
        wait_for_callback(&listener, &expected_state, FLOW_TIMEOUT)
    })
    .await
    .map_err(|e| Error::Internal(format!("OAuth callback task failed: {e}")))?
    .map_err(Error::Internal)?;

    // Exchange code + verifier for tokens.
    let client = reqwest::Client::builder()
        .timeout(TOKEN_REQUEST_TIMEOUT)
        .build()
        .map_err(|e| Error::Internal(format!("failed to build HTTP client: {e}")))?;
    let response = client
        .post(input.token_url.trim())
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
            ("client_id", client_id.as_str()),
            ("code_verifier", code_verifier.as_str()),
        ])
        .send()
        .await
        .map_err(|e| {
            Error::Internal(format!(
                "token request failed: {}",
                sanitize_reqwest_error(&e)
            ))
        })?;
    let status = response.status();
    if !status.is_success() {
        // Body deliberately dropped: it may echo the authorization code.
        return Err(Error::Internal(format!(
            "token endpoint returned HTTP {status}"
        )));
    }
    let token: TokenResponse = response
        .json()
        .await
        .map_err(|_| Error::Internal("token endpoint returned an invalid JSON body".into()))?;
    if token.access_token.trim().is_empty() {
        return Err(Error::Internal(
            "token endpoint returned an empty access_token".into(),
        ));
    }
    // Daemon rpc.rs reads `expires_at` with `as_u64()` — epoch seconds, not RFC3339.
    let expires_at = token.expires_in.filter(|s| *s > 0).and_then(|s| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|now| now.as_secs().saturating_add(s as u64))
    });

    // Inject the access token into the daemon (memory-only McpCredentialStore).
    let mut params = serde_json::json!({
        "server_id": server_id,
        "token": token.access_token,
        "token_type": "oauth_access",
    });
    if let Some(expires_at) = expires_at {
        params["expires_at"] = serde_json::Value::from(expires_at);
    }
    crate::daemon_authority::request("mcp.auth.set", params)
        .await
        .map_err(|e| Error::Internal(format!("mcp.auth.set failed: {e}")))?;

    // Persist the refresh token encrypted in capability_secrets (Host natives.db).
    let refresh = token
        .refresh_token
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty());
    let has_refresh = refresh.is_some();
    if let Some(refresh) = refresh {
        let pool_conn = state
            .db
            .get()
            .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
        super::capability_secret::upsert_capability_secret(
            &pool_conn,
            "mcp_oauth_refresh",
            &server_id,
            None,
            refresh,
        )?;
    }

    Ok(McpOauthStartResult {
        ok: true,
        has_refresh,
    })
}

// ── Loopback callback ──

/// Accept connections until the /callback redirect arrives or `timeout` expires.
/// Returns the authorization code. Non-callback paths (e.g. /favicon.ico) get a
/// 404 and the loop continues.
fn wait_for_callback(
    listener: &TcpListener,
    expected_state: &str,
    timeout: Duration,
) -> std::result::Result<String, String> {
    let deadline = Instant::now() + timeout;
    loop {
        if Instant::now() >= deadline {
            return Err("OAuth flow timed out after 180s waiting for the browser redirect".into());
        }
        let (mut stream, _addr) = match listener.accept() {
            Ok(pair) => pair,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            Err(e) => return Err(format!("loopback accept failed: {e}")),
        };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));

        let mut buf = Vec::with_capacity(2048);
        let mut chunk = [0u8; 1024];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    buf.extend_from_slice(&chunk[..n]);
                    if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.len() > 16 * 1024 {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let request = String::from_utf8_lossy(&buf);
        let Some(path) = request_path(&request) else {
            respond(&mut stream, 400, "Bad request.");
            continue;
        };
        if !path.starts_with("/callback") {
            respond(&mut stream, 404, "Not found.");
            continue;
        }

        let query = path.split_once('?').map(|x| x.1).unwrap_or("");
        let params = parse_query(query);
        let get = |key: &str| {
            params
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.as_str())
        };

        if let Some(_error) = get("error") {
            respond(
                &mut stream,
                200,
                "Authorization was denied or failed. You can close this window.",
            );
            // Provider error codes are public protocol values, still kept out of
            // the message to avoid echoing anything attacker-controlled.
            return Err("authorization was denied by the provider".into());
        }
        if get("state") != Some(expected_state) {
            respond(
                &mut stream,
                400,
                "State mismatch. You can close this window.",
            );
            return Err("OAuth state mismatch — possible CSRF, flow aborted".into());
        }
        match get("code").map(str::trim).filter(|c| !c.is_empty()) {
            Some(code) => {
                respond(
                    &mut stream,
                    200,
                    "Authorization complete. You can close this window and return to Natives.",
                );
                return Ok(code.to_string());
            }
            None => {
                respond(&mut stream, 400, "Missing authorization code.");
                return Err("callback carried no authorization code".into());
            }
        }
    }
}

fn request_path(request: &str) -> Option<&str> {
    let line = request.lines().next()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?;
    if method != "GET" {
        return None;
    }
    parts.next()
}

fn respond(stream: &mut std::net::TcpStream, status: u16, body: &str) {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        _ => "Not Found",
    };
    let html = format!(
        "<!doctype html><html><body style=\"font-family:system-ui;margin:3rem\"><p>{body}</p></body></html>"
    );
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}",
        html.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

// ── Small helpers ──

fn validate_endpoint_url(url: &str, field: &str) -> Result<()> {
    let url = url.trim();
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(Error::Internal(format!("{field} must be an http(s) URL")));
    }
    Ok(())
}

/// 32 random bytes, base64url without padding (43 chars — valid PKCE verifier).
fn random_urlsafe_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len() * 3);
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    out.push(byte);
                    i += 3;
                } else {
                    out.push(b'%');
                    i += 1;
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_query(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let mut kv = pair.splitn(2, '=');
            (
                percent_decode(kv.next().unwrap_or("")),
                percent_decode(kv.next().unwrap_or("")),
            )
        })
        .collect()
}

/// reqwest errors can embed the full request URL; keep only the error class.
fn sanitize_reqwest_error(e: &reqwest::Error) -> &'static str {
    if e.is_timeout() {
        "timeout"
    } else if e.is_connect() {
        "connection error"
    } else {
        "request error"
    }
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_matches_rfc7636_vector() {
        // RFC 7636 appendix B reference vector.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        assert_eq!(challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn pkce_verifier_has_valid_length_and_charset() {
        let verifier = random_urlsafe_token();
        assert!((43..=128).contains(&verifier.len()));
        assert!(verifier
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn callback_query_parsing_decodes_params() {
        let params = parse_query("code=abc%2F123&state=xyz&scope=a+b");
        let get = |key: &str| {
            params
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(get("code"), Some("abc/123"));
        assert_eq!(get("state"), Some("xyz"));
        assert_eq!(get("scope"), Some("a b"));
    }

    #[test]
    fn percent_encode_covers_reserved_characters() {
        assert_eq!(
            percent_encode("http://127.0.0.1:7777/callback"),
            "http%3A%2F%2F127.0.0.1%3A7777%2Fcallback"
        );
        assert_eq!(percent_encode("a b&c"), "a%20b%26c");
    }

    #[test]
    fn request_path_extracts_get_target() {
        assert_eq!(
            request_path("GET /callback?code=x HTTP/1.1\r\nHost: x\r\n\r\n"),
            Some("/callback?code=x")
        );
        assert_eq!(request_path("POST /callback HTTP/1.1\r\n\r\n"), None);
    }
}
