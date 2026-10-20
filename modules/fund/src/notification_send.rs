//! 通知发送业务核心（mode/T03 + 审计更正，R-M3/R-B12/R-M7）。
//!
//! 职责：完整、已校验的**类型化命令**进，业务结果或**语义错误**出。
//! 传输解析由各适配层完成后经 [`NotifyCommand::from_raw`] 构造命令——
//! 业务事实（subject／previewToken／requestId）全部在命令内，业务编排
//! 不再接收原始 JSON。语义错误 [`NotifyError`] 只携带稳定 code 与消息；
//! HTTP 状态与 wire JSON 形态由适配层映射。执行细节（渠道协议、SSRF
//! 守卫、有界 HTTPS）属于通知域，收口于 [`production_executor`]。
//!
//! Secret 经 [`SecretReveal`] 窄 trait 取用（生产实现为 Runtime
//! SecretStore），测试可注入 Keychain locked／缺凭据假实现；明文不出
//! 本文件请求作用域。

use crate::channels::{self, OutboundRequest};
use crate::finance::FinanceError;
use crate::storage::Store;
use serde_json::{json, Value};

/// 渠道测试入口允许的字段白名单（既有 HTTP 契约，任务路径同用）。
const TEST_FIELDS_ALLOWED: [&str; 5] = ["id", "title", "body", "bypassSilence", "requestId"];

/// 业务发送事实（预览／确认链契约的 7 字段全部类型化）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BusinessSend {
    pub channel_id: i64,
    pub subject: NotifySubject,
    pub language: NotifyLanguage,
    pub preview_token: String,
    pub request_id: String,
}

/// 通知对象引用（与 notification_subjects::preview 的对象契约一一对应）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum NotifySubject {
    Event(i64),
    Report(i64),
    Candidate {
        snapshot_id: String,
        candidate_id: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum NotifyLanguage {
    ZhCn,
    En,
}

impl BusinessSend {
    /// 确定性重建事实校验输入（恰为 validate_send 的 7 字段契约；
    /// 这是固定 wire 契约的构造点，不是任意 JSON 镜像）。
    fn fact_input(&self) -> Value {
        let mut input = json!({
            "id": self.channel_id,
            "subject": match self.subject {
                NotifySubject::Event(_) => "event",
                NotifySubject::Report(_) => "report",
                NotifySubject::Candidate { .. } => "candidate",
            },
            "language": match self.language {
                NotifyLanguage::ZhCn => "zh_CN",
                NotifyLanguage::En => "en",
            },
            "previewToken": self.preview_token,
            "requestId": self.request_id,
        });
        match &self.subject {
            NotifySubject::Event(id) | NotifySubject::Report(id) => {
                input["subjectId"] = json!(id);
            }
            NotifySubject::Candidate {
                snapshot_id,
                candidate_id,
            } => {
                input["snapshotId"] = json!(snapshot_id);
                input["subjectId"] = json!(candidate_id);
            }
        }
        input
    }
}

/// 渠道测试命令。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TestSend {
    pub channel_id: i64,
    pub title: Option<String>,
    pub body: Option<String>,
    pub bypass_silence: bool,
    pub request_id: String,
}

/// 类型化发送命令（两个入口的唯一业务输入）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum NotifyCommand {
    Business(BusinessSend),
    Test(TestSend),
}

