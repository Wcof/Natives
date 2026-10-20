//! 缠论 Rust 移植与 Python 黄金输出的 A/B 深度相等测试。
//!
//! 黄金 JSON 由临时脚本对 `engine/analysis` 的 Python 参考实现生成
//! （fixture 与 `*_golden.json` 均已冻结入 git），断言逐字段深度相等，
//! 包括中文字符串、round 精度、信号顺序与 int 0 面积的序列化差异。
//! 以 Python 输出为真值；出现不一致时修 Rust，禁止改 fixture 或放宽断言。

use serde_json::Value;

use super::chanlun_daily::{analyze_chanlun_daily, daily_result_to_dict, MIN_KLINE_COUNT};
use super::chanlun_minute::{analyze_chanlun_minute, signals_to_dict};
use super::KlineInput;

/// 读取随包冻结的 JSON fixture / 黄金文件（`testdata/` 下文件名，入 git）。
macro_rules! frozen_json {
    ($file:literal) => {
        serde_json::from_str::<Value>(include_str!(concat!("testdata/", $file)))
            .expect("冻结 JSON 应可解析")
    };
}

/// 从日线 fixture 构造 KlineInput。
fn daily_input(fx: &Value) -> KlineInput {
    let strings = |key: &str| -> Vec<String> {
        fx[key]
            .as_array()
            .expect("fixture 序列应为数组")
            .iter()
            .map(|v| v.as_str().expect("字符串元素").to_string())
            .collect()
    };
    let nums = |key: &str| -> Vec<f64> {
        fx[key]
            .as_array()
            .expect("fixture 序列应为数组")
            .iter()
            .map(|v| v.as_f64().expect("数值元素"))
            .collect()
    };
    KlineInput {
        dates: strings("dates"),
        opens: nums("opens"),
        closes: nums("closes"),
        highs: nums("highs"),
        lows: nums("lows"),
        volumes: nums("volumes"),
    }
}

/// 深度相等断言；失败时输出第一个不一致字段的路径，便于定位。
fn assert_deep_eq(actual: &Value, golden: &Value, ctx: &str) {
    fn diff_path(a: &Value, g: &Value, path: &mut String) -> Option<String> {
        match (a, g) {
            (Value::Object(am), Value::Object(gm)) => {
                for k in gm.keys() {
                    let saved = path.clone();
                    path.push('.');
                    path.push_str(k);
                    let result = match (am.get(k), gm.get(k)) {
                        (Some(av), Some(gv)) => diff_path(av, gv, path),
                        _ => Some(format!("{path}（缺失一侧）")),
                    };
                    if result.is_some() {
                        return result;
                    }
                    *path = saved;
                }
                None
            }
            (Value::Array(av), Value::Array(gv)) => {
                if av.len() != gv.len() {
                    return Some(format!("{}（长度 {} != {}）", path, av.len(), gv.len()));
                }
                for (i, (a_item, g_item)) in av.iter().zip(gv.iter()).enumerate() {
                    let saved = path.clone();
                    path.push_str(&format!("[{i}]"));
                    let result = diff_path(a_item, g_item, path);
                    if result.is_some() {
                        return result;
                    }
                    *path = saved;
                }
                None
            }
            _ => {
                if a != g {
                    return Some(format!("{path}\n    rust  = {a}\n    golden= {g}"));
                }
                None
            }
        }
    }

    let mut path = String::new();
    if let Some(p) = diff_path(actual, golden, &mut path) {
        panic!("A/B 不一致（{ctx}）@ {p}");
    }
}

/// 日线 A/B 用例宏：读 fixture → Rust 分析 → 序列化 → 与黄金 JSON 深度相等。
macro_rules! daily_ab_case {
    ($fn_name:ident, $fixture:literal, $golden:literal) => {
        #[test]
        fn $fn_name() {
            let fx = frozen_json!($fixture);
            let golden = frozen_json!($golden);
            let result = analyze_chanlun_daily(&daily_input(&fx));
            let actual = daily_result_to_dict(&result);
            assert_deep_eq(&actual, &golden, $fixture);
        }
    };
}

/// 分钟 A/B 用例宏。
macro_rules! minute_ab_case {
    ($fn_name:ident, $fixture:literal, $golden:literal) => {
        #[test]
        fn $fn_name() {
            let fx = frozen_json!($fixture);
            let golden = frozen_json!($golden);
            let times: Vec<String> = fx["times"]
                .as_array()
                .expect("times 数组")
                .iter()
                .map(|v| v.as_str().expect("时间字符串").to_string())
                .collect();
            let prices: Vec<f64> = fx["prices"]
                .as_array()
                .expect("prices 数组")
                .iter()
                .map(|v| v.as_f64().expect("价格数值"))
                .collect();
            let volumes: Vec<f64> = fx["volumes"]
                .as_array()
                .expect("volumes 数组")
                .iter()
                .map(|v| v.as_f64().expect("量数值"))
                .collect();
            let result = analyze_chanlun_minute(&times, &prices, &volumes);
            let actual = signals_to_dict(&result);
            assert_deep_eq(&actual, &golden, $fixture);
        }
    };
}

