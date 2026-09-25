//! 设置 / 设备 / 同步 / 采集 / 托盘状态相关 API 处理器。

use super::stats::{chrono_now, chrono_today, format_compact_tokens, panel_json};
use super::{api_error, json_str};
use crate::storage::Store;

pub(super) fn api_sync_push(store: &Store) -> Result<String, (u16, String)> {
    match crate::sync::generate_local_sync_payload(store, "local-device", "My Mac") {
        Ok(payload) => serde_json::to_string(&payload)
            .map_err(|e| api_error(500, "SERIALIZE_FAILED", &e.to_string())),
        Err(e) => Err(api_error(500, "SYNC_PUSH_FAILED", &e)),
    }
}

pub(super) fn api_sync_pull(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let payload: crate::sync::DeviceSyncPayload = serde_json::from_str(body)
        .map_err(|e| api_error(400, "INVALID_PAYLOAD", &e.to_string()))?;

    match crate::sync::reconcile_sync_payload(store, &payload) {
        Ok(()) => Ok("{\"ok\":true}".into()),
        Err(e) => Err(api_error(500, "RECONCILE_FAILED", &e)),
    }
}

pub(super) fn api_devices(store: &Store) -> Result<String, (u16, String)> {
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

pub(super) fn api_get_settings(store: &Store) -> Result<String, (u16, String)> {
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

pub(super) fn api_save_settings(store: &Store, body: &str) -> Result<String, (u16, String)> {
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

pub(super) fn api_collect(store: &Store, _body: &str) -> Result<String, (u16, String)> {
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

pub(super) fn api_tray_state(store: &Store) -> Result<String, (u16, String)> {
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
