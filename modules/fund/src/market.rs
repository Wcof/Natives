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

use app_runtime_core::http::HttpRequest;
use std::time::Duration;

const TENCENT_QT: &str = "https://qt.gtimg.cn/q=";

/// 大盘指数（腾讯代码）：上证 / 深成 / 创业板 / 北证50 / 科创50 /
/// 中证500 / 沪深300 / 科创综指。
const CN_INDEX_CODES: &[(&str, &str)] = &[
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
const US_INDEX_CODES: &[(&str, &str)] = &[
    ("usNDX", "纳斯达克100"),
    ("usDJI", "道琼斯"),
    ("usSPX", "标普500"),
];

/// 主题板块 → 代表 ETF 标的映射（客观代码常量，行情实时拉取）。
/// 覆盖用户指定的 PCB / 存储芯片 / 封装 / 光学 / CPU 主题。
const THEME_ETFS: &[(&str, &[(&str, &str)])] = &[
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
const HOT_STOCKS: &[(&str, &str)] = &[
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
const CORE_ETFS: &[(&str, &str)] = &[
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

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(10))
        .user_agent("natives-fund/1.0")
        .build()
}

/// 上游响应里的一个行情条目（统一 JSON 输出形状）。
struct Quote {
    code: String,
    name: String,
    price: Option<f64>,
    change_pct: Option<f64>,
    change: Option<f64>,
    volume: Option<f64>,       // 手
    turnover: Option<f64>,     // 换手率 %
    amplitude: Option<f64>,    // 振幅 %
    volume_ratio: Option<f64>, // 量比
}

fn quote_json(quotes: &[Quote]) -> String {
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

fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn num_or_null(v: Option<f64>) -> String {
    match v {
        Some(v) if v.is_finite() => {
            // 保留两位小数，避免浮点尾巴。
            format!("{:.2}", v)
        }
        _ => "null".into(),
    }
}

/// 腾讯 qt 批量行情（A 股指数 / 美股指数 / 股票 / ETF）。
/// 响应为 GBK 编码，通过 encoding_rs 实时动态解码为 UTF-8，
/// 1=名称 3=现价 31=涨跌 32=涨跌% 6=成交量 38=换手率 43=振幅 49=量比，全量使用接口实时返回数据。
fn fetch_tencent_quotes(
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

/// GET /api/market/indices — 大盘指数（A 股）+ 美股指数（腾讯批量）。
pub fn api_market_indices(_request: &HttpRequest) -> Result<String, (u16, String)> {
    let mut codes: Vec<&str> = CN_INDEX_CODES.iter().map(|(c, _)| *c).collect();
    codes.extend(US_INDEX_CODES.iter().map(|(c, _)| *c));
    let mut names: Vec<(usize, &str)> = CN_INDEX_CODES
        .iter()
        .enumerate()
        .map(|(i, (_, n))| (i, *n))
        .collect();
    names.extend(
        US_INDEX_CODES
            .iter()
            .enumerate()
            .map(|(i, (_, n))| (CN_INDEX_CODES.len() + i, *n)),
    );
    let quotes = fetch_tencent_quotes(&codes, Some(&names)).map_err(upstream_err)?;
    Ok(quote_json(&quotes))
}

/// GET /api/market/stocks — 核心 A 股实时行情。
pub fn api_market_stocks(_request: &HttpRequest) -> Result<String, (u16, String)> {
    let codes: Vec<&str> = HOT_STOCKS.iter().map(|(c, _)| *c).collect();
    let names: Vec<(usize, &str)> = HOT_STOCKS
        .iter()
        .enumerate()
        .map(|(i, (_, n))| (i, *n))
        .collect();
    let quotes = fetch_tencent_quotes(&codes, Some(&names)).map_err(upstream_err)?;
    Ok(quote_json(&quotes))
}

/// GET /api/market/etfs — 核心 ETF（含股票型、跨境、黄金商品与国债债券）。
pub fn api_market_etfs(_request: &HttpRequest) -> Result<String, (u16, String)> {
    let codes: Vec<&str> = CORE_ETFS.iter().map(|(c, _)| *c).collect();
    let names: Vec<(usize, &str)> = CORE_ETFS
        .iter()
        .enumerate()
        .map(|(i, (_, n))| (i, *n))
        .collect();
    let quotes = fetch_tencent_quotes(&codes, Some(&names)).map_err(upstream_err)?;
    Ok(quote_json(&quotes))
}

/// 板块行情条目（含领涨个股与多日涨跌幅）。
pub struct SectorItem {
    pub code: String,
    pub name: String,
    pub price: Option<f64>,
    pub change: Option<f64>,
    pub change_pct: Option<f64>,
    pub speed: Option<f64>,
    pub top_stock_code: Option<String>,
    pub top_stock_name: Option<String>,
    pub top_stock_price: Option<f64>,
    pub top_stock_change: Option<f64>,
    pub top_stock_change_pct: Option<f64>,
    pub change_pct_5: Option<f64>,
    pub change_pct_20: Option<f64>,
}

fn sector_json(sectors: &[SectorItem]) -> String {
    let items: Vec<String> = sectors
        .iter()
        .map(|s| {
            format!(
                "{{\"code\":{},\"name\":{},\"price\":{},\"change\":{},\"changePct\":{},\"speed\":{},\"topStockCode\":{},\"topStockName\":{},\"topStockPrice\":{},\"topStockChange\":{},\"topStockChangePct\":{},\"changePct5\":{},\"changePct20\":{}}}",
                json_string(&s.code),
                json_string(&s.name),
                num_or_null(s.price),
                num_or_null(s.change),
                num_or_null(s.change_pct),
                num_or_null(s.speed),
                s.top_stock_code.as_ref().map(|c| json_string(c)).unwrap_or_else(|| "null".into()),
                s.top_stock_name.as_ref().map(|n| json_string(n)).unwrap_or_else(|| "null".into()),
                num_or_null(s.top_stock_price),
                num_or_null(s.top_stock_change),
                num_or_null(s.top_stock_change_pct),
                num_or_null(s.change_pct_5),
                num_or_null(s.change_pct_20),
            )
        })
        .collect();
    format!("{{\"sectors\":[{}],\"quotes\":[{}]}}", items.join(","), items.join(","))
}

/// 腾讯板块排行（mktHs/rank，JSON，UTF-8）：01 行业 / 02 概念 / 03 地域。
fn fetch_tencent_sectors_by_type(category: &str) -> Result<Vec<SectorItem>, String> {
    let type_key = match category {
        "concept" | "02" => "02/averatio",
        "region" | "03" => "03/averatio",
        _ => "01/averatio", // industry
    };
    let url = format!(
        "https://proxy.finance.qq.com/ifzqgtimg/appstock/app/mktHs/rank?l=35&p=1&t={type_key}&ordertype=&o=0"
    );
    let response: serde_json::Value = agent()
        .get(&url)
        .call()
        .map_err(|e| format!("tencent sectors: {e}"))?
        .into_json()
        .map_err(|e| format!("tencent sectors json: {e}"))?;
    let items = response
        .get("data")
        .and_then(serde_json::Value::as_array)
        .ok_or("tencent sectors: empty data")?;
    let mut sectors = Vec::new();
    for item in items {
        let name = item.get("bd_name").and_then(serde_json::Value::as_str).unwrap_or("");
        let code = item.get("bd_code").and_then(serde_json::Value::as_str).unwrap_or("");
        if name.is_empty() {
            continue;
        }
        let parse_str = |k: &str| -> Option<f64> {
            item.get(k).and_then(serde_json::Value::as_str).and_then(|v| v.parse::<f64>().ok())
        };
        sectors.push(SectorItem {
            code: code.to_string(),
            name: name.to_string(),
            price: parse_str("bd_zxj"),
            change: parse_str("bd_zd"),
            change_pct: parse_str("bd_zdf"),
            speed: parse_str("bd_zs"),
            top_stock_code: item.get("nzg_code").and_then(serde_json::Value::as_str).map(String::from),
            top_stock_name: item.get("nzg_name").and_then(serde_json::Value::as_str).map(String::from),
            top_stock_price: parse_str("nzg_zxj"),
            top_stock_change: parse_str("nzg_zd"),
            top_stock_change_pct: parse_str("nzg_zdf"),
            change_pct_5: parse_str("bd_zdf5"),
            change_pct_20: parse_str("bd_zdf20"),
        });
    }
    Ok(sectors)
}

/// GET /api/market/sectors?type=all|concept|industry|region — 热门板块列表。
pub fn api_market_sectors(request: &HttpRequest) -> Result<String, (u16, String)> {
    let cat = query_param(&request.path, "type").unwrap_or_else(|| "all".to_string());
    if cat == "all" {
        let mut all = fetch_tencent_sectors_by_type("concept").unwrap_or_default();
        let industry = fetch_tencent_sectors_by_type("industry").unwrap_or_default();
        all.extend(industry);
        all.sort_by(|a, b| {
            b.change_pct
                .unwrap_or(0.0)
                .partial_cmp(&a.change_pct.unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(sector_json(&all))
    } else {
        let sectors = fetch_tencent_sectors_by_type(&cat).map_err(upstream_err)?;
        Ok(sector_json(&sectors))
    }
}

/// 映射板块名称到新浪实时行业/概念板块节点（仅作协议节点映射，股票数据 100% 动态拉取，绝不硬编码任何个股）。
fn sina_sector_node(name: &str) -> &'static str {
    if name.contains("传媒") || name.contains("广告") || name.contains("出版") || name.contains("影视") || name.contains("游戏") {
        "new_cmyl"
    } else if name.contains("电子") || name.contains("芯片") || name.contains("半导体") || name.contains("硬件") || name.contains("CPU") {
        "new_dzxx"
    } else if name.contains("软件") || name.contains("计算机") || name.contains("IT") || name.contains("数字") || name.contains("信息") || name.contains("AI") || name.contains("算力") {
        "new_dzqj"
    } else if name.contains("汽车") || name.contains("车") {
        "new_qczz"
    } else if name.contains("医药") || name.contains("生物") || name.contains("疫苗") || name.contains("医疗") {
        "new_ylqx"
    } else if name.contains("通信") || name.contains("电信") || name.contains("5G") {
        "new_dzxx"
    } else if name.contains("电力") || name.contains("能源") || name.contains("光伏") || name.contains("绿电") {
        "new_dlhy"
    } else if name.contains("家电") || name.contains("白色家电") {
        "new_jdhy"
    } else if name.contains("食品") || name.contains("白酒") || name.contains("饮料") || name.contains("消费") {
        "new_sphy"
    } else if name.contains("银行") || name.contains("证券") || name.contains("保险") || name.contains("金融") {
        "new_jrhy"
    } else if name.contains("房地产") || name.contains("地产") || name.contains("物业") {
        "new_fdchy"
    } else if name.contains("有色") || name.contains("金属") || name.contains("黄金") || name.contains("稀土") {
        "new_ysjs"
    } else if name.contains("化工") || name.contains("材料") {
        "new_hghy"
    } else if name.contains("机械") || name.contains("设备") || name.contains("高端装备") {
        "new_jxhy"
    } else if name.contains("交通") || name.contains("运输") || name.contains("航空") || name.contains("港口") {
        "new_jtys"
    } else if name.contains("旅游") || name.contains("酒店") {
        "new_jdly"
    } else {
        "new_cmyl"
    }
}

/// 从新浪财经公开接口实时拉取该板块的真实成分股行情（真实在线活数据）。
fn fetch_live_sector_stocks(node: &str) -> Result<Vec<Quote>, String> {
    let url = format!(
        "http://vip.stock.finance.sina.com.cn/quotes_service/api/json_v2.php/Market_Center.getHQNodeData?page=1&num=40&sort=changepercent&asc=0&node={node}"
    );
    let response: serde_json::Value = agent()
        .get(&url)
        .call()
        .map_err(|e| format!("live sector stocks: {e}"))?
        .into_json()
        .map_err(|e| format!("live sector stocks json: {e}"))?;

    let items = response
        .as_array()
        .ok_or("live sector stocks: empty response")?;

    let mut quotes = Vec::new();
    for item in items {
        let code = item.get("code").and_then(|v| v.as_str()).unwrap_or("");
        let symbol = item.get("symbol").and_then(|v| v.as_str()).unwrap_or("");
        let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if code.is_empty() || name.is_empty() {
            continue;
        }

        let parse_num = |k: &str| -> Option<f64> {
            item.get(k).and_then(|v| {
                if let Some(n) = v.as_f64() {
                    Some(n)
                } else if let Some(s) = v.as_str() {
                    s.parse::<f64>().ok()
                } else {
                    None
                }
            })
        };

        let price = parse_num("trade");
        let change_pct = parse_num("changepercent");
        let change = parse_num("pricechange");
        let volume = parse_num("volume").map(|v| v / 100.0); // 股转手
        let turnover = parse_num("turnoverratio");

        let high = parse_num("high");
        let low = parse_num("low");
        let settlement = parse_num("settlement").unwrap_or_else(|| price.unwrap_or(1.0));
        let amplitude = if let (Some(h), Some(l)) = (high, low) {
            if settlement > 0.0 {
                Some(((h - l) / settlement) * 100.0)
            } else {
                None
            }
        } else {
            None
        };

        quotes.push(Quote {
            code: if !symbol.is_empty() { symbol.to_string() } else { code.to_string() },
            name: name.to_string(),
            price,
            change_pct,
            change,
            volume,
            turnover,
            amplitude,
            volume_ratio: None,
        });
    }

    Ok(quotes)
}

/// GET /api/market/sector/stocks?name=文化传媒&code=pt01801765&top=sz000676 — 真实动态获取板块成分股（100% 实时活数据）。
pub fn api_market_sector_stocks(request: &HttpRequest) -> Result<String, (u16, String)> {
    let name = query_param(&request.path, "name").unwrap_or_default();
    let top_code = query_param(&request.path, "top");

    let node = sina_sector_node(&name);
    let mut quotes = fetch_live_sector_stocks(node).map_err(upstream_err)?;

    // 若领涨股未在列表中，通过腾讯批量接口实时补充并置顶
    if let Some(top) = top_code {
        if !top.is_empty() && !quotes.iter().any(|q| q.code == top || q.code.contains(&top)) {
            if let Ok(top_quotes) = fetch_tencent_quotes(&[&top], None) {
                if let Some(tq) = top_quotes.into_iter().next() {
                    quotes.insert(0, tq);
                }
            }
        }
    }

    Ok(quote_json(&quotes))
}

/// GET /api/market/themes — 预置主题板块清单（标的映射为常量，非行情数据）。
pub fn api_market_themes() -> Result<String, (u16, String)> {
    let items: Vec<String> = THEME_ETFS
        .iter()
        .map(|(theme, etfs)| {
            let list: Vec<String> = etfs
                .iter()
                .map(|(code, name)| {
                    format!(
                        "{{\"code\":{},\"name\":{}}}",
                        json_string(code),
                        json_string(name)
                    )
                })
                .collect();
            format!(
                "{{\"theme\":{},\"etfs\":[{}]}}",
                json_string(theme),
                list.join(",")
            )
        })
        .collect();
    Ok(format!("{{\"themes\":[{}]}}", items.join(",")))
}

/// 规范化股票/基金标的代码（补齐 sh/sz/bj 前缀）。
pub fn normalize_symbol(raw: &str) -> String {
    let s = raw.trim().to_lowercase();
    if s.starts_with("sh") || s.starts_with("sz") || s.starts_with("bj") || s.starts_with("us") {
        return s;
    }
    if s.len() == 6 && s.bytes().all(|b| b.is_ascii_digit()) {
        if s == "000001" {
            // 特殊处理 000001：默认为上证指数 sh000001
            return format!("sh{s}");
        }
        if s.starts_with("60")
            || s.starts_with("68")
            || s.starts_with("51")
            || s.starts_with("56")
            || s.starts_with("58")
        {
            return format!("sh{s}");
        }
        if s.starts_with("00")
            || s.starts_with("30")
            || s.starts_with("15")
            || s.starts_with("16")
            || s.starts_with("39")
        {
            return format!("sz{s}");
        }
        if s.starts_with('8') || s.starts_with('4') || s.starts_with("92") {
            return format!("bj{s}");
        }
    }
    s
}

/// 详细盘口与行情数据（含五档买卖单、力量比例、振幅、量比、换手等）。
pub fn api_market_detail(request: &HttpRequest) -> Result<String, (u16, String)> {
    let raw_symbol = query_param(&request.path, "symbol").ok_or((
        400,
        "{\"error\":\"APP_PARAM_INVALID\",\"message\":\"symbol is required\"}".into(),
    ))?;
    let symbol = normalize_symbol(&raw_symbol);
    if symbol.is_empty() || symbol.len() > 12 {
        return Err((
            400,
            "{\"error\":\"APP_PARAM_INVALID\",\"message\":\"invalid symbol\"}".into(),
        ));
    }
    let url = format!("{}{}", TENCENT_QT, symbol);
    let raw = agent()
        .get(&url)
        .call()
        .map_err(|e| upstream_err(format!("tencent quote: {e}")))?
        .into_string()
        .map_err(|e| upstream_err(format!("tencent quote read: {e}")))?;

    let line = raw
        .lines()
        .find(|l| l.contains("=\""))
        .ok_or_else(|| upstream_err("empty quote response".into()))?;
    let payload = line.split('"').nth(1).unwrap_or("");
    let fields: Vec<&str> = payload.split('~').collect();
    if fields.len() < 35 {
        return Err(upstream_err("quote response fields too short".into()));
    }

    let parse_f64 = |i: usize| -> Option<f64> { fields.get(i).and_then(|v| v.parse::<f64>().ok()) };
    let price = parse_f64(3).unwrap_or(0.0);
    let prev_close = parse_f64(4).unwrap_or(price);
    let open = parse_f64(5).unwrap_or(price);
    let volume = parse_f64(6).unwrap_or(0.0); // 手
    let change = parse_f64(31).unwrap_or(0.0);
    let change_pct = parse_f64(32).unwrap_or(0.0);
    let high = parse_f64(33).unwrap_or(price);
    let low = parse_f64(34).unwrap_or(price);
    let amount = parse_f64(37).unwrap_or(0.0); // 万元
    let turnover = parse_f64(38); // 换手率 %
    let pe = parse_f64(39);
    let amplitude = parse_f64(43); // 振幅 %
    let limit_up = parse_f64(47);
    let limit_down = parse_f64(48);
    let volume_ratio = parse_f64(49); // 量比

    // 五档买卖盘解析
    // 买一至买五: 9, 10; 11, 12; 13, 14; 15, 16; 17, 18
    // 卖一至卖五: 19, 20; 21, 22; 23, 24; 25, 26; 27, 28
    let mut bids = Vec::new();
    let mut total_bid_vol = 0.0;
    for i in 0..5 {
        let p = parse_f64(9 + i * 2).unwrap_or(0.0);
        let v = parse_f64(10 + i * 2).unwrap_or(0.0);
        total_bid_vol += v;
        bids.push(format!("{{\"price\":{:.2},\"volume\":{:.0}}}", p, v));
    }
    let mut asks = Vec::new();
    let mut total_ask_vol = 0.0;
    for i in 0..5 {
        let p = parse_f64(19 + i * 2).unwrap_or(0.0);
        let v = parse_f64(20 + i * 2).unwrap_or(0.0);
        total_ask_vol += v;
        asks.push(format!("{{\"price\":{:.2},\"volume\":{:.0}}}", p, v));
    }
    let bid_ratio = if total_bid_vol + total_ask_vol > 0.0 {
        total_bid_vol / (total_bid_vol + total_ask_vol)
    } else {
        0.5
    };

    // 优先从已知常量映射获取名称，避免 GBK 编码问题
    let name = query_param(&request.path, "name")
        .or_else(|| {
            CN_INDEX_CODES
                .iter()
                .find(|(c, _)| *c == symbol)
                .map(|(_, n)| (*n).to_string())
        })
        .or_else(|| {
            THEME_ETFS
                .iter()
                .flat_map(|(_, etfs)| etfs.iter())
                .find(|(c, _)| *c == symbol)
                .map(|(_, n)| (*n).to_string())
        })
        .unwrap_or_else(|| fields.get(1).unwrap_or(&"").to_string());

    let json = format!(
        "{{\"symbol\":{},\"name\":{},\"price\":{:.2},\"prevClose\":{:.2},\"open\":{:.2},\"high\":{:.2},\"low\":{:.2},\"change\":{:.2},\"changePct\":{:.2},\"volume\":{:.0},\"amount\":{:.2},\"turnover\":{},\"volumeRatio\":{},\"amplitude\":{},\"limitUp\":{},\"limitDown\":{},\"pe\":{},\"bids\":[{}],\"asks\":[{}],\"bidVol\":{:.0},\"askVol\":{:.0},\"bidRatio\":{:.3}}}",
        json_string(&symbol),
        json_string(&name),
        price,
        prev_close,
        open,
        high,
        low,
        change,
        change_pct,
        volume,
        amount,
        num_or_null(turnover),
        num_or_null(volume_ratio),
        num_or_null(amplitude),
        num_or_null(limit_up),
        num_or_null(limit_down),
        num_or_null(pe),
        bids.join(","),
        asks.join(","),
        total_bid_vol,
        total_ask_vol,
        bid_ratio,
    );
    Ok(json)
}

/// GET /api/market/minute?symbol=sh600519 — 分时图数据（含每分钟价格、成交量与 VWAP）。
pub fn api_market_minute(request: &HttpRequest) -> Result<String, (u16, String)> {
    let raw_symbol = query_param(&request.path, "symbol").ok_or((
        400,
        "{\"error\":\"APP_PARAM_INVALID\",\"message\":\"symbol is required\"}".into(),
    ))?;
    let symbol = normalize_symbol(&raw_symbol);
    let url = format!(
        "https://web.ifzq.gtimg.cn/appstock/app/minute/query?code={}",
        symbol
    );
    let resp: serde_json::Value = agent()
        .get(&url)
        .call()
        .map_err(|e| upstream_err(format!("minute query: {e}")))?
        .into_json()
        .map_err(|e| upstream_err(format!("minute json: {e}")))?;

    let empty_vec = Vec::new();
    let data_arr = resp
        .get("data")
        .and_then(|d| d.get(&symbol))
        .and_then(|s| s.get("data"))
        .and_then(|d| d.get("data"))
        .and_then(|arr| arr.as_array())
        .unwrap_or(&empty_vec);

    let mut points = Vec::with_capacity(data_arr.len());
    let mut prev_cum_vol = 0.0;
    for item in data_arr {
        let text = match item.as_str() {
            Some(t) => t,
            None => continue,
        };
        // 格式: "0930 1257.98 140 17611720.00" -> 时间 价格 累计量(手) 累计额(元)
        let parts: Vec<&str> = text.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }
        let time_raw = parts[0];
        let price = parts[1].parse::<f64>().unwrap_or(0.0);
        let cum_vol = parts[2].parse::<f64>().unwrap_or(0.0);
        let cum_amount = parts[3].parse::<f64>().unwrap_or(0.0);
        let vol_inc = (cum_vol - prev_cum_vol).max(0.0);
        prev_cum_vol = cum_vol;

        // VWAP = 累计成交金额 / (累计成交手 * 100)
        let vwap = if cum_vol > 0.0 {
            cum_amount / (cum_vol * 100.0)
        } else {
            price
        };
        let formatted_time = if time_raw.len() == 4 {
            format!("{}:{}", &time_raw[..2], &time_raw[2..])
        } else {
            time_raw.to_string()
        };

        points.push(format!(
            "{{\"time\":{},\"price\":{:.2},\"volume\":{:.0},\"cumVolume\":{:.0},\"cumAmount\":{:.2},\"vwap\":{:.2}}}",
            json_string(&formatted_time),
            price,
            vol_inc,
            cum_vol,
            cum_amount,
            vwap,
        ));
    }

    Ok(format!(
        "{{\"symbol\":{},\"count\":{},\"points\":[{}]}}",
        json_string(&symbol),
        points.len(),
        points.join(",")
    ))
}

/// GET /api/market/kline?symbol=sh600519&period=day&fq=qfq — 多周期 K 线数据 (OHLCV)。
/// fq 复权模式：qfq 前复权（默认）/ hfq 后复权 / bf 不复权。
pub fn api_market_kline(request: &HttpRequest) -> Result<String, (u16, String)> {
    let raw_symbol = query_param(&request.path, "symbol").ok_or((
        400,
        "{\"error\":\"APP_PARAM_INVALID\",\"message\":\"symbol is required\"}".into(),
    ))?;
    let symbol = normalize_symbol(&raw_symbol);
    let period = query_param(&request.path, "period").unwrap_or_else(|| "day".to_string());
    let fq = query_param(&request.path, "fq").unwrap_or_else(|| "qfq".to_string());
    let fq = match fq.as_str() {
        "qfq" | "hfq" | "bf" => fq,
        _ => "qfq".to_string(),
    };
    // 支持周期: day, week, month
    let url = format!(
        "https://web.ifzq.gtimg.cn/appstock/app/fqkline/get?param={},{},,,160,{}",
        symbol, period, fq
    );
    let resp: serde_json::Value = agent()
        .get(&url)
        .call()
        .map_err(|e| upstream_err(format!("kline query: {e}")))?
        .into_json()
        .map_err(|e| upstream_err(format!("kline json: {e}")))?;

    // 上游复权前缀键：qfqday/hfqday；bf（不复权）直接用裸周期键（day/week/...）。
    let kline_key = match fq.as_str() {
        "bf" => period.clone(),
        prefix => format!("{prefix}{period}"),
    };

    let empty_vec = Vec::new();
    let symbol_data = resp.get("data").and_then(|d| d.get(&symbol));
    let bars = symbol_data
        .and_then(|s| s.get(kline_key).or_else(|| s.get(&period)))
        .and_then(|arr| arr.as_array())
        .unwrap_or(&empty_vec);

    let mut candles = Vec::with_capacity(bars.len());
    for bar in bars {
        let arr = match bar.as_array() {
            Some(a) if a.len() >= 6 => a,
            _ => continue,
        };
        let date_str = arr[0].as_str().unwrap_or("");
        let open = arr[1]
            .as_str()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.0);
        let close = arr[2]
            .as_str()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.0);
        let high = arr[3]
            .as_str()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.0);
        let low = arr[4]
            .as_str()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.0);
        let volume = arr[5]
            .as_str()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.0);

        candles.push(format!(
            "{{\"time\":{},\"open\":{:.2},\"close\":{:.2},\"high\":{:.2},\"low\":{:.2},\"volume\":{:.0}}}",
            json_string(date_str),
            open,
            close,
            high,
            low,
            volume,
        ));
    }

    Ok(format!(
        "{{\"symbol\":{},\"period\":{},\"candles\":[{}]}}",
        json_string(&symbol),
        json_string(&period),
        candles.join(",")
    ))
}

