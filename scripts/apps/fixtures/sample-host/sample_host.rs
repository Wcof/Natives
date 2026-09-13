// A1/A2 标准样例 App Host（测试夹具，非生产代码）。
// 官方托管应用契约 v1 的最小可运行实现：Native Messaging stdio、origin 校验、
// 运行锁/运行槽、127.0.0.1 动态端口 loopback 服务、bearer 会话鉴权、EOF 两秒退出。
// 安全协议（framing 限额、锁、会话、随机数）复用 crates/app-host-support；
// 本夹具只保留 HTTP 服务、内嵌 UI 与方法分发，作为支持库的第二接入范例。
use app_host_support::framing::{read_frame, write_frame};
use app_host_support::http::{write_preflight, write_response as write_http_response, HttpRequest};
use app_host_support::lock::{acquire_runtime, RuntimeLease, RuntimeUnavailable};
use app_host_support::origin::chrome_extension_origin;
use app_host_support::session::{random_id, SessionManager};
use std::io::Read;
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn package_identity() -> (String, String) {
    app_host_support::layout::installed_identity()
        .unwrap_or_else(|| ("sample".into(), "1.0.0".into()))
}

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

/// 提取顶层对象的正整数字段值（支持数字或引号包裹的数字；夹具输入均为受控 JSON）。
fn json_get_u64(input: &str, key: &str) -> Option<u64> {
    let needle = format!("\"{key}\"");
    let start = input.find(&needle)? + needle.len();
    let rest = &input[start..];
    let colon = rest.find(':')?;
    let rest = rest[colon + 1..].trim_start();
    if rest.starts_with('"') {
        let end = rest[1..].find('"')?;
        rest[1..=end].parse::<u64>().ok()
    } else {
        let end = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        if end == 0 {
            return None;
        }
        rest[..end].parse::<u64>().ok()
    }
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
    apps_root: PathBuf,
    data_dir: PathBuf,
    stopping: AtomicBool,
    instance: Mutex<Option<Instance>>,
    server_shutdown: Arc<AtomicBool>,
}

