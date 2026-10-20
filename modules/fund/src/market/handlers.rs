//! market HTTP handlers（拆分自 market.rs，仅移动，无逻辑变更）。

use super::cache::cached;
use super::quotes::{
    agent, fetch_tencent_quotes, quote_json, CN_INDEX_CODES, CORE_ETFS, HOT_STOCKS, TENCENT_QT,
    THEME_ETFS, US_INDEX_CODES,
};
use super::sectors::{
    fetch_live_sector_stocks, fetch_tencent_sectors_by_type, sector_json, sina_sector_node,
};
use super::smartbox::normalize_symbol;
use super::util::{json_string, num_or_null, query_param, upstream_err};
use app_runtime_core::http::HttpRequest;
use std::io::Read;
use std::time::{Duration, Instant};

/// GET /api/market/indices — 大盘指数（A 股）+ 美股指数（腾讯批量）。
pub fn api_market_indices(_request: &HttpRequest) -> Result<String, (u16, String)> {
    // 3s TTL：与前端 3s 轮询错峰，TTL 内直接复用上游实测响应
    cached("indices", Duration::from_secs(3), || {
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
    })
}

/// GET /api/market/stocks — 核心 A 股实时行情。
pub fn api_market_stocks(_request: &HttpRequest) -> Result<String, (u16, String)> {
    cached("stocks", Duration::from_secs(3), || {
        let codes: Vec<&str> = HOT_STOCKS.iter().map(|(c, _)| *c).collect();
        let names: Vec<(usize, &str)> = HOT_STOCKS
            .iter()
            .enumerate()
            .map(|(i, (_, n))| (i, *n))
            .collect();
        let quotes = fetch_tencent_quotes(&codes, Some(&names)).map_err(upstream_err)?;
        Ok(quote_json(&quotes))
    })
}

/// GET /api/market/etfs — 核心 ETF（含股票型、跨境、黄金商品与国债债券）。
pub fn api_market_etfs(_request: &HttpRequest) -> Result<String, (u16, String)> {
    cached("etfs", Duration::from_secs(3), || {
        let codes: Vec<&str> = CORE_ETFS.iter().map(|(c, _)| *c).collect();
        let names: Vec<(usize, &str)> = CORE_ETFS
            .iter()
            .enumerate()
            .map(|(i, (_, n))| (i, *n))
            .collect();
        let quotes = fetch_tencent_quotes(&codes, Some(&names)).map_err(upstream_err)?;
        Ok(quote_json(&quotes))
    })
}

/// GET /api/market/sectors?type=all|concept|industry|region — 热门板块列表。
pub fn api_market_sectors(request: &HttpRequest) -> Result<String, (u16, String)> {
    let cat = query_param(&request.path, "type").unwrap_or_else(|| "all".to_string());
    // 板块排行变化以分钟计，60s TTL 足够新鲜且大幅减轻上游压力
    cached(format!("sectors:{cat}"), Duration::from_secs(60), || {
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
    })
}

