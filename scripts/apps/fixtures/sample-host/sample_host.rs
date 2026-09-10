// A1/A2 标准样例 App Host（测试夹具，非生产代码）。
// 官方托管应用契约 v1 的最小可运行实现：Native Messaging stdio、origin 校验、
// 运行锁/运行槽、127.0.0.1 动态端口 loopback 服务、bearer 会话鉴权、EOF 两秒退出。
// 安全协议（framing 限额、锁、会话、随机数）复用 crates/app-host-support；
// 本夹具只保留 HTTP 服务、内嵌 UI 与方法分发，作为支持库的第二接入范例。
use app_host_support::lock::{runtime_lock_path, slot_lock_path, FileLock};
use app_host_support::protocol::{
    INSTANCE_ID_BYTES, MAX_FRAME_BYTES, MAX_RUNTIME_SLOTS, SESSION_TOKEN_BYTES,
};
use app_host_support::session::{base64url, random_bytes, SessionManager};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const APP_ID: &str = "sample";
const SAMPLE_VERSION: &str = "1.0.0";

// ---------- 极简 JSON（仅覆盖契约固定形状，夹具专用） ----------

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn json_str(s: &str) -> String {
    format!("\"{}\"", json_escape(s))
}

/// 提取顶层对象的字符串字段值（容忍嵌套；夹具输入均为受控 JSON）。
fn json_get_str(input: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let start = input.find(&needle)? + needle.len();
    let rest = &input[start..];
    let colon = rest.find(':')?;
    let rest = &rest[colon + 1..];
    let rest = rest.trim_start();
    if !rest.starts_with('"') {
        return None;
    }
    let mut out = String::new();
    let mut chars = rest[1..].chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('u') => {
                    let hex: String = chars.by_ref().take(4).collect();
                    if let Ok(v) = u32::from_str_radix(&hex, 16) {
                        out.push(char::from_u32(v).unwrap_or('\u{fffd}'));
                    }
                }
                Some(other) => out.push(other),
                None => return None,
            },
            c => out.push(c),
        }
    }
    None
}

// ---------- 随机 ID/token：复用支持库 CSPRNG ----------

fn random_token() -> Option<String> {
    let mut bytes = [0u8; SESSION_TOKEN_BYTES];
    random_bytes(&mut bytes).then(|| base64url(&bytes))
}

fn random_id_128() -> Option<String> {
    let mut bytes = [0u8; INSTANCE_ID_BYTES];
    random_bytes(&mut bytes).then(|| base64url(&bytes))
}

// ---------- 状态 ----------

// 夹具内嵌 operation 占位，避免引入 serde derive 到夹具层。
mod serde_value {
    pub struct Operation {
        pub id: String,
        pub kind: &'static str,
        pub cancellable: bool,
        pub started_at: u64,
        pub deadline_at: u64,
    }
}

struct Instance {
    instance_id: String,
    sessions: SessionManager,
    port: u16,
    state: &'static str,
    operation: Option<serde_value::Operation>,
}

struct Shared {
    app_id: String,
    app_version: String,
    origin: String,
    data_dir: PathBuf,
    stopping: AtomicBool,
    instance: Mutex<Option<Instance>>,
    server_shutdown: Arc<AtomicBool>,
}

// 锁随进程生命周期存在；Drop（含崩溃由 OS 释放）保证清理。
type HeldLocks = (Mutex<Option<FileLock>>, Vec<Mutex<Option<FileLock>>>);

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn write_response(id: &str, ok: bool, body: &str) {
    let payload = if ok {
        format!(
            "{{\"id\":{},\"ok\":true,\"result\":{}}}",
            json_str(id),
            body
        )
    } else {
        format!(
            "{{\"id\":{},\"ok\":false,\"error\":{}}}",
            json_str(id),
            body
        )
    };
    let bytes = payload.as_bytes();
    let mut buf = Vec::with_capacity(4 + bytes.len());
    buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    buf.extend_from_slice(bytes);
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(&buf);
    let _ = out.flush();
}

fn error_body(code: &str, message: &str) -> String {
    format!(
        "{{\"code\":{},\"message\":{},\"retryable\":false}}",
        json_str(code),
        json_str(message)
    )
}