// 真实 start 后才持有 app 锁和一个全局槽；Drop（含崩溃）保证释放。
type HeldLocks = Mutex<Option<RuntimeLease>>;

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
    let mut out = std::io::stdout().lock();
    let _ = write_frame(&mut out, payload.as_bytes());
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
    let request = match HttpRequest::read(&mut stream) {
        Ok(request) => request,
        Err(_) => return,
    };

    // Host header 必须是 127.0.0.1:<port>（防 DNS rebinding / 代理伪装）。
    if !request.has_loopback_host(shared_port(&shared)) {
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
    if request.method == "OPTIONS" {
        if write_preflight(&mut stream, &request).is_err() {
            let _ = write_http(
                &mut stream,
                403,
                "Forbidden",
                "text/plain",
                b"bad CORS preflight",
            );
        }
        return;
    }

    let route = request.path.split('?').next().unwrap_or(&request.path);
    if route == "/healthz" {
        let _ = write_http(&mut stream, 200, "OK", "application/json", b"{\"ok\":true}");
        return;
    }

    // UI 静态资源：无 token 也可读（初始化页面不能含敏感业务数据）。
    if route == "/" || route == "/index.html" {
        let body = embedded_ui(&shared.app_version);
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
                .authorize(
                    instance.sessions.generation(),
                    request.header("authorization").unwrap_or(""),
                )
                .is_ok(),
            None => false,
        }
    };
    if !request.sandbox_origin() || !authorized {
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
        if request.method == "POST" {
            if request.body.len() > 4096 || std::str::from_utf8(&request.body).is_err() {
                let _ = write_http(
                    &mut stream,
                    400,
                    "Bad Request",
                    "text/plain",
                    b"invalid value",
                );
            } else if write_saved_value(&shared.data_dir, &request.body).is_ok() {
                let _ = write_http(
                    &mut stream,
                    200,
                    "OK",
                    "application/json",
                    b"{\"saved\":true}",
                );
            } else {
                let _ = write_http(
                    &mut stream,
                    500,
                    "Internal Server Error",
                    "text/plain",
                    b"save failed",
                );
            }
            return;
        }
        let value = read_saved_value(&shared.data_dir).unwrap_or_default();
        let body = format!("{{\"value\":{}}}", json_str(&value));
        let _ = write_http(&mut stream, 200, "OK", "application/json", body.as_bytes());
        return;
    }
    if route == "/api/operation/start" && request.method == "POST" {
        let mut guard = shared.instance.lock().unwrap();
        if let Some(instance) = guard.as_mut() {
            let id = random_id().unwrap_or_else(|| "unavailable".into());
            instance.operation = Some(serde_value::Operation {
                id: id.clone(),
                kind: "sample_wait",
                cancellable: true,
                started_at: now_epoch(),
                deadline_at: now_epoch() + 30,
            });
            let body = format!(
                "{{\"operationId\":{},\"state\":\"running\"}}",
                json_str(&id)
            );
            let _ = write_http(&mut stream, 200, "OK", "application/json", body.as_bytes());
        }
        return;
    }
    if route == "/api/operation/cancel" && request.method == "POST" {
        let mut guard = shared.instance.lock().unwrap();
        let cancelled = guard
            .as_mut()
            .is_some_and(|instance| instance.operation.take().is_some());
        let body = format!("{{\"cancelled\":{cancelled}}}");
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
    write_http_response(
        stream,
        code,
        reason,
        ctype,
        body,
        "default-src 'none'; script-src 'unsafe-inline'; connect-src 'self'; style-src 'unsafe-inline'; form-action 'none'; base-uri 'none'; frame-ancestors chrome-extension:",
    )
}

fn read_saved_value(data_dir: &std::path::Path) -> Option<String> {
    let mut value = String::new();
    std::fs::File::open(data_dir.join("value.txt"))
        .and_then(|mut f| f.read_to_string(&mut value))
        .ok()?;
    let trimmed = value.trim().to_string();
    Some(trimmed)
}

fn write_saved_value(data_dir: &std::path::Path, value: &[u8]) -> std::io::Result<()> {
    std::fs::create_dir_all(data_dir)?;
    let path = data_dir.join("value.txt");
    let temp = data_dir.join("value.txt.tmp");
    std::fs::write(&temp, value)?;
    std::fs::rename(temp, path)
}

// ---------- 内嵌 UI（构建期嵌在一起的静态 HTML/JS；无外联、无模块加载） ----------

fn embedded_ui(version: &str) -> String {
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
<button id="start-op">开始可取消操作</button>
<button id="cancel-op">取消操作</button>
<p id="status">加载中…</p>
<p id="value"></p>
<script>{}</script>
</body>
</html>"#,
        version,
        embedded_ui_js()
    )
}

