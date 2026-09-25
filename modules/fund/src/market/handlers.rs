//! market HTTP handlers（拆分自 market.rs，仅移动，无逻辑变更）。

use super::quotes::{agent, fetch_tencent_quotes, quote_json, CN_INDEX_CODES, CORE_ETFS, HOT_STOCKS, THEME_ETFS, TENCENT_QT, US_INDEX_CODES};
use super::sectors::{fetch_live_sector_stocks, fetch_tencent_sectors_by_type, sector_json, sina_sector_node};
use super::smartbox::normalize_symbol;
use super::util::{json_string, num_or_null, query_param, upstream_err};
use app_runtime_core::http::HttpRequest;

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