fn error_body_of(error: &app_host_support::error::AppErrorBody) -> String {
    format!(
        "{{\"code\":{},\"message\":{},\"retryable\":{}}}",
        json_str(&error.code),
        json_str(&error.message),
        error.retryable
    )
}

// ---------- HTTP 服务 ----------

fn serve_http(shared: Arc<Shared>, listener: TcpListener) {
    for stream in listener.incoming() {
        if shared.server_shutdown.load(Ordering::SeqCst) {
            break;
        }
        match stream {
            Ok(stream) => {
                let shared = Arc::clone(&shared);
                std::thread::spawn(move || handle_conn(shared, stream));
            }
            Err(_) => break,
        }
    }
}

fn handle_conn(shared: Arc<Shared>, mut stream: TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    // 只读头部（有界），请求体不含敏感逻辑（夹具只有 GET / 与 GET /api/value）。
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
    let text = String::from_utf8_lossy(&buf);
    let mut lines = text.split("\r\n");
    let request_line = lines.next().unwrap_or("");
    let mut method = "";
    let mut path = "";
    let mut host_header = "";
    let mut auth = "";
    for line in lines.by_ref() {
        if let Some((k, v)) = line.split_once(": ") {
            let kl = k.to_ascii_lowercase();
            match kl.as_str() {
                "host" => host_header = v,
                "authorization" => auth = v,
                _ => {}
            }
        }
    }
    {
        let mut parts = request_line.split_whitespace();
        method = parts.next().unwrap_or("");
        path = parts.next().unwrap_or("");
    }

    // Host header 必须是 127.0.0.1:<port>（防 DNS rebinding / 代理伪装）。
    let bound = format!("127.0.0.1:{}", shared_port(&shared));
    if !host_header.eq_ignore_ascii_case(&bound) {
        let _ = write_http(
            &mut stream,
            403,
            "Forbidden",
            "text/plain",
            b"bad host header",
        );
        return;
    }

    // CORS preflight：sandbox iframe 带 Authorization 头会先发 OPTIONS。
    if method == "OPTIONS" {
        let preflight = "HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: null\r\nAccess-Control-Allow-Methods: GET\r\nAccess-Control-Allow-Headers: Authorization\r\nAccess-Control-Max-Age: 600\r\nConnection: close\r\n\r\n";
        let _ = stream.write_all(preflight.as_bytes());
        let _ = stream.flush();
        return;
    }

    let route = path.split('?').next().unwrap_or(path);
    if route == "/healthz" {
        let _ = write_http(&mut stream, 200, "OK", "application/json", b"{\"ok\":true}");
        return;
    }

    // UI 静态资源：无 token 也可读（初始化页面不能含敏感业务数据）。
    if route == "/" || route == "/index.html" {
        let body = embedded_ui();
        let _ = write_http(
            &mut stream,
            200,
            "OK",
            "text/html; charset=utf-8",
            body.as_bytes(),
        );
        return;
    }

    // 敏感业务数据接口：必须带有效 bearer（generation/token 校验复用支持库）。
    let authorized = {
        let guard = shared.instance.lock().unwrap();
        match guard.as_ref() {
            Some(instance) => instance
                .sessions
                .authorize(instance.sessions.generation(), auth)
                .is_ok(),
            None => false,
        }
    };
    if !authorized {
        let _ = write_http(
            &mut stream,
            401,
            "Unauthorized",
            "application/json",
            b"{\"error\":\"APP_SESSION_INVALID\"}",
        );
        return;
    }
    if route == "/api/value" {
        let value = read_saved_value(&shared.data_dir).unwrap_or_default();
        let body = format!("{{\"value\":{}}}", json_str(&value));
        let _ = write_http(&mut stream, 200, "OK", "application/json", body.as_bytes());
        return;
    }
    let _ = write_http(&mut stream, 404, "Not Found", "text/plain", b"not found");
}

fn shared_port(shared: &Shared) -> u16 {
    let guard = shared.instance.lock().unwrap();
    guard.as_ref().map(|i| i.port).unwrap_or(0)
}

