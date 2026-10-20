//! 有界 loopback HTTP 请求辅助（托管应用契约 v1 §7）。

use std::collections::BTreeMap;
use std::io::{self, Read, Write};

pub const MAX_HTTP_HEADER_BYTES: usize = 16 * 1024;
pub const MAX_HTTP_BODY_BYTES: usize = 256 * 1024;

pub struct HttpRequest {
    pub method: String,
    pub path: String,
    pub body: Vec<u8>,
    headers: BTreeMap<String, String>,
}

impl HttpRequest {
    /// 仅解析有界 HTTP/1 请求头；业务 body 不应从未经认证的路由读取。
    pub fn read(reader: &mut impl Read) -> io::Result<Self> {
        let mut bytes = Vec::new();
        while bytes.len() <= MAX_HTTP_HEADER_BYTES && !bytes.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            if reader.read(&mut byte)? == 0 {
                break;
            }
            bytes.push(byte[0]);
        }
        if bytes.len() > MAX_HTTP_HEADER_BYTES
            || !bytes.windows(4).any(|window| window == b"\r\n\r\n")
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid HTTP headers",
            ));
        }
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "non-UTF-8 HTTP headers"))?;
        let mut lines = text.split("\r\n");
        let mut first = lines.next().unwrap_or("").split_whitespace();
        let method = first.next().unwrap_or("");
        let path = first.next().unwrap_or("");
        let version = first.next().unwrap_or("");
        if method.is_empty() || path.is_empty() || version != "HTTP/1.1" || first.next().is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid request line",
            ));
        }
        let mut headers = BTreeMap::new();
        for line in lines {
            if line.is_empty() {
                break;
            }
            let Some((name, value)) = line.split_once(':') else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid HTTP header",
                ));
            };
            if name.is_empty()
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
                || value.bytes().any(|b| b < 32 && b != b'\t' || b == 127)
                || headers
                    .insert(name.to_ascii_lowercase(), value.trim().to_string())
                    .is_some()
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "ambiguous HTTP header",
                ));
            }
        }
        // These bounded, connection-closing servers only support Content-Length framing.
        if headers.contains_key("transfer-encoding") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported transfer encoding",
            ));
        }
        let content_length = headers
            .get("content-length")
            .map(|value| value.parse::<usize>())
            .transpose()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid content length"))?
            .unwrap_or(0);
        if content_length > MAX_HTTP_BODY_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "HTTP body too large",
            ));
        }
        let mut body = vec![0; content_length];
        reader.read_exact(&mut body)?;
        Ok(Self {
            method: method.to_string(),
            path: path.to_string(),
            body,
            headers,
        })
    }

    /// 显式内部构造器：Runtime 内部 dispatch（如 `app:invoke` 语义桥接）把
    /// `app:invoke` 参数转成既有 api 路由语义时使用。仅限同进程内
    /// 受信调用；网络入口仍必须经 `HttpRequest::read` 的边界解析。
    pub fn for_internal(method: &str, path: &str, body: Vec<u8>) -> Self {
        Self {
            method: method.to_string(),
            path: path.to_string(),
            body,
            headers: BTreeMap::new(),
        }
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }

    pub fn has_loopback_host(&self, port: u16) -> bool {
        self.header("host")
            .is_some_and(|host| host.eq_ignore_ascii_case(&format!("127.0.0.1:{port}")))
    }

    pub fn sandbox_origin(&self) -> bool {
        self.header("origin") == Some("null")
    }
}

pub fn write_response(
    writer: &mut impl Write,
    code: u16,
    reason: &str,
    content_type: &str,
    body: &[u8],
    csp: &str,
) -> io::Result<()> {
    let cors = "Access-Control-Allow-Origin: null\r\nAccess-Control-Allow-Methods: GET, POST, PUT, DELETE\r\nAccess-Control-Allow-Headers: Authorization, Content-Type\r\n";
    write!(writer, "HTTP/1.1 {code} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nContent-Security-Policy: {csp}\r\n{cors}Connection: close\r\n\r\n", body.len())?;
    writer.write_all(body)?;
    writer.flush()
}