// ---- 日线：3 只基准股 + 1 个合成全包含边界序列（9 根 < 10，纯函数不拒绝）----
daily_ab_case!(
    ab_daily_600519,
    "daily_600519.json",
    "daily_600519_golden.json"
);
daily_ab_case!(
    ab_daily_300750,
    "daily_300750.json",
    "daily_300750_golden.json"
);
daily_ab_case!(
    ab_daily_601318,
    "daily_601318.json",
    "daily_601318_golden.json"
);
daily_ab_case!(
    ab_daily_synth_boundary,
    "daily_synth_boundary.json",
    "daily_synth_boundary_golden.json"
);

// ---- 分钟：3 只基准股 + 跨日合成 + 末笔背驰 + 极小序列 ----
minute_ab_case!(
    ab_minute_600519,
    "minute_600519.json",
    "minute_600519_golden.json"
);
minute_ab_case!(
    ab_minute_300750,
    "minute_300750.json",
    "minute_300750_golden.json"
);
minute_ab_case!(
    ab_minute_601318,
    "minute_601318.json",
    "minute_601318_golden.json"
);
minute_ab_case!(
    ab_minute_synth_boundary,
    "minute_synth_boundary.json",
    "minute_synth_boundary_golden.json"
);
minute_ab_case!(
    ab_minute_div_tail,
    "minute_div_tail.json",
    "minute_div_tail_golden.json"
);
minute_ab_case!(
    ab_minute_tiny,
    "minute_tiny.json",
    "minute_tiny_golden.json"
);

#[test]
fn min_kline_count_is_ten() {
    // engine handle_chanlun_daily 的接口层门槛（app.py: len(klines) < 10 拒绝），
    // P3 接线时使用；纯函数本身与 Python 一致不做限制。
    assert_eq!(MIN_KLINE_COUNT, 10);
}

#[test]
fn py_round_matches_python_semantics() {
    use super::{py_round, py_round_i};
    // Python round(x, n)：对二进制精确值做十进制半偶舍入
    assert_eq!(py_round(2.675, 2), 2.67);
    assert_eq!(py_round(0.125, 2), 0.12);
    assert_eq!(py_round(-0.125, 2), -0.12);
    assert_eq!(py_round(1.005, 2), 1.0);
    assert_eq!(py_round(1234.567, 2), 1234.57);
    assert_eq!(py_round(0.0, 4), 0.0);
    // Python round(x)：半偶舍入到整数
    assert_eq!(py_round_i(2.5), 2);
    assert_eq!(py_round_i(3.5), 4);
    assert_eq!(py_round_i(0.5), 0);
    assert_eq!(py_round_i(-0.5), 0);
    assert_eq!(py_round_i(94.5 - 35.0 * 0.9), 63);
    assert_eq!(py_round_i(100.0 - 40.0 * 0.875), 65);
}

macro_rules! modules_ab_case {
    ($fn_name:ident, $fx_name:literal, $golden_name:literal) => {
        #[test]
        fn $fn_name() {
            use super::modules::breakout::analyze_breakout;
            use super::modules::canslim::analyze_canslim;
            use super::modules::pattern::analyze_patterns;
            use super::modules::trend::analyze_trend;
            use super::modules::volume_price::analyze_volume_price;

            let fx = frozen_json!($fx_name);
            let golden = frozen_json!($golden_name);
            let input = daily_input(&fx);
            let bars = input.to_kline_bars();

            let trend = analyze_trend(&bars);
            let breakout = analyze_breakout(&bars);
            let vp = analyze_volume_price(&bars, None, None);
            let pattern = analyze_patterns(&bars);
            let canslim = analyze_canslim(&bars, None, None, None);

            let actual = serde_json::json!({
                "trend": trend,
                "breakout": breakout,
                "volume_price": vp,
                "pattern": pattern,
                "canslim": canslim,
            });
            assert_deep_eq(&actual, &golden, $fx_name);
        }
    };
}

modules_ab_case!(
    ab_modules_600519,
    "daily_600519.json",
    "modules_600519_golden.json"
);
modules_ab_case!(
    ab_modules_300750,
    "daily_300750.json",
    "modules_300750_golden.json"
);
modules_ab_case!(
    ab_modules_601318,
    "daily_601318.json",
    "modules_601318_golden.json"
);

macro_rules! signal_ab_case {
    ($fn_name:ident, $fx_name:literal, $golden_name:literal) => {
        #[test]
        fn $fn_name() {
            use super::engine::run_analysis;
            use super::postprocess::apply_signal_optimization;
            use super::serialize::signal_to_dict;

            let fx = frozen_json!($fx_name);
            let golden = frozen_json!($golden_name);
            let input = daily_input(&fx);
            let bars = input.to_kline_bars();

            let mut result = run_analysis(&bars, None, None, None);
            apply_signal_optimization(&mut result, &bars, None);
            let actual = signal_to_dict(&result);

            assert_deep_eq(&actual, &golden, $fx_name);
        }
    };
}

