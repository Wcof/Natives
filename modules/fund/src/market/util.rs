//! market 内部小工具（拆分自 market.rs，仅移动，无逻辑变更）。

pub(crate) fn json_string(s: &str) -> String {
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

pub(crate) fn num_or_null(v: Option<f64>) -> String {
    match v {
        Some(v) if v.is_finite() => {
            // 保留两位小数，避免浮点尾巴。
            format!("{:.2}", v)
        }
        _ => "null".into(),
    }
}

pub(crate) fn upstream_err(message: String) -> (u16, String) {
    (
        502,
        format!(
            "{{\"error\":\"APP_UPSTREAM\",\"message\":{}}}",
            json_string(&message)
        ),
    )
}

pub(crate) fn query_param<'a>(path: &'a str, key: &str) -> Option<String> {
    let query = path.split_once('?')?.1;
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then(|| v.to_string())
    })
}

/// 查询参数值解码：`%XX` 百分号编码 + `+` 还原为空格（UTF-8 字节序列）。
pub(crate) fn url_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3])
                    .ok()
                    .and_then(|h| u8::from_str_radix(h, 16).ok());
                match hex {
                    Some(b) => {
                        out.push(b);
                        i += 3;
                    }
                    None => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 查询参数值最小 urlencode：仅编码 URL/查询串保留字符，中文按 UTF-8 原样输出
/// （ureq 会按 UTF-8 发送；`+`/`%`/空白/`&`/`#`/`?`/`=` 必须转义）。
pub(crate) fn urlencode(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for b in raw.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