pub fn write_preflight(writer: &mut impl Write, request: &HttpRequest) -> io::Result<()> {
    if !request.sandbox_origin()
        || !request
            .header("access-control-request-method")
            .is_some_and(|method| matches!(method, "GET" | "POST" | "PUT" | "DELETE"))
        || !request
            .header("access-control-request-headers")
            .is_some_and(|headers| {
                let names: Vec<_> = headers
                    .split(',')
                    .map(|name| name.trim().to_ascii_lowercase())
                    .collect();
                names.iter().any(|name| name == "authorization")
                    && names
                        .iter()
                        .all(|name| name == "authorization" || name == "content-type")
            })
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "invalid CORS preflight",
        ));
    }
    writer.write_all(b"HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: null\r\nAccess-Control-Allow-Methods: GET, POST, PUT, DELETE\r\nAccess-Control-Allow-Headers: Authorization, Content-Type\r\nAccess-Control-Max-Age: 600\r\nConnection: close\r\n\r\n")?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ambiguous_headers_and_transfer_encoding_are_rejected() {
        for headers in [
            "Host: attacker.test\r\nhOsT: 127.0.0.1:8123\r\n",
            "Origin: https://attacker.test\r\nOrigin: null\r\n",
            "Content-Length: 1\r\nContent-Length: 0\r\n",
            "Transfer-Encoding: chunked\r\nContent-Length: 0\r\n",
            "Host : 127.0.0.1:8123\r\n",
            " Bad: folded\r\n",
        ] {
            let raw = format!("POST /api/value HTTP/1.1\r\n{headers}\r\n");
            assert!(HttpRequest::read(&mut raw.as_bytes()).is_err(), "{headers}");
        }
    }

    #[test]
    fn declared_put_preflight_requires_sandbox_origin_and_authorization() {
        let raw=b"OPTIONS /api/home/group-plan HTTP/1.1\r\nHost: 127.0.0.1:8123\r\nOrigin: null\r\nAccess-Control-Request-Method: PUT\r\nAccess-Control-Request-Headers: authorization,content-type\r\n\r\n";
        let mut req = HttpRequest::read(&mut raw.as_slice()).unwrap();
        let mut out = vec![];
        write_preflight(&mut out, &req).unwrap();
        assert!(String::from_utf8(out)
            .unwrap()
            .contains("GET, POST, PUT, DELETE"));
        req.headers
            .insert("access-control-request-method".into(), "DELETE".into());
        write_preflight(&mut vec![], &req).unwrap();
        for method in ["PATCH", "*"] {
            req.headers
                .insert("access-control-request-method".into(), method.into());
            assert!(write_preflight(&mut vec![], &req).is_err());
        }
        req.headers
            .insert("access-control-request-method".into(), "PUT".into());
        req.headers
            .insert("origin".into(), "https://example.com".into());
        assert!(write_preflight(&mut vec![], &req).is_err());
        req.headers.insert("origin".into(), "null".into());
        req.headers.insert(
            "access-control-request-headers".into(),
            "content-type".into(),
        );
        assert!(write_preflight(&mut vec![], &req).is_err());
    }

    #[test]
    fn request_is_bounded_and_checks_host_and_origin() {
        let raw = b"POST /api/value HTTP/1.1\r\nHost: 127.0.0.1:8123\r\nOrigin: null\r\nAuthorization: Bearer x\r\nContent-Length: 5\r\n\r\nhello";
        let request = HttpRequest::read(&mut raw.as_slice()).unwrap();
        assert!(request.has_loopback_host(8123));
        assert!(request.sandbox_origin());
        assert_eq!(request.header("authorization"), Some("Bearer x"));
        assert_eq!(request.body, b"hello");
    }
}