/// GET /api/market/sector/stocks?name=文化传媒&code=pt01801765&top=sz000676 — 真实动态获取板块成分股（100% 实时活数据）。
pub fn api_market_sector_stocks(request: &HttpRequest) -> Result<String, (u16, String)> {
    let name = query_param(&request.path, "name").unwrap_or_default();
    let top_code = query_param(&request.path, "top");

    cached(
        format!("sector-stocks:{name}:{}", top_code.as_deref().unwrap_or("")),
        Duration::from_secs(30),
        || {
            let node = sina_sector_node(&name);
            let mut quotes = fetch_live_sector_stocks(node).map_err(upstream_err)?;

            // 若领涨股未在列表中，通过腾讯批量接口实时补充并置顶
            if let Some(top) = top_code.as_deref() {
                if !top.is_empty() && !quotes.iter().any(|q| q.code == top || q.code.contains(top))
                {
                    if let Ok(top_quotes) = fetch_tencent_quotes(&[top], None) {
                        if let Some(tq) = top_quotes.into_iter().next() {
                            quotes.insert(0, tq);
                        }
                    }
                }
            }

            Ok(quote_json(&quotes))
        },
    )
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
    // name 参数会透传进响应，参与缓存键
    let name_param = query_param(&request.path, "name").unwrap_or_default();
    cached(
        format!("detail:{symbol}:{name_param}"),
        Duration::from_secs(3),
        || {
            let url = format!("{}{}", TENCENT_QT, symbol);
            let mut reader = agent()
                .get(&url)
                .call()
                .map_err(|e| upstream_err(format!("tencent quote: {e}")))?
                .into_reader();
            let mut bytes = Vec::new();
            reader
                .read_to_end(&mut bytes)
                .map_err(|e| upstream_err(format!("tencent quote read: {e}")))?;
            if bytes.len() > 2 * 1024 * 1024 {
                return Err(upstream_err("response_too_large".into()));
            }
            let (decoded, _, _) = encoding_rs::GBK.decode(&bytes);
            let raw = decoded.into_owned();

            let line = raw
                .lines()
                .find(|l| l.contains("=\""))
                .ok_or_else(|| upstream_err("empty quote response".into()))?;
            let payload = line.split('"').nth(1).unwrap_or("");
            let fields: Vec<&str> = payload.split('~').collect();
            if fields.len() < 35 {
                return Err(upstream_err("quote response fields too short".into()));
            }

            let parse_f64 =
                |i: usize| -> Option<f64> { fields.get(i).and_then(|v| v.parse::<f64>().ok()) };
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
            let pe = parse_f64(39); // PE(TTM)
            let circ_market_cap = parse_f64(44); // 流通市值 (亿元)
            let market_cap = parse_f64(45); // 总市值 (亿元)
            let pb = parse_f64(46); // 市净率 PB
            let limit_up = parse_f64(47);
            let limit_down = parse_f64(48);
            let volume_ratio = parse_f64(49); // 量比
            let avg_price = parse_f64(51); // 当日均价
            let pe_dyn = parse_f64(52); // PE(动)
            let dividend_yield = parse_f64(64); // 股息率 %
                                                // 股本位与市值位数学自洽：流通市值=circ[72]×现价、总市值=total[73]×现价，
                                                // [72]=流通股本、[73]=总股本（此前互换导致部分流通标的股本卡错位）。
            let total_shares = parse_f64(73); // 总股本 (股)
            let circ_shares = parse_f64(72); // 流通股本 (股)
            let wei_cha = parse_f64(50); // 委差 (手)
            let amplitude = parse_f64(43); // 振幅 %

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
            // 委比%（(买五总-卖五总)/(买五总+卖五总)×100），与委托差 [50] 同口径
            let wei_bi = if total_bid_vol + total_ask_vol > 0.0 {
                (total_bid_vol - total_ask_vol) / (total_bid_vol + total_ask_vol) * 100.0
            } else {
                0.0
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
        "{{\"symbol\":{},\"name\":{},\"price\":{:.2},\"prevClose\":{:.2},\"open\":{:.2},\"high\":{:.2},\"low\":{:.2},\"change\":{:.2},\"changePct\":{:.2},\"volume\":{:.0},\"amount\":{:.2},\"turnover\":{},\"volumeRatio\":{},\"amplitude\":{},\"limitUp\":{},\"limitDown\":{},\"pe\":{},\"peDyn\":{},\"pb\":{},\"marketCap\":{},\"circMarketCap\":{},\"dividendYield\":{},\"totalShares\":{},\"circShares\":{},\"avgPrice\":{},\"weiBi\":{},\"weiCha\":{},\"bids\":[{}],\"asks\":[{}],\"bidVol\":{:.0},\"askVol\":{:.0},\"bidRatio\":{:.3}}}",
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
        num_or_null(pe_dyn),
        num_or_null(pb),
        num_or_null(market_cap),
        num_or_null(circ_market_cap),
        num_or_null(dividend_yield),
        num_or_null(total_shares),
        num_or_null(circ_shares),
        num_or_null(avg_price),
        num_or_null(Some(wei_bi)),
        num_or_null(wei_cha),
        bids.join(","),
        asks.join(","),
        total_bid_vol,
        total_ask_vol,
        bid_ratio,
    );
            Ok(json)
        },
    )
}

