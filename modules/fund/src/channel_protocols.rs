//! 渠道官方协议适配层（R5）：出站请求结构、SSRF 守卫、九渠道请求构造（逐字段
//! fixture 可对照）、上游业务码解释、钉钉 HMAC-SHA256 加签与发送引擎。与
//! CRUD／策略域（channels.rs）分离以守规模门禁；发送落投递事实经 channels::record。
use crate::channels::{in_silence, SendPolicy};
use crate::finance::FinanceError;
use crate::storage::Store;
use rusqlite::params;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub(crate) fn field_text(config: &Value, key: &str) -> String {
    config
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

/// 适配后的出站请求；执行器收到后才触网。
pub struct OutboundRequest {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
    /// 上游「业务成功」判据说明；HTTP 200 不等于业务成功（方案 10）。
    pub expect: &'static str,
}

/// dingtalk 加签（官方协议：timestamp+"\n"+secret 的 HMAC-SHA256，Base64）。
fn hmac_sha256_base64(key: &str, message: &str) -> String {
    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    let key_bytes = key.as_bytes();
    let mut block = [0u8; 64];
    block[..key_bytes.len().min(64)].copy_from_slice(&key_bytes[..key_bytes.len().min(64)]);
    if key_bytes.len() > 64 {
        let hashed = Sha256::digest(key_bytes);
        block.fill(0);
        block[..32].copy_from_slice(&hashed);
    }
    for i in 0..64 {
        ipad[i] ^= block[i];
        opad[i] ^= block[i];
    }
    let mut inner = Sha256::new();
    inner.update(ipad);
    inner.update(message.as_bytes());
    let inner = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner);
    base64_standard(&outer.finalize())
}

/// RFC 4648 标准 Base64（带 padding）；避免仅为钉钉加签引入新依赖。
fn base64_standard(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

fn form_encode(values: &[(&str, &str)]) -> String {
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    serializer.extend_pairs(values.iter().copied());
    serializer.finish()
}

/// 按官方协议构造各渠道出站请求（fixture 验证逐字段对照）。
/// 秘密值由调用方从 Keychain reveal 后注入；本函数不读库。
pub fn build_request(
    kind: &str,
    config: &Value,
    secret_values: &Value,
    title: &str,
    body: &str,
) -> Result<OutboundRequest, FinanceError> {
    let text = format!("{title}\n{body}");
    let secret = |f: &str| -> Result<String, FinanceError> {
        secret_values
            .get(f)
            .and_then(Value::as_str)
            .map(str::to_string)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| FinanceError::Invalid(format!("渠道 Secret {f} 未配置")))
    };
    match kind {
        "telegram" => Ok(OutboundRequest {
            url: format!("https://api.telegram.org/bot{}/sendMessage", secret("bot_token")?),
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: json!({"chat_id": field_text(config, "chat_id"), "text": text}).to_string(),
            expect: "response.ok==true",
        }),
        "bark" => {
            let server = field_text(config, "server_url");
            let base = if server.is_empty() { "https://api.day.app".to_string() } else { server.trim_end_matches('/').to_string() };
            Ok(OutboundRequest {
                url: format!("{base}/{}", secret("device_key")?),
                headers: vec![("Content-Type".into(), "application/json".into())],
                body: json!({"title": title, "body": body}).to_string(),
                expect: "response.code==200",
            })
        }
        "dingtalk" => {
            let token = secret("token")?;
            let mut query = form_encode(&[("access_token", &token)]);
            if let Some(sign_secret) = secret_values["secret"].as_str().filter(|s| !s.is_empty()) {
                let ts: i64 = crate::finance::now_secs().parse().unwrap_or(0) * 1000;
                let sign = hmac_sha256_base64(sign_secret, &format!("{ts}\n{sign_secret}"));
                query.push('&');
                query.push_str(&form_encode(&[("timestamp", &ts.to_string()), ("sign", &sign)]));
            }
            let keyword = field_text(config, "keyword");
            let content = if keyword.is_empty() || text.contains(&keyword) { text.clone() } else { format!("{text} {keyword}") };
            Ok(OutboundRequest {
                url: format!("https://oapi.dingtalk.com/robot/send?{query}"),
                headers: vec![("Content-Type".into(), "application/json".into())],
                body: json!({"msgtype":"text","text":{"content":content},"at":{"atMobiles":field_text(config,"phones").split(',').map(str::trim).filter(|s|!s.is_empty()).collect::<Vec<_>>(),"isAtAll":false}}).to_string(),
                expect: "response.errcode==0",
            })
        }
        "wecom" => Ok(OutboundRequest {
            url: format!("https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key={}", secret("webhook_key")?),
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: json!({"msgtype":"text","text":{"content":text}}).to_string(),
            expect: "response.errcode==0",
        }),
        "feishu" => Ok(OutboundRequest {
            url: format!("https://open.feishu.cn/open-apis/bot/v2/hook/{}", secret("webhook_token")?),
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: json!({"msg_type":"text","content":{"text":text}}).to_string(),
            expect: "response.code==0",
        }),
        "serverchan" => {
            let sendkey = secret("sendkey")?;
            Ok(OutboundRequest {
                url: format!("https://sctapi.ftqq.com/{sendkey}.send"),
                headers: vec![("Content-Type".into(), "application/x-www-form-urlencoded".into())],
                body: form_encode(&[("title",title),("desp",body)]),
                expect: "response.code==0",
            })
        }
        "pushplus" => Ok(OutboundRequest {
            url: "https://www.pushplus.plus/send".into(),
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: json!({"token": secret("token")?, "title": title, "content": body, "topic": field_text(config, "topic")}).to_string(),
            expect: "response.code==200",
        }),
        "discord" => Ok(OutboundRequest {
            url: format!(
                "https://discord.com/api/v10/webhooks/{}/{}",
                field_text(config, "webhook_id"),
                secret("webhook_token")?
            ),
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: json!({"content": text}).to_string(),
            expect: "http==200",
        }),
        "pushover" => Ok(OutboundRequest {
            url: "https://api.pushover.net/1/messages.json".into(),
            headers: vec![("Content-Type".into(), "application/x-www-form-urlencoded".into())],
            body: form_encode(&[("token",&secret("app_token")?),("user",&secret("user_key")?),("title",title),("message",body)]),
            expect: "response.status==1",
        }),
        other => Err(FinanceError::Invalid(format!("未知的通知渠道类型 {other}"))),
    }
}

