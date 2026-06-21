//! iLink client — Tencent official iLink HTTP JSON protocol (ilinkai.weixin.qq.com)
//!
//! Reference: fanbox/electron/wechat/ilink.js (213L)

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const LOGIN_BASE: &str = "https://ilinkai.weixin.qq.com";
const CDN_BASE: &str = "https://novac2c.cdn.weixin.qq.com/c2c";
const BOT_TYPE: &str = "3";
const CHANNEL_VERSION: &str = "1.0.11";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub token: String,
    pub base_url: String,
    pub account_id: String,
    pub user_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QrCode {
    pub qrcode: String,
    pub qrcode_img_content: String,
}

fn client_version(v: &str) -> String {
    let parts: Vec<&str> = v.split('.').collect();
    let maj = parts.first().and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    let min = parts.get(1).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    let pat = parts.get(2).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    ((maj & 0xff) << 16 | (min & 0xff) << 8 | (pat & 0xff)).to_string()
}

fn common_headers() -> Vec<(&'static str, String)> {
    vec![("iLink-App-ClientVersion", client_version(CHANNEL_VERSION))]
}

fn wechat_uin() -> String {
    use base64::Engine;
    let n: u32 = rand::random();
    base64::engine::general_purpose::STANDARD.encode(n.to_string().as_bytes())
}

fn post_headers(token: Option<&str>) -> Vec<(&'static str, String)> {
    let mut h = common_headers();
    h.push(("Content-Type", "application/json".to_string()));
    h.push(("AuthorizationType", "ilink_bot_token".to_string()));
    h.push(("X-WECHAT-UIN", wechat_uin()));
    if let Some(t) = token {
        h.push(("Authorization", format!("Bearer {}", t)));
    }
    h
}

fn base_info() -> Value {
    json!({ "channel_version": CHANNEL_VERSION, "bot_agent": "FanBox" })
}

fn http_json(
    url: &str,
    method: &str,
    headers: Vec<(&str, String)>,
    body: Option<&Value>,
    timeout_ms: u64,
) -> Result<Value, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_millis(timeout_ms))
        .build()
        .map_err(|e| format!("http client: {e}"))?;

    let method = reqwest::Method::from_bytes(method.as_bytes()).unwrap_or(reqwest::Method::POST);
    let mut req = client.request(method, url);
    for (k, v) in headers {
        req = req.header(k, v);
    }
    if let Some(b) = body {
        req = req.json(b);
    }

    let resp = req.send().map_err(|e| format!("http send: {e}"))?;
    let status = resp.status().as_u16();
    let text = resp.text().map_err(|e| format!("http read: {e}"))?;
    let json: Value = if text.is_empty() {
        json!({})
    } else {
        serde_json::from_str(&text).unwrap_or(json!({ "_raw": text }))
    };
    Ok(json!({ "ok": status < 400, "status": status, "json": json }))
}

fn urlencode(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') {
                c.to_string()
            } else {
                format!("%{:02X}", c as u8)
            }
        })
        .collect()
}

fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    for b in &mut buf {
        *b = rand::random();
    }
    hex::encode(&buf)
}

/// Fetch QR code for login
pub fn fetch_qrcode() -> Result<QrCode, String> {
    let url = format!("{}/ilink/bot/get_bot_qrcode?bot_type={}", LOGIN_BASE, BOT_TYPE);
    let body = json!({ "local_token_list": [] });
    let r = http_json(&url, "POST", post_headers(None), Some(&body), 15000)?;
    if !r["ok"].as_bool().unwrap_or(false) {
        return Err(format!("get_bot_qrcode failed: {}", r["json"]));
    }
    serde_json::from_value(r["json"].clone()).map_err(|e| format!("parse qrcode: {e}"))
}

/// Poll QR scan status (long-poll, ~35s per round)
pub fn poll_qr_status(base_url: &str, qrcode: &str, verify_code: Option<&str>) -> Result<Value, String> {
    let mut url = format!("{}/ilink/bot/get_qrcode_status?qrcode={}", base_url, urlencode(qrcode));
    if let Some(vc) = verify_code {
        url.push_str(&format!("&verify_code={}", urlencode(vc)));
    }
    let r = http_json(&url, "GET", common_headers(), None, 35000)?;
    Ok(r["json"].clone())
}

/// Long-poll for new messages/updates
pub fn get_updates(account: &Account, get_updates_buf: &str, timeout_ms: u64) -> Result<Value, String> {
    let url = format!("{}/ilink/bot/getupdates", account.base_url);
    let body = json!({
        "get_updates_buf": get_updates_buf,
        "base_info": base_info()
    });
    let r = http_json(&url, "POST", post_headers(Some(&account.token)), Some(&body), timeout_ms)?;
    Ok(r["json"].clone())
}

/// Send a text message
pub fn send_text(account: &Account, to_user_id: &str, text: &str, context_token: &str) -> Result<Value, String> {
    let url = format!("{}/ilink/bot/sendmessage", account.base_url);
    let body = json!({
        "msg": {
            "from_user_id": "",
            "to_user_id": to_user_id,
            "client_id": random_hex(8),
            "message_type": 2,
            "message_state": 2,
            "item_list": [{ "type": 1, "text_item": { "text": text } }],
            "context_token": context_token
        },
        "base_info": base_info()
    });
    let r = http_json(&url, "POST", post_headers(Some(&account.token)), Some(&body), 15000)?;
    Ok(r["json"].clone())
}

/// Lightweight ping to check token validity
pub fn ping(account: &Account) -> Result<Value, String> {
    let url = format!("{}/ilink/bot/getconfig", account.base_url);
    let body = json!({ "ilink_user_id": account.user_id, "base_info": base_info() });
    http_json(&url, "POST", post_headers(Some(&account.token)), Some(&body), 8000)
}

/// Extract content from a received message: (text, medias)
pub fn content_from_msg(msg: &Value) -> (String, Vec<Value>) {
    let mut text = String::new();
    let mut medias = Vec::new();
    if let Some(items) = msg["item_list"].as_array() {
        for it in items {
            match it["type"].as_i64().unwrap_or(0) {
                1 => {
                    if let Some(t) = it["text_item"]["text"].as_str() {
                        text = t.to_string();
                    }
                }
                3 => {
                    if let Some(t) = it["voice_item"]["text"].as_str() {
                        text = t.to_string();
                    }
                }
                2 => {
                    if let Some(img) = it.get("image_item") {
                        medias.push(json!({ "kind": "image", "name": img["file_name"].as_str().unwrap_or("图片"), "item": img }));
                    }
                }
                4 => {
                    if let Some(file) = it.get("file_item") {
                        medias.push(json!({ "kind": "file", "name": file["file_name"].as_str().unwrap_or("文件"), "item": file }));
                    }
                }
                _ => {}
            }
        }
    }
    (text, medias)
}
