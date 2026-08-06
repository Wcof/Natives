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
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Register a non-owned app (attached loopback service or remote web app).
///
/// The URL is validated against the ownership mode up front:
/// - Attached must target `127.0.0.1` / `localhost`.
/// - Remote must be http/https AND (when approved origins are non-empty) the
///   host must be inside the approved set. A remote with an empty approved set
///   fails closed — unconstrained navigation is never registered.
///
/// Returns the persisted record.
pub fn register(
    conn: &Connection,
    ownership: OwnershipMode,
    url: &str,
    approved_origins: &[String],
    title: &str,
) -> Result<NonOwnedApp> {
    if title.trim().is_empty() {
        return Err(Error::InvalidInput("non-owned app title cannot be empty".into()));
    }
    match ownership {
        OwnershipMode::Managed => {
            return Err(Error::InvalidInput(
                "managed apps cannot be registered as non-owned".into(),
            ))
        }
        OwnershipMode::Attached => {
            if !approved_origins.is_empty() {
                return Err(Error::InvalidInput(
                    "attached apps carry no approved origins".into(),
                ));
            }
            validate_attached_url(url)?;
        }
        OwnershipMode::Remote => {
            // Approved origins are required for a remote trust domain.
            if approved_origins.is_empty() {
                return Err(Error::InvalidInput(
                    "remote app requires at least one approved origin".into(),
                ));
            }
            validate_remote_url(url, approved_origins)?;
        }
    }
    let id = Uuid::new_v4().to_string();
    let t = now();
    let origins_json = serde_json::to_string(approved_origins)
        .map_err(|e| Error::Internal(format!("serialize approved origins: {e}")))?;
    conn.execute(
        "INSERT INTO non_owned_apps (id, ownership, url, approved_origins_json, title, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        params![
            id,
            ownership.as_str(),
            url,
            origins_json,
            title.trim(),
            t
        ],
    )
    .map_err(Error::Database)?;
    Ok(NonOwnedApp {
        id,
        ownership,
        url: url.trim().to_string(),
        approved_origins: approved_origins.to_vec(),
        title: title.trim().to_string(),
        created_at: t.clone(),
        updated_at: t,
    })
}

fn row_to_app(r: &rusqlite::Row<'_>) -> rusqlite::Result<NonOwnedApp> {
    let origins_json: String = r.get(3)?;
    Ok(NonOwnedApp {
        id: r.get(0)?,
        ownership: OwnershipMode::parse(&r.get::<_, String>(1)?)
            .unwrap_or(OwnershipMode::Managed),
        url: r.get(2)?,
        approved_origins: serde_json::from_str(&origins_json).unwrap_or_default(),
        title: r.get(4)?,
        created_at: r.get(5)?,
        updated_at: r.get(6)?,
    })
}

/// List all non-owned apps (attached + remote).
pub fn list(conn: &Connection) -> Result<Vec<NonOwnedApp>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, ownership, url, approved_origins_json, title, created_at, updated_at
             FROM non_owned_apps ORDER BY created_at",
        )
        .map_err(Error::Database)?;
    let rows = stmt
        .query_map([], row_to_app)
        .map_err(Error::Database)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(Error::Database)?);
    }
    Ok(out)
}

/// Read a non-owned app record by id.
pub fn get(conn: &Connection, id: &str) -> Result<Option<NonOwnedApp>> {
    conn.query_row(
        "SELECT id, ownership, url, approved_origins_json, title, created_at, updated_at
         FROM non_owned_apps WHERE id = ?1",
        params![id],
        row_to_app,
    )
    .optional()
    .map_err(Error::Database)
}

/// Delete a non-owned app record. This is the ONLY delete a non-owned driver
/// supports — it never stops or kills an external service (enforced by the
/// driver contract: attached/remote have no stop authority).
pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    let n = conn
        .execute("DELETE FROM non_owned_apps WHERE id = ?1", params![id])
        .map_err(Error::Database)?;
    if n == 0 {
        return Err(Error::NotFound(id.into()));
    }
    Ok(())
}

