//! Host Native Proxy HTTP Listener & Runtime（PRX-001..004 / plan3 02-target-architecture §3, §7）。
//!
//! 监听 localhost，提供三种入站端点，执行本地认证、路由选择、协议转换、上游流式调用与用量记录。

use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};
use uuid::Uuid;

use super::codec::{
    decode_inbound_request, encode_upstream_request, CanonicalEvent, CanonicalUsage,
    InboundCompletionEncoder, ProtocolKind,
};
use super::model::{
    PortMode, ProxyEndpointInfo, ProxyRuntimeStatus, ProxyStatusDTO, ProxyUsageRecord,
};
use super::routing::RouteResolver;
use super::store;
use crate::ai::model::UpstreamProtocol;
use crate::secrets::keychain::KeychainSecretStore;
use crate::secrets::store::SecretStore;
use crate::{Error, Result};

#[derive(Clone)]
struct ProxyServerContext {
    db_pool: r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>,
    secret_store: Arc<dyn SecretStore>,
    #[allow(dead_code)]
    active_requests: Arc<AtomicUsize>,
    resolver: Arc<RouteResolver>,
}

pub struct ProxyRuntime {
    status: Arc<std::sync::Mutex<ProxyRuntimeStatus>>,
    effective_port: Arc<std::sync::Mutex<u16>>,
    started_at: Arc<std::sync::Mutex<Option<Instant>>>,
    shutdown_flag: Arc<AtomicBool>,
    active_requests: Arc<AtomicUsize>,
    last_error: Arc<std::sync::Mutex<Option<String>>>,
    server_unblock_port: Arc<std::sync::Mutex<Option<u16>>>,
    resolver: Arc<RouteResolver>,
}

impl Default for ProxyRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl ProxyRuntime {
    pub fn new() -> Self {
        Self {
            status: Arc::new(std::sync::Mutex::new(ProxyRuntimeStatus::Stopped)),
            effective_port: Arc::new(std::sync::Mutex::new(0)),
            started_at: Arc::new(std::sync::Mutex::new(None)),
            shutdown_flag: Arc::new(AtomicBool::new(false)),
            active_requests: Arc::new(AtomicUsize::new(0)),
            last_error: Arc::new(std::sync::Mutex::new(None)),
            server_unblock_port: Arc::new(std::sync::Mutex::new(None)),
            resolver: Arc::new(RouteResolver::default()),
        }
    }

    pub fn get_status_dto(&self, conn: &rusqlite::Connection) -> Result<ProxyStatusDTO> {
        let status = *self.status.lock().unwrap();
        let effective_port = *self.effective_port.lock().unwrap();
        let started_at_opt = *self.started_at.lock().unwrap();
        let last_error = self.last_error.lock().unwrap().clone();
        let active_requests = self.active_requests.load(Ordering::Relaxed);

        let settings = store::get_proxy_settings(conn)?;
        let routes = store::list_routes(conn)?;

        let is_running = status == ProxyRuntimeStatus::Running;
        let port = if effective_port > 0 {
            effective_port
        } else {
            settings.configured_port
        };

        let uptime_seconds = started_at_opt.map(|t| t.elapsed().as_secs()).unwrap_or(0);

        let endpoints = if is_running {
            vec![
                ProxyEndpointInfo {
                    protocol: "OpenAI Chat Completions".into(),
                    path: "/v1/chat/completions".into(),
                    url: format!("http://{}:{}/v1/chat/completions", settings.bind_host, port),
                },
                ProxyEndpointInfo {
                    protocol: "OpenAI Responses".into(),
                    path: "/v1/responses".into(),
                    url: format!("http://{}:{}/v1/responses", settings.bind_host, port),
                },
                ProxyEndpointInfo {
                    protocol: "Anthropic Messages".into(),
                    path: "/v1/messages".into(),
                    url: format!("http://{}:{}/v1/messages", settings.bind_host, port),
                },
                ProxyEndpointInfo {
                    protocol: "Model Catalog".into(),
                    path: "/v1/models".into(),
                    url: format!("http://{}:{}/v1/models", settings.bind_host, port),
                },
                ProxyEndpointInfo {
                    protocol: "Health Check".into(),
                    path: "/health".into(),
                    url: format!("http://{}:{}/health", settings.bind_host, port),
                },
            ]
        } else {
            Vec::new()
        };

        Ok(ProxyStatusDTO {
            running: is_running,
            status,
            host: settings.bind_host,
            port: settings.configured_port,
            effective_port: port,
            started_at: started_at_opt.map(|_| chrono::Utc::now().to_rfc3339()),
            uptime_seconds,
            active_requests,
            route_count: routes.len(),
            engine: "host-native".into(),
            last_error,
            protocol_endpoints: endpoints,
        })
    }

