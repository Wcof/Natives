//! Startup phase timing (PERF-01).
//!
//! 生产构建默认静默；仅在设置环境变量 `NATIVES_STARTUP_TIMING=1` 时输出
//! 每个关键启动阶段的耗时与累计毫秒数，用于定位首屏前的耗时排名。
//!
//! 设计约束：
//! - 不引入新依赖；使用 `std::time::Instant`。
//! - 禁止 panic；所有写入经 `Mutex`，读取失败按 0 处理。
//! - 仅诊断用途：脚本读取后即关闭，不污染日常日志（R-B2 不打印敏感信息，
//!   这里只记录阶段名 + 毫秒数）。

use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// 单阶段计时记录。
#[derive(Debug, Clone, Copy)]
pub struct PhaseSpan {
    /// 阶段名（如 "db_init" / "daemon" / "http"）。
    pub name: &'static str,
    /// 该阶段耗时（毫秒）。
    pub millis: u64,
}

struct TimingState {
    enabled: bool,
    start: Instant,
    phases: Mutex<Vec<PhaseSpan>>,
}

static STATE: OnceLock<TimingState> = OnceLock::new();

fn state() -> &'static TimingState {
    STATE.get_or_init(|| TimingState {
        enabled: std::env::var("NATIVES_STARTUP_TIMING")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false),
        start: Instant::now(),
        phases: Mutex::new(Vec::new()),
    })
}

/// 启用诊断输出。进程入口调用一次以提前捕获 env（默认惰性读取也安全）。
pub fn init() {
    let _ = state();
}

/// 是否启用启动阶段计时。未设置环境变量时返回 false（生产静默）。
#[allow(dead_code)]
pub fn enabled() -> bool {
    state().enabled
}

/// 记录一个阶段的耗时。`span` 为该阶段自身的耗时（毫秒）；
/// 同时记录自进程启动以来的累计毫秒数。
pub fn record(name: &'static str, span_ms: u64) {
    let st = state();
    if !st.enabled {
        return;
    }
    if let Ok(mut phases) = st.phases.lock() {
        phases.push(PhaseSpan {
            name,
            millis: span_ms,
        });
    }
}

/// 记录一个阶段：在闭包执行前后取差作为该阶段耗时。
/// 用于 setup 链中同步阻塞步骤的内联测量。
pub fn measure<T>(name: &'static str, f: impl FnOnce() -> T) -> T {
    let st = state();
    if !st.enabled {
        return f();
    }
    let phase_start = Instant::now();
    let out = f();
    let span_ms = phase_start.elapsed().as_millis() as u64;
    if let Ok(mut phases) = st.phases.lock() {
        phases.push(PhaseSpan {
            name,
            millis: span_ms,
        });
    }
    out
}

/// 输出已记录的阶段表与累计启动耗时。仅在 enabled 时生效。
/// 在窗口可交互后调用一次（FOUC guard 解除后），随后阶段表不再增长。
pub fn report() {
    let st = state();
    if !st.enabled {
        return;
    }
    let total_ms = st.start.elapsed().as_millis();
    let phases = st.phases.lock().map(|p| p.clone()).unwrap_or_default();
    eprintln!("[natives.startup] phase timing (ms):");
    for span in &phases {
        eprintln!(
            "[natives.startup]   {:<22} {:>6} ms",
            span.name, span.millis
        );
    }
    eprintln!("[natives.startup] total until report: {total_ms} ms");
}
