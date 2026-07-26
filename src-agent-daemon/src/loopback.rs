//! Authenticated localhost compatibility API backed by the daemon route provider.

use agent_core::{EngineMessage, EngineProvider, EngineProviderEvent, EngineToolCall};
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
    NativesDbBroker::open(crate::natives_db_broker::default_natives_db_path())?.loopback_settings()
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
    let conn = Connection::open(crate::natives_db_broker::default_natives_db_path())
        .map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare("SELECT DISTINCT model_id FROM provider_route_bindings WHERE enabled=1 ORDER BY model_id").map_err(|e| e.to_string())?;
    let models = stmt
        .query_map([], |row| {
            Ok(json!({"id": row.get::<_, String>(0)?, "object": "model", "owned_by": "natives"}))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(models)
}

fn local_provider(model: &str) -> Result<crate::routing::RoutedProvider, String> {
    let conn = Connection::open(crate::natives_db_broker::default_natives_db_path())
        .map_err(|e| e.to_string())?;
    let mut statement = conn
        .prepare(
            "SELECT provider_id, credential_kind, credential_id, model_id
         FROM provider_route_bindings
         WHERE enabled=1 AND model_id=?1 ORDER BY position, id",
        )
        .map_err(|error| error.to_string())?;
    let targets = statement
        .query_map([model], |row| {
            Ok(crate::routing::RouteTarget {
                provider_id: row.get(0)?,
                credential_kind: row.get(1)?,
                credential_id: row.get(2)?,
                model_id: row.get(3)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
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
                let text = content_text(item.get("content").unwrap_or(&item));
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
                    ..Default::default()
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
        (_, EngineProviderEvent::Completed) if path == "/v1/messages" => {
            Ok("event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".into())
        }
        (_, EngineProviderEvent::Completed) if path == "/v1/responses" => {
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