/// 执行结果：HTTP 状态＋响应体，或传输层错误（连接／超时）。
pub type ExecuteResult = Result<(u16, String), String>;

/// 上游「业务成功」判据（方案 10：HTTP 200 不等于业务成功）；按官方协议逐渠道核对。
pub(crate) fn business_ok(kind: &str, http: u16, body: &str) -> Result<(), String> {
    if !(200..=299).contains(&http) {
        return Err(format!("upstream_http_{http}"));
    }
    let parsed: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    match kind {
        "telegram" => {
            if parsed["ok"] == json!(true) {
                Ok(())
            } else {
                Err("upstream_business_rejected".into())
            }
        }
        "bark" => {
            if parsed["code"].as_i64() == Some(200) {
                Ok(())
            } else {
                Err("upstream_business_rejected".into())
            }
        }
        "dingtalk" | "wecom" => {
            if parsed["errcode"].as_i64() == Some(0) {
                Ok(())
            } else {
                Err("upstream_business_rejected".into())
            }
        }
        "feishu" => {
            let code = parsed["code"]
                .as_i64()
                .or_else(|| parsed["StatusCode"].as_i64());
            if code == Some(0) {
                Ok(())
            } else {
                Err("upstream_business_rejected".into())
            }
        }
        "serverchan" => {
            if parsed["code"].as_i64() == Some(0) {
                Ok(())
            } else {
                Err("upstream_business_rejected".into())
            }
        }
        "pushplus" => {
            if parsed["code"].as_i64() == Some(200) {
                Ok(())
            } else {
                Err("upstream_business_rejected".into())
            }
        }
        // 官方 webhook execute 返回 204 No Content（也接受 200）。
        "discord" => {
            if (200..=299).contains(&http) {
                Ok(())
            } else {
                Err("upstream_business_rejected".into())
            }
        }
        "pushover" => {
            if parsed["status"].as_i64() == Some(1) {
                Ok(())
            } else {
                Err("upstream_business_rejected".into())
            }
        }
        other => Err(format!("未知的通知渠道类型 {other}")),
    }
}

