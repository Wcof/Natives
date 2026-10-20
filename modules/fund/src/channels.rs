//! 通知渠道域（R5，S02～S04/S15，方案 10）：九渠道元数据 CRUD、受控 Secret
//! 引用（ADR-0038，明文只进 OS Keychain，DB 只存引用与状态）、静默／重试／
//! 去重策略与投递事实。请求适配器可注入：生产经有界 HTTPS 执行器+SSRF 守卫，
//! 隔离验证注入记录型执行器，不触网、不发真实消息。

use crate::finance::FinanceError;
use crate::storage::Store;
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};

#[cfg(test)]
use crate::channel_protocols::business_ok;
pub(crate) use crate::channel_protocols::field_text;
pub use crate::channel_protocols::{
    build_request, deliveries, send, ExecuteResult, OutboundRequest,
};

pub const CHANNEL_KINDS: &[&str] = &[
    "telegram",
    "bark",
    "dingtalk",
    "wecom",
    "feishu",
    "serverchan",
    "pushplus",
    "discord",
    "pushover",
];

/// 每渠道允许的 Secret 字段；钉钉 secret 为可选加签，token 必填。
pub fn secret_fields(kind: &str) -> &'static [&'static str] {
    match kind {
        "telegram" => &["bot_token"],
        "bark" => &["device_key"],
        "dingtalk" => &["token", "secret"],
        "wecom" => &["webhook_key"],
        "feishu" => &["webhook_token"],
        "serverchan" => &["sendkey"],
        "pushplus" => &["token"],
        "discord" => &["webhook_token"],
        "pushover" => &["user_key", "app_token"],
        _ => &[],
    }
}

pub fn required_secret_fields(kind: &str) -> &'static [&'static str] {
    if kind == "dingtalk" {
        &["token"]
    } else {
        secret_fields(kind)
    }
}

/// 每渠道非秘密配置字段白名单；未知字段拒绝，不让用户写 JSON。
pub fn config_fields(kind: &str) -> &'static [&'static str] {
    match kind {
        "telegram" => &["chat_id"],
        "bark" => &["server_url"],
        "dingtalk" => &["phones", "keyword"],
        "pushplus" => &["topic"],
        "discord" => &["webhook_id"],
        _ => &[],
    }
}

pub(crate) fn validate_kind(kind: &str) -> Result<(), FinanceError> {
    if !CHANNEL_KINDS.contains(&kind) {
        return Err(FinanceError::Invalid("未知的通知渠道类型".into()));
    }
    Ok(())
}

/// SSRF 守卫（方案 10 前置 5）：仅 https；拒绝本地、私网、link-local 与云
/// metadata 主机；域名返回 false 由调用方按解析失败拒绝（DNS 期间不放宽）。
pub(crate) fn host_allowed(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    if parsed.scheme() != "https"
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.port_or_known_default() != Some(443)
        || parsed.fragment().is_some()
    {
        return false;
    }
    let Some(host) = parsed.host_str() else {
        return false;
    };
    let host = host.trim_end_matches('.');
    if host.is_empty() {
        return false;
    }
    // 字面量 IP 直接判定；域名按黑名单与明显内网后缀拒绝（公网解析在执行期生效）。
    if let Ok(ip) = host.trim_matches(['[', ']']).parse::<std::net::IpAddr>() {
        return public_ip(ip);
    }
    let lower = host.to_ascii_lowercase();
    !(lower == "localhost"
        || lower.ends_with(".localhost")
        || lower.ends_with(".local")
        || lower.ends_with(".internal")
        || lower == "metadata.google.internal")
}

pub(crate) fn public_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v) => {
            let o = v.octets();
            !(v.is_private()
                || v.is_loopback()
                || v.is_link_local()
                || v.is_multicast()
                || v.is_broadcast()
                || v.is_unspecified()
                || v.is_documentation()
                || o[0] == 0
                || o[0] >= 240
                || o[0] == 100 && (64..=127).contains(&o[1])
                || o[0] == 198 && (18..=19).contains(&o[1]))
        }
        std::net::IpAddr::V6(v) => {
            if let Some(v4) = v.to_ipv4_mapped() {
                return public_ip(v4.into());
            }
            let s = v.segments();
            !v.is_loopback()
                && !v.is_unspecified()
                && !v.is_multicast()
                && s[0] & 0xe000 == 0x2000
                && !(s[0] == 0x2001 && s[1] == 0xdb8)
        }
    }
}