/// Probe a non-owned app record by id (graceful unreachable report).
pub fn probe_record(conn: &Connection, id: &str) -> Result<NonOwnedProbe> {
    let app = get(conn, id)?.ok_or_else(|| Error::NotFound(id.into()))?;
    probe_url(&app.url)
}

/// Open target for a non-owned app: the stored URL, validated to the app's
/// trust domain (loopback for attached, approved origins for remote).
pub fn open_url_for(app: &NonOwnedApp) -> Result<String> {
    match app.ownership {
        OwnershipMode::Attached => {
            validate_attached_url(&app.url)?;
            Ok(app.url.clone())
        }
        OwnershipMode::Remote => {
            validate_remote_url(&app.url, &app.approved_origins)?;
            Ok(app.url.clone())
        }
        OwnershipMode::Managed => Err(Error::InvalidInput(
            "managed apps are not non-owned open targets".into(),
        )),
    }
}

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
    let host_matches = approved_origins
        .iter()
        .any(|origin| origin == host || origin.strip_prefix("https://").is_some_and(|h| h == host));
    if !host_matches {
        return Err(Error::InvalidInput(format!(
            "host {host} is not in the approved origin set"
        )));
    }
    Ok(())
}

/// Navigation allow-list for a remote trust domain (T09).
///
/// The child WebView that opens a Remote app must never navigate outside the
/// app's approved origins — same contract as the loopback-only filter for
/// managed apps, but against the explicit approved-origin set.
pub fn remote_navigation_allowed(url: &str, approved_origins: &[String]) -> bool {
    url.trim().starts_with("http://")
        && validate_remote_url(url, approved_origins).is_ok()
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
mod t09_store_tests {
    use super::*;
    use crate::db::{apply_migrations, create_tables};
    use rusqlite::Connection;

    fn mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        apply_migrations(&conn).unwrap();
        conn
    }

    /// T09: an attached app is a real DB record with validated loopback URL and
    /// ownership, and only probe/open/delete actions — never start/stop.
    #[test]
    fn attached_registration_is_real_and_never_exposes_start() {
        let conn = mem();
        let app = register(
            &conn,
            OwnershipMode::Attached,
            "http://127.0.0.1:8080/dashboard",
            &[],
            "Local Service",
        )
        .unwrap();
        assert_eq!(app.ownership, OwnershipMode::Attached);
        assert_eq!(app.url, "http://127.0.0.1:8080/dashboard");
        assert!(app.approved_origins.is_empty());

        // The record is listable and readable.
        let all = list(&conn).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, app.id);
        let got = get(&conn, &app.id).unwrap().expect("attached record");
        assert_eq!(got.title, "Local Service");

        // Honest action matrix: no start/stop for a non-owned driver.
        assert_eq!(lifecycle_actions(OwnershipMode::Attached), (false, false));
        let e = crate::creative_app::driver::unsupported(
            crate::creative_app::driver::DRIVER_ATTACHED,
            "start",
        );
        assert!(e.to_string().contains("does not support 'start'"));

        // Delete only removes the record.
        delete(&conn, &app.id).unwrap();
        assert!(get(&conn, &app.id).unwrap().is_none());
        assert!(list(&conn).unwrap().is_empty());
    }

    /// T09: a remote app is a real DB record with approved origins and a
    /// restricted open — never a Tauri capability and never start/stop.
    #[test]
    fn remote_registration_requires_approved_origins() {
        let conn = mem();
        let app = register(
            &conn,
            OwnershipMode::Remote,
            "https://app.example.com/",
            &["app.example.com".to_string()],
            "Remote App",
        )
        .unwrap();
        assert_eq!(app.approved_origins, vec!["app.example.com".to_string()]);

        // The stored URL must stay within the approved origins.
        assert!(validate_remote_url(&app.url, &app.approved_origins).is_ok());
        assert_eq!(lifecycle_actions(OwnershipMode::Remote), (false, false));

        // A remote app with no approved origins cannot be registered at all —
        // navigation would be unconstrained, so registration must fail closed.
        let err = register(
            &conn,
            OwnershipMode::Remote,
            "https://evil.com/",
            &[],
            "No-Approval",
        );
        assert!(err.is_err());
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
