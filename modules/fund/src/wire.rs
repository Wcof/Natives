//! wire JSON 工具（mode/T04，R-M3/R-T3）。
//!
//! 领域投影（cards、home 等）与 HTTP 适配层共用的 JSON 字符串字面量转义。
//! 自 api 路由层迁出，使业务／投影文件不再反向依赖 HTTP 适配层；
//! api/mod.rs 以 `pub(crate) use` 聚合给各 handler 使用。

/// JSON 字符串字面量（含控制字符转义）。
pub(crate) fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// 稳定错误 wire 投影（code + 固定文案 → {"error":…,"message":…}）。
/// 纯 wire 序列化助手：HTTP 适配层与任务协议共用同一函数（T04/T08 收口），
/// 不携带业务规则。
pub(crate) fn api_error(code: u16, error_code: &str, message: &str) -> (u16, String) {
    (
        code,
        format!(
            "{{\"error\":{},\"message\":{}}}",
            json_str(error_code),
            json_str(message)
        ),
    )
}