impl NotifyCommand {
    /// 从传输载荷构造命令（适配层已完成 JSON 解析）。`business` 由路由决定
    /// （/api/notify/send=true，/api/notify/channels/test=false）。缺省与
    /// 显式 null 走既有默认；非法类型一律 Invalid 且无副作用。
    pub(crate) fn from_raw(raw: &Value, business: bool) -> Result<Self, NotifyError> {
        let request_id = raw["requestId"]
            .as_str()
            .filter(|s| !s.is_empty() && s.chars().count() <= 128)
            .ok_or_else(|| NotifyError::invalid_param("requestId"))?
            .to_owned();
        if !business {
            if !raw
                .as_object()
                .is_some_and(|o| o.keys().all(|k| TEST_FIELDS_ALLOWED.contains(&k.as_str())))
            {
                return Err(NotifyError::Invalid {
                    message: "发送字段无效".into(),
                });
            }
            let channel_id = raw["id"]
                .as_i64()
                .ok_or_else(|| NotifyError::invalid_param("id"))?;
            let text = |key: &str| -> Result<Option<String>, NotifyError> {
                match raw.get(key) {
                    None | Some(Value::Null) => Ok(None),
                    Some(v) => match v.as_str() {
                        Some(text) => Ok(Some(text.to_owned())),
                        None => Err(NotifyError::invalid_param(key)),
                    },
                }
            };
            return Ok(Self::Test(TestSend {
                channel_id,
                title: text("title")?,
                body: text("body")?,
                bypass_silence: raw["bypassSilence"].as_bool().unwrap_or(false),
                request_id,
            }));
        }
        let channel_id = raw["id"]
            .as_i64()
            .ok_or_else(|| NotifyError::invalid_param("id"))?;
        let subject = match raw["subject"].as_str() {
            Some("event") => NotifySubject::Event(
                raw["subjectId"]
                    .as_i64()
                    .ok_or_else(|| NotifyError::invalid_param("subjectId"))?,
            ),
            Some("report") => NotifySubject::Report(
                raw["subjectId"]
                    .as_i64()
                    .ok_or_else(|| NotifyError::invalid_param("subjectId"))?,
            ),
            Some("candidate") => NotifySubject::Candidate {
                snapshot_id: raw["snapshotId"]
                    .as_str()
                    .filter(|s| !s.is_empty() && s.chars().count() <= 256)
                    .ok_or_else(|| NotifyError::invalid_param("snapshotId"))?
                    .to_owned(),
                candidate_id: raw["subjectId"]
                    .as_str()
                    .filter(|s| !s.is_empty() && s.chars().count() <= 256)
                    .ok_or_else(|| NotifyError::invalid_param("subjectId"))?
                    .to_owned(),
            },
            _ => return Err(NotifyError::invalid_param("subject")),
        };
        let language = match raw["language"].as_str() {
            None | Some("zh_CN") => NotifyLanguage::ZhCn,
            Some("en") => NotifyLanguage::En,
            Some(_) => return Err(NotifyError::invalid_param("language")),
        };
        let preview_token = raw["previewToken"]
            .as_str()
            .filter(|s| !s.is_empty() && s.chars().count() <= 128)
            .ok_or_else(|| NotifyError::invalid_param("previewToken"))?
            .to_owned();
        Ok(Self::Business(BusinessSend {
            channel_id,
            subject,
            language,
            preview_token,
            request_id,
        }))
    }

    pub(crate) fn channel_id(&self) -> i64 {
        match self {
            Self::Business(send) => send.channel_id,
            Self::Test(send) => send.channel_id,
        }
    }
}

/// 语义错误（R-B1：稳定 code；HTTP 状态与 wire 形态由适配层映射）。
/// Upstream 预先完成稳定 code 与 Db 掩码投影（FinanceError 非 Clone，
/// 语义投影在构造点一次性完成）。
#[derive(Debug)]
pub(crate) enum NotifyError {
    /// 输入无效（FIN_INVALID）。
    Invalid { message: String },
    /// 渠道不存在或已归档（FIN_NOT_FOUND）。
    NotFound,
    /// 冲突：渠道停用／事实已变化／token 失效（FIN_CONFLICT）。
    Conflict { message: String },
    /// 凭据不可用：未配置、Keychain locked、读取失败（FIN_INVALID 语义族，
    /// 单独分类供任务与页面给出可行动提示）。
    SecretUnavailable { message: String },
    /// 渠道协议／存储执行失败（保留 FinanceError 的稳定 code 与 Db 掩码）。
    Upstream { code: &'static str, message: String },
}

impl NotifyError {
    fn invalid_param(field: &str) -> Self {
        Self::Invalid {
            message: format!("缺少 {field}"),
        }
    }

    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::Invalid { .. } => "FIN_INVALID",
            Self::NotFound => "FIN_NOT_FOUND",
            Self::Conflict { .. } => "FIN_CONFLICT",
            Self::SecretUnavailable { .. } => "FIN_INVALID",
            Self::Upstream { code, .. } => code,
        }
    }

    pub(crate) fn message(&self) -> String {
        match self {
            Self::Invalid { message } => format!("输入无效：{message}"),
            Self::NotFound => "记录不存在：渠道".into(),
            Self::Conflict { message } => format!("冲突：{message}"),
            Self::SecretUnavailable { message } => format!("输入无效：{message}"),
            // 与既有 err() 同口径：Db 掩码在构造点已完成。
            Self::Upstream { message, .. } => message.clone(),
        }
    }

    fn from_finance(e: &FinanceError) -> Self {
        match e.code() {
            "FIN_CONFLICT" => Self::Conflict {
                message: e.message(),
            },
            "FIN_NOT_FOUND" => Self::NotFound,
            // Db 掩码与既有 err() 同口径。
            _ if matches!(e, FinanceError::Db(_)) => Self::Upstream {
                code: e.code(),
                message: "数据操作失败，请重试".into(),
            },
            _ => Self::Upstream {
                code: e.code(),
                message: e.message(),
            },
        }
    }
}

