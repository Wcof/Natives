//! Chrome Native Messaging 轻量 Relay 客户端（ADR-0042）。
//! 将 stdin/stdout Native 帧无损中继至本地唯一的 native-file-host 服务实例。

use crate::protocol::{read_frame, Request, MAX_MESSAGE_BYTES};
use app_runtime_core::session::{base64url, constant_time_eq, random_bytes};
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

pub fn write_raw_frame(writer: &mut impl Write, body: &[u8]) -> io::Result<()> {
    if body.len() >= MAX_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "relay response exceeds Native limit",
        ));
    }
    let len = (body.len() as u32).to_ne_bytes();
    writer.write_all(&len)?;
    writer.write_all(body)?;
    writer.flush()
}

pub fn send_relay_http(port: u16, body: &[u8]) -> io::Result<Vec<u8>> {
    send_relay_http_owned(port, body, Duration::from_secs(30), None)
}

fn send_relay_http_with_timeout(port: u16, body: &[u8], timeout: Duration) -> io::Result<Vec<u8>> {
    send_relay_http_owned(port, body, timeout, None)
}

fn send_relay_http_owned(
    port: u16,
    body: &[u8],
    timeout: Duration,
    surface: Option<&str>,
) -> io::Result<Vec<u8>> {
    let secret = super::read_relay_secret().unwrap_or_default();
    if secret.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "relay unavailable",
        ));
    }
    let mut stream = TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;

    write!(
        stream,
        "POST /native/relay HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Natives-Relay-Secret: {secret}\r\nX-Natives-Surface-Id: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        surface.unwrap_or(""), body.len()
    )?;
    stream.write_all(body)?;
    stream.flush()?;

    let mut resp_bytes = Vec::new();
    stream
        .take((MAX_MESSAGE_BYTES + 8192) as u64)
        .read_to_end(&mut resp_bytes)?;
    parse_relay_response(resp_bytes)
}

fn parse_relay_response(resp_bytes: Vec<u8>) -> io::Result<Vec<u8>> {
    let Some(header_end) = resp_bytes.windows(4).position(|w| w == b"\r\n\r\n") else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid HTTP response from service",
        ));
    };

    let body = &resp_bytes[header_end + 4..];
    if header_end > 8192
        || body.len() >= MAX_MESSAGE_BYTES
        || !resp_bytes.starts_with(b"HTTP/1.1 200 OK\r\n")
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid relay response",
        ));
    }
    let value: serde_json::Value = serde_json::from_slice(body)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid relay JSON"))?;
    if !value.is_object() || !value["id"].is_string() || !value["ok"].is_boolean() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid relay envelope",
        ));
    }
    Ok(body.to_vec())
}

fn relay_failure(id: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "id": id, "ok": false,
        "error": {"code": "ERR_RELAY_FAILED", "message": "Native relay unavailable"}
    }))
    .expect("serializable relay failure")
}

pub fn check_service_health(port: u16) -> bool {
    let Ok(secret) = super::read_relay_secret() else {
        return false;
    };
    check_service_health_with_secret(port, &secret)
}

pub(crate) fn check_service_health_with_secret(port: u16, secret: &str) -> bool {
    if secret.is_empty() {
        return false;
    }
    let mut challenge = [0u8; 16];
    if !random_bytes(&mut challenge) {
        return false;
    }
    let nonce = base64url(&challenge);
    let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(1000)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(1000)));
    let req = format!("GET /native/ping HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Natives-Probe-Nonce: {nonce}\r\nConnection: close\r\n\r\n");
    if stream.write_all(req.as_bytes()).is_err() || stream.flush().is_err() {
        return false;
    }
    let mut resp = Vec::new();
    if stream.take(4096).read_to_end(&mut resp).is_ok() {
        let text = String::from_utf8_lossy(&resp);
        let expected = app_runtime_core::sha256_hex(format!("{secret}:{nonce}").as_bytes());
        text.starts_with("HTTP/1.1 200 OK")
            && text
                .split_once("\r\n\r\n")
                .is_some_and(|(_, body)| constant_time_eq(body, &expected))
    } else {
        false
    }
}

