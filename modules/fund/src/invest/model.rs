//! 投资账本 V2 的对外视图模型。
//!
//! 金额与数量一律以**字符串**出入 API：内部是 `i64` 定点（见 `crate::fixed`），
//! 前端不做精度运算。若走 JSON number，`f64` 会在 `>2^53` 或小数位上丢精度，
//! 这是金融数据最常见的静默错误来源。
//!
//! 字段命名对前端用 `type`（serde rename），Rust 侧用 `kind` 规避关键字。

use serde::{Deserialize, Serialize};

/// 账户渠道。渠道天然绑定市场与币种，因此不再单独建 market 维度。
pub const CHANNELS: [&str; 4] = [
    "domestic_broker",
    "fund_platform",
    "overseas_broker",
    "crypto_exchange",
];

/// 资产类别。
pub const ASSET_CLASSES: [&str; 3] = ["stock", "fund", "crypto"];

/// 市场。
pub const MARKETS: [&str; 4] = ["CN", "HK", "US", "CRYPTO"];

/// 流水类型。
pub const FLOW_TYPES: [&str; 5] = ["BUY", "SELL", "DIVIDEND", "SPLIT", "FEE"];

#[derive(Debug, Clone, Serialize)]
pub struct AccountView {
    pub id: i64,
    pub name: String,
    pub channel: String,
    pub currency: String,
    pub note: String,
    pub archived: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstrumentView {
    pub id: i64,
    pub code: String,
    pub name: String,
    pub asset_class: String,
    pub market: String,
    pub currency: String,
    pub group_code: Option<String>,
    pub share_class: Option<String>,
    /// 最新价（4 位定点字符串），无行情时为 null。
    pub latest_price: Option<String>,
    pub latest_price_at: Option<String>,
    /// 价格为陈旧缓存（非当日）时为 true，UI 必须显式提示。
    pub price_stale: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct FlowView {
    pub id: i64,
    pub account_id: i64,
    pub instrument_id: i64,
    #[serde(rename = "type")]
    pub kind: String,
    pub quantity: String,
    pub price: String,
    pub amount: String,
    pub fee: String,
    pub split_from: Option<i64>,
    pub split_to: Option<i64>,
    pub trade_date: String,
    pub note: String,
}

/// 单个持仓。数量、成本、盈亏全部由流水回放得出，不接受外部写入。
#[derive(Debug, Clone, Serialize)]
pub struct HoldingView {
    pub account_id: i64,
    pub account_name: String,
    pub instrument_id: i64,
    pub code: String,
    pub name: String,
    pub asset_class: String,
    pub market: String,
    pub currency: String,
    pub group_code: Option<String>,
    pub share_class: Option<String>,
    /// 持有数量（4 位定点字符串）。
    pub quantity: String,
    /// 持仓均价 = 成本 ÷ 数量。
    pub avg_cost: String,
    /// 持仓总成本。
    pub cost: String,
    /// 最新价；无行情时为空。
    pub price: Option<String>,
    /// 最新价的行情日期，用于判断数据新鲜度。
    pub price_at: Option<String>,
    /// 市值 = 数量 × 最新价；无行情时为 null。
    pub market_value: Option<String>,
    /// 浮动盈亏；无行情时为 null。
    pub unrealized: Option<String>,
    /// 浮动盈亏率（百分比数值，如 8.93 表示 +8.93%）；无行情或成本为 0 时为 null。
    /// 比率不涉及分位精度，直接给数值，前端无需再解析。
    pub unrealized_ratio: Option<f64>,
    /// 已实现盈亏（含分红、费用）。
    pub realized: String,
    /// 持有天数，用于日均收益分母；无买入时为 0。
    pub held_days: i64,
    /// 日均收益 = (市值 − 净投入) ÷ 持有天数。正数表示「每天赚多少」。
    pub daily_gain: Option<String>,
    pub price_stale: bool,
    pub opened_at: Option<String>,
}

/// 总览的单个分类小计。
#[derive(Debug, Clone, Serialize)]
pub struct ClassTotalView {
    pub asset_class: String,
    pub market_value: String,
    pub cost: String,
    pub weight: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OverviewView {
    pub currency: String,
    pub net_worth: String,
    pub cost: String,
    pub unrealized: String,
    pub realized: String,
    pub by_class: Vec<ClassTotalView>,
    /// 快照序列（日期升序）。
    pub series: Vec<SnapshotPointView>,
    /// 有持仓缺行情时为 true，UI 需提示估值不完整。
    pub incomplete: bool,
    pub as_of: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SnapshotPointView {
    pub date: String,
    pub net_worth: String,
    pub cost: String,
}

// ---------- 请求体 ----------

#[derive(Debug, Deserialize)]
pub struct CreateAccount {
    pub name: String,
    pub channel: String,
    pub currency: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateAccount {
    pub name: Option<String>,
    pub note: Option<String>,
    pub archived: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct CreateInstrument {
    pub code: String,
    pub name: String,
    pub asset_class: String,
    pub market: String,
    pub currency: String,
    pub group_code: Option<String>,
    pub share_class: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateInstrument {
    pub name: Option<String>,
    pub group_code: Option<String>,
    pub share_class: Option<String>,
    pub archived: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct CreateFlow {
    pub account_id: i64,
    pub instrument_id: i64,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub quantity: String,
    #[serde(default)]
    pub price: String,
    #[serde(default)]
    pub amount: String,
    #[serde(default)]
    pub fee: String,
    pub split_from: Option<i64>,
    pub split_to: Option<i64>,
    pub trade_date: String,
    #[serde(default)]
    pub note: String,
    /// 手工录入幂等键；同一键重复提交返回既有记录。
    pub request_id: Option<String>,
}