/// GET /api/market/minute?symbol=sh600519 — 分时图数据（含每分钟价格、成交量与 VWAP）。
pub fn api_market_minute(request: &HttpRequest) -> Result<String, (u16, String)> {
    let raw_symbol = query_param(&request.path, "symbol").ok_or((
        400,
        "{\"error\":\"APP_PARAM_INVALID\",\"message\":\"symbol is required\"}".into(),
    ))?;
    let symbol = normalize_symbol(&raw_symbol);
    // 5s TTL：与开市期 3s 轮询错峰，分时曲线 5s 内的新增分钟点延迟可接受
    cached(format!("minute:{symbol}"), Duration::from_secs(5), || {
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

        // 交易日与昨收（前收）取自上游权威字段：data.<symbol>.data.date 与
        // qt.<symbol>[4]。分时图 y 轴基准与涨跌幅右轴必须锚定真实昨收，
        // 不允许回退到开盘价（高开/低开时整轴错位）。
        let symbol_node = resp.get("data").and_then(|d| d.get(&symbol));
        let trade_date = symbol_node
            .and_then(|s| s.get("data"))
            .and_then(|d| d.get("date"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let pre_close = symbol_node
            .and_then(|s| s.get("qt"))
            .and_then(|q| q.get(&symbol))
            .and_then(|a| a.as_array())
            .and_then(|a| a.get(4))
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);

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
            "{{\"symbol\":{},\"date\":{},\"source\":\"web.ifzq.gtimg.cn\",\"preClose\":{},\"count\":{},\"points\":[{}]}}",
            json_string(&symbol),
            json_string(&trade_date),
            pre_close,
            points.len(),
            points.join(",")
        ))
    })
}

/// GET /api/market/kline?symbol=sh600519&period=day&fq=qfq — 多周期 K 线数据 (OHLCV)。
fn prev_day_str(date_str: &str) -> Option<String> {
    let parts: Vec<&str> = date_str.split('-').collect();
    if parts.len() != 3 {
        return None;
    }
    let mut y: i32 = parts[0].parse().ok()?;
    let mut m: u32 = parts[1].parse().ok()?;
    let mut d: u32 = parts[2].parse().ok()?;
    if d > 1 {
        d -= 1;
    } else {
        if m > 1 {
            m -= 1;
        } else {
            y -= 1;
            m = 12;
        }
        d = match m {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => {
                if (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0) {
                    29
                } else {
                    28
                }
            }
            _ => 30,
        };
    }
    Some(format!("{:04}-{:02}-{:02}", y, m, d))
}

/// fq 复权模式：qfq 前复权（默认）/ hfq 后复权 / bf 不复权。
/// 支持周期: day, week, month。针对 day (日K) 支持多页历史向后翻页合并，覆盖多达十余年全部走势。
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
        _ => {
            return Err((
                400,
                "{\"error\":\"APP_PARAM_INVALID\",\"message\":\"unsupported fq\"}".into(),
            ))
        }
    };

    // 日 K 串行拉 4 页上游（~0.8s），30s TTL 内重复切换直接复用整页结果
    let cache_key = format!("kline:{symbol}:{period}:{fq}");
    cached(cache_key, Duration::from_secs(30), move || {
        kline_fetch_upstream(&symbol, &period, &fq)
    })
}