pub fn run_relay_loop(port: u16) -> io::Result<()> {
    let mut input = io::stdin().lock();
    let output = std::sync::Arc::new(std::sync::Mutex::new(io::stdout()));
    let mut owner_bytes = [0u8; 16];
    if !random_bytes(&mut owner_bytes) {
        return Err(io::Error::other(
            "OS randomness unavailable for Native Surface",
        ));
    }
    let surface_id = format!("native_{}", base64url(&owner_bytes));
    let _lease = RelayLease::start(port, &surface_id)?;
    let _events = super::relay_events::RelayEvents::start(port, &surface_id, output.clone())?;

    let pending = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    while let Some(body) = read_frame(&mut input) {
        let id = serde_json::from_slice::<Request>(&body)
            .map(|r| r.id)
            .unwrap_or_default();
        let forwarded = match serde_json::from_slice::<Request>(&body) {
            Ok(request) if request.method.starts_with("native:") => {
                let error = serde_json::json!({"id":request.id,"ok":false,"error":"ERR_METHOD_NOT_ALLOWED"});
                write_raw_frame(
                    &mut *output
                        .lock()
                        .map_err(|_| io::Error::other("relay output unavailable"))?,
                    &serde_json::to_vec(&error).map_err(io::Error::other)?,
                )?;
                continue;
            }
            Ok(mut request) if request.method.starts_with("app:") => {
                if let Some(params) = request.params.as_object_mut() {
                    params.insert("surfaceId".into(), surface_id.clone().into());
                }
                serde_json::to_vec(&request).map_err(io::Error::other)?
            }
            _ => body,
        };
        if pending.fetch_add(1, std::sync::atomic::Ordering::AcqRel) >= 16 {
            pending.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
            let body = serde_json::to_vec(
                &serde_json::json!({"id":id,"ok":false,"error":"ERR_REQUEST_LIMIT"}),
            )
            .map_err(io::Error::other)?;
            write_raw_frame(
                &mut *output
                    .lock()
                    .map_err(|_| io::Error::other("relay output unavailable"))?,
                &body,
            )?;
            continue;
        }
        let (output, pending, surface) = (output.clone(), pending.clone(), surface_id.clone());
        std::thread::spawn(move || {
            let response =
                send_relay_http_owned(port, &forwarded, Duration::from_secs(30), Some(&surface))
                    .unwrap_or_else(|_| relay_failure(&id));
            if let Ok(mut output) = output.lock() {
                let _ = write_raw_frame(&mut *output, &response);
            }
            pending.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_response_is_bounded_and_validated() {
        assert!(parse_relay_response(
            b"HTTP/1.1 200 OK\r\n\r\n{\"id\":\"x\",\"ok\":true}".to_vec()
        )
        .is_ok());
        assert!(parse_relay_response(b"HTTP/1.1 401 Unauthorized\r\n\r\n{}".to_vec()).is_err());
        assert!(parse_relay_response(b"HTTP/1.1 200 OK\r\n\r\n{}".to_vec()).is_err());
        assert!(write_raw_frame(&mut Vec::new(), &vec![0; MAX_MESSAGE_BYTES]).is_err());
        let failure: serde_json::Value =
            serde_json::from_slice(&relay_failure("quote\"\\id")).unwrap();
        assert_eq!(failure["id"], "quote\"\\id");
        assert_eq!(failure["error"]["code"], "ERR_RELAY_FAILED");
    }
}

struct RelayLease {
    port: u16,
    id: String,
    stop: std::sync::mpsc::Sender<()>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl RelayLease {
    fn message(id: &str, method: &str) -> Vec<u8> {
        serde_json::to_vec(
            &serde_json::json!({"id": id, "method": method, "params": {"surfaceId": id}}),
        )
        .unwrap()
    }

    fn start(port: u16, id: &str) -> io::Result<Self> {
        let body = Self::message(id, "native:lease");
        let response = send_relay_http_with_timeout(port, &body, Duration::from_secs(1))?;
        if serde_json::from_slice::<serde_json::Value>(&response)
            .ok()
            .and_then(|v| v["ok"].as_bool())
            != Some(true)
        {
            return Err(io::Error::other("Native Surface registration rejected"));
        }
        let (stop, receiver) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            while receiver.recv_timeout(Duration::from_secs(20))
                == Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            {
                let _ = send_relay_http_with_timeout(port, &body, Duration::from_secs(1));
            }
        });
        Ok(Self {
            port,
            id: id.into(),
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for RelayLease {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = send_relay_http_with_timeout(
            self.port,
            &Self::message(&self.id, "native:release"),
            Duration::from_secs(1),
        );
    }
}
