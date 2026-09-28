//! 趋势分析数据对象的 JSON 序列化（1:1 移植自 `engine/serve/signal.py`）。
//!
//! 确保输出字段名、键顺序与数据形状与 Python 接口绝对一致。

use crate::trends::engine::SignalEngineResult;
use crate::trends::{FundFlowInput, KlineBar, QuoteInput};
use serde_json::{json, Value};

/// 单根 K 线序列化为字典格式。
pub fn kline_to_dict(k: &KlineBar) -> Value {
    json!({
        "date": k.date,
        "open": k.open,
        "close": k.close,
        "high": k.high,
        "low": k.low,
        "volume": k.volume,
        "amount": k.amount,
        "pct": k.pct,
        "turnover": k.turnover,
    })
}

/// 盘口数据序列化为字典格式。
pub fn quote_to_dict(q: &QuoteInput) -> Value {
    json!({
        "symbol": q.symbol,
        "name": q.name,
        "price": q.price,
        "pct": q.pct,
        "change": q.change,
        "high": q.high,
        "low": q.low,
        "open": q.open,
        "pre_close": q.pre_close,
        "volume": q.volume,
        "amount": q.amount,
        "turnover": q.turnover,
    })
}

/// 资金流数据序列化为字典格式。
pub fn flow_to_dict(f: &FundFlowInput) -> Value {
    json!({
        "date": f.date,
        "main_net": f.main_net,
        "super_large_net": f.super_large_net,
        "large_net": f.large_net,
        "main_pct": f.main_pct,
    })
}

/// 将信号引擎结果序列化为 JSON Value。
pub fn signal_to_dict(r: &SignalEngineResult) -> Value {
    let trend_val = r.trend.as_ref().map(|t| {
        json!({
            "direction": t.direction,
            "strength": t.strength,
            "stage": t.stage,
            "ma_arrangement": t.ma_arrangement,
            "ma_scores": t.ma_scores,
            "trendline": t.trendline.as_ref().map(|tl| json!({
                "type": tl.trendline_type,
                "slope": tl.slope,
                "current_price": tl.current_price,
                "points": tl.points,
            })),
            "signals": t.signals,
        })
    });

    let patterns_val: Vec<Value> = r
        .patterns
        .iter()
        .map(|p| {
            json!({
                "name": p.name,
                "direction": p.direction,
                "confidence": p.confidence,
                "status": p.status,
                "target_price": p.target_price,
                "key_levels": p.key_levels,
                "description": p.description,
            })
        })
        .collect();

    let vp_val = r.volume_price.as_ref().map(|vp| {
        json!({
            "pattern": vp.pattern,
            "direction": vp.direction,
            "confidence": vp.confidence,
            "volume_ratio": vp.volume_ratio,
            "turnover": vp.turnover,
            "obv_trend": vp.obv_trend,
            "signals": vp.signals,
            "description": vp.description,
        })
    });

    let breakouts_val: Vec<Value> = r
        .breakouts
        .iter()
        .map(|b| {
            json!({
                "system": b.system,
                "signal": b.signal,
                "breakout_price": b.breakout_price,
                "current_n": b.current_n,
                "stop_loss": b.stop_loss,
                "entry_price": b.entry_price,
                "position_units": b.position_units,
                "exit_price": b.exit_price,
                "channel_high": b.channel_high,
                "channel_low": b.channel_low,
                "next_add_price": b.next_add_price,
                "signals": b.signals,
                "description": b.description,
            })
        })
        .collect();

    let canslim_val = r.canslim.as_ref().map(|c| {
        json!({
            "c_score": c.c_score,
            "a_score": c.a_score,
            "n_score": c.n_score,
            "s_score": c.s_score,
            "l_score": c.l_score,
            "i_score": c.i_score,
            "m_score": c.m_score,
            "total": c.total,
            "grade": c.grade,
            "signals": c.signals,
            "cup_handle": c.cup_handle.as_ref().map(|ch| json!({
                "pattern": ch.pattern,
                "cup_high": ch.cup_high,
                "cup_low": ch.cup_low,
                "handle_high": ch.handle_high,
                "handle_low": ch.handle_low,
                "cup_depth": ch.cup_depth,
                "handle_depth": ch.handle_depth,
                "breakout": ch.breakout,
                "buy_point": ch.buy_point,
                "target": ch.target,
            })),
            "description": c.description,
        })
    });

    let mut map = serde_json::Map::new();
    map.insert("action".into(), json!(r.action));
    map.insert("score".into(), json!(r.score));
    map.insert("confidence".into(), json!(r.confidence));
    map.insert("risk_level".into(), json!(r.risk_level));
    map.insert("signal_strength".into(), json!(r.signal_strength));
    map.insert("plain_summary".into(), json!(r.plain_summary));
    map.insert("trade_plan".into(), json!(r.trade_plan));
    map.insert("module_scores".into(), json!(r.module_scores));
    map.insert("buy_signals".into(), json!(r.buy_signals));
    map.insert("sell_signals".into(), json!(r.sell_signals));
    map.insert("risk_warnings".into(), json!(r.risk_warnings));
    map.insert("key_levels".into(), json!(r.key_levels));
    map.insert("description".into(), json!(r.description));
    map.insert("trend".into(), json!(trend_val));
    map.insert("patterns".into(), json!(patterns_val));
    map.insert("volume_price".into(), json!(vp_val));
    map.insert("breakouts".into(), json!(breakouts_val));
    map.insert("canslim".into(), json!(canslim_val));

    if let Some(ref opt) = r.optimized_action {
        map.insert("optimized_action".into(), json!(opt));
    }
    if let Some(ref orig) = r.original_action {
        map.insert("original_action".into(), json!(orig));
    }
    if let Some(ref vr) = r.veto_reason {
        map.insert("veto_reason".into(), json!(vr));
    }
    if let Some(ref pa) = r.position_advice {
        map.insert("position_advice".into(), json!(pa));
    }
    if r.optimized_action.is_some() || r.original_action.is_some() {
        map.insert("risk_notes".into(), json!(r.risk_notes));
        map.insert("risk_reward".into(), json!(r.risk_reward));
    }

    Value::Object(map)
}
