mod client;

pub(crate) use client::{handle_http_client, write_http};

use app_runtime_core::error::{AppErrorBody, AppErrorCode};
use app_runtime_core::framing::write_frame;
use app_runtime_core::protocol::MAX_FRAME_BYTES;
use app_runtime_core::protocol::AppResponse;
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub(crate) fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for ch in s.chars() {
        match ch {
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

pub(crate) fn write_error(id: &str, code: AppErrorCode, message: impl Into<String>, retryable: bool) {
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

/// ADR-0032：默认绑定固定端口 8765（可用环境变量 `NATIVES_APP_RUNTIME_PORT`
/// 覆盖，设为 `0` 强制动态端口）；默认端口被占用时回退动态端口（`:0`），
/// 实际端口经 `StartResult.port` 上报。绑定始终限于 127.0.0.1。
pub(crate) fn bind_loopback_listener() -> std::io::Result<TcpListener> {
    const DEFAULT_PORT: u16 = 8765;
    let port = std::env::var("NATIVES_APP_RUNTIME_PORT")
        .ok()
        .and_then(|v| v.trim().parse::<u16>().ok())
        .unwrap_or(DEFAULT_PORT);
    match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => Ok(l),
        Err(e) if port != 0 && e.kind() == std::io::ErrorKind::AddrInUse => {
            TcpListener::bind(("127.0.0.1", 0))
        }
        Err(e) => Err(e),
    }
}

pub(crate) fn serve_loopback_http(listener: TcpListener, shutdown: Arc<AtomicBool>) {
    for stream in listener.incoming() {
        if shutdown.load(Ordering::SeqCst) {
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
                    handle_http_client(s);
                    crate::ACTIVE_HTTP_REQUESTS.fetch_sub(1, Ordering::SeqCst);
                });
            }
            Err(_) => break,
        }
    }
}
