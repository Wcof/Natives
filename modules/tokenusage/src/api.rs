//! HTTP API 路由与业务请求处理（tokenusage 模块）。

use crate::storage::Store;
use app_runtime_core::http::HttpRequest;

pub fn handle_api_request(
    store: &Store,
    request: &HttpRequest,
    route: &str,
) -> Result<String, (u16, String)> {
    let body_text = String::from_utf8_lossy(&request.body).to_string();
    let method = request.method.as_str();
    match (method, route) {
        ("GET", "/api/overview") => api_overview(store),
        ("GET", "/api/tools") => api_tools(store),
        ("GET", "/api/models") => api_models(store),
        ("GET", "/api/sessions") => api_sessions(store, request.path.as_str()),
        ("GET", "/api/sessions/detail") => api_session_detail(store, request.path.as_str()),
        ("GET", "/api/limits") => api_limits(store),
        ("POST", "/api/limits/refresh") => api_limits_refresh(store, &body_text),
        ("GET", "/api/trends") => api_trends(store, request.path.as_str()),
        ("GET", "/api/devices") => api_devices(store),
        ("POST", "/api/sync/push") => api_sync_push(store),
        ("POST", "/api/sync/pull") => api_sync_pull(store, &body_text),
        ("GET", "/api/settings") => api_get_settings(store),
        ("POST", "/api/settings") => api_save_settings(store, &body_text),
        ("POST", "/api/collect") => api_collect(store, &body_text),
        ("GET", "/api/tray/state") => api_tray_state(store),
        _ => Err((404, "{\"error\":\"not found\"}".into())),
    }
}