/// Secret 短时取用窄能力（R-M7）：业务只依赖本 trait；生产实现为
/// Runtime SecretStore，测试注入 locked／缺凭据假实现。
pub(crate) trait SecretReveal {
    /// 返回 Err((code, retryable)) 表示 Keychain 不可用（locked 等）。
    fn reveal_secret(&self, account: &str) -> Result<Option<String>, (String, bool)>;
}

impl SecretReveal for app_runtime_core::secrets::SecretStore {
    fn reveal_secret(&self, account: &str) -> Result<Option<String>, (String, bool)> {
        self.reveal(account)
            .map_err(|e| (e.code.as_str().to_owned(), e.retryable))
    }
}

/// 统一发送入口：HTTP 与任务调用同一实现（生产执行器）。
pub(crate) fn send(
    store: &Store,
    secrets: Option<&dyn SecretReveal>,
    command: &NotifyCommand,
    cancelled: impl Fn() -> bool,
) -> Result<Value, NotifyError> {
    send_with_executor(store, secrets, command, cancelled, production_executor())
}

/// 执行器注入变体：隔离测试用假渠道执行器验证业务编排（不触网、不触
/// Keychain）；生产入口固定使用 [`production_executor`]。
pub(crate) fn send_with_executor(
    store: &Store,
    secrets: Option<&dyn SecretReveal>,
    command: &NotifyCommand,
    cancelled: impl Fn() -> bool,
    execute: impl Fn(&OutboundRequest) -> channels::ExecuteResult,
) -> Result<Value, NotifyError> {
    let business = matches!(command, NotifyCommand::Business(_));
    let id = command.channel_id();
    // 业务事实／token 校验（唯一事实源 notification_subjects；命令已类型化，
    // 校验输入由命令确定性重建）。
    let preview = if let NotifyCommand::Business(send) = command {
        let input = send.fact_input();
        Some(
            crate::notification_subjects::validate_send(store, &input)
                .map_err(|e| NotifyError::from_finance(&e))?,
        )
    } else {
        None
    };
    let list = channels::list(store).map_err(|e| NotifyError::from_finance(&e))?;
    let channel = list["channels"]
        .as_array()
        .and_then(|a| a.iter().find(|c| c["id"].as_i64() == Some(id)))
        .filter(|c| c["archived"] == json!(false))
        .ok_or(NotifyError::NotFound)?
        .clone();
    if channel["enabled"] == json!(false) {
        return Err(NotifyError::Conflict {
            message: "渠道已停用，请先启用".into(),
        });
    }
    let kind = channel["kind"].as_str().unwrap_or("").to_string();
    let config = channel["config"].clone();
    if cancelled() {
        return Ok(json!({ "status": "cancelled" }));
    }
    // reveal：仅本请求内存内短时使用，不入日志／DB／协议响应。
    let mut secret_values = json!({});
    if let Some(secrets) = secrets {
        for field in channels::secret_fields(&kind) {
            let account = format!("channel:{id}:{field}");
            let value = secrets.reveal_secret(&account).map_err(|(code, _)| {
                NotifyError::SecretUnavailable {
                    message: format!("Secret 读取失败（{code}），请检查钥匙串后重试"),
                }
            })?;
            if let Some(value) = value {
                secret_values[*field] = json!(value);
            }
        }
    }
    // 准备到执行之间事实重验：渠道版本／启用状态不得变化（先于请求构造）。
    let fresh = channels::list(store).map_err(|e| NotifyError::from_finance(&e))?;
    if !fresh["channels"].as_array().is_some_and(|a| {
        a.iter().any(|c| {
            c["id"] == channel["id"]
                && c["version"] == channel["version"]
                && c["enabled"] == true
                && c["archived"] == false
        })
    }) {
        return Err(NotifyError::Conflict {
            message: "渠道已变化".into(),
        });
    }
    let request = channels::build_request(
        &kind,
        &config,
        &secret_values,
        &preview
            .as_ref()
            .and_then(|p| p["title"].as_str())
            .map(str::to_owned)
            .or_else(|| match command {
                NotifyCommand::Test(send) => send.title.clone(),
                _ => None,
            })
            .unwrap_or_else(|| "Natives 测试消息".to_owned()),
        &preview
            .as_ref()
            .and_then(|p| p["body"].as_str())
            .map(str::to_owned)
            .or_else(|| match command {
                NotifyCommand::Test(send) => send.body.clone(),
                _ => None,
            })
            .unwrap_or_else(|| {
                "这是来自 Natives 投资模块的测试消息，只验证渠道连通性。".to_owned()
            }),
    )
    .map_err(|e| match &e {
        FinanceError::Invalid(message) if message.contains("Secret") => {
            NotifyError::SecretUnavailable {
                message: message.clone(),
            }
        }
        other => NotifyError::from_finance(other),
    })?;
    if let NotifyCommand::Business(send) = command {
        // token／事实重验（执行窗口内渠道与对象变化拒绝）。
        crate::notification_subjects::validate_send(store, &send.fact_input())
            .map_err(|e| NotifyError::from_finance(&e))?;
    }
    let policy = channels::load_policy(store);
    channels::send(
        store,
        &kind,
        &request,
        id,
        preview
            .as_ref()
            .and_then(|p| p["subjectRef"].as_str())
            .unwrap_or("test"),
        &policy,
        !business && matches!(command, NotifyCommand::Test(send) if send.bypass_silence),
        !business,
        cancelled,
        execute,
    )
    .map_err(|e| NotifyError::from_finance(&e))
}

