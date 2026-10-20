//! 通知渠道 API（R5，S02～S04/S15）：渠道 CRUD／状态／Secret 引用／策略／
//! 测试发送／投递记录。Secret 明文只经 Keychain（ADR-0038），本文件只透传
//! 引用与状态；测试发送经生产 HTTPS 执行器（SSRF 守卫+有界超时）。

use super::home_api::{body_json, err};
use crate::channels;
use crate::storage::Store;
use serde_json::json;

pub fn api_notify_channels(store: &Store) -> Result<String, (u16, String)> {
    channels::list(store)
        .map(|v| v.to_string())
        .map_err(|e| err(400, &e))
}

pub fn api_notify_channels_save(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let v = body_json(body)?;
    channels::save(store, &v)
        .map(|v| v.to_string())
        .map_err(|e| err(409, &e))
}

pub fn api_notify_channels_secret(
    store: &Store,
    secrets: Option<&std::sync::Arc<app_runtime_core::secrets::SecretStore>>,
    body: &str,
) -> Result<String, (u16, String)> {
    let v = body_json(body)?;
    channels::set_secret(store, secrets, &v)
        .map(|v| v.to_string())
        .map_err(|e| err(400, &e))
}

pub fn api_notify_channels_state(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let v = body_json(body)?;
    channels::set_state(store, &v)
        .map(|v| v.to_string())
        .map_err(|e| err(409, &e))
}

pub fn api_notify_channels_delete(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let v = body_json(body)?;
    channels::delete(store, &v)
        .map(|v| v.to_string())
        .map_err(|e| err(409, &e))
}

pub fn api_notify_settings_get(store: &Store) -> Result<String, (u16, String)> {
    let policy = channels::load_policy(store);
    let version = store
        .with_read(|c| {
            c.query_row(
                "SELECT value FROM app_meta WHERE key='notify_policy_version'",
                [],
                |r| r.get::<_, String>(0),
            )
        })
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(0);
    Ok(json!({
        "version":version,
        "silenceStart": policy.silence_start.unwrap_or_default(),
        "silenceEnd": policy.silence_end.unwrap_or_default(),
        "retryMax": policy.retry_max,
        "backoffBase": policy.backoff_base,
        "dedupMinutes": policy.dedup_minutes,
        "timezone": "local",
    })
    .to_string())
}

pub fn api_notify_settings_save(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let v = body_json(body)?;
    channels::save_policy(store, &v)
        .map(|v| v.to_string())
        .map_err(|e| err(400, &e))
}

pub fn api_notify_deliveries(store: &Store, path: &str) -> Result<String, (u16, String)> {
    let query = path.split('?').nth(1).unwrap_or("");
    let mut channel_id = None;
    let mut page = 1i64;
    for pair in query.split('&') {
        let mut it = pair.splitn(2, '=');
        let (k, v) = (it.next().unwrap_or(""), it.next().unwrap_or(""));
        match k {
            "channelId" => channel_id = v.parse::<i64>().ok(),
            "page" => page = v.parse::<i64>().unwrap_or(1).max(1),
            _ => {}
        }
    }
    channels::deliveries(store, channel_id, page)
        .map(|v| v.to_string())
        .map_err(|e| err(400, &e))
}
