use super::json_str;
use crate::{CONTROLLER_STATE, ControllerState, LOOPBACK_PORT};
use app_runtime_core::http::{write_preflight, write_response as write_http_response, HttpRequest};
use std::net::TcpStream;
use std::sync::atomic::Ordering;
use std::time::Duration;

pub(crate) fn handle_http_client(mut stream: TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let request = match HttpRequest::read(&mut stream) {
        Ok(req) => req,
        Err(_) => return,
    };

    let port = LOOPBACK_PORT.load(Ordering::SeqCst);
    if port != 0 && !request.has_loopback_host(port) {
        let _ = write_http(
            &mut stream,
            403,
            "Forbidden",
            "text/plain",
            b"bad host header",
        );
        return;
    }

    if request.method == "OPTIONS" {
        let _ = write_preflight(&mut stream, &request);
        return;
    }

    let route = request.path.split('?').next().unwrap_or(&request.path);
    if route == "/healthz" {
        let _ = write_http(&mut stream, 200, "OK", "application/json", b"{\"ok\":true}");
        return;
    }

    // 短暂锁：只 clone Arc<Module> 与会话 Arc，绝不在锁内执行业务请求（计划 §17.2）。
    let (module, sessions) = {
        // 采用编译期已知的最小闭包持有锁作用域。
        let guard = CONTROLLER_STATE.lock().unwrap();
        match &*guard {
            ControllerState::Ready(ready) => {
                (ready.bound.module.clone(), Some(ready.sessions.clone()))
            }
            _ => return, // 未就绪：静默关闭连接（端口未对外公布）
        }
    };

    // 1. UI 静态资源（无需 token）
    if let Ok(Some(resp)) = module.lock().unwrap().handle_ui(route) {
        let _ = write_http(
            &mut stream,
            resp.status_code,
            &resp.reason,
            &resp.content_type,
            &resp.body,
        );
        return;
    }

    // 2. 业务 API：验证 sandbox origin 与 Bearer token
    // 例外（ADR-0032）：/api/tray/state 与 /api/limits 为只读聚合，供进程外系统组件
    // （macOS 顶栏）在无 Native Messaging 会话时轮询，免 Bearer 鉴权与
    // sandbox origin 要求，但仍要求 loopback Host 头（上方已校验）。
    let tray_readonly = route == "/api/tray/state" || route == "/api/limits";
    let session_ok = tray_readonly
        || match &sessions {
            Some(sessions) => match sessions.lock() {
                Ok(sessions_guard) => sessions_guard
                    .authorize(
                        sessions_guard.generation(),
                        request.header("authorization").unwrap_or(""),
                    )
                    .is_ok(),
                Err(_) => false,
            },
            None => false,
        };

    if !tray_readonly && (!request.sandbox_origin() || !session_ok) {
        let _ = write_http(
            &mut stream,
            401,
            "Unauthorized",
            "application/json",
            b"{\"error\":\"APP_SESSION_INVALID\"}",
        );
        return;
    }

    let api_result = {
        let module_guard = match module.lock() {
            Ok(guard) => guard,
            Err(_) => {
                let _ = write_http(
                    &mut stream,
                    500,
                    "Internal Server Error",
                    "application/json",
                    b"{\"error\":\"APP_RUNTIME_INVALID_STATE\"}",
                );
                return;
            }
        };
        module_guard.handle_api(&request, route)
    };
    match api_result {
        Ok(resp) => {
            let _ = write_http(
                &mut stream,
                resp.status_code,
                &resp.reason,
                &resp.content_type,
                &resp.body,
            );
        }
        Err((code, msg)) => {
            let body = format!("{{\"error\":{}}}", json_str(&msg));
            let _ = write_http(
                &mut stream,
                code,
                "Error",
                "application/json",
                body.as_bytes(),
            );
        }
    }
}

/// 全局 Runtime 状态（进程级单实例；HTTP 线程与主循环共享同一状态机）。
/// 定义于文件头部：main 循环与 loopback HTTP 线程共享同一 `Mutex<ControllerState>`。

pub(crate) fn write_http(
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
        "default-src 'none'; script-src 'self' 'unsafe-inline'; connect-src 'self'; style-src 'unsafe-inline'; form-action 'none'; base-uri 'none'; frame-ancestors chrome-extension:",
    )
}