/// 生产执行器：SSRF 守卫 + 有界 HTTPS（10s 超时，跟随方案网络约束）。
fn production_executor() -> impl Fn(&OutboundRequest) -> channels::ExecuteResult {
    use std::io::Read;
    let agent = crate::network::client();
    move |request: &OutboundRequest| {
        if !channels::host_allowed(&request.url) {
            return Err("target_url_blocked_by_guard".into());
        }
        let mut req = agent.post(&request.url);
        for (k, v) in &request.headers {
            req = req.set(k, v);
        }
        match req.send_string(&request.body) {
            Ok(resp) => {
                let status = resp.status() as u16;
                let mut body = String::new();
                resp.into_reader()
                    .take(8193)
                    .read_to_string(&mut body)
                    .map_err(|_| "response_read_failed".to_string())?;
                if body.len() > 8192 {
                    return Err("response_too_large".into());
                }
                Ok((status, body))
            }
            Err(crate::network::Error::Status(code, resp)) => {
                let mut body = String::new();
                resp.into_reader()
                    .take(8193)
                    .read_to_string(&mut body)
                    .map_err(|_| "response_read_failed".to_string())?;
                if body.len() > 8192 {
                    return Err("response_too_large".into());
                }
                Ok((code, body))
            }
            Err(_) => Err("network_failed_or_delivery_unknown".into()),
        }
    }
}

#[cfg(test)]
#[path = "notification_send_tests.rs"]
mod tests;