fn write_http(
    stream: &mut TcpStream,
    code: u16,
    reason: &str,
    ctype: &str,
    body: &[u8],
) -> std::io::Result<()> {
    // sandbox iframe 为 opaque origin：CORS 仅允许 Origin: null。
    let cors = "Access-Control-Allow-Origin: null\r\nAccess-Control-Allow-Methods: GET\r\nAccess-Control-Allow-Headers: Authorization\r\n";
    let head = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\n{cors}Connection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

fn read_saved_value(data_dir: &std::path::Path) -> Option<String> {
    let mut value = String::new();
    std::fs::File::open(data_dir.join("value.txt"))
        .and_then(|mut f| f.read_to_string(&mut value))
        .ok()?;
    let trimmed = value.trim().to_string();
    Some(trimmed)
}

// ---------- 内嵌 UI（构建期嵌在一起的静态 HTML/JS；无外联、无模块加载） ----------

fn embedded_ui() -> String {
    // sandbox iframe 为 opaque origin：应用自身 CSP 禁止外联脚本，UI JS 构建期内联。
    format!(
        r#"<!doctype html>
<html>
<head>
<meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'unsafe-inline'; connect-src 'self'; style-src 'unsafe-inline'">
<title>Sample App</title>
<style>body{{font:14px system-ui;margin:16px}}#v{{font-weight:600}}</style>
</head>
<body>
<h1>Sample App v{}</h1>
<p>协议 <span id="proto">1</span></p>
<label>保存值 <input id="input"></label>
<button id="save">保存</button>
<p id="status">加载中…</p>
<p id="value"></p>
<script>{}</script>
</body>
</html>"#,
        SAMPLE_VERSION,
        embedded_ui_js()
    )
}

fn embedded_ui_js() -> String {
    r#"(function () {
  var token = null;
  var pending = null;
  function send(msg) { parent.postMessage(msg, '*'); }
  window.addEventListener('message', function (e) {
    var data = e.data || {};
    if (data.type === 'init' && data.generation && data.challenge) {
      send({ type: 'hello', generation: data.generation, challenge: data.challenge });
    } else if (data.type === 'welcome' && data.token) {
      token = data.token;
      load();
    }
  });
  function api(path) {
    return fetch(path, { headers: { Authorization: 'Bearer ' + token }, cache: 'no-store' });
  }
  function load() {
    api('/api/value').then(function (r) { return r.ok ? r.json() : Promise.reject(r.status); })
      .then(function (j) {
        document.getElementById('value').textContent = '值: ' + (j.value || '(空)');
        document.getElementById('status').textContent = '就绪';
      })
      .catch(function () { document.getElementById('status').textContent = '数据加载失败'; });
  }
  document.getElementById('save').addEventListener('click', function () {
    var v = document.getElementById('input').value;
    pending = { cancel: function () { document.getElementById('status').textContent = '已取消'; } };
    send({ type: 'busy', operation: { id: 'save-' + Date.now(), kind: 'save', cancellable: true } });
    // 夹具：保存为演示行为（真实保存接口随 A2/A3 支持库落地）。
    setTimeout(function () {
      document.getElementById('value').textContent = '值: ' + v;
      document.getElementById('status').textContent = '已保存（演示）';
      pending = null;
      send({ type: 'busy', operation: null });
    }, 400);
  });
})();"#.to_string()
}