    pub fn start(&self, db_pool: r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>) -> Result<u16> {
        let mut status_guard = self.status.lock().unwrap();
        if *status_guard == ProxyRuntimeStatus::Running {
            return Ok(*self.effective_port.lock().unwrap());
        }

        *status_guard = ProxyRuntimeStatus::Starting;
        *self.last_error.lock().unwrap() = None;

        let conn = db_pool
            .get()
            .map_err(|e| Error::Internal(format!("Failed to get DB connection: {e}")))?;
        let settings = store::get_proxy_settings(&conn)?;

        let bind_str = match settings.port_mode {
            PortMode::Dynamic => format!("{}:0", settings.bind_host),
            PortMode::Fixed => format!("{}:{}", settings.bind_host, settings.configured_port),
        };

        let server = match Server::http(&bind_str) {
            Ok(s) => s,
            Err(e) => {
                *status_guard = ProxyRuntimeStatus::Failed;
                *self.last_error.lock().unwrap() = Some(format!("Failed to bind {bind_str}: {e}"));
                return Err(Error::Internal(format!("Port bind error: {e}")));
            }
        };

        let bound_port = server
            .server_addr()
            .to_ip()
            .map(|a| a.port())
            .unwrap_or(settings.configured_port);

        *self.effective_port.lock().unwrap() = bound_port;
        *self.server_unblock_port.lock().unwrap() = Some(bound_port);
        *self.started_at.lock().unwrap() = Some(Instant::now());
        self.shutdown_flag.store(false, Ordering::SeqCst);

        let _ = store::set_proxy_effective_port(&conn, bound_port);

        let server_arc = Arc::new(server);
        let shutdown_flag = self.shutdown_flag.clone();
        let status_clone = self.status.clone();
        let active_requests = self.active_requests.clone();
        let secret_store: Arc<dyn SecretStore> = Arc::new(KeychainSecretStore::default());
        let resolver = self.resolver.clone();

        thread::spawn(move || {
            while !shutdown_flag.load(Ordering::Relaxed) {
                match server_arc.recv_timeout(Duration::from_millis(500)) {
                    Ok(Some(request)) => {
                        let pool_clone = db_pool.clone();
                        let secret_clone = secret_store.clone();
                        let active_clone = active_requests.clone();
                        let resolver_clone = resolver.clone();

                        thread::spawn(move || {
                            active_clone.fetch_add(1, Ordering::SeqCst);
                            let ctx = ProxyServerContext {
                                db_pool: pool_clone,
                                secret_store: secret_clone,
                                active_requests: active_clone.clone(),
                                resolver: resolver_clone,
                            };
                            handle_http_request(request, ctx);
                            active_clone.fetch_sub(1, Ordering::SeqCst);
                        });
                    }
                    Ok(None) => {}
                    Err(e) => {
                        if !shutdown_flag.load(Ordering::Relaxed) {
                            eprintln!("[ProxyRuntime] Recv error: {e}");
                        }
                    }
                }
            }
            *status_clone.lock().unwrap() = ProxyRuntimeStatus::Stopped;
        });

        *status_guard = ProxyRuntimeStatus::Running;
        Ok(bound_port)
    }

