//! Bridge — Core orchestration for WeChat ClawBot
//!
//! State machine: uninstalled → installed_off → gateway_online → selecting_agent
//!                → qr_generated → qr_showing → connected → (expired → reconnect)
//!
//! Reference: fanbox/electron/wechat/bridge.js (467L)

use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::ilink;

pub const WX_PERSONA_DEFAULT: &str = "你正通过微信被花叔遥控，回复会显示在手机微信里。请：用中文、简洁直接、适合手机阅读；先给结论，细节按需再展开；除非花叔明确要求，别贴大段代码或长列表；做了改动用一两句话说清改了什么。";

#[derive(Debug, Clone, PartialEq)]
pub enum ConnState {
    Uninstalled,
    InstalledOff,
    GatewayOnline,
    SelectingAgent,
    QrGenerated,
    QrShowing,
    Connected,
    Expired,
}

impl ConnState {
    pub fn as_str(&self) -> &'static str {
        match self {
            ConnState::Uninstalled => "uninstalled",
            ConnState::InstalledOff => "installed_off",
            ConnState::GatewayOnline => "gateway_online",
            ConnState::SelectingAgent => "selecting_agent",
            ConnState::QrGenerated => "qr_generated",
            ConnState::QrShowing => "qr_showing",
            ConnState::Connected => "connected",
            ConnState::Expired => "expired",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Conversation {
    pub id: String,
    pub label: String,
    pub messages: Vec<Message>,
    pub context_token: String,
}

#[derive(Debug, Clone)]
pub struct Message {
    pub role: String,    // "user" | "assistant" | "system"
    pub text: String,
    pub time: u64,
}

pub struct Bridge {
    pub target: String,                // "claude" | "codex"
    pub persona: String,
    pub cwd: String,
    pub conversations: HashMap<String, Conversation>,
    pub active_cid: String,
    pub account: Option<ilink::Account>,
    pub state: ConnState,
    pub poll_abort: Arc<Mutex<bool>>,
}

impl Bridge {
    pub fn new() -> Self {
        Self {
            target: "claude".to_string(),
            persona: WX_PERSONA_DEFAULT.to_string(),
            cwd: dirs::home_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| "/".to_string()),
            conversations: HashMap::new(),
            active_cid: "desktop".to_string(),
            account: None,
            state: ConnState::Uninstalled,
            poll_abort: Arc::new(Mutex::new(false)),
        }
    }

    pub fn is_connected(&self) -> bool {
        self.account.is_some() && self.state == ConnState::Connected
    }

    pub fn env(&self) -> Value {
        json!({
            "target": self.target,
            "cwd": self.cwd,
            "persona": self.persona,
            "state": self.state.as_str(),
            "connected": self.is_connected()
        })
    }

    pub fn set_target(&mut self, target: &str) {
        self.target = target.to_string();
    }

    pub fn set_cwd(&mut self, cwd: &str) {
        self.cwd = cwd.to_string();
    }

    pub fn set_persona(&mut self, persona: &str) {
        self.persona = persona.to_string();
    }

    /// Start login flow: fetch QR code → poll status → save account
    pub fn login(&mut self) -> Result<Value, String> {
        let qr = ilink::fetch_qrcode()?;
        self.state = ConnState::QrShowing;

        // For now, return the QR code; the frontend will display it
        // and call `poll_login` to check status.
        Ok(json!({
            "qrcode": qr.qrcode,
            "qrcode_img_content": qr.qrcode_img_content,
            "state": self.state.as_str()
        }))
    }

    /// Poll login status (called repeatedly from frontend)
    pub fn poll_login(&mut self, qrcode: &str, verify_code: Option<&str>) -> Result<Value, String> {
        let status = ilink::poll_qr_status("", qrcode, verify_code)?;
        let s = status["status"].as_str().unwrap_or("wait");

        match s {
            "expired" => {
                self.state = ConnState::Expired;
                Ok(json!({ "state": "expired", "error": "二维码过期，请重试" }))
            }
            "canceled" => {
                self.state = ConnState::InstalledOff;
                Ok(json!({ "state": "canceled" }))
            }
            "confirmed" => {
                // Extract account info from response
                if let (Some(token), Some(base_url), Some(account_id), Some(user_id)) = (
                    status["token"].as_str(),
                    status["base_url"].as_str(),
                    status["account_id"].as_str(),
                    status["user_id"].as_str(),
                ) {
                    self.account = Some(ilink::Account {
                        token: token.to_string(),
                        base_url: base_url.to_string(),
                        account_id: account_id.to_string(),
                        user_id: user_id.to_string(),
                    });
                    self.state = ConnState::Connected;
                    return Ok(json!({ "state": "connected" }));
                }
                Ok(json!({ "state": "confirmed", "raw": status }))
            }
            _ => Ok(json!({ "state": "wait" })),
        }
    }

    /// Disconnect and clear account
    pub fn disconnect(&mut self) -> Value {
        if let Ok(mut abort) = self.poll_abort.lock() {
            *abort = true;
        }
        self.account = None;
        self.state = ConnState::InstalledOff;
        json!({ "ok": true })
    }

    /// Send a message to a user (or default desktop conversation)
    pub fn send(&mut self, text: &str) -> Result<Value, String> {
        if !self.is_connected() {
            return Err("not connected".to_string());
        }
        let account = self.account.as_ref().unwrap();
        let cid = self.active_cid.clone();

        // Get or create conversation
        let conv = self
            .conversations
            .entry(cid.clone())
            .or_insert_with(|| Conversation {
                id: cid.clone(),
                label: cid.clone(),
                messages: Vec::new(),
                context_token: String::new(),
            });

        // Add user message
        conv.messages.push(Message {
            role: "user".to_string(),
            text: text.to_string(),
            time: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        });

        // Send via ilink
        let _ = ilink::send_text(account, &account.user_id, text, &conv.context_token);

        Ok(json!({ "ok": true, "cid": cid }))
    }

    /// Check connection status (lightweight ping)
    pub fn check(&mut self) -> Value {
        if let Some(account) = &self.account {
            match ilink::ping(account) {
                Ok(r) => {
                    let ok = r["ok"].as_bool().unwrap_or(false);
                    if ok {
                        self.state = ConnState::Connected;
                    } else {
                        self.state = ConnState::Expired;
                    }
                    json!({ "ok": true, "state": self.state.as_str() })
                }
                Err(_) => json!({ "ok": false, "state": "unreachable" }),
            }
        } else {
            json!({ "ok": true, "state": "uninstalled" })
        }
    }
}
