//! 本机服务的有界 SSE 连接与失效事件。

use super::http_helpers::{extract_cookie_value, validate_origin, write_http_raw};
use super::server::ServiceState;
use app_runtime_core::http::HttpRequest;
use app_runtime_core::session::constant_time_eq;
use serde_json::json;
use std::io::Write;
use std::net::TcpStream;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::Duration;

fn surface_query(req: &HttpRequest) -> Option<String> {
    let path = req.path.split('?').nth(1)?;
    path.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == "surface" && !value.is_empty()).then(|| value.to_string())
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn another_web_session_cannot_subscribe_to_owned_task_progress() {
        use std::io::{Read, Write};
        use std::net::{TcpListener, TcpStream};
        use std::sync::atomic::Ordering;
        use std::time::Duration;

        let state = super::super::tests::make_test_state(0);
        let (other, surface) = {
            let mut auth = state.auth.lock().unwrap();
            let (owner, _) = auth.exchange_bootstrap("test-bootstrap").unwrap();
            let surface = auth.register_surface(&owner, None).unwrap();
            let bootstrap = auth.generate_bootstrap_token().unwrap();
            let (other, _) = auth.exchange_bootstrap(&bootstrap).unwrap();
            (other, surface)
        };
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let owned_state = state.clone();
        let handler = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            super::super::server::handle_client(stream, owned_state);
        });
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        write!(client, "GET /web/events?surface={surface} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nCookie: natives_dev_session={other}\r\n\r\n").unwrap();
        let mut buffer = [0; 512];
        let size = client.read(&mut buffer).unwrap();
        state.shutdown_signal.store(true, Ordering::SeqCst);
        handler.join().unwrap();
        assert!(String::from_utf8_lossy(&buffer[..size]).starts_with("HTTP/1.1 403"));
    }
}

pub(super) fn handle_events_sse(stream: &mut TcpStream, state: &ServiceState, req: &HttpRequest) {
    state.touch_activity();
    let cookie_name = if cfg!(debug_assertions) {
        "natives_dev_session"
    } else {
        "natives_session"
    };
    let session_id = extract_cookie_value(req, cookie_name).unwrap_or("");
    let native = req.path.split('?').next() == Some("/native/events");
    let native_id = req.header("x-natives-surface-id").unwrap_or("");
    let valid_native = || {
        state
            .native_surfaces
            .lock()
            .is_ok_and(|leases| leases.contains_key(native_id))
    };
    if (native
        && (req.header("origin").is_some()
            || !constant_time_eq(
                req.header("x-natives-relay-secret").unwrap_or(""),
                &state.relay_secret,
            )
            || !valid_native()))
        || (!native && !state.auth.lock().unwrap().validate_session(session_id))
    {
        let _ = write_http_raw(
            stream,
            401,
            "Unauthorized",
            "text/plain",
            &[],
            b"unauthorized SSE",
        );
        return;
    }

    if req
        .header("origin")
        .is_some_and(|_| !validate_origin(req, state.port))
    {
        let _ = write_http_raw(
            stream,
            403,
            "Forbidden",
            "text/plain",
            &[],
            b"invalid origin",
        );
        return;
    }
    let sse_surface = if native {
        Some(native_id.to_string())
    } else {
        let surface = surface_query(req);
        if let Some(id) = surface.as_deref() {
            if !state.auth.lock().unwrap().owns_surface(session_id, id) {
                let _ = write_http_raw(
                    stream,
                    403,
                    "Forbidden",
                    "text/plain",
                    &[],
                    b"unknown surface",
                );
                return;
            }
        }
        surface
    };
    let (tx, rx) = mpsc::sync_channel(256);
    let sse_id = state.next_sse_id.fetch_add(1, Ordering::Relaxed);
    let sequence = {
        let mut senders = state.sse_senders.lock().unwrap();
        if senders.len() >= 64 {
            let _ = write_http_raw(
                stream,
                503,
                "Service Unavailable",
                "text/plain",
                &[],
                b"too many event streams",
            );
            return;
        }
        senders.push((sse_id, tx, sse_surface.clone()));
        state.event_sequence.load(Ordering::Relaxed)
    };

    let headers = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache, no-transform\r\nConnection: keep-alive\r\n\r\n";
    if stream.write_all(headers.as_bytes()).is_err() || stream.flush().is_err() {
        state
            .sse_senders
            .lock()
            .unwrap()
            .retain(|(id, _, _)| *id != sse_id);
        return;
    }

    let initial = format!(
        "data: {}\n\n",
        json!({ "type": "connected", "domain":"core", "epoch": state.epoch, "sequence":sequence, "revision":null })
    );
    if stream.write_all(initial.as_bytes()).is_err() || stream.flush().is_err() {
        state
            .sse_senders
            .lock()
            .unwrap()
            .retain(|(id, _, _)| *id != sse_id);
        return;
    }

    // SSE 保活与事件分发循环（F05）
    while !state.shutdown_signal.load(Ordering::SeqCst) {
        // Do not extend session activity just because an SSE socket is open.
        let valid = if native {
            valid_native()
        } else {
            let mut auth = state.auth.lock().unwrap();
            auth.reap_expired();
            auth.get_csrf_token(session_id).is_some()
                && sse_surface
                    .as_deref()
                    .is_none_or(|id| auth.owns_active_surface(session_id, id))
        };
        if !valid {
            break;
        }
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(msg) => {
                let frame = format!("data: {msg}\n\n");
                if stream.write_all(frame.as_bytes()).is_err() || stream.flush().is_err() {
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if stream.write_all(b": keepalive\n\n").is_err() || stream.flush().is_err() {
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    state
        .sse_senders
        .lock()
        .unwrap()
        .retain(|(id, _, _)| *id != sse_id);
}
