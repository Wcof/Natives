//! 板块行情（拆分自 market.rs，仅移动，无逻辑变更）。

use super::quotes::{agent, Quote};
use super::util::{json_string, num_or_null};

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

pub(crate) fn sector_json(sectors: &[SectorItem]) -> String {
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
pub(crate) fn fetch_tencent_sectors_by_type(category: &str) -> Result<Vec<SectorItem>, String> {
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

/// 映射板块名称到新浪实时行业/概念板块节点（仅作协议节点映射，股票数据 100% 动态拉取，绝不硬编码任何个股）。
pub(crate) fn sina_sector_node(name: &str) -> &'static str {
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
pub(crate) fn fetch_live_sector_stocks(node: &str) -> Result<Vec<Quote>, String> {
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

