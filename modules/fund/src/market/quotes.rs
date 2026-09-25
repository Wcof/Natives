//! 行情实时拉取（拆分自 market.rs，仅移动，无逻辑变更）。

use super::util::{json_string, num_or_null};
use std::time::Duration;

pub(crate) const TENCENT_QT: &str = "https://qt.gtimg.cn/q=";

/// 大盘指数（腾讯代码）：上证 / 深成 / 创业板 / 北证50 / 科创50 /
/// 中证500 / 沪深300 / 科创综指。
pub(crate) const CN_INDEX_CODES: &[(&str, &str)] = &[
    ("sh000001", "上证指数"),
    ("sz399001", "深证成指"),
    ("sz399006", "创业板指"),
    ("bj899050", "北证50"),
    ("sh000688", "科创50"),
    ("sh000905", "中证500"),
    ("sh000300", "沪深300"),
    ("sh000680", "科创综指"),
];

/// 美股指数（腾讯代码）。
pub(crate) const US_INDEX_CODES: &[(&str, &str)] = &[
    ("usNDX", "纳斯达克100"),
    ("usDJI", "道琼斯"),
    ("usSPX", "标普500"),
];

/// 主题板块 → 代表 ETF 标的映射（客观代码常量，行情实时拉取）。
/// 覆盖用户指定的 PCB / 存储芯片 / 封装 / 光学 / CPU 主题。
pub(crate) const THEME_ETFS: &[(&str, &[(&str, &str)])] = &[
    (
        "PCB概念",
        &[("sh512260", "PCB ETF"), ("sz159557", "PCB ETF华夏")],
    ),
    (
        "存储芯片",
        &[("sh561920", "存储芯片ETF"), ("sz159825", "芯片ETF基金")],
    ),
    (
        "半导体封装",
        &[("sh561980", "半导体设备ETF"), ("sh512480", "半导体ETF")],
    ),
    (
        "光学光电子",
        &[("sh516730", "光学ETF"), ("sz159732", "消费电子ETF")],
    ),
    (
        "CPU处理器",
        &[("sh560570", "芯片ETF龙头"), ("sh512760", "芯片ETF")],
    ),
    ("半导体材料", &[("sh562590", "半导体材料ETF")]),
];

/// 核心精选 A 股（蓝筹龙头与成长代表）。
pub(crate) const HOT_STOCKS: &[(&str, &str)] = &[
    ("sh600519", "贵州茅台"),
    ("sz300750", "宁德时代"),
    ("sz002594", "比亚迪"),
    ("sh601318", "中国平安"),
    ("sh600036", "招商银行"),
    ("sh600030", "中信证券"),
    ("sz000001", "平安银行"),
    ("sz000858", "五粮液"),
    ("sz000538", "云南白药"),
    ("sh688981", "中芯国际"),
    ("sh600900", "长江电力"),
    ("sz002475", "立讯精密"),
];

/// 核心 ETF 基金（含宽基、行业、黄金商品、国债债券、跨境海外）。
pub(crate) const CORE_ETFS: &[(&str, &str)] = &[
    ("sh510300", "沪深300ETF"),
    ("sh510500", "中证500ETF"),
    ("sz159915", "创业板ETF"),
    ("sh588000", "科创50ETF"),
    ("sh512880", "证券ETF"),
    ("sh512690", "酒ETF"),
    ("sh512480", "半导体ETF"),
    ("sh515050", "5GETF"),
    ("sh518880", "黄金ETF"),
    ("sz159937", "黄金基金ETF"),
    ("sh511010", "国债ETF"),
    ("sh511260", "十年国债ETF"),
    ("sz159920", "恒生ETF"),
    ("sh513050", "中概互联ETF"),
    ("sh513100", "纳指ETF"),
];

pub(crate) fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(10))
        .user_agent("natives-fund/1.0")
        .build()
}

/// 上游响应里的一个行情条目（统一 JSON 输出形状）。
pub(crate) struct Quote {
    pub(crate) code: String,
    pub(crate) name: String,
    pub(crate) price: Option<f64>,
    pub(crate) change_pct: Option<f64>,
    pub(crate) change: Option<f64>,
    pub(crate) volume: Option<f64>,       // 手
    pub(crate) turnover: Option<f64>,     // 换手率 %
    pub(crate) amplitude: Option<f64>,    // 振幅 %
    pub(crate) volume_ratio: Option<f64>, // 量比
}

pub(crate) fn quote_json(quotes: &[Quote]) -> String {
    let items: Vec<String> = quotes
        .iter()
        .map(|q| {
            format!(
                "{{\"code\":{},\"name\":{},\"price\":{},\"changePct\":{},\"change\":{},\"volume\":{},\"turnover\":{},\"amplitude\":{},\"volumeRatio\":{}}}",
                json_string(&q.code),
                json_string(&q.name),
                num_or_null(q.price),
                num_or_null(q.change_pct),
                num_or_null(q.change),
                num_or_null(q.volume),
                num_or_null(q.turnover),
                num_or_null(q.amplitude),
                num_or_null(q.volume_ratio),
            )
        })
        .collect();
    format!("{{\"quotes\":[{}]}}", items.join(","))
}



/// 腾讯 qt 批量行情（A 股指数 / 美股指数 / 股票 / ETF）。
/// 响应为 GBK 编码，通过 encoding_rs 实时动态解码为 UTF-8，
/// 1=名称 3=现价 31=涨跌 32=涨跌% 6=成交量 38=换手率 43=振幅 49=量比，全量使用接口实时返回数据。
pub(crate) fn fetch_tencent_quotes(
    codes: &[&str],
    display_names: Option<&[(usize, &str)]>,
) -> Result<Vec<Quote>, String> {
    let url = format!("{}{}", TENCENT_QT, codes.join(","));
    let mut reader = agent()
        .get(&url)
        .call()
        .map_err(|e| format!("tencent quotes: {e}"))?
        .into_reader();
    let mut bytes = Vec::new();
    use std::io::Read;
    reader
        .read_to_end(&mut bytes)
        .map_err(|e| format!("tencent quotes read: {e}"))?;
    let (decoded, _, _) = encoding_rs::GBK.decode(&bytes);
    let raw = decoded.into_owned();

    let mut quotes = Vec::new();
    for (index, line) in raw.lines().filter(|l| l.contains("=\"")).enumerate() {
        let payload = line.split('"').nth(1).unwrap_or("");
        let fields: Vec<&str> = payload.split('~').collect();
        if fields.len() < 32 {
            continue;
        }
        let parse = |i: usize| -> Option<f64> { fields.get(i).and_then(|v| v.parse::<f64>().ok()) };
        let live_name = fields.get(1).unwrap_or(&"").trim();
        let name = display_names
            .and_then(|map| map.iter().find(|(i, _)| *i == index).map(|(_, n)| *n))
            .map(String::from)
            .unwrap_or_else(|| {
                if !live_name.is_empty() {
                    live_name.to_string()
                } else {
                    codes.get(index).unwrap_or(&"").to_string()
                }
            });
        quotes.push(Quote {
            code: codes.get(index).map(|c| c.to_string()).unwrap_or_default(),
            name,
            price: parse(3),
            change_pct: parse(32),
            change: parse(31),
            volume: parse(6),
            turnover: parse(38),
            amplitude: parse(43),
            volume_ratio: parse(49),
        });
    }
    Ok(quotes)
}