/// 经执行器发送并落投递事实。静默跳过（测试可显式绕过）、窗口内去重；
/// 重试有界（0~3）带基数退避；取消在尝试之间生效。超时／传输失败语义：
/// 不承诺失败消息一定未送达，重试可能造成重复送达（方案 10）。
pub fn send(
    store: &Store,
    kind: &str,
    request: &OutboundRequest,
    channel_id: i64,
    subject_ref: &str,
    policy: &SendPolicy,
    bypass_silence: bool,
    is_test: bool,
    cancelled: impl Fn() -> bool,
    execute: impl Fn(&OutboundRequest) -> ExecuteResult,
) -> Result<Value, FinanceError> {
    let secs: i64 = crate::finance::now_secs().parse().unwrap_or(0);
    let now_hhmm: String = store.with_read(|c| {
        c.query_row("SELECT strftime('%H:%M','now','localtime')", [], |r| {
            r.get(0)
        })
    })?;
    if !(is_test && bypass_silence) && in_silence(policy, &now_hhmm) {
        record(
            store,
            channel_id,
            "skipped_silent",
            subject_ref,
            "",
            "本地静默时段内不发送",
            false,
        )?;
        return Ok(json!({"status": "skipped_silent"}));
    }
    if policy.dedup_minutes > 0 && !subject_ref.is_empty() {
        let recent: i64 = store.with_read(|c| {
            c.query_row(
                "SELECT COUNT(*) FROM notify_deliveries WHERE channel_id=?1 AND subject_ref=?2 AND status='sent' AND created_at>=?3",
                params![channel_id, subject_ref, (secs - policy.dedup_minutes as i64 * 60).to_string()],
                |r| r.get(0),
            )
        })?;
        if recent > 0 {
            record(
                store,
                channel_id,
                "skipped_dedup",
                subject_ref,
                "",
                "去重窗口内已发送",
                false,
            )?;
            return Ok(json!({"status": "skipped_dedup"}));
        }
    }
    let mut attempts = 0usize;
    let total = policy.retry_max as usize + 1;
    loop {
        if cancelled() {
            record(
                store,
                channel_id,
                "cancelled",
                subject_ref,
                "",
                "会话关闭取消发送",
                is_test,
            )?;
            return Ok(json!({"status": "cancelled"}));
        }
        attempts += 1;
        let (status, code, detail) = match execute(request) {
            Ok((http, body)) => match business_ok(kind, http, &body) {
                Ok(()) => ("sent", String::new(), format!("http={http}")),
                Err(reason) => ("failed", "upstream_business_error".to_string(), reason),
            },
            Err(_) => (
                "failed",
                "network_error".to_string(),
                "network_failed_or_delivery_unknown".to_string(),
            ),
        };
        let done = status == "sent" || attempts >= total;
        if !done {
            let delay = std::time::Duration::from_secs(
                (policy.backoff_base as u64).saturating_mul(1u64 << (attempts - 1).min(3)),
            );
            let until = std::time::Instant::now() + delay;
            while std::time::Instant::now() < until && !cancelled() {
                std::thread::sleep(
                    until
                        .saturating_duration_since(std::time::Instant::now())
                        .min(std::time::Duration::from_millis(50)),
                );
            }
            continue;
        }
        record(
            store,
            channel_id,
            status,
            subject_ref,
            &code,
            &detail,
            is_test,
        )?;
        return Ok(json!({"status": status, "attempts": attempts, "errorCode": code}));
    }
}

fn record(
    store: &Store,
    channel_id: i64,
    status: &str,
    subject_ref: &str,
    error_code: &str,
    detail: &str,
    update_channel: bool,
) -> Result<(), FinanceError> {
    store.with_write(|c| {
        c.execute(
            "INSERT INTO notify_deliveries(channel_id,kind,subject_ref,status,error_code,detail,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                channel_id,
                if update_channel { "test" } else { "event" },
                subject_ref,
                status,
                error_code,
                detail,
                crate::finance::now_secs().to_string()
            ],
        )
        .map_err(|e| FinanceError::Invalid(format!("投递记录失败：{e}")))?;
        if update_channel && (status == "sent" || status == "failed") {
            c.execute(
                "UPDATE notify_channels SET last_test_at=?2,last_test_ok=?3,last_error=?4,version=version+1,updated_at=?2 WHERE id=?1",
                params![channel_id, crate::finance::now_secs(), (status == "sent") as i64, error_code],
            )
            .map_err(|e| FinanceError::Invalid(format!("渠道测试状态更新失败：{e}")))?;
        }
        Ok(())
    })
}

pub fn deliveries(
    store: &Store,
    channel_id: Option<i64>,
    page: i64,
) -> Result<Value, FinanceError> {
    if !(1..=1_000_000).contains(&page) {
        return Err(FinanceError::Invalid("分页无效".into()));
    }
    store.with_read(|c| {
        let mut q = c.prepare(
            "SELECT d.id,d.channel_id,c.name,d.status,d.error_code,d.detail,d.created_at FROM notify_deliveries d JOIN notify_channels c ON c.id=d.channel_id \
             WHERE (?1 IS NULL OR d.channel_id=?1) ORDER BY d.id DESC LIMIT 20 OFFSET ?2",
        )?;
        let rows = q
            .query_map(params![channel_id, page.saturating_sub(1) * 20], |r| {
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "channelId": r.get::<_, i64>(1)?,
                    "channelName": r.get::<_, String>(2)?,
                    "status": r.get::<_, String>(3)?,
                    "errorCode": r.get::<_, String>(4)?,
                    "detail": r.get::<_, String>(5)?,
                    "createdAt": r.get::<_, String>(6)?,
                }))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(json!({"deliveries": rows, "page": page}))
    })
}