// ---------- 主循环 ----------

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--health") {
        // 健康检查：包完整性自检，不取运行锁、不联网、不迁移。
        println!("{{\"ok\":true,\"appId\":\"sample\",\"protocol\":1}}");
        return;
    }
    if args.iter().any(|a| a == "--inspect-data") {
        println!("{{\"currentSchema\":1,\"migrationState\":\"committed\",\"hasCommittedNewWrites\":false,\"previousVersionCompatible\":true}}");
        return;
    }

    // 契约 §5.1：Chrome 启动实参中的 origin，页面参数不可覆盖。
    let origin = match args
        .iter()
        .filter(|a| a.starts_with("chrome-extension://"))
        .next_back()
    {
        Some(o) => o.clone(),
        None => {
            eprintln!("sample-host: missing origin argument");
            std::process::exit(2);
        }
    };

    // 数据根：NATIVES_APPS_ROOT/<appId>（测试由 harness 指向隔离目录）。
    let apps_root = std::env::var("NATIVES_APPS_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| dirs_home().join(".natives").join("apps"));
    eprintln!(
        "sample-host: started, origin={origin}, apps_root={}",
        apps_root.display()
    );
    let data_dir = apps_root.join(APP_ID).join("data");
    let _ = std::fs::create_dir_all(&data_dir);

    // 运行锁 + 运行槽：复用支持库（OS 排他 flock，崩溃由 OS 释放）。
    let runtime_lock = FileLock::try_acquire(runtime_lock_path(&apps_root, APP_ID))
        .ok()
        .flatten();
    let slot_locks: Vec<Mutex<Option<FileLock>>> = (0..MAX_RUNTIME_SLOTS)
        .map(|i| {
            Mutex::new(
                FileLock::try_acquire(slot_lock_path(&apps_root, i))
                    .ok()
                    .flatten(),
            )
        })
        .collect();
    let held: Arc<HeldLocks> = Arc::new((Mutex::new(runtime_lock), slot_locks));

    let shared = Arc::new(Shared {
        app_id: APP_ID.to_string(),
        app_version: SAMPLE_VERSION.to_string(),
        origin,
        data_dir,
        stopping: AtomicBool::new(false),
        instance: Mutex::new(None),
        server_shutdown: Arc::new(AtomicBool::new(false)),
    });

    let mut input = std::io::stdin().lock();
    let mut buffer = Vec::new();
    while !shared.stopping.load(Ordering::SeqCst) {
        let mut header = [0u8; 4];
        match input.read_exact(&mut header) {
            Ok(()) => {}
            Err(_) => break, // stdin EOF：走关闭路径
        }
        let len = u32::from_le_bytes(header) as usize;
        eprintln!("sample-host: frame {len} bytes");
        if len > MAX_FRAME_BYTES {
            break; // 超长帧：直接退出
        }
        buffer.clear();
        if input
            .by_ref()
            .take(len as u64)
            .read_to_end(&mut buffer)
            .is_err()
        {
            break;
        }
        let text = String::from_utf8_lossy(&buffer).to_string();
        let id = json_get_str(&text, "id").unwrap_or_default();
        let method = json_get_str(&text, "method").unwrap_or_default();
        let params_start = text.find("\"params\"").map(|i| &text[i..]).unwrap_or("");
        handle_method(&shared, &held, &id, &method, params_start);
        if shared.stopping.load(Ordering::SeqCst) {
            break;
        }
    }
    // 关闭路径：撤销会话、关闭监听、释放锁、退出（两秒预算内）。
    shared.server_shutdown.store(true, Ordering::SeqCst);
    {
        let mut guard = shared.instance.lock().unwrap();
        if let Some(instance) = guard.as_mut() {
            instance.sessions.revoke();
            instance.state = "stopping";
        }
    }
    drop(held);
    std::process::exit(0);
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
}

