//! App Secret 窄平台能力（ADR-0038）：模块 Secret 只进 OS Keychain，
//! 协议层只有 set/status/delete，没有读取明文的 op；明文仅经
//! `reveal` 供同进程模块业务在发送窗口短时取用，绝不进日志、DB 或 iframe。

use crate::error::AppErrorCode;

pub const SECRET_CAPABILITY: &str = "platform.secret";
pub const MAX_SECRET_VALUE_BYTES: usize = 8192;

/// accountId 只允许小写安全字符集（`channel:<id>:<field>` 一类固定引用）。
pub fn is_valid_account_id(account: &str) -> bool {
    let mut chars = account.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() || first.is_ascii_digit() => {}
        _ => return false,
    }
    account.len() <= 64
        && chars.all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, ':' | '.' | '_' | '-')
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecretError {
    pub code: AppErrorCode,
    pub retryable: bool,
}

impl SecretError {
    fn locked() -> Self {
        Self {
            code: AppErrorCode::KeychainLocked,
            retryable: true,
        }
    }
    // 仅非 macOS 分支调用；macOS 上后端走 /usr/bin/security，不存在不支持路径。
    #[cfg(not(target_os = "macos"))]
    fn unsupported() -> Self {
        Self {
            code: AppErrorCode::KeychainUnsupported,
            retryable: false,
        }
    }
    fn invalid() -> Self {
        Self {
            code: AppErrorCode::ProtocolMismatch,
            retryable: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretStatus {
    pub exists: bool,
    pub updated_at: Option<String>,
}

#[derive(Debug)]
pub struct SecretStore {
    namespace: String,
}

impl SecretStore {
    pub fn for_app(app_id: &str) -> Self {
        Self {
            namespace: crate::module::app_keychain_namespace(app_id),
        }
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    pub fn set(&self, account: &str, value: &str) -> Result<(), SecretError> {
        self.validate(account, value)?;
        #[cfg(target_os = "macos")]
        {
            let args = vec![
                "add-generic-password".to_string(),
                "-s".to_string(),
                self.namespace.clone(),
                "-a".to_string(),
                account.to_string(),
                "-w".to_string(),
                value.to_string(),
                "-U".to_string(),
            ];
            match run_security(&args) {
                Some(true) => Ok(()),
                _ => Err(SecretError::locked()),
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = account;
            Err(SecretError::unsupported())
        }
    }

    pub fn status(&self, account: &str) -> Result<SecretStatus, SecretError> {
        self.validate(account, "")?;
        #[cfg(target_os = "macos")]
        {
            let args = security_find_args(&self.namespace, account);
            match run_security_capture(&args) {
                Some((0, stdout)) => Ok(SecretStatus {
                    exists: true,
                    updated_at: parse_mdat(&stdout),
                }),
                Some((44, _)) => Ok(SecretStatus {
                    exists: false,
                    updated_at: None,
                }),
                _ => Err(SecretError::locked()),
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = account;
            Err(SecretError::unsupported())
        }
    }

    /// 幂等删除：条目不存在视为成功。
    pub fn delete(&self, account: &str) -> Result<(), SecretError> {
        self.validate(account, "")?;
        #[cfg(target_os = "macos")]
        {
            let args = vec![
                "delete-generic-password".to_string(),
                "-s".to_string(),
                self.namespace.clone(),
                "-a".to_string(),
                account.to_string(),
            ];
            match run_security(&args) {
                Some(true) => Ok(()),
                _ => Err(SecretError::locked()),
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = account;
            Err(SecretError::unsupported())
        }
    }

    /// 仅同进程模块业务可调（发送窗口短时取明文）；不暴露给任何协议方法。
    pub fn reveal(&self, account: &str) -> Result<Option<String>, SecretError> {
        self.validate(account, "")?;
        #[cfg(target_os = "macos")]
        {
            let args = vec![
                "find-generic-password".to_string(),
                "-s".to_string(),
                self.namespace.clone(),
                "-a".to_string(),
                account.to_string(),
                "-w".to_string(),
            ];
            match run_security_capture(&args) {
                Some((0, stdout)) => Ok(Some(stdout.trim_end_matches('\n').to_string())),
                Some((44, _)) => Ok(None),
                _ => Err(SecretError::locked()),
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = account;
            Err(SecretError::unsupported())
        }
    }

    fn validate(&self, account: &str, value: &str) -> Result<(), SecretError> {
        if !is_valid_account_id(account) || self.namespace.is_empty() {
            return Err(SecretError::invalid());
        }
        if value.len() > MAX_SECRET_VALUE_BYTES || value.contains('\0') {
            return Err(SecretError::invalid());
        }
        Ok(())
    }
}

#[cfg(any(target_os = "macos", test))]
fn security_find_args(namespace: &str, account: &str) -> Vec<String> {
    vec![
        "find-generic-password".to_string(),
        "-s".to_string(),
        namespace.to_string(),
        "-a".to_string(),
        account.to_string(),
    ]
}

/// `security` 的 find 输出里唯一可上送的非秘密字段是修改时间。真实形态：
/// `"mdat"<timedate>=0x32303236…5A00  "20261002113411Z\000"`（hex 后还有文本描述，
/// 必须截断），以及文本形态 `"mdat" -> 20261002123456Z`。
/// 其余属性（含 svc/acct 与任何 password 字段）绝不回传。
#[cfg(any(target_os = "macos", test))]
fn parse_mdat(find_output: &str) -> Option<String> {
    let raw = find_output.lines().find_map(|line| {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("\"mdat\"<timedate>=0x") {
            // 只取 0x 后到第一个空白为止的 hex token，忽略其后的文本描述。
            let token = rest.split_whitespace().next()?;
            decode_hex_ascii(token)
        } else if let Some(rest) = line.strip_prefix("\"mdat\"") {
            rest.split("-> ").nth(1).map(str::trim).map(str::to_owned)
        } else {
            None
        }
    })?;
    // 只取数字（允许中间存在 'T' 或尾部 'Z'/'\0'）。
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    let bytes = digits.as_bytes();
    if bytes.len() != 14 || !bytes.iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let num = |r: std::ops::Range<usize>| digits[r].parse::<u32>();
    let (month, day, hour, minute, second) = (
        num(4..6).ok()?,
        num(6..8).ok()?,
        num(8..10).ok()?,
        num(10..12).ok()?,
        num(12..14).ok()?,
    );
    let year = num(0..4).ok()?;
    if (1..=12).contains(&month)
        && (1..=31).contains(&day)
        && hour < 24
        && minute < 60
        && second < 61
    {
        Some(format!(
            "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z"
        ))
    } else {
        None
    }
}

#[cfg(any(target_os = "macos", test))]
fn decode_hex_ascii(hex: &str) -> Option<String> {
    let bytes = hex.as_bytes();
    if bytes.len() % 2 != 0 {
        return None;
    }
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        let hi = char::from(bytes[i]).to_digit(16)?;
        let lo = char::from(bytes[i + 1]).to_digit(16)?;
        out.push(char::from((hi * 16 + lo) as u8));
        i += 2;
    }
    Some(out)
}

#[cfg(target_os = "macos")]
fn run_security(args: &[String]) -> Option<bool> {
    run_security_capture(args).map(|(code, _)| code == 0 || code == 44)
}

#[cfg(target_os = "macos")]
fn run_security_capture(args: &[String]) -> Option<(i32, String)> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let mut child = Command::new("/usr/bin/security")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let stdout = child.stdout.take()?;
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout.take(65537).read_to_end(&mut bytes).map(|_| bytes);
        let _ = sender.send(result);
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        if let Some(status) = child.try_wait().ok().flatten() {
            let code = status.code().unwrap_or(-1);
            let bytes = receiver.recv().ok()?.ok()?;
            if bytes.len() > 65536 {
                return None;
            }
            return Some((code, String::from_utf8_lossy(&bytes).into_owned()));
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_ids_are_narrowly_patterned() {
        assert!(is_valid_account_id("channel:telegram:bot_token"));
        assert!(is_valid_account_id("a"));
        assert!(is_valid_account_id(&"x".repeat(64)));
        assert!(!is_valid_account_id(""));
        assert!(!is_valid_account_id("-lead"));
        assert!(!is_valid_account_id(&"x".repeat(65)));
        assert!(!is_valid_account_id("has space"));
        assert!(!is_valid_account_id("Upper"));
        assert!(!is_valid_account_id("semi;colon"));
        assert!(!is_valid_account_id("path/traversal"));
    }

    #[test]
    fn value_limits_are_enforced_before_any_backend_call() {
        let store = SecretStore {
            namespace: "com.natives.app.test".into(),
        };
        // 非法 accountId 与超界 value 一律 ProtocolMismatch（macOS 与非 macOS 同路径）。
        assert_eq!(
            store.set("../bad", "x").unwrap_err().code,
            AppErrorCode::ProtocolMismatch
        );
        assert_eq!(
            store.status("bad space").unwrap_err().code,
            AppErrorCode::ProtocolMismatch
        );
        let oversized = "x".repeat(MAX_SECRET_VALUE_BYTES + 1);
        assert_eq!(
            store.set("a", &oversized).unwrap_err().code,
            AppErrorCode::ProtocolMismatch
        );
        assert_eq!(
            store.set("a", "nul\0value").unwrap_err().code,
            AppErrorCode::ProtocolMismatch
        );
    }

    #[test]
    fn mdat_parsing_never_leaks_other_attributes() {
        let output = "keychain: \"/x.db\" attributes:\n    \"svce\" <blob>=\"com.natives.app.test\"\n    \"mdat\" <ctrd> -> 20261002123456Z\npassword: \"super-secret\"\n";
        assert_eq!(parse_mdat(output).as_deref(), Some("2026-10-02T12:34:56Z"));
        assert_eq!(parse_mdat("no mdat line here\npassword: \"x\"\n"), None);
        assert_eq!(
            parse_mdat("\"mdat\" -> 20261302123456Z"),
            None,
            "month 13 must be rejected"
        );
        assert_eq!(parse_mdat("\"mdat\" -> notatime"), None);
        // 真实 find-generic-password 形态：hex 编码 timedate 后还跟文本描述，
        // 解析必须截断 hex token，绝不能把文本段当 hex 继续消费。
        assert_eq!(
            parse_mdat(
                "attributes:\n    \"acct\"<blob>=\"ch\"\n    \"mdat\"<timedate>=0x3230323631303032543132333435365A00  \"20261002T123456Z\\000\"\n    \"svce\"<blob>=\"com.natives.app.test\"\npassword: never-echoed\n"
            )
            .as_deref(),
            Some("2026-10-02T12:34:56Z")
        );
    }

    #[test]
    fn find_args_shape() {
        assert_eq!(
            security_find_args("com.natives.app.fund", "channel:bark:key"),
            vec![
                "find-generic-password",
                "-s",
                "com.natives.app.fund",
                "-a",
                "channel:bark:key"
            ]
        );
    }
}