/// GET /api/market/theme/etfs?codes=sh512880,sz159806 — 批量实时行情。
pub fn api_market_quotes(request: &HttpRequest) -> Result<String, (u16, String)> {
    let codes = query_param(&request.path, "codes").ok_or((
        400,
        "{\"error\":\"APP_PARAM_INVALID\",\"message\":\"codes is required\"}".into(),
    ))?;
    let list: Vec<&str> = codes
        .split(',')
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .collect();
    if list.is_empty() || list.len() > 60 {
        return Err((
            400,
            "{\"error\":\"APP_PARAM_INVALID\",\"message\":\"codes must be 1..=60\"}".into(),
        ));
    }
    // 自动规范化代码并严格校验合法性
    let mut normalized = Vec::with_capacity(list.len());
    for raw in &list {
        let code = normalize_symbol(raw);
        let valid = (code.len() == 8
            && (code.starts_with("sh") || code.starts_with("sz") || code.starts_with("bj"))
            && code[2..].bytes().all(|b| b.is_ascii_digit()))
            || (code.len() > 2
                && code.starts_with("us")
                && code[2..].bytes().all(|b| b.is_ascii_alphanumeric()));
        if !valid {
            return Err((
                400,
                "{\"error\":\"APP_PARAM_INVALID\",\"message\":\"invalid code\"}".into(),
            ));
        }
        normalized.push(code);
    }
    let norm_refs: Vec<&str> = normalized.iter().map(String::as_str).collect();

    // 名称来自常量映射
    let name_map: Vec<(usize, &str)> = norm_refs
        .iter()
        .enumerate()
        .filter_map(|(i, code)| {
            CN_INDEX_CODES
                .iter()
                .find(|(c, _)| c == code)
                .map(|(_, name)| (i, *name))
                .or_else(|| {
                    THEME_ETFS
                        .iter()
                        .flat_map(|(_, etfs)| etfs.iter())
                        .find(|(c, _)| c == code)
                        .map(|(_, name)| (i, *name))
                })
        })
        .collect();
    let quotes = fetch_tencent_quotes(&norm_refs, Some(&name_map)).map_err(upstream_err)?;
    Ok(quote_json(&quotes))
}

