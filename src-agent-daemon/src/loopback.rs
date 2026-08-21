//! Authenticated localhost compatibility API backed by the daemon route provider.

mod completion_encoder;
mod protocol;
mod stream_encoder;

use agent_core::{EngineProvider, EngineProviderEvent};
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::collections::HashMap;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::natives_db_broker::{LoopbackSettings, NativesDbBroker};
use completion_encoder::CompletionEncoder;
use protocol::parse_request;
use stream_encoder::StreamEncoder;

const MAX_BODY: usize = 32 * 1024 * 1024;

pub fn spawn_supervisor() {
    tokio::spawn(async {
        let mut active: Option<(u16, tokio::task::JoinHandle<()>)> = None;
        loop {
            let settings = read_settings().unwrap_or_else(|_| disabled_settings());
            let wanted = settings.enabled
                && settings
                    .bearer_token
                    .as_deref()
                    .is_some_and(|v| !v.is_empty());
            match (&active, wanted) {
                (Some((port, _)), true) if *port == settings.port => {}
                (Some((_, task)), _) => {
                    task.abort();
                    active = None;
                }
                _ => {}
            }
            if wanted && active.is_none() {
                let port = settings.port;
                let task = tokio::spawn(async move {
                    if let Err(error) = serve(port).await {
                        eprintln!("[loopback] service stopped: {error}");
                    }
                });
                active = Some((port, task));
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    });
}

async fn serve(port: u16) -> Result<(), String> {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
        .await
        .map_err(|e| format!("bind 127.0.0.1:{port}: {e}"))?;
    loop {
        let (stream, _) = listener.accept().await.map_err(|e| e.to_string())?;
        tokio::spawn(async move {
            let _ = handle_connection(stream).await;
        });
    }
}

async fn handle_connection(mut stream: TcpStream) -> Result<(), String> {
    let request = read_request(&mut stream).await?;
    let settings = read_settings()?;
    if !settings.enabled
        || !authorized(
            request.headers.get("authorization"),
            settings.bearer_token.as_deref(),
        )
    {
        return write_response(
            &mut stream,
            401,
            "application/json",
            br#"{"error":{"message":"Unauthorized","type":"authentication_error"}}"#,
        )
        .await;
    }
    if request.method == "GET" && request.path == "/health" {
        return write_response(&mut stream, 200, "application/json", br#"{"status":"ok"}"#).await;
    }
    if request.method == "GET" && request.path == "/v1/models" {
        let models = route_models()?;
        return write_response(
            &mut stream,
            200,
            "application/json",
            serde_json::to_string(&json!({"object":"list","data":models}))
                .unwrap()
                .as_bytes(),
        )
        .await;
    }
    if request.method != "POST"
        || !matches!(
            request.path.as_str(),
            "/v1/chat/completions" | "/v1/responses" | "/v1/messages"
        )
    {
        return write_response(
            &mut stream,
            404,
            "application/json",
            br#"{"error":{"message":"Not found"}}"#,
        )
        .await;
    }
    if request.body.len() > MAX_BODY {
        return write_response(
            &mut stream,
            413,
            "application/json",
            br#"{"error":{"message":"Request too large"}}"#,
        )
        .await;
    }
    let body: Value =
        serde_json::from_slice(&request.body).map_err(|_| "invalid JSON request".to_string())?;
    let body = crate::request_rectifier::rectify(
        body,
        settings
            .rectifier
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    );
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| "model is required".to_string())?
        .to_string();
    if !route_models()?.iter().any(|entry| entry["id"] == model) {
        return write_response(
            &mut stream,
            404,
            "application/json",
            br#"{"error":{"message":"Model is not bound to local routing"}}"#,
        )
        .await;
    }
    let parsed = parse_request(&request.path, &body)?;
    let stream_requested = body.get("stream").and_then(Value::as_bool).unwrap_or(false);
    let provider = local_provider(&model)?.with_controls(parsed.controls);
    let cancellation = tokio_util::sync::CancellationToken::new();
    let events = provider
        .stream(
            &model,
            parsed.messages,
            &parsed.tools,
            parsed.system.as_deref(),
            cancellation,
        )
        .await
        .map_err(|e| e.to_string())?;
    if stream_requested {
        write_stream(&mut stream, &request.path, events).await
    } else {
        tokio::time::timeout(
            std::time::Duration::from_secs(600),
            write_completion(&mut stream, &request.path, &model, events),
        )
        .await
        .map_err(|_| "provider response timed out".to_string())?
    }
}

struct HttpRequest {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

async fn read_request(stream: &mut TcpStream) -> Result<HttpRequest, String> {
    let mut bytes = Vec::with_capacity(8192);
    let mut chunk = [0_u8; 8192];
    let header_end = loop {
        let read =
            tokio::time::timeout(std::time::Duration::from_secs(15), stream.read(&mut chunk))
                .await
                .map_err(|_| "request timeout".to_string())?
                .map_err(|e| e.to_string())?;
        if read == 0 {
            return Err("connection closed".into());
        }
        bytes.extend_from_slice(&chunk[..read]);
        if bytes.len() > MAX_BODY + 16 * 1024 {
            return Err("request too large".into());
        }
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let header =
        std::str::from_utf8(&bytes[..header_end]).map_err(|_| "invalid headers".to_string())?;
    let mut lines = header.split("\r\n");
    let start = lines.next().ok_or("missing request line")?;
    let mut start = start.split_whitespace();
    let method = start.next().ok_or("missing method")?.to_string();
    let path = start
        .next()
        .ok_or("missing path")?
        .split('?')
        .next()
        .unwrap_or("/")
        .to_string();
    let mut headers = HashMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    let content_length = headers
        .get("content-length")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    if content_length > MAX_BODY {
        return Err("request too large".into());
    }
    while bytes.len() - header_end < content_length {
        let read = stream.read(&mut chunk).await.map_err(|e| e.to_string())?;
        if read == 0 {
            return Err("incomplete body".into());
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
    Ok(HttpRequest {
        method,
        path,
        headers,
        body: bytes[header_end..header_end + content_length].to_vec(),
    })
}

fn authorized(header: Option<&String>, token: Option<&str>) -> bool {
    let Some(token) = token.filter(|v| !v.is_empty()) else {
        return false;
    };
    let Some(value) = header.and_then(|v| v.strip_prefix("Bearer ")) else {
        return false;
    };
    // Equal-length comparison avoids leaking a valid prefix over localhost.
    value.len() == token.len()
        && value
            .as_bytes()
            .iter()
            .zip(token.as_bytes())
            .fold(0_u8, |diff, (a, b)| diff | (a ^ b))
            == 0
}

fn read_settings() -> Result<LoopbackSettings, String> {
    NativesDbBroker::open_default()?.loopback_settings()
}
fn disabled_settings() -> LoopbackSettings {
    LoopbackSettings {
        enabled: false,
        port: 15721,
        bearer_token: None,
        rectifier: json!({}),
    }
}

fn route_models() -> Result<Vec<Value>, String> {
    // T104 / W1: route bindings are Host-owned (natives.db); fetch them via the
    // broker lease so the daemon never opens natives.db.
    let plan = NativesDbBroker::open_default()?.routing_plan("loopback")?;
    let mut models: Vec<Value> = plan
        .targets
        .into_iter()
        .map(|t| t.model_id)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .map(|id| json!({"id": id, "object": "model", "owned_by": "natives"}))
        .collect();
    models.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    Ok(models)
}

fn local_provider(model: &str) -> Result<crate::routing::RoutedProvider, String> {
    // T104 / W1: route bindings are Host-owned (natives.db); fetch them via the
    // broker lease so the daemon never opens natives.db.
    let plan = NativesDbBroker::open_default()?.routing_plan("loopback")?;
    let targets: Vec<crate::routing::RouteTarget> = plan
        .targets
        .into_iter()
        .filter(|t| t.model_id == model)
        .map(|t| crate::routing::RouteTarget {
            provider_id: t.provider_id,
            credential_kind: t.credential_kind,
            credential_id: t.credential_id,
            model_id: t.model_id,
        })
        .collect();
    if targets.is_empty() {
        return Err("model is not bound to local routing".into());
    }
    Ok(crate::routing::RoutedProvider::new(
        crate::routing::RoutingPlan {
            enabled: true,
            targets,
        },
    ))
}

async fn write_response(
    stream: &mut TcpStream,
    code: u16,
    content_type: &str,
    body: &[u8],
) -> Result<(), String> {
    let status = match code {
        200 => "OK",
        401 => "Unauthorized",
        404 => "Not Found",
        413 => "Payload Too Large",
        _ => "Bad Request",
    };
    stream.write_all(format!("HTTP/1.1 {code} {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).as_bytes()).await.map_err(|e| e.to_string())?;
    stream.write_all(body).await.map_err(|e| e.to_string())
}

async fn write_stream(
    stream: &mut TcpStream,
    path: &str,
    mut events: agent_core::EngineProviderEventStream,
) -> Result<(), String> {
    stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n").await.map_err(|e| e.to_string())?;
    let mut encoder = StreamEncoder::new(path)?;
    while let Some(event) = events.next().await {
        let frame = encoder.encode(event)?;
        if !frame.is_empty() {
            stream
                .write_all(frame.as_bytes())
                .await
                .map_err(|e| e.to_string())?;
        }
        if encoder.is_terminal() {
            break;
        }
    }
    if !encoder.is_terminal() {
        return Err("provider stream ended without a terminal event".into());
    }
    if let Some(done) = encoder.chat_done_frame() {
        stream
            .write_all(done.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

async fn write_completion(
    stream: &mut TcpStream,
    path: &str,
    model: &str,
    mut events: agent_core::EngineProviderEventStream,
) -> Result<(), String> {
    let mut encoder = CompletionEncoder::new(path, model)?;
    while let Some(event) = events.next().await {
        if let EngineProviderEvent::Error { message, .. } = &event {
            return write_response(
                stream,
                400,
                "application/json",
                serde_json::to_string(&json!({"error":{"message":message}}))
                    .unwrap()
                    .as_bytes(),
            )
            .await;
        }
        encoder.push(event)?;
        if encoder.is_terminal() {
            break;
        }
    }
    let payload = encoder.finish()?;
    write_response(
        stream,
        200,
        "application/json",
        serde_json::to_string(&payload).unwrap().as_bytes(),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_auth_requires_exact_token() {
        assert!(authorized(Some(&"Bearer abc".into()), Some("abc")));
        assert!(!authorized(Some(&"Bearer ab".into()), Some("abc")));
        assert!(!authorized(None, Some("abc")));
    }
}
