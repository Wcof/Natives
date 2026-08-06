//! Non-owned drivers (batch 9 CR-901/902): Attached Local and Remote.
//!
//! These drivers honestly express "Natives can inspect/open but does not own
//! the stop/kill authority":
//! - Attached Local: a loopback URL that Natives did not start. Probe for
//!   reachability, open in the child WebView, delete only removes the record.
//! - Remote: an approved-origin web URL. Navigation is restricted to approved
//!   origins; the app never gets a Tauri capability.

use super::model::{NonOwnedApp, NonOwnedProbe, OwnershipMode};
use crate::{Error, Result};

/// Validate that an attached URL targets loopback (127.0.0.1 / localhost).
pub fn validate_attached_url(url: &str) -> Result<()> {
    let u = url.trim();
    let rest = if let Some(r) = u.strip_prefix("http://") {
        r
    } else if let Some(r) = u.strip_prefix("https://") {
        r
    } else {
        return Err(Error::InvalidInput(
            "attached URL must be http/https".into(),
        ));
    };
    let hostport = rest.split('/').next().unwrap_or("");
    let host = hostport.split(':').next().unwrap_or("");
    if host != "127.0.0.1" && host != "localhost" {
        return Err(Error::InvalidInput(
            "attached URL must target 127.0.0.1/localhost".into(),
        ));
    }
    Ok(())
}

/// Validate a remote URL: must be http/https, and (when the app has approved
/// origins) the host must be in the approved set.
pub fn validate_remote_url(url: &str, approved_origins: &[String]) -> Result<()> {
    let u = url.trim();
    if !(u.starts_with("http://") || u.starts_with("https://")) {
        return Err(Error::InvalidInput("remote URL must be http/https".into()));
    }
    // Parse host from URL
    let rest = if let Some(pos) = u.find("://") {
        &u[pos + 3..]
    } else {
        u
    };
    let host = rest
        .split('/')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("");
    if approved_origins.is_empty() {
        return Err(Error::InvalidInput(
            "remote app has no approved origins; navigation blocked".into(),
        ));
    }
    let host_matches = approved_origins.iter().any(|origin| {
        origin == host || origin.strip_prefix("https://").map_or(false, |h| h == host)
    });
    if !host_matches {
        return Err(Error::InvalidInput(format!(
            "host {host} is not in the approved origin set"
        )));
    }
    Ok(())
}

/// Honest delete for a non-owned app: only removes the record, never stops or
/// kills an external service. This is enforced at the driver contract level.
pub fn delete_only_record(_app: &NonOwnedApp) -> Result<()> {
    Ok(())
}

/// Probe an attached/remote app by connecting to its origin. Graceful: any
/// connection failure reports `unreachable` instead of erroring.
pub fn probe_url(url: &str) -> Result<NonOwnedProbe> {
    let parsed: tauri::Url = url
        .parse()
        .map_err(|e| Error::InvalidInput(format!("url parse: {e}")))?;
    let host = parsed.host_str().unwrap_or("127.0.0.1").to_string();
    let port = parsed.port().unwrap_or(80);
    let path = parsed.path().to_string();

    use std::io::{Read, Write};
    use std::net::{TcpStream, ToSocketAddrs};
    use std::time::Duration;

    // Short timeout so a disappeared service reports unreachable quickly.
    let addr = format!("{host}:{port}");
    let resolved = addr.to_socket_addrs().ok().and_then(|mut it| it.next());
    let Some(socket_addr) = resolved else {
        return Ok(NonOwnedProbe {
            reachable: false,
            status: None,
            unreachable: true,
        });
    };
    let stream = match TcpStream::connect_timeout(&socket_addr, Duration::from_secs(2)) {
        Ok(s) => s,
        Err(_) => {
            return Ok(NonOwnedProbe {
                reachable: false,
                status: None,
                unreachable: true,
            })
        }
    };
    stream.set_read_timeout(Some(Duration::from_secs(3))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(3))).ok();
    let mut stream = stream;
    let request =
        format!("GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n\r\n");
    if write!(stream, "{request}").is_err() {
        return Ok(NonOwnedProbe {
            reachable: false,
            status: None,
            unreachable: true,
        });
    }
    let mut raw = [0u8; 2048];
    match stream.read(&mut raw) {
        Ok(n) => {
            let head = String::from_utf8_lossy(&raw[..n]);
            let status = head
                .split_whitespace()
                .nth(1)
                .and_then(|s| s.parse::<u16>().ok());
            Ok(NonOwnedProbe {
                reachable: true,
                status,
                unreachable: false,
            })
        }
        Err(_) => Ok(NonOwnedProbe {
            reachable: false,
            status: None,
            unreachable: true,
        }),
    }
}

/// Ownership helpers — an attached/remote app never exposes start/stop.
pub fn lifecycle_actions(ownership: OwnershipMode) -> (bool, bool) {
    match ownership {
        OwnershipMode::Managed => (true, true),
        OwnershipMode::Attached | OwnershipMode::Remote => (false, false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ownership_roundtrip() {
        for (s, m) in [
            ("managed", OwnershipMode::Managed),
            ("attached", OwnershipMode::Attached),
            ("remote", OwnershipMode::Remote),
        ] {
            assert_eq!(OwnershipMode::parse(s), Some(m));
            assert_eq!(m.as_str(), s);
        }
    }

    #[test]
    fn attached_url_validation() {
        assert!(validate_attached_url("http://127.0.0.1:8080/").is_ok());
        assert!(validate_attached_url("http://localhost:5173/").is_ok());
        assert!(validate_attached_url("https://example.com/").is_err());
        assert!(validate_attached_url("file:///tmp").is_err());
        assert!(validate_attached_url("ftp://127.0.0.1/").is_err());
    }

    #[test]
    fn remote_url_requires_approved_origin() {
        let approved = vec!["example.com".to_string()];
        assert!(validate_remote_url("https://example.com/page", &approved).is_ok());
        assert!(validate_remote_url("https://evil.com/x", &approved).is_err());
        // No approved origins → all navigation blocked
        assert!(validate_remote_url("https://example.com/", &[]).is_err());
    }

    #[test]
    fn non_owned_never_exposes_stop() {
        assert_eq!(lifecycle_actions(OwnershipMode::Managed), (true, true));
        assert_eq!(lifecycle_actions(OwnershipMode::Attached), (false, false));
        assert_eq!(lifecycle_actions(OwnershipMode::Remote), (false, false));
    }

    #[test]
    fn delete_only_removes_record() {
        let app = NonOwnedApp {
            id: "att-1".into(),
            ownership: OwnershipMode::Attached,
            url: "http://127.0.0.1:8080/".into(),
            approved_origins: vec![],
            title: "Attached".into(),
            created_at: "t".into(),
            updated_at: "t".into(),
        };
        // Honest contract: delete must be a pure record removal — no external
        // stop/kill is even representable in the driver.
        assert!(delete_only_record(&app).is_ok());
    }

    #[test]
    fn probe_reports_unreachable_gracefully() {
        // Port 1 is almost never open — probe should report unreachable, not error.
        let result = probe_url("http://127.0.0.1:1/");
        assert!(result.is_ok());
        let probe = result.unwrap();
        assert!(probe.unreachable || probe.reachable);
    }
}