pub fn json_str(s: &str) -> String {
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

pub fn api_error(code: u16, error_code: &str, message: &str) -> (u16, String) {
    (
        code,
        format!(
            "{{\"error\":{},\"message\":{}}}",
            json_str(error_code),
            json_str(message)
        ),
    )
}

fn api_overview(store: &Store) -> Result<String, (u16, String)> {
    store.with_read(|conn| {
        // 今日统计 (UTC / 本地日期)
        let today = chrono_today();
        let week_start = chrono_days_ago(7);
        let month_start = format!("{}-01", &today[..7.min(today.len())]);

        let (today_tokens, today_cost_micros, today_sessions): (i64, i64, i64) = conn
            .query_row(
                "SELECT COALESCE(SUM(total_tokens), 0), COALESCE(SUM(cost_micros), 0), COALESCE(SUM(session_count), 0) FROM daily_aggregates WHERE date = ?1",
                [&today],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap_or((0, 0, 0));

        let (week_tokens, week_cost_micros, week_sessions): (i64, i64, i64) = conn
            .query_row(
                "SELECT COALESCE(SUM(total_tokens), 0), COALESCE(SUM(cost_micros), 0), COALESCE(SUM(session_count), 0) FROM daily_aggregates WHERE date >= ?1",
                [&week_start],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap_or((0, 0, 0));

        let (month_tokens, month_cost_micros, month_sessions): (i64, i64, i64) = conn
            .query_row(
                "SELECT COALESCE(SUM(total_tokens), 0), COALESCE(SUM(cost_micros), 0), COALESCE(SUM(session_count), 0) FROM daily_aggregates WHERE date >= ?1",
                [&month_start],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap_or((0, 0, 0));

        let (all_tokens, all_cost_micros, all_sessions): (i64, i64, i64) = conn
            .query_row(
                "SELECT COALESCE(SUM(total_tokens), 0), COALESCE(SUM(cost_micros), 0), COALESCE(SUM(session_count), 0) FROM daily_aggregates",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap_or((0, 0, 0));

        let today_cost = today_cost_micros as f64 / 1_000_000.0;
        let week_cost = week_cost_micros as f64 / 1_000_000.0;
        let month_cost = month_cost_micros as f64 / 1_000_000.0;
        let all_cost = all_cost_micros as f64 / 1_000_000.0;

        Ok(format!(
            "{{\"currency\":\"USD\",\"periods\":{{\"today\":{{\"totalTokens\":{},\"costUsd\":{:.4},\"sessionCount\":{}}},\"thisWeek\":{{\"totalTokens\":{},\"costUsd\":{:.4},\"sessionCount\":{}}},\"thisMonth\":{{\"totalTokens\":{},\"costUsd\":{:.4},\"sessionCount\":{}}},\"allTime\":{{\"totalTokens\":{},\"costUsd\":{:.4},\"sessionCount\":{}}}}}}}",
            today_tokens, today_cost, today_sessions,
            week_tokens, week_cost, week_sessions,
            month_tokens, month_cost, month_sessions,
            all_tokens, all_cost, all_sessions
        ))
    }).map_err(|e| api_error(500, "DB_ERROR", &e))
}

fn api_tools(store: &Store) -> Result<String, (u16, String)> {
    store.with_read(|conn| {
        let mut stmt = conn
            .prepare(
                "SELECT s.id, s.display_name, s.category, s.enabled,
                        COALESCE(SUM(d.total_tokens), 0),
                        COALESCE(SUM(d.cost_micros), 0),
                        COALESCE(SUM(d.session_count), 0),
                        COALESCE(s.last_scanned_at, '')
                 FROM usage_sources s
                 LEFT JOIN daily_aggregates d ON s.id = d.source_id
                 GROUP BY s.id ORDER BY SUM(d.total_tokens) DESC, s.display_name ASC",
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([], |row| {
                let id: String = row.get(0)?;
                let name: String = row.get(1)?;
                let category: String = row.get(2)?;
                let enabled: bool = row.get::<_, i64>(3)? != 0;
                let tokens: i64 = row.get(4)?;
                let cost_micros: i64 = row.get(5)?;
                let sessions: i64 = row.get(6)?;
                let last_scanned: String = row.get(7)?;
                let cost_usd = cost_micros as f64 / 1_000_000.0;

                Ok(format!(
                    "{{\"id\":{},\"name\":{},\"category\":{},\"enabled\":{},\"totalTokens\":{},\"costUsd\":{:.4},\"sessionCount\":{},\"lastScannedAt\":{}}}",
                    json_str(&id), json_str(&name), json_str(&category), enabled, tokens, cost_usd, sessions, json_str(&last_scanned)
                ))
            })
            .map_err(|e| e.to_string())?;

        let mut items = Vec::new();
        for r in rows {
            items.push(r.map_err(|e| e.to_string())?);
        }
        Ok(format!("{{\"tools\":[{}]}}", items.join(",")))
    }).map_err(|e| api_error(500, "DB_ERROR", &e))
}

fn api_models(store: &Store) -> Result<String, (u16, String)> {
    store.with_read(|conn| {
        let mut stmt = conn
            .prepare(
                "SELECT model,
                        COALESCE(SUM(total_tokens), 0),
                        COALESCE(SUM(input_tokens), 0),
                        COALESCE(SUM(output_tokens), 0),
                        COALESCE(SUM(cost_micros), 0)
                 FROM daily_aggregates
                 GROUP BY model ORDER BY SUM(total_tokens) DESC",
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([], |row| {
                let model: String = row.get(0)?;
                let total: i64 = row.get(1)?;
                let input: i64 = row.get(2)?;
                let output: i64 = row.get(3)?;
                let cost_micros: i64 = row.get(4)?;
                let cost_usd = cost_micros as f64 / 1_000_000.0;

                Ok(format!(
                    "{{\"model\":{},\"totalTokens\":{},\"inputTokens\":{},\"outputTokens\":{},\"costUsd\":{:.4}}}",
                    json_str(&model), total, input, output, cost_usd
                ))
            })
            .map_err(|e| e.to_string())?;

        let mut items = Vec::new();
        for r in rows {
            items.push(r.map_err(|e| e.to_string())?);
        }
        Ok(format!("{{\"models\":[{}]}}", items.join(",")))
    }).map_err(|e| api_error(500, "DB_ERROR", &e))
}

fn api_sessions(store: &Store, _path: &str) -> Result<String, (u16, String)> {
    store.with_read(|conn| {
        let mut stmt = conn
            .prepare(
                "SELECT session_id, source_id, title, project_path, started_at, last_used_at,
                        total_input_tokens + total_output_tokens, total_cost_micros, message_count
                 FROM sessions ORDER BY last_used_at DESC LIMIT 100",
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([], |row| {
                let sid: String = row.get(0)?;
                let source_id: String = row.get(1)?;
                let title: String = row.get(2)?;
                let project: String = row.get(3)?;
                let started: String = row.get(4)?;
                let last_used: String = row.get(5)?;
                let tokens: i64 = row.get(6)?;
                let cost_micros: i64 = row.get(7)?;
                let count: i64 = row.get(8)?;
                let cost_usd = cost_micros as f64 / 1_000_000.0;

                Ok(format!(
                    "{{\"sessionId\":{},\"sourceId\":{},\"title\":{},\"projectPath\":{},\"startedAt\":{},\"lastUsedAt\":{},\"totalTokens\":{},\"costUsd\":{:.4},\"messageCount\":{}}}",
                    json_str(&sid), json_str(&source_id), json_str(&title), json_str(&project),
                    json_str(&started), json_str(&last_used), tokens, cost_usd, count
                ))
            })
            .map_err(|e| e.to_string())?;

        let mut items = Vec::new();
        for r in rows {
            items.push(r.map_err(|e| e.to_string())?);
        }
        Ok(format!("{{\"sessions\":[{}]}}", items.join(",")))
    }).map_err(|e| api_error(500, "DB_ERROR", &e))
}

fn api_session_detail(store: &Store, path: &str) -> Result<String, (u16, String)> {
    let session_id = parse_query_param(path, "id").unwrap_or_default();
    if session_id.is_empty() {
        return Err(api_error(400, "MISSING_ID", "Missing session id"));
    }

    store.with_read(|conn| {
        let mut stmt = conn
            .prepare(
                "SELECT id, turn_id, model, input_tokens, output_tokens, cache_read_tokens,
                        reasoning_tokens, cost_micros, recorded_at
                 FROM usage_records WHERE session_id = ?1 ORDER BY recorded_at ASC, id ASC",
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([&session_id], |row| {
                let id: i64 = row.get(0)?;
                let turn: Option<String> = row.get(1)?;
                let model: String = row.get(2)?;
                let input: i64 = row.get(3)?;
                let output: i64 = row.get(4)?;
                let cache_read: i64 = row.get(5)?;
                let reasoning: i64 = row.get(6)?;
                let cost_micros: i64 = row.get(7)?;
                let recorded_at: String = row.get(8)?;
                let cost_usd = cost_micros as f64 / 1_000_000.0;

                Ok(format!(
                    "{{\"id\":{},\"turnId\":{},\"model\":{},\"inputTokens\":{},\"outputTokens\":{},\"cacheReadTokens\":{},\"reasoningTokens\":{},\"costUsd\":{:.4},\"recordedAt\":{}}}",
                    id, json_str(&turn.unwrap_or_default()), json_str(&model), input, output, cache_read, reasoning, cost_usd, json_str(&recorded_at)
                ))
            })
            .map_err(|e| e.to_string())?;

        let mut items = Vec::new();
        for r in rows {
            items.push(r.map_err(|e| e.to_string())?);
        }
        Ok(format!("{{\"sessionId\":{},\"records\":[{}]}}", json_str(&session_id), items.join(",")))
    }).map_err(|e| api_error(500, "DB_ERROR", &e))
}

fn api_limits(store: &Store) -> Result<String, (u16, String)> {
    store.with_read(|conn| {
        let mut stmt = conn
            .prepare(
                "SELECT provider_id, account_id, window_kind, label, used_percent,
                        remaining_percent, used_units, total_units, unit_type, resets_at,
                        fetched_at, status
                 FROM limits_cache ORDER BY provider_id ASC, window_kind ASC",
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([], |row| {
                let provider: String = row.get(0)?;
                let account: String = row.get(1)?;
                let window: String = row.get(2)?;
                let label: String = row.get(3)?;
                let used_pct: Option<f64> = row.get(4)?;
                let remaining_pct: Option<f64> = row.get(5)?;
                let used_units: Option<f64> = row.get(6)?;
                let total_units: Option<f64> = row.get(7)?;
                let unit_type: String = row.get(8)?;
                let resets_at: Option<String> = row.get(9)?;
                let fetched_at: String = row.get(10)?;
                let status: String = row.get(11)?;

                Ok(format!(
                    "{{\"providerId\":{},\"accountId\":{},\"windowKind\":{},\"label\":{},\"usedPercent\":{},\"remainingPercent\":{},\"usedUnits\":{},\"totalUnits\":{},\"unitType\":{},\"resetsAt\":{},\"fetchedAt\":{},\"status\":{}}}",
                    json_str(&provider), json_str(&account), json_str(&window), json_str(&label),
                    used_pct.map(|v| format!("{:.1}", v)).unwrap_or_else(|| "null".into()),
                    remaining_pct.map(|v| format!("{:.1}", v)).unwrap_or_else(|| "null".into()),
                    used_units.map(|v| format!("{:.2}", v)).unwrap_or_else(|| "null".into()),
                    total_units.map(|v| format!("{:.2}", v)).unwrap_or_else(|| "null".into()),
                    json_str(&unit_type),
                    json_str(&resets_at.unwrap_or_default()),
                    json_str(&fetched_at),
                    json_str(&status)
                ))
            })
            .map_err(|e| e.to_string())?;

        let mut items = Vec::new();
        for r in rows {
            items.push(r.map_err(|e| e.to_string())?);
        }
        Ok(format!("{{\"limits\":[{}]}}", items.join(",")))
    }).map_err(|e| api_error(500, "DB_ERROR", &e))
}

fn api_limits_refresh(store: &Store, _body: &str) -> Result<String, (u16, String)> {
    match crate::limits::refresh_limits(store) {
        Ok(count) => Ok(format!("{{\"ok\":true,\"refreshed\":{}}}", count)),
        Err(e) => Err(api_error(500, "LIMITS_REFRESH_FAILED", &e)),
    }
}

fn api_sync_push(store: &Store) -> Result<String, (u16, String)> {
    match crate::sync::generate_local_sync_payload(store, "local-device", "My Mac") {
        Ok(payload) => serde_json::to_string(&payload)
            .map_err(|e| api_error(500, "SERIALIZE_FAILED", &e.to_string())),
        Err(e) => Err(api_error(500, "SYNC_PUSH_FAILED", &e)),
    }
}

fn api_sync_pull(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let payload: crate::sync::DeviceSyncPayload = serde_json::from_str(body)
        .map_err(|e| api_error(400, "INVALID_PAYLOAD", &e.to_string()))?;

    match crate::sync::reconcile_sync_payload(store, &payload) {
        Ok(()) => Ok("{\"ok\":true}".into()),
        Err(e) => Err(api_error(500, "RECONCILE_FAILED", &e)),
    }
}

fn api_trends(store: &Store, _path: &str) -> Result<String, (u16, String)> {
    store.with_read(|conn| {
        let mut stmt = conn
            .prepare(
                "SELECT date, source_id, SUM(total_tokens), SUM(cost_micros), SUM(session_count)
                 FROM daily_aggregates GROUP BY date, source_id ORDER BY date ASC",
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([], |row| {
                let date: String = row.get(0)?;
                let source_id: String = row.get(1)?;
                let tokens: i64 = row.get(2)?;
                let cost_micros: i64 = row.get(3)?;
                let sessions: i64 = row.get(4)?;
                let cost_usd = cost_micros as f64 / 1_000_000.0;

                Ok(format!(
                    "{{\"date\":{},\"sourceId\":{},\"totalTokens\":{},\"costUsd\":{:.4},\"sessionCount\":{}}}",
                    json_str(&date), json_str(&source_id), tokens, cost_usd, sessions
                ))
            })
            .map_err(|e| e.to_string())?;

        let mut items = Vec::new();
        for r in rows {
            items.push(r.map_err(|e| e.to_string())?);
        }
        Ok(format!("{{\"trends\":[{}]}}", items.join(",")))
    }).map_err(|e| api_error(500, "DB_ERROR", &e))
}

fn api_devices(store: &Store) -> Result<String, (u16, String)> {
    store.with_read(|conn| {
        let mut stmt = conn
            .prepare("SELECT device_id, device_name, last_synced_at, status FROM sync_state ORDER BY last_synced_at DESC")
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([], |row| {
                let id: String = row.get(0)?;
                let name: String = row.get(1)?;
                let last: String = row.get(2)?;
                let status: String = row.get(3)?;

                Ok(format!(
                    "{{\"deviceId\":{},\"deviceName\":{},\"lastSyncedAt\":{},\"status\":{}}}",
                    json_str(&id), json_str(&name), json_str(&last), json_str(&status)
                ))
            })
            .map_err(|e| e.to_string())?;

        let mut items = Vec::new();
        for r in rows {
            items.push(r.map_err(|e| e.to_string())?);
        }
        Ok(format!("{{\"devices\":[{}]}}", items.join(",")))
    }).map_err(|e| api_error(500, "DB_ERROR", &e))
}

fn api_get_settings(store: &Store) -> Result<String, (u16, String)> {
    store
        .with_read(|conn| {
            let mut stmt = conn
                .prepare("SELECT key, value_json FROM settings")
                .map_err(|e| e.to_string())?;

            let rows = stmt
                .query_map([], |row| {
                    let k: String = row.get(0)?;
                    let v: String = row.get(1)?;
                    Ok(format!("{}:{}", json_str(&k), v))
                })
                .map_err(|e| e.to_string())?;

            let mut entries = Vec::new();
            for r in rows {
                entries.push(r.map_err(|e| e.to_string())?);
            }
            Ok(format!("{{{}}}", entries.join(",")))
        })
        .map_err(|e| api_error(500, "DB_ERROR", &e))
}

fn api_save_settings(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let parsed: serde_json::Value =
        serde_json::from_str(body).map_err(|e| api_error(400, "INVALID_JSON", &e.to_string()))?;

    let now = chrono_now();
    store.with_write(|conn| {
        if let Some(obj) = parsed.as_object() {
            let mut stmt = conn
                .prepare("INSERT INTO settings (key, value_json, updated_at) VALUES (?1, ?2, ?3) ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json, updated_at = excluded.updated_at")
                .map_err(|e| e.to_string())?;
            for (k, v) in obj {
                stmt.execute([k, &v.to_string(), &now])
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok("{\"ok\":true}".into())
    }).map_err(|e| api_error(500, "DB_ERROR", &e))
}

fn api_collect(store: &Store, _body: &str) -> Result<String, (u16, String)> {
    match crate::collector::collect_all(store, None, None) {
        Ok(summary) => Ok(format!(
            "{{\"ok\":true,\"scannedSources\":{},\"newRecords\":{},\"totalTokens\":{},\"totalCostMicros\":{},\"hostEvents\":{}}}",
            summary.local_sources,
            summary.local_records + summary.host_events,
            summary.local_tokens + summary.host_tokens,
            summary.local_cost_micros + summary.host_cost_micros,
            summary.host_events
        )),
        Err(e) => Err(api_error(500, "SCAN_FAILED", &e)),
    }
}

fn api_tray_state(store: &Store) -> Result<String, (u16, String)> {
    store.with_read(|conn| {
        let today = chrono_today();
        let (today_tokens, today_cost_micros): (i64, i64) = conn
            .query_row(
                "SELECT COALESCE(SUM(total_tokens), 0), COALESCE(SUM(cost_micros), 0) FROM daily_aggregates WHERE date = ?1",
                [&today],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap_or((0, 0));

        let (all_tokens, all_cost_micros): (i64, i64) = conn
            .query_row(
                "SELECT COALESCE(SUM(total_tokens), 0), COALESCE(SUM(cost_micros), 0) FROM daily_aggregates",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap_or((0, 0));

        let today_cost = today_cost_micros as f64 / 1_000_000.0;
        let all_cost = all_cost_micros as f64 / 1_000_000.0;

        // 如果今日有 tokens，展示今日；若今日为 0 且有历史数据，展示最近活跃日数据
        let (display_tokens, display_cost) = if today_tokens > 0 {
            (today_tokens, today_cost)
        } else {
            let recent: Option<(i64, i64)> = conn
                .query_row(
                    "SELECT SUM(total_tokens), SUM(cost_micros) FROM daily_aggregates GROUP BY date ORDER BY date DESC LIMIT 1",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .ok();
            match recent {
                Some((rt, rc)) if rt > 0 => (rt, rc as f64 / 1_000_000.0),
                _ => (0, 0.0),
            }
        };

        let formatted_tokens = format_compact_tokens(display_tokens);
        let formatted_cost = format!("${:.2}", display_cost);
        let display_text = format!("{} · {}", formatted_tokens, formatted_cost);

        // 最低剩余额度
        let worst_limit: Option<(String, String, f64, String)> = conn
            .query_row(
                "SELECT provider_id, window_kind, remaining_percent, resets_at FROM limits_cache WHERE remaining_percent IS NOT NULL ORDER BY remaining_percent ASC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .ok();

        let worst_json = match worst_limit {
            Some((p, w, r, res)) => format!(
                "{{\"providerId\":{},\"windowKind\":{},\"remainingPercent\":{:.1},\"resetsAt\":{}}}",
                json_str(&p), json_str(&w), r, json_str(&res)
            ),
            None => "null".into(),
        };

        let tooltip = if today_tokens > 0 {
            format!("Token Monitor: 今日 {} (${:.2}) · 累计 {} (${:.2})", formatted_tokens, today_cost, format_compact_tokens(all_tokens), all_cost)
        } else if all_tokens > 0 {
            format!("Token Monitor: 最近活跃 {} (${:.2}) · 累计 {} (${:.2})", formatted_tokens, display_cost, format_compact_tokens(all_tokens), all_cost)
        } else {
            "Token Monitor: 暂无用量记录".to_string()
        };

        Ok(format!(
            "{{\"mode\":\"both\",\"displayText\":{},\"tooltip\":{},\"worstLimit\":{},\"panel\":{}}}",
            json_str(&display_text),
            json_str(&tooltip),
            worst_json,
            panel_json(conn)
        ))
    }).map_err(|e| api_error(500, "DB_ERROR", &e))
}

// 面板聚合数据（ADR-0032 顶栏只读接口）：总览、工具 Top、最近会话、趋势。
// 注意：调用方必须已持有 conn（with_read 闭包内），严禁再走 with_read——
// conn 是非重入 Mutex，二次加锁会自锁死锁。
fn panel_json(conn: &rusqlite::Connection) -> String {
    {
        let today = chrono_today();

        let week_start = chrono_days_ago(7);
        let month_start = format!("{}-01", &today[..7.min(today.len())]);

        // 总览：今日 + 本周 + 本月 + 累计
        let (today_tokens, today_cost_micros): (i64, i64) = conn
            .query_row(
                "SELECT COALESCE(SUM(total_tokens), 0), COALESCE(SUM(cost_micros), 0) FROM daily_aggregates WHERE date = ?1",
                [&today],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap_or((0, 0));
        let (week_tokens, week_cost_micros): (i64, i64) = conn
            .query_row(
                "SELECT COALESCE(SUM(total_tokens), 0), COALESCE(SUM(cost_micros), 0) FROM daily_aggregates WHERE date >= ?1",
                [&week_start],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap_or((0, 0));
        let (month_tokens, month_cost_micros): (i64, i64) = conn
            .query_row(
                "SELECT COALESCE(SUM(total_tokens), 0), COALESCE(SUM(cost_micros), 0) FROM daily_aggregates WHERE date >= ?1",
                [&month_start],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap_or((0, 0));
        let (all_tokens, all_cost_micros): (i64, i64) = conn
            .query_row(
                "SELECT COALESCE(SUM(total_tokens), 0), COALESCE(SUM(cost_micros), 0) FROM daily_aggregates",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap_or((0, 0));

        // 工具 Top5（按 token 降序）
        let mut stmt = conn
            .prepare(
                "SELECT s.display_name, COALESCE(SUM(d.total_tokens), 0), COALESCE(SUM(d.cost_micros), 0)
                 FROM usage_sources s LEFT JOIN daily_aggregates d ON s.id = d.source_id
                 GROUP BY s.id ORDER BY SUM(d.total_tokens) DESC LIMIT 5",
            )
            .unwrap_or_else(|_| unreachable!());
        let tools: Vec<String> = stmt
            .query_map([], |row| {
                let name: String = row.get(0)?;
                let tokens: i64 = row.get(1)?;
                let cost: f64 = row.get::<_, i64>(2)? as f64 / 1_000_000.0;
                Ok(format!(
                    "{{\"name\":{},\"tokens\":{},\"costUsd\":{:.2}}}",
                    json_str(&name),
                    tokens,
                    cost
                ))
            })
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
            .unwrap_or_default();
        drop(stmt);

        // 最近会话 Top5
        let mut stmt = conn
            .prepare(
                "SELECT session_id, source_id, total_input_tokens + total_output_tokens + total_cache_read_tokens + total_cache_write_tokens + total_reasoning_tokens, total_cost_micros, last_used_at
                 FROM sessions ORDER BY last_used_at DESC LIMIT 5",
            )
            .unwrap_or_else(|_| unreachable!());
        let sessions_json: Vec<String> = stmt
            .query_map([], |row| {
                let sid: String = row.get(0)?;
                let src: String = row.get(1)?;
                let tokens: i64 = row.get(2)?;
                let cost: f64 = row.get::<_, i64>(3)? as f64 / 1_000_000.0;
                let active: String = row.get(4)?;
                Ok(format!(
                    "{{\"id\":{},\"source\":{},\"totalTokens\":{},\"costUsd\":{:.2},\"lastActive\":{}}}",
                    json_str(&sid), json_str(&src), tokens, cost, json_str(&active)
                ))
            })
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
            .unwrap_or_default();
        drop(stmt);

        // 近 7 天趋势
        let mut stmt = conn
            .prepare(
                "SELECT date, SUM(total_tokens), SUM(cost_micros) FROM daily_aggregates
                 GROUP BY date ORDER BY date DESC LIMIT 7",
            )
            .unwrap_or_else(|_| unreachable!());
        let trends: Vec<String> = stmt
            .query_map([], |row| {
                let date: String = row.get(0)?;
                let tokens: i64 = row.get(1)?;
                let cost: f64 = row.get::<_, i64>(2)? as f64 / 1_000_000.0;
                Ok(format!(
                    "{{\"date\":{},\"totalTokens\":{},\"costUsd\":{:.2}}}",
                    json_str(&date),
                    tokens,
                    cost
                ))
            })
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
            .unwrap_or_default();
        drop(stmt);

        format!(
            "{{\"today\":{{\"totalTokens\":{},\"costUsd\":{:.2}}},\"thisWeek\":{{\"totalTokens\":{},\"costUsd\":{:.2}}},\"thisMonth\":{{\"totalTokens\":{},\"costUsd\":{:.2}}},\"allTime\":{{\"totalTokens\":{},\"costUsd\":{:.2}}},\"overview\":{{\"periods\":{{\"today\":{{\"totalTokens\":{},\"costUsd\":{:.2}}},\"thisWeek\":{{\"totalTokens\":{},\"costUsd\":{:.2}}},\"thisMonth\":{{\"totalTokens\":{},\"costUsd\":{:.2}}},\"allTime\":{{\"totalTokens\":{},\"costUsd\":{:.2}}}}}}},\"tools\":[{}],\"sessions\":[{}],\"trends\":[{}]}}",
            today_tokens,
            today_cost_micros as f64 / 1_000_000.0,
            week_tokens,
            week_cost_micros as f64 / 1_000_000.0,
            month_tokens,
            month_cost_micros as f64 / 1_000_000.0,
            all_tokens,
            all_cost_micros as f64 / 1_000_000.0,
            today_tokens,
            today_cost_micros as f64 / 1_000_000.0,
            week_tokens,
            week_cost_micros as f64 / 1_000_000.0,
            month_tokens,
            month_cost_micros as f64 / 1_000_000.0,
            all_tokens,
            all_cost_micros as f64 / 1_000_000.0,
            tools.join(","),
            sessions_json.join(","),
            trends.join(",")
        )
    }
}

fn format_compact_tokens(tokens: i64) -> String {
    if tokens >= 1_000_000_000 {
        format!("{:.2}B", tokens as f64 / 1_000_000_000.0)
    } else if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}K", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

fn chrono_days_ago(n_days: u64) -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = (dur / 86400).saturating_sub(n_days);
    let (y, m, d) = days_to_ymd(days);
    format!("{:04}-{:02}-{:02}", y, m, d)
}

fn chrono_today() -> String {
    // 简易 UTC 日期格式 YYYY-MM-DD
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = dur / 86400;
    // 粗略公历算法
    let (y, m, d) = days_to_ymd(days);
    format!("{:04}-{:02}-{:02}", y, m, d)
}

fn chrono_now() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = dur / 86400;
    let secs_of_day = dur % 86400;
    let (y, m, d) = days_to_ymd(days);
    let h = secs_of_day / 3600;
    let min = (secs_of_day % 3600) / 60;
    let s = secs_of_day % 60;
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, m, d, h, min, s)
}

fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let yr = if m <= 2 { y + 1 } else { y };
    (yr, m, d)
}

fn parse_query_param(path: &str, key: &str) -> Option<String> {
    let q = path.split_once('?')?;
    for pair in q.1.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == key {
                return Some(v.to_string());
            }
        }
    }
    None
}
