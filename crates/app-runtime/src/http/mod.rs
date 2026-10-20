mod client;

pub(crate) use client::{handle_http_client, write_http};

use app_runtime_core::error::{AppErrorBody, AppErrorCode};
use app_runtime_core::framing::write_frame;
use app_runtime_core::protocol::AppResponse;
use app_runtime_core::protocol::MAX_FRAME_BYTES;
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub(crate) fn error_json(message: &str) -> String {
    serde_json::from_str::<serde_json::Value>(message)
        .ok()
        .filter(|v| v.is_object() && (v.get("error").is_some() || v.get("message").is_some()))
        .unwrap_or_else(|| serde_json::json!({"error":message}))
        .to_string()
}

#[cfg(test)]
mod error_tests {
    #[test]
    fn module_errors_are_valid_json_and_keep_their_code() {
        let msg = r#"{"error":"GROUP_PLAN_CONFLICT","message":"请刷新"}"#;
        let value: serde_json::Value = serde_json::from_str(&super::error_json(msg)).unwrap();
        assert_eq!(value["error"], "GROUP_PLAN_CONFLICT");
        assert_eq!(value["message"], "请刷新");
        let text = "quote \" and newline\n";
        let value: serde_json::Value = serde_json::from_str(&super::error_json(text)).unwrap();
        assert_eq!(value["error"], text);
    }
}

pub(crate) fn write_json_response<T: serde::Serialize>(
    id: &str,
    ok: bool,
    result: Option<T>,
    error: Option<AppErrorBody>,
) {
    let body = AppResponse {
        id: id.to_string(),
        ok,
        result,
        error,
    };
    match serde_json::to_vec(&body) {
        Ok(bytes) if bytes.len() <= MAX_FRAME_BYTES => {
            if let Err(err) = write_frame(&mut std::io::stdout().lock(), &bytes) {
                eprintln!("natives-app-runtime: write frame failed: {err:?}");
                std::process::exit(1);
            }
        }
        Ok(_) => {
            eprintln!("natives-app-runtime: response exceeds frame budget");
            std::process::exit(1);
        }
        Err(err) => {
            eprintln!("natives-app-runtime: serialize response failed: {err}");
            std::process::exit(1);
        }
    }
}

pub(crate) fn write_error(
    id: &str,
    code: AppErrorCode,
    message: impl Into<String>,
    retryable: bool,
) {
    write_json_response::<serde_json::Value>(
        id,
        false,
        None,
        Some(AppErrorBody::new(code, message, retryable)),
    );
}

pub(crate) fn dirs_home() -> PathBuf {
    std::env::var("NATIVES_HOME")
        .map(PathBuf::from)
        .or_else(|_| std::env::var("HOME").map(PathBuf::from))
        .unwrap_or_else(|_| PathBuf::from("/"))
}

pub(crate) fn apps_root() -> PathBuf {
    std::env::var("NATIVES_APPS_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            // 与 Core 的 natives_dir_name()（app_signing.rs）保持一致：
            // debug 构建（本地候选）用 ~/.natives-local，否则 ~/.natives。
            let dir = if cfg!(debug_assertions) {
                ".natives-local"
            } else {
                ".natives"
            };
            dirs_home().join(dir).join("apps")
        })
}

/// ADR-0035：Release App Runtime 统一绑定动态端口（127.0.0.1:0），废除固定 8765 抢占。
/// 实际分配端口经 Native Messaging `app:start` 响应 `StartResult.port` 上报。
/// 测试环境可通过环境变量 `NATIVES_APP_RUNTIME_PORT` 注入特定端口。
pub(crate) fn bind_loopback_listener() -> std::io::Result<TcpListener> {
    let port = std::env::var("NATIVES_APP_RUNTIME_PORT")
        .ok()
        .and_then(|v| v.trim().parse::<u16>().ok())
        .unwrap_or(0);
    TcpListener::bind(("127.0.0.1", port))
}

/// HTTP 并发槽的 RAII 守卫：请求线程异常返回（panic 等）也保证计数回落，
/// 不让异常请求永久占用并发槽导致后续全部 503（L15）。
struct HttpSlotGuard;

impl Drop for HttpSlotGuard {
    fn drop(&mut self) {
        crate::ACTIVE_HTTP_REQUESTS.fetch_sub(1, Ordering::SeqCst);
    }
}

pub(crate) fn serve_loopback_http(listener: TcpListener, shutdown: Arc<AtomicBool>) {
    for stream in listener.incoming() {
        if shutdown.load(Ordering::SeqCst) {
            // 关闭路径的唤醒连接或迟到连接：不再处理，直接丢弃。
            drop(stream);
            break;
        }
        match stream {
            Ok(s) => {
                // 并发硬门：超过 max_http_concurrency 直接拒绝，不无限建线程。
                if crate::ACTIVE_HTTP_REQUESTS.load(Ordering::SeqCst)
                    >= crate::HTTP_LIMITS.max_http_concurrency
                {
                    let mut busy = s;
                    let _ = write_http(
                        &mut busy,
                        503,
                        "Service Unavailable",
                        "application/json",
                        b"{\"error\":\"APP_RUNTIME_BUSY\"}",
                    );
                    continue;
                }
                crate::ACTIVE_HTTP_REQUESTS.fetch_add(1, Ordering::SeqCst);
                std::thread::spawn(move || {
                    let _slot = HttpSlotGuard;
                    handle_http_client(s);
                });
            }
            Err(_) => break,
        }
    }
}
