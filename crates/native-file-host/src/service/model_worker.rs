//! A Surface-owned thin Model client. The existing Go Hub remains the sole AI writer.
use crate::protocol::{read_frame, Request, MAX_MESSAGE_BYTES};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::io::{self, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

type Reply = (String, mpsc::Sender<Result<Value, String>>);
type Pending = Arc<Mutex<HashMap<String, Reply>>>;

pub(super) struct ModelWorker {
    child: Mutex<Child>,
    input: Option<mpsc::SyncSender<Request>>,
    threads: Vec<JoinHandle<()>>,
    pending: Pending,
    oauth: Arc<Mutex<VecDeque<String>>>,
    events: Arc<Mutex<VecDeque<Value>>>,
}

impl ModelWorker {
    pub fn spawn(changed: Arc<AtomicBool>) -> io::Result<Self> {
        let mut child = Command::new(super::model_host::resolve_model_host_binary()?)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let (Some(mut stdin), Some(mut stdout)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::other("Model pipes unavailable"));
        };
        let pending: Pending = Default::default();
        let oauth: Arc<Mutex<VecDeque<String>>> = Default::default();
        let events: Arc<Mutex<VecDeque<Value>>> = Default::default();
        let (input, receiver) = mpsc::sync_channel::<Request>(16);
        let write_pending = pending.clone();
        let writer = thread::spawn(move || {
            while let Ok(request) = receiver.recv() {
                let result = serde_json::to_vec(&request)
                    .ok()
                    .filter(|b| b.len() < MAX_MESSAGE_BYTES)
                    .ok_or_else(|| io::Error::other("invalid Model frame"))
                    .and_then(|body| {
                        stdin
                            .write_all(&(body.len() as u32).to_le_bytes())
                            .and_then(|_| stdin.write_all(&body))
                            .and_then(|_| stdin.flush())
                    });
                if result.is_err() {
                    fail_pending(&write_pending);
                    break;
                }
            }
        });
        let (read_pending, read_oauth, read_events) =
            (pending.clone(), oauth.clone(), events.clone());
        let reader = thread::spawn(move || {
            while let Some(body) = read_frame(&mut stdout) {
                if body.len() >= MAX_MESSAGE_BYTES {
                    break;
                }
                let Ok(mut frame) = serde_json::from_slice::<Value>(&body) else {
                    break;
                };
                if frame["event"].is_string() {
                    changed.store(true, Ordering::Release);
                    if frame["event"] == "model_oauth_state_changed" {
                        if let Some(result) = frame["result"].as_object_mut() {
                            result.remove("snapshot");
                        }
                        if frame.to_string().len() <= 4096 {
                            if let Ok(mut events) = read_events.lock() {
                                if events.len() >= 32 {
                                    events.pop_front();
                                }
                                events.push_back(frame);
                            }
                        }
                    }
                    continue;
                }
                let Some(id) = frame["id"].as_str() else {
                    break;
                };
                let reply = read_pending
                    .lock()
                    .ok()
                    .and_then(|mut pending| pending.remove(id));
                let Some((method, reply)) = reply else {
                    break;
                };
                if frame["ok"] == true {
                    if matches!(
                        method.as_str(),
                        "model_oauth_start" | "model_account_reauth"
                    ) {
                        if let Some(id) = frame["result"]["sessionId"].as_str() {
                            if let Ok(mut sessions) = read_oauth.lock() {
                                if sessions.len() >= 32 {
                                    sessions.pop_front();
                                }
                                sessions.push_back(id.into());
                            }
                        }
                    }
                    let _ = reply.send(Ok(frame["result"].clone()));
                } else {
                    let _ = reply.send(Err(frame["errorCode"]
                        .as_str()
                        .or(frame["error"].as_str())
                        .unwrap_or("MODEL_REQUEST_FAILED")
                        .into()));
                }
            }
            fail_pending(&read_pending);
        });
        Ok(Self {
            child: Mutex::new(child),
            input: Some(input),
            threads: vec![writer, reader],
            pending,
            oauth,
            events,
        })
    }

    pub fn call(&self, request: &Request) -> Result<Value, String> {
        let (reply, receiver) = mpsc::channel();
        {
            let mut pending = self.pending.lock().map_err(|_| "MODEL_HOST_UNAVAILABLE")?;
            if pending.len() >= 16 || pending.contains_key(&request.id) {
                return Err("MODEL_REQUEST_LIMIT".into());
            }
            pending.insert(request.id.clone(), (request.method.clone(), reply));
        }
        if self
            .input
            .as_ref()
            .ok_or("MODEL_HOST_UNAVAILABLE")?
            .try_send(request.clone())
            .is_err()
        {
            if let Ok(mut pending) = self.pending.lock() {
                pending.remove(&request.id);
            }
            return Err("MODEL_REQUEST_LIMIT".into());
        }
        match receiver.recv_timeout(Duration::from_secs(30)) {
            Ok(result) => result,
            Err(_) => {
                self.stop();
                Err("MODEL_REQUEST_TIMEOUT".into())
            }
        }
    }

    pub fn owns_oauth(&self, id: &str) -> bool {
        self.oauth
            .lock()
            .is_ok_and(|sessions| sessions.iter().any(|s| s == id))
    }
    pub fn is_alive(&self) -> bool {
        self.child
            .lock()
            .is_ok_and(|mut child| matches!(child.try_wait(), Ok(None)))
    }
    pub fn events(&self) -> Value {
        let events = self
            .events
            .lock()
            .map(|events| {
                events
                    .iter()
                    .filter(|event| {
                        self.owns_oauth(event["result"]["sessionId"].as_str().unwrap_or(""))
                    })
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        json!({"events":events})
    }
    pub fn stop(&self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
        }
        fail_pending(&self.pending);
    }
}

fn fail_pending(pending: &Pending) {
    if let Ok(mut pending) = pending.lock() {
        for (_, (_, reply)) in pending.drain() {
            let _ = reply.send(Err("MODEL_HOST_DISCONNECTED".into()));
        }
    }
}

impl Drop for ModelWorker {
    fn drop(&mut self) {
        self.input.take();
        let deadline = Instant::now() + Duration::from_secs(1);
        if let Ok(mut child) = self.child.lock() {
            while matches!(child.try_wait(), Ok(None)) && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(5));
            }
            if matches!(child.try_wait(), Ok(None)) {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}