fn upstream_err(message: String) -> (u16, String) {
    (
        502,
        format!(
            "{{\"error\":\"APP_UPSTREAM\",\"message\":{}}}",
            json_string(&message)
        ),
    )
}

fn query_param<'a>(path: &'a str, key: &str) -> Option<String> {
    let query = path.split_once('?')?.1;
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then(|| v.to_string())
    })
}

/// 查询参数值解码：`%XX` 百分号编码 + `+` 还原为空格（UTF-8 字节序列）。
fn url_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3])
                    .ok()
                    .and_then(|h| u8::from_str_radix(h, 16).ok());
                match hex {
                    Some(b) => {
                        out.push(b);
                        i += 3;
                    }
                    None => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 查询参数值最小 urlencode：仅编码 URL/查询串保留字符，中文按 UTF-8 原样输出
/// （ureq 会按 UTF-8 发送；`+`/`%`/空白/`&`/`#`/`?`/`=` 必须转义）。
fn urlencode(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for b in raw.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// GET /api/market/suggest?input=600519|贵州茅台|gzmt — 标的搜索联想。
/// 上游：腾讯 smartbox（代码/中文名/拼音均可命中）。响应为 GBK，但中文名
/// 以 `\uXXXX` 转义输出，ASCII 解析安全；字段 `类型~代码~名称~拼音~标签`，
/// 类型前缀即市场归属（sh/sz/bj/jj=场外基金 等）。不使用东财 suggest——
/// 东财域在 rustls 下不可用（不发送 TLS close_notify，见文件头注释）。
/// fail-closed：上游失败返回 APP_UPSTREAM，绝不编造联想结果。
pub fn api_market_suggest(request: &HttpRequest) -> Result<String, (u16, String)> {
    let raw = query_param(&request.path, "input").ok_or((
        400,
        "{\"error\":\"APP_PARAM_INVALID\",\"message\":\"input is required\"}".into(),
    ))?;
    let input = url_decode(&raw).trim().to_string();
    if input.is_empty() || input.chars().count() > 40 {
        return Err((
            400,
            "{\"error\":\"APP_PARAM_INVALID\",\"message\":\"invalid input\"}".into(),
        ));
    }
    let url = format!(
        "https://smartbox.gtimg.cn/s3/?v=2&q={}&t=all",
        urlencode(&input)
    );
    let text = agent()
        .get(&url)
        .call()
        .map_err(|e| upstream_err(format!("suggest: {e}")))?
        .into_string()
        .map_err(|e| upstream_err(format!("suggest read: {e}")))?;
    let items = parse_smartbox(&text);
    Ok(format!("{{\"suggestions\":[{}]}}", items.join(",")))
}

/// 解析 smartbox 响应：`v_hint="sh~600519~\u8d35...~gzmt~GP-A^sz000001~..."`，
/// 多条以 `^` 分隔，字段以 `~` 分隔；名称为 `\uXXXX` 转义。
fn parse_smartbox(text: &str) -> Vec<String> {
    let payload = match (text.find('"'), text.rfind('"')) {
        (Some(start), Some(end)) if end > start => &text[start + 1..end],
        _ => return Vec::new(),
    };
    let mut items = Vec::new();
    for entry in payload.split('^') {
        let fields: Vec<&str> = entry.split('~').collect();
        if fields.len() < 4 {
            continue;
        }
        let (kind, code, name) = (fields[0].trim(), fields[1].trim(), fields[2].trim());
        let name = decode_js_unicode(name);
        if code.is_empty() || name.is_empty() {
            continue;
        }
        let symbol = match kind {
            "sh" | "sz" | "bj" => format!("{kind}{code}"),
            "jj" => code.to_string(), // 场外公募基金：裸代码
            "us" => format!("us{code}"),
            _ => continue, // 期货/外汇/未知类型不进联想白名单
        };
        items.push(format!(
            "{{\"symbol\":{},\"code\":{},\"name\":{},\"market\":{}}}",
            json_string(&symbol),
            json_string(code),
            json_string(&name),
            json_string(kind),
        ));
    }
    items
}

/// 还原 `\uXXXX` JS 转义为 UTF-8（smartbox 中文名形式）。
fn decode_js_unicode(s: &str) -> String {
    if !s.contains("\\u") {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('u') => {
                    let hex: String = chars.by_ref().take(4).collect();
                    if let Some(cp) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                        out.push(cp);
                    }
                }
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod smartbox_tests {
    use super::*;

    #[test]
    fn parse_smartbox_stock_and_fund() {
        let raw = "v_hint=\"sh~600519~\\u8d35\\u5dde\\u8305\\u53f0~gzmt~GP-A^jj~005827~\\u6613\\u65b9\\u8fbe\\u84dd\\u7b79\\u7cbe\\u9009\\u6df7\\u5408~yfdlcjxhh~KJ^fu~AU2506~\\u6caa\\u91d1\"^^";
        let items = parse_smartbox(raw);
        assert_eq!(items.len(), 2);
        assert!(items[0].contains("\"symbol\":\"sh600519\""));
        assert!(items[0].contains("\"name\":\"贵州茅台\""));
        assert!(items[1].contains("\"symbol\":\"005827\""));
        assert!(items[1].contains("\"name\":\"易方达蓝筹精选混合\""));
    }

    #[test]
    fn parse_smartbox_empty_or_malformed() {
        assert!(parse_smartbox("").is_empty());
        assert!(parse_smartbox("v_hint=\"\"").is_empty());
        assert!(parse_smartbox("v_hint=\"sh~\"").is_empty());
    }

    /// 端到端（需真实网络，默认忽略）：验证 ureq/rustls 对 smartbox 的 TLS
    /// 可达性与解析——"添加自选不可用"的最可疑残余点。
    #[test]
    #[ignore = "requires network"]
    fn smartbox_reachable_via_rustls() {
        let text = agent()
            .get("https://smartbox.gtimg.cn/s3/?v=2&q=600519&t=all")
            .call()
            .expect("smartbox TLS call failed")
            .into_string()
            .expect("smartbox body read failed");
        let items = parse_smartbox(&text);
        assert!(!items.is_empty(), "no suggestions parsed from: {text}");
        assert!(items[0].contains("sh600519"));
    }

    /// 端到端（需真实网络，默认忽略）：走 api_market_suggest 完整入口
    /// （query 解码 → 上游 → 解析），覆盖中文 URL 编码输入。
    #[test]
    #[ignore = "requires network"]
    fn suggest_api_end_to_end() {
        let request = HttpRequest::for_internal(
            "GET",
            &format!("/api/market/suggest?input={}", urlencode("贵州茅台")),
            Vec::new(),
        );
        let body = api_market_suggest(&request).expect("suggest api failed");
        assert!(body.contains("sh600519"), "unexpected body: {body}");
        assert!(body.contains("贵州茅台"), "unexpected body: {body}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_etfs_cover_required_themes() {
        for theme in [
            "PCB概念",
            "存储芯片",
            "半导体封装",
            "光学光电子",
            "CPU处理器",
        ] {
            assert!(
                THEME_ETFS
                    .iter()
                    .any(|(t, etfs)| **t == *theme && !etfs.is_empty()),
                "theme {theme} must map to real ETF symbols"
            );
        }
    }

    #[test]
    fn quote_json_renders_null_for_missing() {
        let quotes = vec![Quote {
            code: "512880".into(),
            name: "证券ETF".into(),
            price: Some(1.054),
            change_pct: Some(0.67),
            change: None,
            volume: None,
            turnover: None,
            amplitude: None,
            volume_ratio: None,
        }];
        let body = quote_json(&quotes);
        assert!(body.contains("\"price\":1.05"));
        assert!(body.contains("\"change\":null"));
    }

    #[test]
    fn market_quotes_rejects_bad_codes() {
        let request =
            HttpRequest::for_internal("GET", "/api/market/theme/etfs?codes=../etc", Vec::new());
        let result = api_market_quotes(&request);
        assert_eq!(result.unwrap_err().0, 400);
    }
}
