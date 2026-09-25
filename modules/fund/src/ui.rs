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

/// 页面骨架仅为开发参考（含装配顺序注释），不直接内嵌；生产 HTML 由下方 INDEX_HTML 编译期装配。
const ECHARTS_JS: &str = include_str!("../ui/dist/echarts.min.js");

/// 页面装配顺序（与 ui/dist/index.html 头部注释保持一致）：
/// head 骨架 → 主题/布局/面板样式 → 图标 sprite → 趋势样式 → 头部与各视图片段 → 侧栏/浮层 → 收尾。
/// 胶水行（<style>/<body>/</html> 等边界标签）内联在下方 concat! 中，片段文件本身保持纯净。
/// 编译期装配的完整页面（等价于拆分前的单一 index.html，字节一致）。
const INDEX_HTML: &str = concat!(
    include_str!("../ui/dist/partials/head.html"),
    "<style>\n",
    include_str!("../ui/dist/styles/theme.css"),
    include_str!("../ui/dist/styles/layout.css"),
    include_str!("../ui/dist/styles/panel.css"),
    "</style>\n</head>\n<body>\n",
    include_str!("../ui/dist/partials/svg-symbols.html"),
    "  <style>\n",
    include_str!("../ui/dist/styles/trends.css"),
    include_str!("../ui/dist/styles/trends-cards.css"),
    "</style>\n",
    include_str!("../ui/dist/partials/header.html"),
    include_str!("../ui/dist/partials/view-watchlist.html"),
    include_str!("../ui/dist/partials/view-market.html"),
    include_str!("../ui/dist/partials/view-trends.html"),
    include_str!("../ui/dist/partials/view-positions.html"),
    include_str!("../ui/dist/partials/view-tx.html"),
    include_str!("../ui/dist/partials/view-import.html"),
    include_str!("../ui/dist/partials/view-nav.html"),
    include_str!("../ui/dist/partials/side-panel.html"),
    include_str!("../ui/dist/partials/overlays.html"),
    "<script src=\"app.js\"></script>\n</body>\n</html>\n",
);

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
    include_str!("../ui/dist/chart-drawings.js"),
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
    include_str!("../ui/dist/trends-flow.js"),
    "\n",
    include_str!("../ui/dist/trends-drawings.js"),
    "\n",
    include_str!("../ui/dist/trends-panel.js"),
    "\n",
    include_str!("../ui/dist/trends-watch.js"),
    "\n",
    include_str!("../ui/dist/trends-scan.js"),
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
