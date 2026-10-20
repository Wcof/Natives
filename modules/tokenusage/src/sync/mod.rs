//! sync: 多设备 Hub 同步协议与数据协调（Reconciliation）。
//!
//! 依据架构整改 Wave 2 / ADR-0030 / ADR-0033：
//! - 远端同步数据作为快照写入 `tokenusage_app.db` 的 `remote_usage_snapshots`；
//! - 严禁将远端事件反写到本机 Model Host 的 `usage.db`。

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
    pub snapshot_json: String,
}

pub fn generate_local_sync_payload(
    _store: &Store,
    device_id: &str,
    device_name: &str,
) -> Result<DeviceSyncPayload, String> {
    let now = chrono_now();
    Ok(DeviceSyncPayload {
        device_id: device_id.to_string(),
        device_name: device_name.to_string(),
        timestamp: now,
        today: PeriodStats {
            total_tokens: 0,
            cost_usd: 0.0,
            session_count: 0,
        },
        all_time: PeriodStats {
            total_tokens: 0,
            cost_usd: 0.0,
            session_count: 0,
        },
        snapshot_json: "{}".into(),
    })
}

pub fn reconcile_sync_payload(store: &Store, payload: &DeviceSyncPayload) -> Result<(), String> {
    let payload_str = serde_json::to_string(payload).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    hasher.update(payload_str.as_bytes());
    let payload_hash = format!("{:x}", hasher.finalize());

    store.with_write(|conn| {
        // 1. 记录同步设备状态
        conn.execute(
            "INSERT INTO sync_devices (device_id, device_name, last_synced_at, status, payload_hash)
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

        // 2. 写入远端快照记录（remote_usage_snapshots）
        let snapshot_id = format!("{}_{}", payload.device_id, payload.timestamp);
        conn.execute(
            "INSERT INTO remote_usage_snapshots (id, device_id, range_label, snapshot_json, generated_at)
             VALUES (?1, ?2, 'latest', ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET
                snapshot_json = excluded.snapshot_json,
                generated_at = excluded.generated_at",
            rusqlite::params![
                snapshot_id,
                payload.device_id,
                payload.snapshot_json,
                payload.timestamp
            ],
        )
        .map_err(|e| e.to_string())?;

        Ok(())
    })
}

fn chrono_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", dur.as_secs())
}