pub(crate) fn fetch_minute_points(
    symbol: &str,
) -> Result<(Vec<String>, Vec<f64>, Vec<f64>), (u16, String)> {
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
        .and_then(|d| d.get(symbol))
        .and_then(|s| s.get("data"))
        .and_then(|d| d.get("data"))
        .and_then(|arr| arr.as_array())
        .unwrap_or(&empty_vec);

    let mut times = Vec::with_capacity(data_arr.len());
    let mut prices = Vec::with_capacity(data_arr.len());
    let mut volumes = Vec::with_capacity(data_arr.len());
    let mut prev_cum_vol = 0.0;
    for item in data_arr {
        let text = match item.as_str() {
            Some(t) => t,
            None => continue,
        };
        let parts: Vec<&str> = text.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }
        let time_raw = parts[0];
        let price = parts[1].parse::<f64>().unwrap_or(0.0);
        let cum_vol = parts[2].parse::<f64>().unwrap_or(0.0);
        let vol_inc = (cum_vol - prev_cum_vol).max(0.0);
        prev_cum_vol = cum_vol;

        let formatted_time = if time_raw.len() == 4 {
            format!("{}:{}", &time_raw[..2], &time_raw[2..])
        } else {
            time_raw.to_string()
        };

        times.push(formatted_time);
        prices.push(price);
        volumes.push(vol_inc);
    }
    Ok((times, prices, volumes))
}

pub(crate) fn fetch_kline_data(
    symbol: &str,
    period: &str,
    fq: &str,
) -> Result<(Vec<crate::trends::KlineBar>, &'static str), (u16, String)> {
    let kline_key = match fq {
        "bf" => period.to_string(),
        prefix => format!("{prefix}{period}"),
    };

    let is_day = period == "day";
    let max_pages = if is_day { 4 } else { 1 };
    let mut all_bars = Vec::new();
    let mut end_date: Option<String> = None;
    let start_time = Instant::now();
    let max_total_duration = Duration::from_secs(6);

    for page_idx in 0..max_pages {
        if page_idx > 0 && start_time.elapsed() >= max_total_duration {
            break;
        }

        let end_param = end_date.as_deref().unwrap_or("");
        let url = format!(
            "https://proxy.finance.qq.com/ifzqgtimg/appstock/app/newfqkline/get?param={},{},,{end_param},800,{}",
            symbol, period, fq
        );
        let resp_res = agent().get(&url).call();
        let resp_val = match resp_res {
            Ok(r) => r.into_json::<serde_json::Value>(),
            Err(e) => {
                if !all_bars.is_empty() {
                    break;
                }
                return Err(upstream_err(format!("kline query: {e}")));
            }
        };

        let resp = match resp_val {
            Ok(v) => v,
            Err(e) => {
                if !all_bars.is_empty() {
                    break;
                }
                return Err(upstream_err(format!("kline parse: {e}")));
            }
        };

        let symbol_data = resp.get("data").and_then(|d| d.get(symbol));
        let bars = symbol_data
            .and_then(|s| s.get(&kline_key))
            .and_then(|arr| arr.as_array());

        let Some(bars_vec) = bars else { break };
        if bars_vec.is_empty() {
            break;
        }

        let first_date = bars_vec
            .first()
            .and_then(|b| b.as_array())
            .and_then(|a| a.first())
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let batch_len = bars_vec.len();
        all_bars.extend(bars_vec.clone());

        if !is_day || batch_len < 800 {
            break;
        }
        if let Some(fd) = first_date {
            end_date = prev_day_str(&fd);
            if end_date.is_none() {
                break;
            }
        } else {
            break;
        }
    }

    let mut map: std::collections::BTreeMap<String, crate::trends::KlineBar> =
        std::collections::BTreeMap::new();
    for bar in all_bars {
        let arr = match bar.as_array() {
            Some(a) if a.len() >= 6 => a,
            _ => continue,
        };
        let date_str = arr[0].as_str().unwrap_or("");
        if date_str.is_empty() {
            continue;
        }
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

        map.insert(
            date_str.to_string(),
            crate::trends::KlineBar {
                date: date_str.to_string(),
                open,
                close,
                high,
                low,
                volume,
                amount: 0.0,
                pct: 0.0,
                turnover: 0.0,
            },
        );
    }

    let mut bars: Vec<crate::trends::KlineBar> = map.into_values().collect();
    for i in 1..bars.len() {
        let prev_close = bars[i - 1].close;
        if prev_close > 0.0 {
            bars[i].pct =
                crate::trends::py_round((bars[i].close - prev_close) / prev_close * 100.0, 2);
        }
    }
    Ok((bars, "proxy.finance.qq.com"))
}

