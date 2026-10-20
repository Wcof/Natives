use super::error_json;
use crate::{ControllerState, CONTROLLER_STATE, LOOPBACK_PORT};
use app_runtime_core::http::{write_preflight, write_response as write_http_response, HttpRequest};
use std::net::TcpStream;
use std::sync::atomic::Ordering;
use std::time::Duration;

pub(crate) fn handle_http_client(mut stream: TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    // 写超时：慢客户端不能长期占用请求线程（响应写出同样有界）。
    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
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

    // 短暂状态快照：只 clone Arc<dyn ModuleService> 与会话 Arc，绝不在锁内执行业务请求（架构 Wave 4）。
    let (service, sessions, app_id, worker_epoch, capabilities) = {
        let guard = CONTROLLER_STATE.lock().unwrap();
        match &*guard {
            ControllerState::Ready(ready) => (
                ready.service.clone(),
                ready.sessions.clone(),
                ready.bound.app_id.clone(),
                ready.bound.instance_id.clone(),
                ready.bound.capabilities.clone(),
            ),
            _ => return, // 未就绪：静默关闭连接
        }
    };

    // 1. UI 静态资源（无需 token）
    if let Ok(Some(resp)) = service.handle_ui(route) {
        let _ = write_http(
            &mut stream,
            resp.status_code,
            &resp.reason,
            &resp.content_type,
            &resp.body,
        );
        return;
    }

    // 2. 业务 API：强制验证 sandbox origin 与 Bearer token
    // ADR-0035：废除 8765 固定端口与无 Bearer 特权，所有 /api/* 一律要求有效会话与沙箱 Origin。
    let context = sessions.lock().ok().and_then(|sessions| {
        sessions
            .authorized_session(request.header("authorization").unwrap_or(""))
            .ok()
            .map(|session| app_runtime_core::module::RequestContext {
                principal: session.surface_id().to_string(),
                surface_id: session.surface_id().to_string(),
                generation: session.generation.clone(),
                app_id,
                worker_epoch,
                capabilities,
            })
    });

    if !request.sandbox_origin() || context.is_none() {
        let _ = write_http(
            &mut stream,
            401,
            "Unauthorized",
            "application/json",
            b"{\"error\":\"APP_SESSION_INVALID\"}",
        );
        return;
    }

    let api_result = service.handle_authenticated_api(&context.unwrap(), &request, route);
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
            let body = error_json(&msg);
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
    let parent = crate::PARENT_ORIGIN
        .get()
        .map(String::as_str)
        .unwrap_or("'none'");
    let web_port = std::env::var("NATIVES_PARENT_WEB_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|port| *port != 0);
    let csp = runtime_csp(parent, web_port);
    write_http_response(stream, code, reason, ctype, body, &csp)
}

fn runtime_csp(parent_origin: &str, web_port: Option<u16>) -> String {
    let web_origins = web_port
        .map(|port| format!(" http://127.0.0.1:{port} http://localhost:{port}"))
        .unwrap_or_default();
    format!("default-src 'none'; script-src 'self' 'unsafe-inline'; connect-src 'self'; style-src 'self' 'unsafe-inline'; form-action 'none'; base-uri 'none'; frame-ancestors {parent_origin}{web_origins};")
}

#[cfg(test)]
mod csp_tests {
    use super::runtime_csp;

    #[test]
    fn only_verified_parent_and_selected_web_port_can_embed() {
        let csp = runtime_csp(
            "chrome-extension://abcdefghijklmnopabcdefghijklmnop",
            Some(49152),
        );
        assert!(csp.contains("frame-ancestors chrome-extension://abcdefghijklmnopabcdefghijklmnop http://127.0.0.1:49152 http://localhost:49152;"));
        assert!(!csp.contains("chrome-extension: "));
        assert!(!csp.contains(":8770"));
        assert!(csp.contains("style-src 'self' 'unsafe-inline';"));
    }
}