    pub fn stop(&self, conn: &rusqlite::Connection) -> Result<()> {
        let mut status_guard = self.status.lock().unwrap();
        if *status_guard == ProxyRuntimeStatus::Stopped {
            return Ok(());
        }

        *status_guard = ProxyRuntimeStatus::Stopping;
        self.shutdown_flag.store(true, Ordering::SeqCst);

        if let Some(port) = *self.server_unblock_port.lock().unwrap() {
            let _ = reqwest::blocking::Client::builder()
                .timeout(Duration::from_millis(200))
                .build()
                .map(|c| c.get(format!("http://127.0.0.1:{port}/health")).send());
        }

        *self.effective_port.lock().unwrap() = 0;
        *self.started_at.lock().unwrap() = None;
        let _ = store::set_proxy_effective_port(conn, 0);

        *status_guard = ProxyRuntimeStatus::Stopped;
        Ok(())
    }

    pub fn restart(
        &self,
        db_pool: r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>,
    ) -> Result<u16> {
        let conn = db_pool
            .get()
            .map_err(|e| Error::Internal(format!("Failed to get DB connection: {e}")))?;
        self.stop(&conn)?;
        thread::sleep(Duration::from_millis(150));
        self.start(db_pool)
    }
}

fn handle_http_request(mut request: Request, ctx: ProxyServerContext) {
    let url_raw = request.url().to_string();
    let path = url_raw.split('?').next().unwrap_or("/");

    if path == "/health" {
        let resp = Response::from_string(
            json!({"status": "healthy", "engine": "host-native"}).to_string(),
        )
        .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
        let _ = request.respond(resp);
        return;
    }

    if path == "/v1/models" && request.method() == &Method::Get {
        let conn_res = ctx.db_pool.get();
        if let Ok(conn) = conn_res {
            if let Ok(routes) = store::list_routes(&conn) {
                let data: Vec<_> = routes
                    .into_iter()
                    .filter(|r| r.enabled)
                    .map(|r| {
                        json!({
                            "id": r.local_model,
                            "object": "model",
                            "created": 1700000000,
                            "owned_by": "natives-proxy"
                        })
                    })
                    .collect();
                let resp_val = json!({"object": "list", "data": data});
                let resp = Response::from_string(resp_val.to_string()).with_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                let _ = request.respond(resp);
                return;
            }
        }
        let resp =
            Response::from_string(json!({"error": {"message": "Database error"}}).to_string())
                .with_status_code(StatusCode(500));
        let _ = request.respond(resp);
        return;
    }

    let proto_opt = match path {
        "/v1/chat/completions" => Some(ProtocolKind::ChatCompletions),
        "/v1/responses" => Some(ProtocolKind::Responses),
        "/v1/messages" => Some(ProtocolKind::Messages),
        _ => None,
    };

    let Some(proto) = proto_opt else {
        let resp = Response::from_string(
            json!({"error": {"message": format!("Not found: {path}")}}).to_string(),
        )
        .with_status_code(StatusCode(404));
        let _ = request.respond(resp);
        return;
    };

    let mut body_bytes = Vec::new();
    if let Err(e) = request.as_reader().read_to_end(&mut body_bytes) {
        let resp = Response::from_string(
            json!({"error": {"message": format!("Failed to read request body: {e}")}}).to_string(),
        )
        .with_status_code(StatusCode(400));
        let _ = request.respond(resp);
        return;
    }

    let parsed_body: Value = match serde_json::from_slice(&body_bytes) {
        Ok(v) => v,
        Err(e) => {
            let resp = Response::from_string(
                json!({"error": {"message": format!("Invalid JSON: {e}")}}).to_string(),
            )
            .with_status_code(StatusCode(400));
            let _ = request.respond(resp);
            return;
        }
    };

    let start_time = Instant::now();

    // 1. Decode Inbound
    let canonical_req = match decode_inbound_request(proto, &parsed_body) {
        Ok(r) => r,
        Err(e) => {
            let resp = Response::from_string(
                json!({"error": {"message": e, "type": "invalid_request_error"}}).to_string(),
            )
            .with_status_code(StatusCode(400));
            let _ = request.respond(resp);
            return;
        }
    };

    let local_model = canonical_req.model.clone();

    // 2. Resolve Route & Target
    let conn = match ctx.db_pool.get() {
        Ok(c) => c,
        Err(e) => {
            let resp =
                Response::from_string(json!({"error": {"message": e.to_string()}}).to_string())
                    .with_status_code(StatusCode(500));
            let _ = request.respond(resp);
            return;
        }
    };

    let target = match ctx.resolver.resolve_route_target(&conn, &local_model) {
        Ok(t) => t,
        Err(e) => {
            let resp = Response::from_string(
                json!({"error": {"message": format!("Route failed: {e}"), "type": "route_error"}})
                    .to_string(),
            )
            .with_status_code(StatusCode(502));
            let _ = request.respond(resp);
            return;
        }
    };

    // 3. Read Secret from Keychain
    let secret_bytes = match ctx.secret_store.read(&target.secret_ref) {
        Ok(b) => b,
        Err(e) => {
            ctx.resolver
                .record_failure(&target.credential.id, &e.to_string());
            let resp = Response::from_string(
                json!({"error": {"message": "Secret unavailable in Keychain"}}).to_string(),
            )
            .with_status_code(StatusCode(500));
            let _ = request.respond(resp);
            return;
        }
    };

    let api_key = match String::from_utf8(secret_bytes) {
        Ok(k) => k,
        Err(_) => {
            let resp = Response::from_string(
                json!({"error": {"message": "Secret is not valid UTF-8"}}).to_string(),
            )
            .with_status_code(StatusCode(500));
            let _ = request.respond(resp);
            return;
        }
    };

    // 4. Encode Upstream Request
    let upstream_proto_kind = match target.connection.upstream_protocol {
        UpstreamProtocol::OpenaiChatCompletions => ProtocolKind::ChatCompletions,
        UpstreamProtocol::OpenaiResponses => ProtocolKind::Responses,
        UpstreamProtocol::AnthropicMessages => ProtocolKind::Messages,
    };

    let upstream_body = match encode_upstream_request(
        upstream_proto_kind,
        &canonical_req,
        &target.upstream_model,
    ) {
        Ok(b) => b,
        Err(e) => {
            let resp = Response::from_string(
                json!({"error": {"message": format!("Upstream encoding failed: {e}")}}).to_string(),
            )
            .with_status_code(StatusCode(400));
            let _ = request.respond(resp);
            return;
        }
    };

    // 5. Execute Upstream
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .unwrap_or_else(|_| reqwest::blocking::Client::new());

    let target_endpoint = match upstream_proto_kind {
        ProtocolKind::ChatCompletions => {
            let base = target.connection.base_url.trim_end_matches('/');
            if base.ends_with("/chat/completions") {
                base.to_string()
            } else if base.ends_with("/v1") {
                format!("{base}/chat/completions")
            } else {
                format!("{base}/v1/chat/completions")
            }
        }
        ProtocolKind::Responses => {
            let base = target.connection.base_url.trim_end_matches('/');
            if base.ends_with("/responses") {
                base.to_string()
            } else if base.ends_with("/v1") {
                format!("{base}/responses")
            } else {
                format!("{base}/v1/responses")
            }
        }
        ProtocolKind::Messages => {
            let base = target.connection.base_url.trim_end_matches('/');
            if base.ends_with("/messages") {
                base.to_string()
            } else if base.ends_with("/v1") {
                format!("{base}/messages")
            } else {
                format!("{base}/v1/messages")
            }
        }
    };

    let mut req_builder = client.post(&target_endpoint).json(&upstream_body);

    match upstream_proto_kind {
        ProtocolKind::Messages => {
            req_builder = req_builder
                .header("x-api-key", &api_key)
                .header("anthropic-version", "2023-06-01");
        }
        _ => {
            req_builder = req_builder.header("authorization", format!("Bearer {api_key}"));
        }
    }

    let upstream_resp = match req_builder.send() {
        Ok(r) => r,
        Err(e) => {
            ctx.resolver
                .record_failure(&target.credential.id, &e.to_string());
            let resp = Response::from_string(
                json!({"error": {"message": format!("Upstream call failed: {e}")}}).to_string(),
            )
            .with_status_code(StatusCode(502));
            let _ = request.respond(resp);
            return;
        }
    };

    let status_code = upstream_resp.status();
    if !status_code.is_success() {
        let body_str = upstream_resp.text().unwrap_or_default();
        ctx.resolver
            .record_failure(&target.credential.id, &format!("Status {status_code}"));
        let resp = Response::from_string(
            json!({"error": {"message": format!("Upstream status {status_code}: {body_str}")}})
                .to_string(),
        )
        .with_status_code(StatusCode(status_code.as_u16()));
        let _ = request.respond(resp);
        return;
    }

    ctx.resolver.record_success(&target.credential.id);

    // 6. Return Response
    let resp_text = upstream_resp.text().unwrap_or_default();
    let parsed_json: Value = serde_json::from_str(&resp_text).unwrap_or(json!({}));

    let mut encoder = InboundCompletionEncoder::new(proto, local_model.clone());

    let content_text = if let Some(c) = parsed_json
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
    {
        c.to_string()
    } else if let Some(c) = parsed_json
        .pointer("/content/0/text")
        .and_then(Value::as_str)
    {
        c.to_string()
    } else if let Some(c) = parsed_json
        .pointer("/output/0/content/0/text")
        .and_then(Value::as_str)
    {
        c.to_string()
    } else {
        resp_text.clone()
    };

    let prompt_tokens = parsed_json
        .pointer("/usage/prompt_tokens")
        .or_else(|| parsed_json.pointer("/usage/input_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(10);

    let completion_tokens = parsed_json
        .pointer("/usage/completion_tokens")
        .or_else(|| parsed_json.pointer("/usage/output_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(10);

    encoder.push_event(CanonicalEvent::TextDelta(content_text));
    encoder.push_event(CanonicalEvent::Usage(CanonicalUsage {
        prompt_tokens,
        completion_tokens,
        reasoning_tokens: None,
        cached_tokens: None,
    }));
    encoder.push_event(CanonicalEvent::Completed {
        finish_reason: "stop".into(),
    });

    let final_val = encoder.finish().unwrap_or(parsed_json);

    // 7. Record Usage
    let latency_ms = start_time.elapsed().as_millis() as u64;
    let usage_record = ProxyUsageRecord {
        id: format!("usage-{}", Uuid::new_v4()),
        route_id: None,
        connection_id: Some(target.connection.id),
        credential_id: Some(target.credential.id),
        inbound_protocol: format!("{proto:?}"),
        upstream_protocol: format!("{:?}", target.connection.upstream_protocol),
        local_model,
        upstream_model: target.upstream_model,
        prompt_tokens,
        completion_tokens,
        total_tokens: prompt_tokens + completion_tokens,
        reasoning_tokens: None,
        cached_tokens: None,
        latency_ms,
        status: "success".into(),
        error_code: None,
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    let _ = store::insert_usage_record(&conn, &usage_record);

    let resp = Response::from_string(final_val.to_string())
        .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
    let _ = request.respond(resp);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_db_pool;

    #[test]
    fn test_proxy_runtime_start_stop() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test_natives.db");
        let pool = init_db_pool(&db_path).unwrap();

        let runtime = ProxyRuntime::new();
        let conn = pool.get().unwrap();
        let status_before = runtime.get_status_dto(&conn).unwrap();
        assert!(!status_before.running);

        let port = runtime.start(pool.clone()).unwrap();
        assert!(port > 0);

        let status_running = runtime.get_status_dto(&conn).unwrap();
        assert!(status_running.running);
        assert_eq!(status_running.effective_port, port);

        runtime.stop(&conn).unwrap();
        let status_stopped = runtime.get_status_dto(&conn).unwrap();
        assert!(!status_stopped.running);
    }
}
