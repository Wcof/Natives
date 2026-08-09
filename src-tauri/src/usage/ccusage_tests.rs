use super::*;

#[test]
fn parses_claude_style_entry_with_model_breakdowns() {
    let raw = r#"{
          "daily": [{
            "date": "2026-07-19",
            "inputTokens": 10,
            "outputTokens": 2,
            "cacheReadTokens": 100,
            "totalTokens": 112,
            "totalCost": 1.5,
            "modelBreakdowns": [
              {"modelName": "claude-opus-4-8", "inputTokens": 10, "outputTokens": 2, "cacheReadTokens": 100, "totalTokens": 112, "cost": 1.5}
            ]
          }],
          "totals": {"totalTokens": 112}
        }"#;
    let resp: CcusageResponse = serde_json::from_str(raw).unwrap();
    assert_eq!(resp.daily.len(), 1);
    let e = &resp.daily[0];
    assert_eq!(e.normalized_date(), "2026-07-19");
    assert_eq!(e.effective_total_tokens(), 112);
    assert_eq!(e.effective_breakdowns().len(), 1);
    assert_eq!(e.effective_breakdowns()[0].model, "claude-opus-4-8");
}

#[test]
fn parses_codex_style_models_object() {
    let raw = r#"{
          "daily": [{
            "date": "2026-07-18",
            "inputTokens": 100,
            "outputTokens": 20,
            "cacheReadTokens": 1000,
            "totalTokens": 1120,
            "costUSD": 9.5,
            "models": {
              "gpt-5.5": {
                "inputTokens": 80,
                "outputTokens": 15,
                "cacheReadTokens": 900,
                "totalTokens": 995
              },
              "gpt-5.6-sol": {
                "inputTokens": 20,
                "outputTokens": 5,
                "cacheReadTokens": 100,
                "totalTokens": 125
              }
            }
          }]
        }"#;
    let resp: CcusageResponse = serde_json::from_str(raw).unwrap();
    let e = &resp.daily[0];
    let bds = e.effective_breakdowns();
    assert_eq!(bds.len(), 2);
    assert!(bds.iter().any(|b| b.model == "gpt-5.5"));
}

#[test]
fn rejects_legacy_by_agent_flag_in_source() {
    // Guard against regressions: the production argv builder must not include
    // the removed ccusage 20 flag.
    let src = include_str!("ccusage.rs");
    assert!(
        !src.contains("\"--by-agent\""),
        "ccusage 20 removed --by-agent; do not pass it"
    );
}

#[tokio::test]
async fn missing_cli_returns_empty_not_hard_failure() {
    // Force PATH without ccusage.
    let old = std::env::var_os("PATH");
    std::env::set_var("PATH", "/tmp/natives-empty-path-no-ccusage");
    let result = scan_ccusage_all("20260701", "20260720", "Asia/Shanghai").await;
    match old {
        Some(v) => std::env::set_var("PATH", v),
        None => std::env::remove_var("PATH"),
    }
    assert!(result.results.is_empty());
    assert!(result.verification.is_empty());
    assert!(result
        .warnings
        .iter()
        .any(|w| matches!(w.code, UsageWarningCode::CliNotFound)));
}