signal_ab_case!(
    ab_signal_600519,
    "daily_600519.json",
    "signal_600519_golden.json"
);
signal_ab_case!(
    ab_signal_300750,
    "daily_300750.json",
    "signal_300750_golden.json"
);
signal_ab_case!(
    ab_signal_601318,
    "daily_601318.json",
    "signal_601318_golden.json"
);

#[test]
fn golden_files_cover_expected_counts() {
    // 防御：确认冻结数据规模与 A/B 用例一致，避免误删 fixture。
    let fx = frozen_json!("daily_600519.json");
    assert_eq!(fx["dates"].as_array().unwrap().len(), 250);
    let minute_fx = frozen_json!("minute_synth_boundary.json");
    assert!(minute_fx["times"].as_array().unwrap().len() > 500);
}

/// `/api/trends/*` 路由活数据契约：前端 K 线主图（`klines`）与右侧 AI 决策
/// 卡片流（`signal` 的 action/module_scores/trade_plan）数据源必须非空。
/// 回归背景一：任一字段缺失时，趋势分析页 K 线与右侧栏整体空白。
/// 回归背景二：前端趋势入口默认传裸 6 位代码（如 600519），`fetch_kline_bars`
/// 缺交易所前缀归一化时上游返回空 → `K线数据不足（仅0根）` → 整页空白；
/// `/search` 参数名（keyword vs input）与响应形状（results）错配时搜索恒 400。
#[test]
fn trends_routes_serve_frontend_contract_live() {
    use app_runtime_core::http::HttpRequest;

    // analyze：裸 6 位代码（前端默认入口形状）必须与带前缀等价
    for symbol in ["600519", "sh600519"] {
        let analyze_path = format!("/api/trends/analyze?symbol={symbol}");
        let req = HttpRequest::for_internal("GET", &analyze_path, Vec::new());
        // 运行链契约（client.rs）：route 为剥掉 query 的纯路径，query 从 request.path 取。
        let body = super::handle_trends_request(&req, "/api/trends/analyze").unwrap_or_else(|e| {
            panic!("/api/trends/analyze?symbol={symbol} 应成功（真实上游行情）: {e:?}")
        });
        let data: Value = serde_json::from_str(&body).unwrap();
        assert!(
            data.get("error").is_none(),
            "analyze({symbol}) 返回错误: {body}"
        );
        let klines = data["klines"]
            .as_array()
            .expect("klines 必须为数组（K线主图数据源）");
        assert!(
            klines.len() >= 10,
            "klines({symbol}) 应非空，实际 {} 根",
            klines.len()
        );
        for key in ["date", "open", "close", "high", "low", "volume"] {
            assert!(
                klines[0].get(key).is_some(),
                "klines({symbol})[0] 缺少前端渲染必需字段 {key}"
            );
        }
        let signal = &data["signal"];
        assert!(
            signal["action"].as_str().is_some_and(|s| !s.is_empty()),
            "signal.action 缺失（右侧结论卡数据源）"
        );
        assert!(signal["score"].is_number(), "signal.score 缺失");
        assert!(
            signal["module_scores"]
                .as_object()
                .is_some_and(|m| !m.is_empty()),
            "module_scores 缺失（模块评分卡数据源）"
        );
        assert!(signal["buy_signals"].is_array(), "buy_signals 缺失");
        assert!(signal["sell_signals"].is_array(), "sell_signals 缺失");
        assert!(
            signal.get("trade_plan").is_some(),
            "trade_plan 键必须存在（操作计划卡依赖）"
        );
    }

    // search：前端发 keyword=，应返回 results[{code,name}]（搜索联想契约）
    let search_path = "/api/trends/search?keyword=600519";
    let req = HttpRequest::for_internal("GET", search_path, Vec::new());
    let body = super::handle_trends_request(&req, "/api/trends/search")
        .expect("/api/trends/search 应成功（真实上游 smartbox）");
    let data: Value = serde_json::from_str(&body).unwrap();
    let results = data["results"].as_array().expect("results 必须为数组");
    assert!(
        results
            .iter()
            .any(|r| r["code"] == "600519" || r["symbol"] == "sh600519"),
        "results 应命中 600519，实际: {body}"
    );
    assert!(
        results
            .iter()
            .all(|r| r.get("code").is_some() && r.get("name").is_some()),
        "results 元素必须含 code/name（前端 selectStock 契约）"
    );

    let chanlun_path = "/api/trends/chanlun_daily?symbol=600519";
    let req = HttpRequest::for_internal("GET", chanlun_path, Vec::new());
    let body = super::handle_trends_request(&req, "/api/trends/chanlun_daily")
        .expect("/api/trends/chanlun_daily 应成功（真实上游行情）");
    let cl: Value = serde_json::from_str(&body).unwrap();
    assert!(cl.get("error").is_none(), "chanlun_daily 返回错误: {body}");
    assert!(
        cl.get("signals").is_some(),
        "缠论日线必须输出 signals（缠论卡数据源）"
    );
}
