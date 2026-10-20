//! One bounded event stream owned by a Native Relay; EOF closes only its socket.
use super::relay::write_raw_frame;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

pub(super) struct RelayEvents {
    socket: TcpStream,
    worker: Option<JoinHandle<()>>,
}

impl RelayEvents {
    pub fn start(port: u16, surface: &str, output: Arc<Mutex<io::Stdout>>) -> io::Result<Self> {
        let secret = super::read_relay_secret()?;
        let mut socket =
            TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_secs(1))?;
        socket.set_read_timeout(Some(Duration::from_secs(2)))?;
        socket.set_write_timeout(Some(Duration::from_secs(1)))?;
        write!(socket,"GET /native/events HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Natives-Relay-Secret: {secret}\r\nX-Natives-Surface-Id: {surface}\r\n\r\n")?;
        let owned = socket.try_clone()?;
        let worker = std::thread::spawn(move || {
            let mut reader = BufReader::new(owned);
            let mut line = Vec::new();
            let mut headers = true;
            let mut count = 0;
            loop {
                line.clear();
                if (&mut reader)
                    .take(4097)
                    .read_until(b'\n', &mut line)
                    .ok()
                    .filter(|n| *n > 0 && *n <= 4096)
                    .is_none()
                {
                    break;
                }
                if headers {
                    count += line.len();
                    if count > 8192
                        || (count == line.len() && !line.starts_with(b"HTTP/1.1 200 OK"))
                    {
                        break;
                    }
                    if line == b"\r\n" {
                        headers = false;
                    }
                    continue;
                }
                if let Some(body) = line.strip_prefix(b"data: ") {
                    if serde_json::from_slice::<serde_json::Value>(body).is_err() {
                        break;
                    }
                    let Ok(mut writer) = output.lock() else {
                        break;
                    };
                    if write_raw_frame(&mut *writer, body).is_err() {
                        break;
                    }
                }
            }
            // Let the page's bounded reconnect policy own recovery, including a
            // Core restart. A dead event socket must not leave a quiet Port live.
            if let Ok(mut writer) = output.lock() {
                let _ = write_raw_frame(
                    &mut *writer,
                    br#"{"type":"resync","domain":"core","reason":"unavailable"}"#,
                );
            }
        });
        Ok(Self {
            socket,
            worker: Some(worker),
        })
    }
}

impl Drop for RelayEvents {
    fn drop(&mut self) {
        let _ = self.socket.shutdown(Shutdown::Both);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
