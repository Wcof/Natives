//! sync: 多设备 Hub 同步协议与数据协调（Reconciliation）。
//! 支持本地模式与 Hub 远端同步模式，采用单调时间戳（Latest-wins）合并。

use crate::storage::Store;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PeriodStats {
    pub total_tokens: i64,
    pub cost_usd: f64,
    pub session_count: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DeviceSyncPayload {
    pub device_id: String,
    pub device_name: String,
    pub timestamp: String,
    pub today: PeriodStats,
    pub all_time: PeriodStats,
    #[serde(default)]
    pub daily: Vec<DailySyncItem>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DailySyncItem {
    pub date: String,
    pub source_id: String,
    pub model: String,
    pub total_tokens: i64,
    pub cost_micros: i64,
    pub session_count: i64,
}

pub fn generate_local_sync_payload(
    store: &Store,
    device_id: &str,
    device_name: &str,
) -> Result<DeviceSyncPayload, String> {
    let now = chrono_now();
    store.with_read(|conn| {
        let today = if now.len() >= 10 { &now[..10] } else { "1970-01-01" };

        let (today_tokens, today_cost_micros, today_sessions): (i64, i64, i64) = conn
            .query_row(
                "SELECT COALESCE(SUM(total_tokens), 0), COALESCE(SUM(cost_micros), 0), COALESCE(SUM(session_count), 0) FROM daily_aggregates WHERE date = ?1",
                [today],
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

        let mut stmt = conn.prepare(
            "SELECT date, source_id, model, total_tokens, cost_micros, session_count FROM daily_aggregates ORDER BY date DESC LIMIT 100"
        ).map_err(|e| e.to_string())?;

        let rows = stmt.query_map([], |row| {
            Ok(DailySyncItem {
                date: row.get(0)?,
                source_id: row.get(1)?,
                model: row.get(2)?,
                total_tokens: row.get(3)?,
                cost_micros: row.get(4)?,
                session_count: row.get(5)?,
            })
        }).map_err(|e| e.to_string())?;

        let mut daily = Vec::new();
        for r in rows {
            daily.push(r.map_err(|e| e.to_string())?);
        }

        Ok(DeviceSyncPayload {
            device_id: device_id.to_string(),
            device_name: device_name.to_string(),
            timestamp: now,
            today: PeriodStats {
                total_tokens: today_tokens,
                cost_usd: today_cost_micros as f64 / 1_000_000.0,
                session_count: today_sessions,
            },
            all_time: PeriodStats {
                total_tokens: all_tokens,
                cost_usd: all_cost_micros as f64 / 1_000_000.0,
                session_count: all_sessions,
            },
            daily,
        })
    })
}

pub fn reconcile_sync_payload(store: &Store, payload: &DeviceSyncPayload) -> Result<(), String> {
    let payload_str = serde_json::to_string(payload).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    hasher.update(payload_str.as_bytes());
    let payload_hash = format!("{:x}", hasher.finalize());

    store.with_write(|conn| {
        // 检查已存设备的单调时间戳
        let existing_timestamp: Option<String> = conn
            .query_row(
                "SELECT last_synced_at FROM sync_state WHERE device_id = ?1",
                [&payload.device_id],
                |r| r.get(0),
            )
            .ok();

        if let Some(existing) = existing_timestamp {
            if payload.timestamp < existing {
                // 忽略过时 payload（单调递增保护）
                return Ok(());
            }
        }

        // 更新 sync_state
        conn.execute(
            "INSERT INTO sync_state (device_id, device_name, last_synced_at, status, payload_hash)
             VALUES (?1, ?2, ?3, 'online', ?4)
             ON CONFLICT(device_id) DO UPDATE SET
                device_name = excluded.device_name,
                last_synced_at = excluded.last_synced_at,
                status = 'online',
                payload_hash = excluded.payload_hash",
            rusqlite::params![
                payload.device_id,
                payload.device_name,
                payload.timestamp,
                payload_hash
            ],
        )
        .map_err(|e| e.to_string())?;

        // 合并远端 daily 统计（使用 device 隔离的 source_id 前缀或直接合并）
        let mut daily_stmt = conn
            .prepare(
                "INSERT INTO daily_aggregates (
                date, source_id, model, total_tokens, input_tokens, output_tokens,
                cache_read_tokens, cost_micros, session_count
            ) VALUES (?1, ?2, ?3, ?4, 0, 0, 0, ?5, ?6)
            ON CONFLICT(date, source_id, model) DO UPDATE SET
                total_tokens = MAX(daily_aggregates.total_tokens, excluded.total_tokens),
                cost_micros = MAX(daily_aggregates.cost_micros, excluded.cost_micros),
                session_count = MAX(daily_aggregates.session_count, excluded.session_count)",
            )
            .map_err(|e| e.to_string())?;

        for d in &payload.daily {
            let remote_source = format!("{}:{}", payload.device_id, d.source_id);
            daily_stmt
                .execute(rusqlite::params![
                    d.date,
                    remote_source,
                    d.model,
                    d.total_tokens,
                    d.cost_micros,
                    d.session_count
                ])
                .map_err(|e| e.to_string())?;
        }

        Ok(())
    })
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
