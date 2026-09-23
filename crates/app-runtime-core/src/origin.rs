//! Chrome Native Messaging 启动 origin 校验。

use crate::error::{AppErrorBody, AppErrorCode};

/// 从 Chrome 传入的实参中取得唯一扩展 origin。页面消息不能覆盖此值。
pub fn chrome_extension_origin(args: &[String]) -> Result<String, AppErrorBody> {
    let origins: Vec<&String> = args
        .iter()
        .filter(|arg| arg.starts_with("chrome-extension://"))
        .collect();
    let Some(origin) = origins.last() else {
        return Err(AppErrorBody::new(
            AppErrorCode::ProtocolMismatch,
            "missing Chrome extension origin",
            false,
        ));
    };
    if origins.len() != 1 || !valid_extension_origin(origin) {
        return Err(AppErrorBody::new(
            AppErrorCode::ProtocolMismatch,
            "invalid Chrome extension origin",
            false,
        ));
    }
    Ok((*origin).clone())
}

fn valid_extension_origin(origin: &str) -> bool {
    let Some(id) = origin.strip_prefix("chrome-extension://") else {
        return false;
    };
    let id = id.strip_suffix('/').unwrap_or(id);
    id.len() == 32 && id.bytes().all(|byte| (b'a'..=b'p').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_one_valid_chrome_origin() {
        let args = vec![
            "host".to_string(),
            "chrome-extension://abcdefghijklmnopabcdefghijklmnop".to_string(),
        ];
        assert!(chrome_extension_origin(&args).is_ok());
        assert!(chrome_extension_origin(&["host".to_string()]).is_err());
        assert!(chrome_extension_origin(&["chrome-extension://not-an-id".to_string()]).is_err());
    }
}