fn embedded_ui_js() -> String {
    r#"(function () {
  var token = null;
  function send(msg) { parent.postMessage(msg, '*'); }
  window.addEventListener('message', function (e) {
    var data = e.data || {};
    if (data.type === 'probe-capabilities') {
      var parentReadable = true;
      try { void parent.location.href; } catch (_) { parentReadable = false; }
      fetch('https://example.com/', { mode: 'no-cors' }).then(
        function () { send({ type: 'probe-result', chromeRuntime: !!(window.chrome && chrome.runtime), parentReadable: parentReadable, externalFetch: true }); },
        function () { send({ type: 'probe-result', chromeRuntime: !!(window.chrome && chrome.runtime), parentReadable: parentReadable, externalFetch: false }); }
      );
      return;
    }
    if (data.type === 'init' && data.generation && data.challenge) {
      send({ type: 'hello', generation: data.generation, challenge: data.challenge });
    } else if (data.type === 'welcome' && data.token) {
      token = data.token;
      load();
    }
  });
  function api(path, options) {
    options = options || {};
    options.headers = Object.assign({}, options.headers, { Authorization: 'Bearer ' + token });
    options.cache = 'no-store';
    return fetch(path, options);
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
    api('/api/value', { method: 'POST', body: v }).then(function (r) {
      if (!r.ok) throw new Error(String(r.status));
      document.getElementById('status').textContent = '已保存';
      load();
    }).catch(function () { document.getElementById('status').textContent = '保存失败'; });
  });
  document.getElementById('start-op').addEventListener('click', function () {
    api('/api/operation/start', { method: 'POST' }).then(function (r) { return r.json(); })
      .then(function (op) {
        document.getElementById('status').textContent = '操作进行中';
        send({ type: 'busy', operation: { id: op.operationId, kind: 'sample_wait', cancellable: true } });
      });
  });
  document.getElementById('cancel-op').addEventListener('click', function () {
    api('/api/operation/cancel', { method: 'POST' }).then(function (r) { return r.json(); })
      .then(function (result) {
        document.getElementById('status').textContent = result.cancelled ? '已取消' : '没有进行中的操作';
        send({ type: 'busy', operation: null });
      });
  });
})();"#.to_string()
}

