//! Model Host 8766 只读查询 DTO 契约类型定义。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MetricCardsDTO {
    #[serde(default)]
    pub total_requests: i64,
    #[serde(default)]
    pub total_tokens: i64,
    #[serde(default)]
    pub success_rate: f64,
    #[serde(default)]
    pub tps: f64,
    #[serde(default)]
    pub cache_hit_rate: f64,
    #[serde(default)]
    pub estimated_cost_usd: f64,
    #[serde(default)]
    pub priced: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TrendPointDTO {
    #[serde(default)]
    pub timestamp: String,
    #[serde(default)]
    pub requests: i64,
    #[serde(default)]
    pub tokens: i64,
    #[serde(default)]
    pub cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TokenCompositionDTO {
    #[serde(default)]
    pub input: i64,
    #[serde(default)]
    pub output: i64,
    #[serde(default)]
    pub cache_read: i64,
    #[serde(default)]
    pub cache_write: i64,
    #[serde(default)]
    pub reasoning: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OverviewResultDTO {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub total_events_count: i64,
    #[serde(default)]
    pub metrics: MetricCardsDTO,
    #[serde(default, deserialize_with = "nullable_vec")]
    pub trend: Vec<TrendPointDTO>,
    #[serde(default)]
    pub tokens: TokenCompositionDTO,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RankItemDTO {
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub requests: i64,
    #[serde(default)]
    pub tokens: i64,
    #[serde(default)]
    pub cost_usd: f64,
    #[serde(default)]
    pub percent: f64,
    #[serde(default)]
    pub priced: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AnalyticsResultDTO {
    #[serde(default)]
    pub status: String,
    #[serde(default, deserialize_with = "nullable_vec")]
    pub by_model: Vec<RankItemDTO>,
    #[serde(default, deserialize_with = "nullable_vec")]
    pub by_provider: Vec<RankItemDTO>,
    #[serde(default, deserialize_with = "nullable_vec")]
    pub by_source: Vec<RankItemDTO>,
    #[serde(default, deserialize_with = "nullable_vec")]
    pub by_access_key: Vec<RankItemDTO>,
    #[serde(default, deserialize_with = "nullable_vec")]
    pub by_hour: Vec<RankItemDTO>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionStatDTO {
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub tool_id: String,
    #[serde(default)]
    pub source_instance_id: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub requests: i64,
    #[serde(default)]
    pub tokens: i64,
    #[serde(default)]
    pub cost_usd: f64,
    #[serde(default)]
    pub priced: Option<bool>,
    #[serde(default)]
    pub first_at: String,
    #[serde(default)]
    pub last_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DayCountDTO {
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub sessions: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionsResultDTO {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub total_sessions: i64,
    #[serde(default)]
    pub active_days: i64,
    #[serde(default)]
    pub unattributed_requests: i64,
    #[serde(default, deserialize_with = "nullable_vec")]
    pub by_day: Vec<DayCountDTO>,
    #[serde(default, deserialize_with = "nullable_vec")]
    pub by_source: Vec<RankItemDTO>,
    #[serde(default, deserialize_with = "nullable_vec")]
    pub sessions: Vec<SessionStatDTO>,
    #[serde(default)]
    pub total: i64,
    #[serde(default)]
    pub page: i64,
    #[serde(default)]
    pub page_size: i64,
    #[serde(default)]
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EventDTO {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub requested_at: String,
    #[serde(default)]
    pub latency_ms: i64,
    #[serde(default)]
    pub ttft_ms: i64,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub account_id: Option<String>,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub result: String,
    #[serde(default)]
    pub http_status: i64,
    #[serde(default)]
    pub input_tokens: i64,
    #[serde(default)]
    pub output_tokens: i64,
    #[serde(default)]
    pub cache_read_tokens: i64,
    #[serde(default)]
    pub cache_write_tokens: i64,
    #[serde(default)]
    pub reasoning_tokens: i64,
    #[serde(default)]
    pub total_tokens: i64,
    #[serde(default)]
    pub cost_micro: i64,
    #[serde(default)]
    pub cost_status: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub tool_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EventsResultDTO {
    #[serde(default, deserialize_with = "nullable_vec")]
    pub events: Vec<EventDTO>,
    #[serde(default)]
    pub total: i64,
    #[serde(default)]
    pub page: i64,
    #[serde(default)]
    pub page_size: i64,
    #[serde(default)]
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SourceViewDTO {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    #[serde(alias = "name")]
    pub display_name: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub last_success_at: String,
    #[serde(default)]
    pub records_imported: i64,
    #[serde(default)]
    pub bytes_read: i64,
    #[serde(default)]
    pub availability: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SourcesResultDTO {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub generated_at: String,
    #[serde(default, deserialize_with = "nullable_vec")]
    pub sources: Vec<SourceViewDTO>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindowDTO {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub used_percent: Option<f64>,
    #[serde(default)]
    pub remaining_percent: Option<f64>,
    #[serde(default)]
    pub reset_time: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct QuotaEntryDTO {
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub account: String,
    #[serde(default)]
    pub status: String,
    #[serde(default, deserialize_with = "nullable_vec")]
    pub windows: Vec<QuotaWindowDTO>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct QuotaResultDTO {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub generated_at: String,
    #[serde(default)]
    pub entries: std::collections::BTreeMap<String, QuotaEntryDTO>,
}

fn nullable_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(deserializer)?.unwrap_or_default())
}

#[cfg(test)]
mod empty_contract_tests {
    use super::*;
    #[test]
    fn actual_model_empty_collections_are_valid_and_pricing_is_not_assumed() {
        let overview: OverviewResultDTO = serde_json::from_str(
            r#"{"status":"empty","trend":null,"metrics":{"totalTokens":0,"priced":true}}"#,
        )
        .unwrap();
        assert!(overview.trend.is_empty());
        assert_eq!(overview.metrics.priced, Some(true));
        let analysis:AnalyticsResultDTO=serde_json::from_str(r#"{"status":"ready","byModel":null,"byProvider":null,"bySource":null,"byAccessKey":null,"byHour":null}"#).unwrap();
        assert!(analysis.by_model.is_empty());
        let source: SourceViewDTO =
            serde_json::from_str(r#"{"id":"codex","name":"Codex"}"#).unwrap();
        assert_eq!(source.display_name, "Codex");
        let unpriced: RankItemDTO =
            serde_json::from_str(r#"{"tokens":3,"costUsd":0,"priced":false}"#).unwrap();
        assert_eq!(unpriced.priced, Some(false));
    }
}
