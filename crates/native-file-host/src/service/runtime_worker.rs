//! Bounded framing for a product-owned module process.
use super::app_runtime::resolve_runtime_binary;
use crate::protocol::Request;
use app_runtime_core::framing::{read_frame, write_frame};
use serde_json::Value;
use std::io;
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

type Frame = (Request, mpsc::Sender<Result<Value, String>>);

pub(super) struct AppRuntimeWorker {
    child: Arc<Mutex<Child>>,
    sender: Option<mpsc::SyncSender<Frame>>,
    io_thread: Option<JoinHandle<()>>,
}

impl AppRuntimeWorker {
    pub(super) fn spawn(app_id: &str, web_port: u16, origin: &str) -> io::Result<Self> {
        let mut child = Command::new(resolve_runtime_binary()?)
            .arg(origin)
            .env("NATIVES_MANAGED_APP_ID", app_id)
            .env("NATIVES_PARENT_WEB_PORT", web_port.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let Some(mut stdin) = child.stdin.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::other("Runtime stdin missing"));
        };
        let Some(mut stdout) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::other("Runtime stdout missing"));
        };
        let (sender, receiver) = mpsc::sync_channel::<Frame>(8);
        let io_thread = thread::spawn(move || {
            while let Ok((request, reply)) = receiver.recv() {
                let result = exchange(&request, &mut stdin, &mut stdout);
                let _ = reply.send(result);
            }
        });
        Ok(Self {
            child: Arc::new(Mutex::new(child)),
            sender: Some(sender),
            io_thread: Some(io_thread),
        })
    }

    pub(super) fn forward_frame(&self, request: &Request) -> Result<Value, String> {
        let (reply, receiver) = mpsc::channel();
        self.sender
            .as_ref()
            .ok_or("APP_RUNTIME_STOPPED")?
            .try_send((request.clone(), reply))
            .map_err(|_| "APP_RUNTIME_BUSY")?;
        match receiver.recv_timeout(Duration::from_secs(15)) {
            Ok(result) => result,
            Err(_) => {
                if let Ok(mut child) = self.child.lock() {
                    let _ = child.kill();
                }
                Err("APP_RUNTIME_TIMEOUT".into())
            }
        }
    }

    pub(super) fn is_alive(&self) -> bool {
        self.child
            .lock()
            .is_ok_and(|mut child| matches!(child.try_wait(), Ok(None)))
    }
}

fn exchange(
    request: &Request,
    stdin: &mut impl io::Write,
    stdout: &mut impl io::Read,
) -> Result<Value, String> {
    let body = serde_json::to_vec(request).map_err(|_| "APP_FRAME_INVALID")?;
    write_frame(stdin, &body).map_err(|_| "APP_RUNTIME_DISCONNECTED")?;
    // Runtime state events may precede the matching response, especially on stop.
    for _ in 0..32 {
        let body = read_frame(stdout)
            .map_err(|_| "APP_FRAME_INVALID")?
            .ok_or("APP_RUNTIME_DISCONNECTED")?;
        let response: Value = serde_json::from_slice(&body).map_err(|_| "APP_FRAME_INVALID")?;
        if response.get("event").is_some() {
            continue;
        }
        if response.get("id").and_then(Value::as_str) != Some(request.id.as_str()) {
            return Err("APP_FRAME_ID_MISMATCH".into());
        }
        if response["ok"].as_bool() == Some(true) {
            return Ok(response.get("result").cloned().unwrap_or(Value::Null));
        }
        return Err(response["error"]["code"]
            .as_str()
            .unwrap_or("APP_RUNTIME_FAILED")
            .to_string());
    }
    Err("APP_RUNTIME_EVENT_LIMIT".into())
}

impl Drop for AppRuntimeWorker {
    fn drop(&mut self) {
        self.sender.take();
        let deadline = Instant::now() + Duration::from_secs(2);
        while self.is_alive() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        if let Ok(mut child) = self.child.lock() {
            if matches!(child.try_wait(), Ok(None)) {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
        if let Some(thread) = self.io_thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn state_events_do_not_replace_a_matching_response() {
        let request = Request {
            id: "stop".into(),
            method: "app:stop".into(),
            params: json!({}),
        };
        let mut frames = Vec::new();
        write_frame(
            &mut frames,
            &serde_json::to_vec(&json!({"event":"app:state","data":{"state":"stopping"}})).unwrap(),
        )
        .unwrap();
        write_frame(
            &mut frames,
            &serde_json::to_vec(&json!({"id":"stop","ok":true,"result":{"stopped":true}})).unwrap(),
        )
        .unwrap();
        assert_eq!(
            exchange(&request, &mut Vec::new(), &mut frames.as_slice()).unwrap()["stopped"],
            true
        );
        let mut wrong = Vec::new();
        write_frame(
            &mut wrong,
            &serde_json::to_vec(&json!({"id":"other","ok":true})).unwrap(),
        )
        .unwrap();
        assert_eq!(
            exchange(&request, &mut Vec::new(), &mut wrong.as_slice()).unwrap_err(),
            "APP_FRAME_ID_MISMATCH"
        );
    }
}