// ---------- 主循环 ----------

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (app_id, app_version) = package_identity();
    if args.iter().any(|a| a == "--health") {
        // 健康检查：包完整性自检，不取运行锁、不联网、不迁移。
        println!(
            "{{\"ok\":true,\"appId\":{},\"protocol\":1}}",
            json_str(&app_id)
        );
        return;
    }
    if args.iter().any(|a| a == "--inspect-data") {
        let apps_root = std::env::var("NATIVES_APPS_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                app_host_support::layout::installed_apps_root()
                    .unwrap_or_else(|| dirs_home().join(".natives").join("apps"))
            });
        let data_dir = apps_root.join(&app_id).join("data");
        let migration_file = data_dir.join(".migration.json");
        let value_file = data_dir.join("value.txt");
        let has_new_writes = value_file.exists();
        let (schema, migration_state, prev_compatible) = if migration_file.exists() {
            let bytes = std::fs::read(&migration_file).unwrap_or_default();
            let text = String::from_utf8_lossy(&bytes);
            let schema = json_get_str(&text, "currentSchema")
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(1);
            let state = json_get_str(&text, "state").unwrap_or_else(|| "committed".into());
            let comp = !text.contains("\"previousVersionCompatible\":false");
            (schema, state, comp)
        } else {
            (1, "committed".to_string(), true)
        };
        println!(
            "{{\"currentSchema\":{schema},\"migrationState\":{},\"hasCommittedNewWrites\":{has_new_writes},\"previousVersionCompatible\":{prev_compatible}}}",
            json_str(&migration_state)
        );
        return;
    }

    // 契约 §5.1：Chrome 启动实参中的 origin，页面参数不可覆盖。
    let origin = match chrome_extension_origin(&args) {
        Ok(origin) => origin,
        Err(error) => {
            eprintln!("sample-host: {}", error.message);
            std::process::exit(2);
        }
    };

    // 数据根：NATIVES_APPS_ROOT/<appId>（测试由 harness 指向隔离目录）。
    let apps_root = std::env::var("NATIVES_APPS_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            app_host_support::layout::installed_apps_root()
                .unwrap_or_else(|| dirs_home().join(".natives").join("apps"))
        });
    eprintln!(
        "sample-host: started, origin={origin}, apps_root={}",
        apps_root.display()
    );
    let data_dir = apps_root.join(&app_id).join("data");
    let _ = std::fs::create_dir_all(&data_dir);

    let held: Arc<HeldLocks> = Arc::new(Mutex::new(None));

    let shared = Arc::new(Shared {
        app_id,
        app_version,
        origin: origin.clone(),
        apps_root,
        data_dir,
        stopping: AtomicBool::new(false),
        instance: Mutex::new(None),
        server_shutdown: Arc::new(AtomicBool::new(false)),
    });

    let mut input = std::io::stdin().lock();
    while !shared.stopping.load(Ordering::SeqCst) {
        let buffer = match read_frame(&mut input) {
            Ok(Some(frame)) => frame,
            Ok(None) | Err(_) => break, // EOF/半帧/超限：共用关闭路径
        };
        eprintln!("sample-host: frame {} bytes", buffer.len());
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
    held.lock().unwrap().take();
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
            let expected_gen = json_get_u64(params, "expectedActivationGeneration");
            if let Err((err_code, msg)) = app_host_support::verify_activation_with_generation(
                &shared.apps_root,
                &shared.app_id,
                &shared.app_version,
                Some(&shared.origin),
                expected_gen,
            ) {
                eprintln!("sample-host: pre-lock activation check failed: {err_code:?} - {msg}");
                write_response(id, false, &error_body(err_code.as_str(), &msg));
                return;
            }
            if let Some(existing) = shared.instance.lock().unwrap().as_ref() {
                write_response(
                    id,
                    true,
                    &format!(
                        "{{\"instanceId\":{},\"port\":{},\"generation\":{},\"state\":\"ready\"}}",
                        json_str(&existing.instance_id),
                        existing.port,
                        json_str(existing.sessions.generation())
                    ),
                );
                return;
            }
            let lease = match acquire_runtime(&shared.apps_root, &shared.app_id) {
                Ok(Ok(lease)) => lease,
                Ok(Err(RuntimeUnavailable::AlreadyRunning)) => {
                    write_response(
                        id,
                        false,
                        &error_body("APP_RUNNING_ELSEWHERE", "app is running elsewhere"),
                    );
                    return;
                }
                Ok(Err(RuntimeUnavailable::Busy)) => {
                    write_response(
                        id,
                        false,
                        &error_body("APP_BUSY", "app is being installed or upgraded"),
                    );
                    return;
                }
                Ok(Err(RuntimeUnavailable::Limit)) => {
                    write_response(
                        id,
                        false,
                        &error_body("APP_RUNTIME_LIMIT", "no free runtime slot"),
                    );
                    return;
                }
                Err(_) => {
                    write_response(
                        id,
                        false,
                        &error_body("APP_START_FAILED", "runtime lock failed"),
                    );
                    return;
                }
            };
            // 先检查、取得锁后再检查，消除检查后被停用/更新的窗口
            if let Err((err_code, msg)) = app_host_support::verify_activation_with_generation(
                &shared.apps_root,
                &shared.app_id,
                &shared.app_version,
                Some(&shared.origin),
                expected_gen,
            ) {
                drop(lease);
                eprintln!("sample-host: post-lock activation check failed: {err_code:?} - {msg}");
                write_response(id, false, &error_body(err_code.as_str(), &msg));
                return;
            }
            let instance_id = match random_id() {
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
                *guard = Some(Instance {
                    instance_id: instance_id.clone(),
                    sessions,
                    port,
                    state: "ready",
                    operation: None,
                });
            }
            *held.lock().unwrap() = Some(lease);
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
                Some(instance) => {
                    let operation = instance.operation.as_ref().map_or_else(
                        || "null".to_string(),
                        |op| format!(
                            "{{\"id\":{},\"kind\":{},\"cancellable\":{},\"startedAt\":{},\"deadlineAt\":{}}}",
                            json_str(&op.id), json_str(op.kind), op.cancellable, op.started_at, op.deadline_at
                        ),
                    );
                    write_response(
                        id,
                        true,
                        &format!(
                            "{{\"instanceId\":{},\"state\":\"{}\",\"operation\":{operation}}}",
                            json_str(&instance.instance_id),
                            instance.state
                        ),
                    )
                }
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
