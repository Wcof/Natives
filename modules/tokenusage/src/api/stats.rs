//! 统计类 API 处理器（经 Model Host 8766 窄接口获取）。

use super::{api_error, json_str};
use crate::model_host::ModelUsageQueryClient;

pub(super) fn api_overview(client: &ModelUsageQueryClient) -> Result<String, (u16, String)> {
    let today = client
        .get_overview(Some("range=today"))
        .map_err(|e| api_error(503, "MODEL_HOST_UNAVAILABLE", &e.to_string()))?;
    let week = client
        .get_overview(Some("range=7d"))
        .map_err(|e| api_error(503, "MODEL_HOST_UNAVAILABLE", &e.to_string()))?;
    let month = client
        .get_overview(Some("range=30d"))
        .map_err(|e| api_error(503, "MODEL_HOST_UNAVAILABLE", &e.to_string()))?;
    let all = client
        .get_overview(Some("range=all"))
        .map_err(|e| api_error(503, "MODEL_HOST_UNAVAILABLE", &e.to_string()))?;

    Ok(format!(
        "{{\"currency\":\"USD\",\"periods\":{{\"today\":{{\"totalTokens\":{},\"costUsd\":{},\"sessionCount\":{}}},\"thisWeek\":{{\"totalTokens\":{},\"costUsd\":{},\"sessionCount\":{}}},\"thisMonth\":{{\"totalTokens\":{},\"costUsd\":{},\"sessionCount\":{}}},\"allTime\":{{\"totalTokens\":{},\"costUsd\":{},\"sessionCount\":{}}}}}}}",
        today.metrics.total_tokens, known_cost(today.metrics.estimated_cost_usd,today.metrics.priced), today.total_events_count,
        week.metrics.total_tokens, known_cost(week.metrics.estimated_cost_usd,week.metrics.priced), week.total_events_count,
        month.metrics.total_tokens, known_cost(month.metrics.estimated_cost_usd,month.metrics.priced), month.total_events_count,
        all.metrics.total_tokens, known_cost(all.metrics.estimated_cost_usd,all.metrics.priced), all.total_events_count
    ))
}

pub(super) fn api_tools(
    client: &ModelUsageQueryClient,
    path: &str,
) -> Result<String, (u16, String)> {
    let sources_res = client
        .get_sources()
        .map_err(|e| api_error(503, "MODEL_HOST_UNAVAILABLE", &e.to_string()))?;
    let analysis_res = client
        .get_analysis(path.split_once('?').map(|(_, query)| query))
        .map_err(|e| api_error(503, "MODEL_HOST_UNAVAILABLE", &e.to_string()))?;

    let mut analysis_map = std::collections::HashMap::new();
    for item in analysis_res.by_source {
        analysis_map.insert(item.key.to_lowercase(), item);
    }

    let mut items = Vec::new();
    for s in sources_res.sources {
        let stats = analysis_map
            .get(&s.id.to_lowercase())
            .or_else(|| analysis_map.get(&s.display_name.to_lowercase()));
        let tokens = stats.map(|st| st.tokens).unwrap_or(0);
        let cost_usd = stats
            .map(|st| known_cost(st.cost_usd, st.priced))
            .unwrap_or_else(|| "null".into());
        let sessions = stats.map(|st| st.requests).unwrap_or(0);

        items.push(format!(
            "{{\"id\":{},\"name\":{},\"category\":{},\"enabled\":{},\"totalTokens\":{},\"costUsd\":{},\"requestCount\":{},\"lastScannedAt\":{}}}",
            json_str(&s.id),
            json_str(&s.display_name),
            json_str(&s.category),
            s.enabled,
            tokens,
            cost_usd,
            sessions,
            json_str(&s.last_success_at)
        ));
    }

    Ok(format!("{{\"tools\":[{}]}}", items.join(",")))
}

