//! Authenticated localhost compatibility API backed by the daemon route provider.

use agent_core::{EngineImage, EngineMessage, EngineProvider, EngineProviderEvent, EngineToolCall};
use futures_util::StreamExt;
use rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::HashMap;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::natives_db_broker::{LoopbackSettings, NativesDbBroker};

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
    let messages = protocol_messages(&request.path, &body)?;
    let stream_requested = body.get("stream").and_then(Value::as_bool).unwrap_or(false);
    let provider = local_provider(&model)?;
    let cancellation = tokio_util::sync::CancellationToken::new();
    let events = provider
        .stream(
            &model,
            messages,
            &[],
            body.get("system").and_then(Value::as_str),
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

fn protocol_messages(path: &str, body: &Value) -> Result<Vec<EngineMessage>, String> {
    let source = if path == "/v1/responses" {
        body.get("input").cloned().unwrap_or(Value::Array(vec![]))
    } else {
        body.get("messages")
            .cloned()
            .unwrap_or(Value::Array(vec![]))
    };
    match source {
        Value::String(text) => Ok(vec![message("user", text)]),
        Value::Array(items) => items
            .into_iter()
            .map(|item| {
                let role = item.get("role").and_then(Value::as_str).unwrap_or("user");
                let content = item.get("content").unwrap_or(&item);
                let text = content_text(content);
                let images = content_images(content);
                let tool_calls = item
                    .get("tool_calls")
                    .and_then(Value::as_array)
                    .map(|calls| {
                        calls
                            .iter()
                            .filter_map(|call| {
                                let function = call.get("function").unwrap_or(call);
                                Some(EngineToolCall {
                                    id: call.get("id")?.as_str()?.to_string(),
                                    name: function.get("name")?.as_str()?.to_string(),
                                    arguments: function
                                        .get("arguments")
                                        .map(|value| {
                                            if let Some(text) = value.as_str() {
                                                text.to_string()
                                            } else {
                                                value.to_string()
                                            }
                                        })
                                        .unwrap_or_else(|| "{}".into()),
                                })
                            })
                            .collect::<Vec<_>>()
                    })
                    .filter(|calls| !calls.is_empty());
                let tool_call_id = item
                    .get("tool_call_id")
                    .or_else(|| item.get("tool_use_id"))
                    .and_then(Value::as_str)
                    .map(str::to_string);
                Ok(EngineMessage {
                    role: role.to_string(),
                    content: text,
                    tool_call_id,
                    tool_name: item.get("name").and_then(Value::as_str).map(str::to_string),
                    tool_calls,
                    images,
                })
            })
            .collect(),
        _ => Err("messages/input must be a string or array".into()),
    }
}

fn message(role: &str, content: String) -> EngineMessage {
    EngineMessage::text(role, content)
}
fn content_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| {
                part.get("text")
                    .or_else(|| part.get("content"))
                    .and_then(Value::as_str)
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(object) => object
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        _ => String::new(),
    }
}

