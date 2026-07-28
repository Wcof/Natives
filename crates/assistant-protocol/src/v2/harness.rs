//! Typed Harness control-plane streaming contracts.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessSubscribeRequest {
    #[serde(default, alias = "after_cursor", alias = "afterCursor")]
    pub cursor: i64,
    #[serde(default, alias = "waitMs")]
    pub wait_ms: u64,
    #[serde(default = "default_notice_limit")]
    pub limit: i64,
}

fn default_notice_limit() -> i64 {
    100
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessNoticeKind {
    Published,
    BindingChanged,
    SourceDrift,
    TraceUpdated,
    ResetRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessNotice {
    pub cursor: i64,
    pub kind: HarnessNoticeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessSubscribeResponse {
    pub notices: Vec<HarnessNotice>,
    pub next_cursor: i64,
    pub reset_required: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscribe_request_accepts_the_documented_cursor_alias() {
        let request: HarnessSubscribeRequest =
            serde_json::from_value(serde_json::json!({"after_cursor": 7, "wait_ms": 20})).unwrap();
        assert_eq!(request.cursor, 7);
        assert_eq!(request.wait_ms, 20);
        assert_eq!(request.limit, 100);
    }
}