pub(super) fn api_models(
    client: &ModelUsageQueryClient,
    path: &str,
) -> Result<String, (u16, String)> {
    let analysis = client
        .get_analysis(path.split_once('?').map(|(_, query)| query))
        .map_err(|e| api_error(503, "MODEL_HOST_UNAVAILABLE", &e.to_string()))?;

    let mut items = Vec::new();
    for m in analysis.by_model {
        items.push(format!(
            "{{\"model\":{},\"totalTokens\":{},\"inputTokens\":null,\"outputTokens\":null,\"costUsd\":{}}}",
            json_str(&m.name),
            m.tokens,
            known_cost(m.cost_usd,m.priced)
        ));
    }

    Ok(format!("{{\"models\":[{}]}}", items.join(",")))
}

pub(super) fn api_limits(client: &ModelUsageQueryClient) -> Result<String, (u16, String)> {
    let quota = client
        .get_quota()
        .map_err(|e| api_error(503, "MODEL_HOST_UNAVAILABLE", &e.to_string()))?;

    let mut items = Vec::new();
    for (_key, entry) in quota.entries {
        for w in entry.windows {
            items.push(format!(
                "{{\"providerId\":{},\"accountId\":{},\"windowKind\":{},\"label\":{},\"usedPercent\":{},\"remainingPercent\":{},\"usedUnits\":null,\"totalUnits\":null,\"unitType\":\"percent\",\"resetsAt\":{},\"fetchedAt\":{},\"status\":{}}}",
                json_str(&entry.provider),
                json_str(&entry.account),
                json_str(&w.name),
                json_str(&w.name),
                w.used_percent.map(|v| format!("{:.1}", v)).unwrap_or_else(|| "null".into()),
                w.remaining_percent.map(|v| format!("{:.1}", v)).unwrap_or_else(|| "null".into()),
                json_str(&w.reset_time.unwrap_or_default()),
                json_str(&quota.generated_at),
                json_str(&entry.status)
            ));
        }
    }

    Ok(format!("{{\"limits\":[{}]}}", items.join(",")))
}

pub(super) fn api_limits_refresh(
    client: &ModelUsageQueryClient,
    _body: &str,
) -> Result<String, (u16, String)> {
    match client.refresh_quota() {
        Ok(_) => Ok("{\"ok\":true,\"refreshed\":true}".into()),
        Err(e) => Err(api_error(503, "LIMITS_REFRESH_FAILED", &e.to_string())),
    }
}

pub(super) fn api_trends(
    client: &ModelUsageQueryClient,
    path: &str,
) -> Result<String, (u16, String)> {
    let query = path.split_once('?').map(|(_, q)| q).unwrap_or("range=7d");
    let overview = client
        .get_overview(Some(query))
        .map_err(|e| api_error(503, "MODEL_HOST_UNAVAILABLE", &e.to_string()))?;

    let mut items = Vec::new();
    for p in overview.trend {
        items.push(format!(
            "{{\"date\":{},\"sourceId\":\"all\",\"totalTokens\":{},\"costUsd\":{},\"sessionCount\":{}}}",
            json_str(&p.timestamp),
            p.tokens,
            known_cost(p.cost_usd,overview.metrics.priced),
            p.requests
        ));
    }

    Ok(format!("{{\"trends\":[{}]}}", items.join(",")))
}

pub fn format_compact_tokens(n: i64) -> String {
    if n >= 1_000_000_000 {
        format!("{:.1}B", n as f64 / 1_000_000_000.0)
    } else if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

pub(super) fn known_cost(value: f64, priced: Option<bool>) -> String {
    if priced == Some(true) {
        format!("{value:.4}")
    } else {
        "null".into()
    }
}

#[cfg(test)]
mod tests {
    use super::known_cost;

    #[test]
    fn unknown_cost_is_distinct_from_a_known_zero_price() {
        assert_eq!(known_cost(0.0, None), "null");
        assert_eq!(known_cost(0.0, Some(false)), "null");
        assert_eq!(known_cost(0.0, Some(true)), "0.0000");
    }
}
