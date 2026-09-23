//! Runtime 资源硬门（计划 §16）：任何资源必须有上限，最终数值可实测调整。

/// RuntimeLimits 注入 ModuleContext，HTTP 服务与业务侧共用同一组硬门。
#[derive(Debug, Clone, Copy)]
pub struct RuntimeLimits {
    pub max_header_bytes: usize,
    pub max_request_body_bytes: usize,
    pub max_response_bytes: usize,
    pub max_http_concurrency: usize,
    pub max_workers: usize,
    pub max_page_size: usize,
    pub max_network_timeout_ms: u64,
    pub shutdown_deadline_ms: u64,
}

impl RuntimeLimits {
    /// 编译期可用硬门（供 static 使用；数值与 default 一致）。
    pub const fn hard_gate() -> Self {
        Self {
            max_header_bytes: 16 * 1024,
            max_request_body_bytes: 256 * 1024,
            max_response_bytes: 2 * 1024 * 1024,
            max_http_concurrency: 8,
            max_workers: 8,
            max_page_size: 200,
            max_network_timeout_ms: 10_000,
            shutdown_deadline_ms: 2_000,
        }
    }
}

impl Default for RuntimeLimits {
    fn default() -> Self {
        Self::hard_gate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_plan_hard_gates() {
        let limits = RuntimeLimits::default();
        assert_eq!(limits.max_header_bytes, 16 * 1024);
        assert_eq!(limits.max_request_body_bytes, 256 * 1024);
        assert_eq!(limits.max_response_bytes, 2 * 1024 * 1024);
        assert_eq!(limits.max_http_concurrency, 8);
        assert_eq!(limits.max_workers, 8);
        assert_eq!(limits.shutdown_deadline_ms, 2_000);
    }
}
