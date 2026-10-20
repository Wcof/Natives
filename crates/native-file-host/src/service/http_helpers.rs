//! 本机服务 HTTP 响应与同源边界辅助。

use app_runtime_core::http::HttpRequest;
use std::io::{self, Write};
use std::net::TcpStream;

pub(super) fn write_http_raw(
    stream: &mut TcpStream,
    status_code: u16,
    status_text: &str,
    content_type: &str,
    extra_headers: &[(&str, &str)],
    body: &[u8],
) -> io::Result<()> {
    write_http_raw_with_cache(
        stream,
        status_code,
        status_text,
        content_type,
        "no-store",
        extra_headers,
        body,
    )
}

/// Static documents use `no-cache` instead of `no-store`: the document must
/// revalidate, but Chrome may keep it in the back/forward cache. API responses
/// keep the stricter `no-store` policy through `write_http_raw`.
pub(super) fn write_http_raw_with_cache(
    stream: &mut TcpStream,
    status_code: u16,
    status_text: &str,
    content_type: &str,
    cache_control: &str,
    extra_headers: &[(&str, &str)],
    body: &[u8],
) -> io::Result<()> {
    let mut header_bytes = Vec::new();
    write!(
        header_bytes,
        "HTTP/1.1 {status_code} {status_text}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: {cache_control}\r\nConnection: close\r\n",
        body.len()
    )?;
    for (k, v) in extra_headers {
        write!(header_bytes, "{k}: {v}\r\n")?;
    }
    write!(header_bytes, "\r\n")?;
    stream.write_all(&header_bytes)?;
    stream.write_all(body)?;
    stream.flush()
}

pub(super) fn extract_cookie_value<'a>(req: &'a HttpRequest, cookie_name: &str) -> Option<&'a str> {
    let cookie_header = req.header("cookie")?;
    for pair in cookie_header.split(';') {
        let mut parts = pair.trim().splitn(2, '=');
        if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
            if k == cookie_name {
                return Some(v);
            }
        }
    }
    None
}

pub(super) fn validate_origin(req: &HttpRequest, port: u16) -> bool {
    let Some(origin) = req.header("origin") else {
        return false;
    };
    if port == 0 {
        return origin.starts_with("http://127.0.0.1:") || origin.starts_with("http://localhost:");
    }
    origin == format!("http://127.0.0.1:{port}") || origin == format!("http://localhost:{port}")
}

pub(super) fn is_loopback_host(host_header: Option<&str>, port: u16) -> bool {
    let Some(host) = host_header else {
        return false;
    };
    if port == 0 {
        return host.starts_with("127.0.0.1:") || host.starts_with("localhost:");
    }
    host == format!("127.0.0.1:{port}") || host == format!("localhost:{port}")
}