pub(crate) fn validate_config(kind: &str, config: &Value) -> Result<(), FinanceError> {
    let allowed = config_fields(kind);
    if let Some(obj) = config.as_object() {
        for key in obj.keys() {
            if !allowed.contains(&key.as_str()) {
                return Err(FinanceError::Invalid(format!(
                    "渠道配置字段 {key} 不在白名单"
                )));
            }
        }
    } else {
        return Err(FinanceError::Invalid("渠道配置必须为对象".into()));
    }
    for key in allowed {
        if config.get(key).is_some_and(|v| !v.is_string()) {
            return Err(FinanceError::Invalid(format!(
                "渠道配置字段 {key} 必须为字符串"
            )));
        }
        let value = field_text(config, key);
        match (*key, kind) {
            ("server_url", "bark") if !value.is_empty() => {
                if !host_allowed(&value) {
                    return Err(FinanceError::Invalid(
                        "Bark 自定义域必须为公网 https 地址".into(),
                    ));
                }
            }
            ("phones", "dingtalk") => {
                if !value.is_empty()
                    && value.split(',').any(|p| {
                        let p = p.trim();
                        p.is_empty() || !p.chars().all(|c| c.is_ascii_digit())
                    })
                {
                    return Err(FinanceError::Invalid("钉钉手机号必须为逗号分隔数字".into()));
                }
            }
            _ => {}
        }
        if value.chars().count() > 512 {
            return Err(FinanceError::Invalid(format!("渠道配置字段 {key} 过长")));
        }
    }
    Ok(())
}

pub fn list(store: &Store) -> Result<Value, FinanceError> {
    store.with_read(|c| {
        let mut q = c.prepare(
            "SELECT id,kind,name,config_json,secret_refs,enabled,is_default,archived,last_test_at,last_test_ok,last_error,version,created_at,updated_at \
             FROM notify_channels ORDER BY is_default DESC, id ASC",
        )?;
        let rows = q
            .query_map([], |r| {
                let kind = r.get::<_, String>(1)?;
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "kind": kind,
                    "requiredSecretFields": required_secret_fields(&kind),
                    "name": r.get::<_, String>(2)?,
                    "config": serde_json::from_str::<Value>(&r.get::<_, String>(3)?).unwrap_or(json!({})),
                    "secrets": serde_json::from_str::<Value>(&r.get::<_, String>(4)?).unwrap_or(json!({})),
                    "enabled": r.get::<_, i64>(5)? == 1,
                    "isDefault": r.get::<_, i64>(6)? == 1,
                    "archived": r.get::<_, i64>(7)? == 1,
                    "lastTestAt": r.get::<_, Option<String>>(8)?,
                    "lastTestOk": r.get::<_, Option<i64>>(9)?.map(|v| v == 1),
                    "lastError": r.get::<_, String>(10)?,
                    "version": r.get::<_, i64>(11)?,
                    "createdAt": r.get::<_, String>(12)?,
                    "updatedAt": r.get::<_, String>(13)?,
                }))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(json!({"channels": rows}))
    })
}

