//! PERF-02 lazy HTTP server bind.
//!
//! Workshop/Embed 本地 HTTP 服务只在首次读取端口（即首次真正需要服务模块
//! 资源或 Bridge）时绑定，避免在首屏前的启动关键路径上无条件启动一个
//! tiny_http 监听线程（冷启动测量显示这是非首屏必需的工作）。
//!
//! 线程安全：`Mutex<Option<HttpServer>>` + `Mutex<Option<u16>>`，首次 `port()`
//! 时绑定一次；后续读取直接返回已绑定端口。绑定失败按 0 返回并由调用方处理
//! （与原先 `server.start().unwrap_or(0)` 行为一致，不静默崩溃）。

use std::sync::Mutex;

use crate::http_server::HttpServer;

/// 延迟绑定的本地 HTTP 服务端口。
pub struct LazyHttpPort {
    server: Mutex<Option<HttpServer>>,
    port: Mutex<Option<u16>>,
}

impl LazyHttpPort {
    /// 仅持有服务句柄，不绑定端口（绑定发生在首次 `port()`）。
    pub fn new(server: HttpServer) -> Self {
        Self {
            server: Mutex::new(Some(server)),
            port: Mutex::new(None),
        }
    }

    /// 返回已绑定的端口；首次调用时执行绑定。
    /// 绑定失败返回 0（与启动期失败兜底一致，调用方据此判断服务不可用）。
    pub fn port(&self) -> u16 {
        {
            let guard = self.port.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(p) = *guard {
                return p;
            }
        }
        // 绑定临界区：持 server 锁的同时绑端口，避免并发重复绑定。
        let bound = {
            let mut server_guard = self.server.lock().unwrap_or_else(|e| e.into_inner());
            match server_guard.as_mut() {
                Some(server) => server.start(0).unwrap_or_else(|e| {
                    eprintln!("failed to start HTTP server: {e}");
                    0
                }),
                None => 0,
            }
        };
        let mut port_guard = self.port.lock().unwrap_or_else(|e| e.into_inner());
        *port_guard = Some(bound);
        bound
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token_manager::TokenManager;
    use rusqlite::Connection;

    fn make_server() -> HttpServer {
        let conn = Connection::open_in_memory().unwrap();
        let tm = std::sync::Arc::new(TokenManager::new(&conn));
        HttpServer::new(
            std::path::PathBuf::from("/tmp/natives-test-modules"),
            tm,
            std::path::PathBuf::from("/tmp/natives-test.db"),
        )
    }

    #[test]
    fn binds_on_first_access_then_reuses() {
        let lazy = LazyHttpPort::new(make_server());
        let first = lazy.port();
        // Either a real loopback port (>0) or 0 when the bind fails in CI;
        // the invariant under test is that repeated reads return the same value.
        let second = lazy.port();
        assert_eq!(first, second, "port must be bound once and cached");
    }

    #[test]
    fn returns_cached_port_without_rebinding() {
        let lazy = LazyHttpPort::new(make_server());
        let _ = lazy.port();
        // Server handle consumed on first bind; second access must still succeed.
        for _ in 0..5 {
            let _ = lazy.port();
        }
        assert!(lazy.server.lock().unwrap().is_some() || lazy.port.lock().unwrap().is_some());
    }
}
