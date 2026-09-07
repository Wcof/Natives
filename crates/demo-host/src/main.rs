//! Minimal demo Native Messaging host (ADR-0025 Phase A5/A6).
//!
//! Two modes:
//! - `demo-host --health`: install-time probe. Prints
//!   `{"status":"ok","version":"1.0.0"}` and exits 0 (the Core Host's
//!   health_check stage spawns exactly this).
//! - `demo-host` (no args): Chrome Native Messaging loop over stdio.
//!   Chrome starts it on connect; stdin EOF exits the process immediately
//!   (≤2 s shutdown budget — there is no background work to wait for).

use serde_json::Value;
use std::io::{self, Read, Write};

const VERSION: &str = "1.0.0";

fn main() {
    if std::env::args().any(|arg| arg == "--health") {
        println!(
            "{}",
            serde_json::json!({ "status": "ok", "version": VERSION, "app": "demo" })
        );
        return;
    }
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();
    loop {
        match read_frame(&mut input) {
            Some(body) => {
                let response = handle(&body);
                if write_frame(&mut output, &response).is_err() {
                    break;
                }
            }
            None => break, // stdin EOF: Chrome closed the port → exit.
        }
    }
}

fn handle(body: &[u8]) -> Vec<u8> {
    let request: Value = match serde_json::from_slice(body) {
        Ok(value) => value,
        Err(error) => {
            return serde_json::to_vec(&serde_json::json!({
                "id": "",
                "ok": false,
                "error": format!("invalid request: {error}"),
            }))
            .expect("serialize error response")
        }
    };
    let id = request
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let result = match method {
        "ping" => serde_json::json!({ "pong": true }),
        "version" => serde_json::json!({ "version": VERSION, "app": "demo" }),
        "health" => serde_json::json!({ "status": "ok", "version": VERSION }),
        other => {
            return serde_json::to_vec(&serde_json::json!({
                "id": id,
                "ok": false,
                "error": format!("unsupported method: {other}"),
            }))
            .expect("serialize error response")
        }
    };
    serde_json::to_vec(&serde_json::json!({ "id": id, "ok": true, "result": result }))
        .expect("serialize response")
}

fn read_frame(input: &mut impl Read) -> Option<Vec<u8>> {
    let mut header = [0u8; 4];
    input.read_exact(&mut header).ok()?;
    let len = u32::from_ne_bytes(header) as usize;
    if len > 1024 * 1024 {
        return None;
    }
    let mut body = vec![0u8; len];
    input.read_exact(&mut body).ok()?;
    Some(body)
}

fn write_frame(output: &mut impl Write, body: &[u8]) -> io::Result<()> {
    output.write_all(&(body.len() as u32).to_ne_bytes())?;
    output.write_all(body)?;
    output.flush()
}