pub(crate) fn kline_fetch_upstream(
    symbol: &str,
    period: &str,
    fq: &str,
) -> Result<String, (u16, String)> {
    let (bars, source) = super::fetch_kline_data(symbol, period, fq)?;
    let candles: Vec<String> = bars
        .iter()
        .map(|k| {
            format!(
                "{{\"time\":{},\"open\":{:.4},\"close\":{:.4},\"high\":{:.4},\"low\":{:.4},\"volume\":{:.0}}}",
                json_string(&k.date),
                k.open,
                k.close,
                k.high,
                k.low,
                k.volume,
            )
        })
        .collect();

    Ok(format!(
        "{{\"symbol\":{},\"period\":{},\"source\":{},\"adjustment\":{},\"count\":{},\"candles\":[{}]}}",
        json_string(&symbol),
        json_string(&period),
        json_string(source),
        json_string(fq),
        candles.len(),
        candles.join(",")
    ))
}

/// N04 统一估值的有界行情获取：每批 60、最多 10 批；上游失败显式 failed，
/// 缺价标的保持缺位不填 0。价格立即转为 4 位定点，不经 f64 回写权威。
pub(crate) struct QuoteBatch {
    pub quotes: std::collections::BTreeMap<String, crate::assets::QuoteInput>,
    pub fetched_at: i64,
    pub failed: bool,
}

pub(crate) fn fetch_quote_batch(symbols: &[String]) -> QuoteBatch {
    let fetched_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    // 单元测试保持离线隔离：域测试不依赖真实上游（真机链覆盖真实取价）。
    #[cfg(test)]
    {
        let _ = symbols;
        return QuoteBatch {
            quotes: Default::default(),
            fetched_at,
            failed: false,
        };
    }
    #[cfg(not(test))]
    {
        let mut map = std::collections::BTreeMap::new();
        let mut failed = symbols.len() > 600;
        for batch in symbols.chunks(60).take(10) {
            let request = HttpRequest::for_internal(
                "GET",
                &format!("/api/market/quotes?codes={}", batch.join(",")),
                vec![],
            );
            match api_market_quotes(&request)
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            {
                Some(value) => {
                    for q in value["quotes"].as_array().into_iter().flatten() {
                        let Some(code) = q["code"].as_str() else {
                            continue;
                        };
                        let Some(price) =
                            q["price"].as_f64().filter(|p| p.is_finite() && *p >= 0.0)
                        else {
                            continue;
                        };
                        let Ok(raw) = crate::fixed::Fixed::parse(&price.to_string(), 4) else {
                            continue;
                        };
                        map.insert(
                            code.to_string(),
                            crate::assets::QuoteInput {
                                price_raw: raw.raw(),
                                change_raw: q["change"]
                                    .as_f64()
                                    .and_then(|v| {
                                        crate::fixed::Fixed::parse(&v.to_string(), 4).ok()
                                    })
                                    .map(|v| v.raw()),
                                as_of: q["asOf"].as_str().map(String::from),
                                source: "market-api".into(),
                            },
                        );
                    }
                }
                None => failed = true,
            }
        }
        QuoteBatch {
            quotes: map,
            fetched_at,
            failed,
        }
    }
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
    // 缓存键用规范化后的代码序列：同一批自选 3s 内的轮询直接复用
    let cache_key = format!("quotes:{}", normalized.join(","));

    // 名称来自常量映射
    let name_map: Vec<(usize, &str)> = norm_refs
        .iter()
        .enumerate()
        .filter_map(|(i, code)| {
            CN_INDEX_CODES
                .iter()
                .find(|(c, _)| *c == *code)
                .map(|(_, name)| (i, *name))
                .or_else(|| {
                    THEME_ETFS
                        .iter()
                        .flat_map(|(_, etfs)| etfs.iter())
                        .find(|(c, _)| *c == *code)
                        .map(|(_, name)| (i, *name))
                })
        })
        .collect();
    cached(cache_key, Duration::from_secs(3), move || {
        let quotes = fetch_tencent_quotes(&norm_refs, Some(&name_map)).map_err(upstream_err)?;
        Ok(quote_json(&quotes))
    })
}