/// Pull image parts out of an OpenAI-shaped `content` array.
///
/// Two spellings reach this ingress and both are accepted:
/// - chat completions — `{"type":"image_url","image_url":{"url":…,"detail":…}}`
/// - Responses API — `{"type":"input_image","image_url":"…"}`
///
/// A part whose URL is missing or empty is skipped rather than turned into an
/// empty [`EngineImage`]: an image the adapters cannot encode would be reported
/// to the model as a degraded note, which would be a lie about what the caller
/// actually sent.
fn content_images(value: &Value) -> Vec<EngineImage> {
    let Value::Array(parts) = value else {
        return Vec::new();
    };
    parts
        .iter()
        .filter_map(|part| {
            let kind = part.get("type").and_then(Value::as_str).unwrap_or("");
            if kind != "image_url" && kind != "input_image" {
                return None;
            }
            let source = part.get("image_url")?;
            // Chat completions nests `{url, detail}`; Responses passes a bare string.
            let (url, detail) = match source {
                Value::String(url) => (url.as_str(), None),
                other => (
                    other.get("url").and_then(Value::as_str)?,
                    other.get("detail").and_then(Value::as_str),
                ),
            };
            if url.trim().is_empty() {
                return None;
            }
            Some(EngineImage {
                url: url.to_string(),
                // `data:` URIs carry their own MIME type; `media_type` exists for
                // references that do not, and this ingress has no other source
                // for it, so leaving it None is honest rather than guessed.
                media_type: part
                    .get("media_type")
                    .or_else(|| part.get("mime_type"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                detail: detail.map(str::to_string),
            })
        })
        .collect()
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
    while let Some(event) = events.next().await {
        let frame = event_frame(path, event)?;
        stream
            .write_all(frame.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
    }
    if path == "/v1/chat/completions" {
        stream
            .write_all(b"data: [DONE]\n\n")
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
    let mut content = String::new();
    let mut tool_calls: std::collections::BTreeMap<usize, (String, String, String)> =
        std::collections::BTreeMap::new();
    while let Some(event) = events.next().await {
        match event {
            EngineProviderEvent::TextDelta(delta) | EngineProviderEvent::ReasoningDelta(delta) => {
                content.push_str(&delta)
            }
            EngineProviderEvent::Error { message, .. } => {
                return write_response(
                    stream,
                    400,
                    "application/json",
                    serde_json::to_string(&json!({"error":{"message":message}}))
                        .unwrap()
                        .as_bytes(),
                )
                .await
            }
            EngineProviderEvent::ToolCallDelta {
                index,
                id,
                name,
                arguments_delta,
            } => {
                let entry = tool_calls.entry(index).or_default();
                if let Some(id) = id {
                    entry.0 = id;
                }
                if let Some(name) = name {
                    entry.1 = name;
                }
                entry.2.push_str(&arguments_delta);
            }
            _ => {}
        }
    }
    let openai_tools: Vec<Value> = tool_calls.values().map(|(id,name,args)| json!({"id":id,"type":"function","function":{"name":name,"arguments":args}})).collect();
    let payload = if path == "/v1/messages" {
        let blocks: Vec<Value> = if tool_calls.is_empty() {
            vec![json!({"type":"text","text":content})]
        } else {
            tool_calls.values().map(|(id,name,args)| json!({"type":"tool_use","id":id,"name":name,"input":serde_json::from_str::<Value>(args).unwrap_or_else(|_| json!({}))})).collect()
        };
        json!({"id":"msg_local","type":"message","role":"assistant","model":model,"content":blocks,"stop_reason":if tool_calls.is_empty() { "end_turn" } else { "tool_use" }})
    } else if path == "/v1/responses" {
        let output = if tool_calls.is_empty() {
            vec![
                json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":content}]}),
            ]
        } else {
            tool_calls.values().map(|(id,name,args)| json!({"type":"function_call","call_id":id,"name":name,"arguments":args})).collect()
        };
        json!({"id":"resp_local","object":"response","model":model,"output":output})
    } else {
        json!({"id":"chatcmpl-local","object":"chat.completion","model":model,"choices":[{"index":0,"message":{"role":"assistant","content":content,"tool_calls":openai_tools},"finish_reason":"stop"}]})
    };
    write_response(
        stream,
        200,
        "application/json",
        serde_json::to_string(&payload).unwrap().as_bytes(),
    )
    .await
}

fn event_frame(path: &str, event: EngineProviderEvent) -> Result<String, String> {
    match (path, event) {
        (_, EngineProviderEvent::TextDelta(delta)) if path == "/v1/messages" => Ok(format!(
            "event: content_block_delta\ndata: {}\n\n",
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":delta}})
        )),
        (_, EngineProviderEvent::TextDelta(delta)) if path == "/v1/responses" => Ok(format!(
            "event: response.output_text.delta\ndata: {}\n\n",
            json!({"type":"response.output_text.delta","delta":delta})
        )),
        (_, EngineProviderEvent::TextDelta(delta)) => Ok(format!(
            "data: {}\n\n",
            json!({"id":"chatcmpl-local","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":delta},"finish_reason":null}]})
        )),
        (_, EngineProviderEvent::ReasoningDelta(delta)) => Ok(format!(
            "data: {}\n\n",
            json!({"type":"response.reasoning.delta","delta":delta})
        )),
        (
            _,
            EngineProviderEvent::ToolCallDelta {
                index,
                id,
                name,
                arguments_delta,
            },
        ) if path == "/v1/chat/completions" => Ok(format!(
            "data: {}\n\n",
            json!({"id":"chatcmpl-local","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"tool_calls":[{"index":index,"id":id,"type":"function","function":{"name":name,"arguments":arguments_delta}}]}}]})
        )),
        (
            _,
            EngineProviderEvent::ToolCallDelta {
                index,
                id,
                name,
                arguments_delta,
            },
        ) if path == "/v1/messages" => Ok(format!(
            "event: content_block_delta\ndata: {}\n\n",
            json!({"type":"content_block_delta","index":index,"delta":{"type":"input_json_delta","partial_json":arguments_delta},"id":id,"name":name})
        )),
        (
            _,
            EngineProviderEvent::ToolCallDelta {
                index,
                id,
                name,
                arguments_delta,
            },
        ) if path == "/v1/responses" => Ok(format!(
            "event: response.function_call_arguments.delta\ndata: {}\n\n",
            json!({"type":"response.function_call_arguments.delta","output_index":index,"call_id":id,"name":name,"delta":arguments_delta})
        )),
        (_, EngineProviderEvent::Completed | EngineProviderEvent::CompletedWithReason { .. })
            if path == "/v1/messages" =>
        {
            Ok("event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".into())
        }
        (_, EngineProviderEvent::Completed | EngineProviderEvent::CompletedWithReason { .. })
            if path == "/v1/responses" =>
        {
            Ok("event: response.completed\ndata: {\"type\":\"response.completed\"}\n\n".into())
        }
        (_, EngineProviderEvent::Error { message, .. }) => Ok(format!(
            "data: {}\n\n",
            json!({"error":{"message":message}})
        )),
        _ => Ok(String::new()),
    }
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

    /// The ingress used to keep only `text` parts, so an image posted to
    /// `/v1/chat/completions` never became an `EngineImage` at all — the
    /// adapters' image arms were unreachable and `image_input: true` was an
    /// empty promise. Pin the whole shape, not just presence.
    #[test]
    fn chat_completions_image_parts_reach_the_engine() {
        let messages = protocol_messages(
            "/v1/chat/completions",
            &json!({"messages":[{"role":"user","content":[
                {"type":"text","text":"what is this"},
                {"type":"image_url","image_url":{
                    "url":"data:image/png;base64,iVBORw0KGgo=",
                    "detail":"high"
                }}
            ]}]}),
        )
        .unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "what is this");
        assert_eq!(messages[0].images.len(), 1);
        assert_eq!(
            messages[0].images[0].url,
            "data:image/png;base64,iVBORw0KGgo="
        );
        assert_eq!(messages[0].images[0].detail.as_deref(), Some("high"));
    }

    /// The Responses API spells the same thing differently — a bare string
    /// under `input_image` rather than a nested object.
    #[test]
    fn responses_input_image_reaches_the_engine() {
        let messages = protocol_messages(
            "/v1/responses",
            &json!({"input":[{"role":"user","content":[
                {"type":"input_image","image_url":"https://example.test/a.png"}
            ]}]}),
        )
        .unwrap();
        assert_eq!(messages[0].images.len(), 1);
        assert_eq!(messages[0].images[0].url, "https://example.test/a.png");
    }

    /// An empty or missing URL must not become a placeholder image: the
    /// adapters would announce a degraded image to the model, claiming the
    /// caller sent a picture when it sent nothing.
    #[test]
    fn image_parts_without_a_url_are_skipped_not_placeheld() {
        let messages = protocol_messages(
            "/v1/chat/completions",
            &json!({"messages":[{"role":"user","content":[
                {"type":"text","text":"hi"},
                {"type":"image_url","image_url":{"url":"   "}},
                {"type":"image_url"}
            ]}]}),
        )
        .unwrap();
        assert!(messages[0].images.is_empty());
        assert_eq!(messages[0].content, "hi");
    }

    #[test]
    fn text_only_requests_carry_no_images() {
        let messages = protocol_messages(
            "/v1/chat/completions",
            &json!({"messages":[{"role":"user","content":"plain"}]}),
        )
        .unwrap();
        assert!(messages[0].images.is_empty());
    }

    #[test]
    fn messages_accept_openai_and_anthropic_content_arrays() {
        let messages = protocol_messages(
            "/v1/messages",
            &json!({"messages":[{"role":"user","content":[{"type":"text","text":"hi"}]}]}),
        )
        .unwrap();
        assert_eq!(messages[0].content, "hi");
    }
}