/// 创建／更新渠道元数据。Secret 不经此入口；config 只收白名单字段。
/// 更新须 expectedVersion；新建同 kind 首个启用渠道自动成为默认。
pub fn save(store: &Store, value: &Value) -> Result<Value, FinanceError> {
    if !value.as_object().is_some_and(|o| {
        o.keys().all(|k| {
            [
                "id",
                "expectedVersion",
                "requestId",
                "kind",
                "name",
                "config",
            ]
            .contains(&k.as_str())
        })
    }) {
        return Err(FinanceError::Invalid("渠道配置字段无效".into()));
    }
    let kind = value["kind"]
        .as_str()
        .ok_or_else(|| FinanceError::Invalid("缺少渠道类型".into()))?;
    validate_kind(kind)?;
    let name = value["name"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.chars().count() <= 64)
        .ok_or_else(|| FinanceError::Invalid("渠道名称须为 1~64 字符".into()))?;
    let config = value.get("config").cloned().unwrap_or(json!({}));
    validate_config(kind, &config)?;
    let now = crate::finance::now_secs();
    crate::config_receipts::write(store, "channel-save", value, |c| {
        if let Some(id) = value["id"].as_i64() {
            let current: Option<(String, String, i64)> = c
                .query_row(
                    "SELECT kind,config_json,version FROM notify_channels WHERE id=?1 AND archived=0",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            let Some((current_kind, current_config, version)) = current else {
                return Err(FinanceError::Invalid("渠道不存在或已归档".into()));
            };
            if current_kind != kind {
                return Err(FinanceError::Invalid("渠道类型不可修改，请新建".into()));
            }
            if version != value["expectedVersion"].as_i64().unwrap_or(-1) {
                return Err(FinanceError::Conflict(
                    "渠道已被其他修改更新，请刷新".into(),
                ));
            }
            let mut merged = serde_json::from_str::<Value>(&current_config).unwrap_or(json!({}));
            if let (Some(obj), Some(new)) = (merged.as_object_mut(), config.as_object()) {
                for (k, v) in new {
                    obj.insert(k.clone(), v.clone());
                }
            }
            c.execute(
                "UPDATE notify_channels SET name=?2,config_json=?3,version=version+1,updated_at=?4 WHERE id=?1",
                params![id, name, merged.to_string(), now],
            )?;
            Ok(json!({"id": id, "version": version + 1}))
        } else {
            let count: i64 =
                c.query_row("SELECT COUNT(*) FROM notify_channels", [], |r| r.get(0))?;
            if count >= 100 {
                return Err(FinanceError::Invalid(
                    "渠道最多 100 个，请先归档整理".into(),
                ));
            }
            c.execute(
                "INSERT INTO notify_channels(kind,name,config_json,secret_refs,created_at,updated_at) VALUES(?1,?2,?3,'[]',?4,?4)",
                params![kind, name, config.to_string(), now],
            )?;
            let id = c.last_insert_rowid();
            Ok(json!({"id": id, "version": 1}))
        }
    })
}

/// 仅同步已由可信 Extension 表单保存的 Keychain 状态；不接受明文。
pub fn set_secret(
    store: &Store,
    secrets: Option<&std::sync::Arc<app_runtime_core::secrets::SecretStore>>,
    value: &Value,
) -> Result<Value, FinanceError> {
    if !value.as_object().is_some_and(|v| {
        v.keys()
            .all(|k| ["id", "field", "expectedVersion"].contains(&k.as_str()))
    }) {
        return Err(FinanceError::Invalid(
            "Secret 明文只能通过可信配置表单录入".into(),
        ));
    }
    let id = value["id"]
        .as_i64()
        .filter(|id| *id > 0)
        .ok_or_else(|| FinanceError::Invalid("缺少渠道编号".into()))?;
    let field = value["field"]
        .as_str()
        .ok_or_else(|| FinanceError::Invalid("缺少凭据字段".into()))?;
    let kind: String = store.with_read(|c| {
        c.query_row(
            "SELECT kind FROM notify_channels WHERE id=?1 AND archived=0",
            [id],
            |r| r.get(0),
        )
    })?;
    if !secret_fields(&kind).contains(&field) {
        return Err(FinanceError::Invalid("凭据字段无效".into()));
    }
    let secrets = secrets.ok_or_else(|| FinanceError::Invalid("Secret 能力不可用".into()))?;
    let account = format!("channel:{id}:{field}");
    let status = secrets
        .status(&account)
        .map_err(|_| FinanceError::Invalid("Keychain 不可用，请解锁后重试".into()))?;
    store.with_write(|c| {
        let (refs, version): (String, i64) = c.query_row(
            "SELECT secret_refs,version FROM notify_channels WHERE id=?1 AND archived=0",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if value["expectedVersion"].as_i64() != Some(version) {
            return Err(FinanceError::Conflict("渠道已变化，请刷新后重试".into()));
        }
        let mut refs =
            serde_json::from_str::<serde_json::Map<String, Value>>(&refs).unwrap_or_default();
        refs.insert(
            field.into(),
            json!({"account":account,"configured":status.exists}),
        );
        c.execute(
            "UPDATE notify_channels SET secret_refs=?2,version=version+1,updated_at=?3 WHERE id=?1",
            params![
                id,
                Value::Object(refs).to_string(),
                crate::finance::now_secs()
            ],
        )?;
        Ok(json!({"ok":true,"configured":status.exists,"version":version+1}))
    })
}

/// 启停／默认／归档状态。默认唯一；归档不删除 Keychain 凭据（方案 10 前置 4）。
pub fn set_state(store: &Store, value: &Value) -> Result<Value, FinanceError> {
    if !value.as_object().is_some_and(|o| {
        o.keys().all(|k| {
            [
                "id",
                "expectedVersion",
                "requestId",
                "enabled",
                "isDefault",
                "archived",
                "restore",
            ]
            .contains(&k.as_str())
        })
    }) || ["enabled", "isDefault", "archived", "restore"]
        .iter()
        .any(|k| value.get(*k).is_some_and(|v| !v.is_boolean()))
    {
        return Err(FinanceError::Invalid("渠道状态字段无效".into()));
    }
    let id = value["id"]
        .as_i64()
        .filter(|v| *v > 0)
        .ok_or_else(|| FinanceError::Invalid("缺少渠道编号".into()))?;
    crate::config_receipts::write(store, "channel-state", value, |c| {
        let (version, enabled, archived, default): (i64, bool, bool, bool) = c.query_row(
            "SELECT version,enabled,archived,is_default FROM notify_channels WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        if value["expectedVersion"].as_i64() != Some(version) {
            return Err(FinanceError::Conflict("渠道已变化，请刷新".into()));
        }
        let archived = if value["restore"] == json!(true) {
            false
        } else {
            value["archived"].as_bool().unwrap_or(archived)
        };
        let enabled = value["enabled"].as_bool().unwrap_or(enabled);
        let requested_default = value["isDefault"].as_bool();
        if requested_default == Some(true) && (archived || !enabled) {
            return Err(FinanceError::Invalid("默认渠道必须启用且未归档".into()));
        }
        let default = requested_default.unwrap_or(default) && enabled && !archived;
        if default {
            c.execute("UPDATE notify_channels SET is_default=0,version=version+1,updated_at=?2 WHERE is_default=1 AND id<>?1", params![id,crate::finance::now_secs()])?;
        }
        c.execute("UPDATE notify_channels SET enabled=?2,archived=?3,is_default=?4,version=version+1,updated_at=?5 WHERE id=?1", params![id,enabled,archived,default,crate::finance::now_secs()])?;
        Ok(json!({"ok":true,"version":version+1}))
    })
}

pub fn delete(store: &Store, value: &Value) -> Result<Value, FinanceError> {
    let id = value["id"]
        .as_i64()
        .ok_or_else(|| FinanceError::Invalid("缺少渠道编号".into()))?;
    store.with_write(|c| {
        let version: Option<i64> = c
            .query_row(
                "SELECT version FROM notify_channels WHERE id=?1",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        let Some(version) = version else {
            return Err(FinanceError::Invalid("渠道不存在".into()));
        };
        if version != value["expectedVersion"].as_i64().unwrap_or(-1) {
            return Err(FinanceError::Conflict(
                "渠道已被其他修改更新，请刷新".into(),
            ));
        }
        c.execute("DELETE FROM notify_deliveries WHERE channel_id=?1", [id])
            .map_err(|e| FinanceError::Invalid(format!("投递记录清理失败：{e}")))?;
        c.execute("DELETE FROM notify_channels WHERE id=?1", [id])
            .map_err(|e| FinanceError::Invalid(format!("渠道删除失败：{e}")))?;
        Ok(json!({"ok": true, "secretsOrphaned": value["clearSecrets"].as_bool().unwrap_or(false)}))
    })
}

// ---------- 策略（S03/S04） ----------

pub struct SendPolicy {
    pub silence_start: Option<String>,
    pub silence_end: Option<String>,
    pub retry_max: u32,
    pub backoff_base: u32,
    pub dedup_minutes: u32,
}

pub fn load_policy(store: &Store) -> SendPolicy {
    let read = |key: &str| -> Option<String> {
        store
            .with_read(|c| {
                c.query_row("SELECT value FROM app_meta WHERE key=?1", [key], |r| {
                    r.get::<_, String>(0)
                })
                .optional()
            })
            .ok()
            .flatten()
    };
    SendPolicy {
        silence_start: read("notify_silence_start").filter(|s| !s.is_empty()),
        silence_end: read("notify_silence_end").filter(|s| !s.is_empty()),
        retry_max: read("notify_retry_max")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
            .min(3),
        backoff_base: read("notify_backoff_base")
            .and_then(|v| v.parse().ok())
            .unwrap_or(1)
            .clamp(1, 30),
        dedup_minutes: read("notify_dedup_window")
            .and_then(|v| v.parse().ok())
            .unwrap_or(60)
            .min(1440),
    }
}

pub fn save_policy(store: &Store, value: &Value) -> Result<Value, FinanceError> {
    if !value.as_object().is_some_and(|o| {
        o.keys().all(|k| {
            [
                "expectedVersion",
                "requestId",
                "silenceStart",
                "silenceEnd",
                "retryMax",
                "backoffBase",
                "dedupMinutes",
            ]
            .contains(&k.as_str())
        })
    }) {
        return Err(FinanceError::Invalid("通知策略字段无效".into()));
    }
    let hhmm = |v: &Value| -> Result<Option<String>, FinanceError> {
        let s = v
            .as_str()
            .ok_or_else(|| FinanceError::Invalid("静默时间类型无效".into()))?;
        if s.is_empty() {
            return Ok(None);
        }
        let valid = s.len() == 5
            && s.as_bytes()[2] == b':'
            && s.chars().enumerate().all(
                |(i, c)| {
                    if i == 2 {
                        c == ':'
                    } else {
                        c.is_ascii_digit()
                    }
                },
            )
            && s[..2].parse::<u8>().map(|h| h < 24).unwrap_or(false)
            && s[3..].parse::<u8>().map(|m| m < 60).unwrap_or(false);
        if !valid {
            return Err(FinanceError::Invalid(
                "静默时间须为 HH:MM（本地时区）".into(),
            ));
        }
        Ok(Some(s.to_string()))
    };
    let start = hhmm(value.get("silenceStart").unwrap_or(&json!("")))?;
    let end = hhmm(value.get("silenceEnd").unwrap_or(&json!("")))?;
    if start.is_some() != end.is_some() {
        return Err(FinanceError::Invalid("静默起止须同时设置或同时关闭".into()));
    }
    let bounded = |key: &str, min: i64, max: i64| -> Result<i64, FinanceError> {
        let v = value
            .get(key)
            .and_then(Value::as_i64)
            .ok_or_else(|| FinanceError::Invalid(format!("缺少 {key}")))?;
        if !(min..=max).contains(&v) {
            return Err(FinanceError::Invalid(format!("{key} 须在 {min}~{max}")));
        }
        Ok(v)
    };
    let retry = bounded("retryMax", 0, 3)?;
    let backoff = bounded("backoffBase", 1, 30)?;
    let dedup = bounded("dedupMinutes", 1, 1440)?;
    crate::config_receipts::write(store, "notify-policy", value, |c| {
        let version: i64 = c
            .query_row(
                "SELECT value FROM app_meta WHERE key='notify_policy_version'",
                [],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .unwrap_or_else(|| "0".into())
            .parse()
            .map_err(|_| FinanceError::Invalid("版本损坏".into()))?;
        if value["expectedVersion"].as_i64().unwrap_or(0) != version {
            return Err(FinanceError::Conflict("通知策略已变化，请刷新".into()));
        }
        let next = version
            .checked_add(1)
            .ok_or_else(|| FinanceError::Invalid("版本超界".into()))?;
        let rows = [
            ("notify_policy_version", next.to_string()),
            ("notify_silence_start", start.unwrap_or_default()),
            ("notify_silence_end", end.unwrap_or_default()),
            ("notify_retry_max", retry.to_string()),
            ("notify_backoff_base", backoff.to_string()),
            ("notify_dedup_window", dedup.to_string()),
        ];
        for (key, v) in rows {
            c.execute(
                "INSERT INTO app_meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                params![key, v],
            )
            .map_err(|e| FinanceError::Invalid(format!("通知设置保存失败：{e}")))?;
        }
        Ok(json!({"ok": true,"version":next}))
    })
}

/// 跨午夜静默（S03）：start<=end 为同日区间，否则跨午夜。
pub fn in_silence(policy: &SendPolicy, hhmm_now: &str) -> bool {
    let (Some(start), Some(end)) = (&policy.silence_start, &policy.silence_end) else {
        return false;
    };
    if start <= end {
        hhmm_now >= start.as_str() && hhmm_now < end.as_str()
    } else {
        hhmm_now >= start.as_str() || hhmm_now < end.as_str()
    }
}

#[cfg(test)]
#[path = "channels_tests.rs"]
mod tests;
