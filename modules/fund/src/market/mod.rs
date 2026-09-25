//! 行情板块（首页「行情」）：大盘指数 / 主题板块 / 板块 ETF。
//!
//! 数据全部来自公开上游实时接口，禁止 mock。统一走腾讯行情：
//! - A 股指数 + 美股指数 + ETF 实时行情：qt.gtimg.cn 批量接口
//!   （东财 push2 在 rustls 下因服务器不发送 TLS close_notify 而不可用，
//!   已实测并放弃；腾讯接口本仓库 TLS 栈验证可用）
//! - 行业板块排行：proxy.finance.qq.com mktHs/rank
//!
//! 板块→ETF 的对应关系是客观标的映射（代码常量），行情值每次实时拉取；
//! 上游请求失败 fail-closed：返回 APP_UPSTREAM 错误，绝不编造数值。

mod handlers;
mod quotes;
mod sectors;
mod smartbox;
mod util;

pub use handlers::{
    api_market_detail, api_market_etfs, api_market_indices, api_market_kline, api_market_minute,
    api_market_quotes, api_market_sector_stocks, api_market_sectors, api_market_stocks,
    api_market_themes,
};
pub use sectors::SectorItem;
pub use smartbox::{api_market_suggest, normalize_symbol};

#[cfg(test)]
mod tests;