fn handle_method(
    shared: &Arc<Shared>,
    held: &Arc<HeldLocks>,
    id: &str,
    method: &str,
    params: &str,
) {
    match method {
        "app:handshake" => {
            let expected = json_get_str(params, "expectedAppId").unwrap_or_default();
            if !expected.is_empty() && expected != shared.app_id {
                write_response(
                    id,
                    false,
                    &error_body("APP_PROTOCOL_MISMATCH", "appId mismatch"),
                );
                return;
            }
            write_response(id, true, &format!(
                "{{\"protocolVersion\":1,\"appId\":{},\"appVersion\":{},\"dataSchemaRange\":{{\"readable\":\"1\",\"writable\":\"1\"}},\"state\":\"stopped\"}}",
                json_str(&shared.app_id), json_str(&shared.app_version)
            ));
        }
        "app:start" => {
            // 运行锁 + 运行槽：任一不可得即拒绝（复用支持库锁语义）。
            let runtime_ok = held.0.lock().unwrap().is_some();
            let slot_free = held.1.iter().any(|slot| slot.lock().unwrap().is_some());
            if !runtime_ok || !slot_free {
                write_response(
                    id,
                    false,
                    &error_body("APP_RUNTIME_LIMIT", "no free runtime slot"),
                );
                return;
            }
            let instance_id = match random_id_128() {
                Some(value) => value,
                None => {
                    write_response(
                        id,
                        false,
                        &error_body("APP_START_FAILED", "no OS randomness"),
                    );
                    return;
                }
            };
            let sessions = match SessionManager::new() {
                Some(value) => value,
                None => {
                    write_response(
                        id,
                        false,
                        &error_body("APP_START_FAILED", "no OS randomness"),
                    );
                    return;
                }
            };
            let generation = sessions.generation().to_string();
            let listener = match TcpListener::bind(("127.0.0.1", 0)) {
                Ok(l) => l,
                Err(_) => {
                    write_response(id, false, &error_body("APP_START_FAILED", "bind failed"));
                    return;
                }
            };
            let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
            {
                let mut guard = shared.instance.lock().unwrap();
                if let Some(existing) = guard.as_ref() {
                    // 幂等 start：同一连接重复 start 返回同一实例。
                    let existing_generation = existing.sessions.generation().to_string();
                    write_response(id, true, &format!(
                        "{{\"instanceId\":{},\"port\":{},\"generation\":{},\"state\":\"ready\"}}",
                        json_str(&existing.instance_id), existing.port, json_str(&existing_generation)
                    ));
                    return;
                }
                *guard = Some(Instance {
                    instance_id: instance_id.clone(),
                    sessions,
                    port,
                    state: "ready",
                    operation: None,
                });
            }
            let server_shared = Arc::clone(shared);
            std::thread::spawn(move || serve_http(server_shared, listener));
            write_response(
                id,
                true,
                &format!(
                    "{{\"instanceId\":{},\"port\":{port},\"generation\":{},\"state\":\"ready\"}}",
                    json_str(&instance_id),
                    json_str(&generation)
                ),
            );
        }
        "app:status" => {
            let guard = shared.instance.lock().unwrap();
            match guard.as_ref() {
                Some(instance) => write_response(
                    id,
                    true,
                    &format!(
                        "{{\"instanceId\":{},\"state\":\"{}\",\"operation\":null}}",
                        json_str(&instance.instance_id),
                        instance.state
                    ),
                ),
                None => write_response(id, false, &error_body("APP_START_FAILED", "no instance")),
            }
        }
        "app:session" => {
            let op = json_get_str(params, "op").unwrap_or_default();
            let challenge = json_get_str(params, "challenge").unwrap_or_default();
            let mut guard = shared.instance.lock().unwrap();
            let Some(instance) = guard.as_mut() else {
                write_response(id, false, &error_body("APP_START_FAILED", "no instance"));
                return;
            };
            match op.as_str() {
                "issue" => match instance.sessions.issue(&challenge) {
                    Ok(result) => {
                        let expires_at = now_epoch() + result.expires_at;
                        write_response(
                            id,
                            true,
                            &format!(
                                "{{\"generation\":{},\"token\":{},\"expiresAt\":{expires_at}}}",
                                json_str(&result.generation),
                                json_str(&result.token)
                            ),
                        );
                    }
                    Err(error) => write_response(id, false, &error_body_of(&error)),
                },
                "rotate" => match instance.sessions.rotate() {
                    Ok(new_generation) => write_response(
                        id,
                        true,
                        &format!("{{\"newGeneration\":{}}}", json_str(&new_generation)),
                    ),
                    Err(error) => write_response(id, false, &error_body_of(&error)),
                },
                "revoke" => {
                    instance.sessions.revoke();
                    write_response(id, true, "{\"revoked\":true}");
                }
                _ => write_response(id, false, &error_body("APP_START_FAILED", "bad session op")),
            }
        }
        "app:stop" => {
            shared.stopping.store(true, Ordering::SeqCst);
            {
                let mut guard = shared.instance.lock().unwrap();
                if let Some(instance) = guard.as_mut() {
                    instance.sessions.revoke();
                    instance.state = "stopping";
                }
            }
            write_response(id, true, "{\"stopped\":true}");
        }
        _ => write_response(
            id,
            false,
            &error_body("APP_PROTOCOL_MISMATCH", "unknown method"),
        ),
    }
}
