//! 内嵌 UI（构建期嵌入的静态资源；无外联、无远程代码，CSP 兼容）。
//!
//! 源文件维护在 `ui/dist/`（index.html + 按能力域拆分的 JS 模块，IIFE +
//! `globalThis.Fund` 命名空间协作）。此处按依赖顺序编译期拼接为单一
//! `/app.js` bundle 响应，页面只发起一次脚本请求；同时保留源文件独立
//! 维护的行数阈值合规（CODE_MODULE_GUIDELINES §2 / §6）。
//!
//! 投资终端 (Invest Terminal) — 1:1 原生复刻 token-monitor 架构：
//! 1. Overview 面板：顶部 KPI 监控矩阵（总览/多空比/领涨/领跌/预警）、自选列表、60px 原生 Canvas Sparkline、红绿跳动闪烁。
//! 2. Trends 面板：多周期走势图（分时 VWAP、天、周、月、7日）、当前价格水平虚线标尺、极值标注、十字标尺 Tooltip。
//! 3. Alerts 预警引擎：日内涨跌幅/突破规则监听、Chrome Badge 动态红绿角标联动、系统通知。
//! 4. 数据管道通信：全双工 WebSocket (ws://127.0.0.1:8765/ws) + Loopback HTTP 弹性回退。

const INDEX_HTML: &str = include_str!("../ui/dist/index.html");
const ECHARTS_JS: &str = include_str!("../ui/dist/echarts.min.js");

/// bundle 拼接顺序 = 依赖顺序：core 基座 → 调度基座 → 领域模块（注册 actions）→ app controller。
const BUNDLE_JS: &str = concat!(
    include_str!("../ui/dist/core.js"),
    "\n",
    include_str!("../ui/dist/market-session.js"),
    "\n",
    include_str!("../ui/dist/dashboard.js"),
    "\n",
    include_str!("../ui/dist/watchlist-table.js"),
    "\n",
    include_str!("../ui/dist/market-table.js"),
    "\n",
    include_str!("../ui/dist/chart.js"),
    "\n",
    include_str!("../ui/dist/ledger.js"),
    "\n",
    include_str!("../ui/dist/focus.js"),
    "\n",
    include_str!("../ui/dist/trends-chart.js"),
    "\n",
    include_str!("../ui/dist/trends-indicator.js"),
    "\n",
    include_str!("../ui/dist/trends-chan.js"),
    "\n",
    include_str!("../ui/dist/trends-panel.js"),
    "\n",
    include_str!("../ui/dist/trends-watch.js"),
    "\n",
    include_str!("../ui/dist/trends.js"),
    "\n",
    include_str!("../ui/dist/app.js"),
);

pub fn index_html() -> String {
    INDEX_HTML.to_string()
}

/// 静态 JS 资源；未命中返回 None（由调用方决定 404）。
pub fn static_js(path: &str) -> Option<String> {
    match path {
        "/app.js" => Some(BUNDLE_JS.to_string()),
        "/echarts.min.js" => Some(ECHARTS_JS.to_string()),
        _ => None,
    }
}
