//! Download destination policy for the Embed surface (T08).
//!
//! Downloads from an Embed child webview are granted (one-time atomic or
//! persistent) and land in a Host-chosen directory with a sanitized filename:
//!
//! - if the `download` grant carries a path scope, that directory is used;
//! - otherwise the Host-managed per-app directory `~/.natives/downloads/{app}`
//!   is used (R-D1 convention root);
//! - the filename is derived from the URL path / suggested name and sanitized
//!   against path traversal and control characters.
//!
//! The Embed webview never picks the destination itself.

use super::grant_store;
use super::model::AppGrant;
use rusqlite::Connection;
use std::path::PathBuf;

/// Strip path separators and traversal from a suggested download name.
pub fn sanitize_filename(name: &str) -> String {
    let base = name.replace('\\', "/");
    let base = base.rsplit('/').next().unwrap_or("");
    let cleaned: String = base.chars().filter(|c| *c != '\0').collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        "download".to_string()
    } else {
        cleaned.chars().take(200).collect()
    }
}

/// Host-chosen per-app download directory under the convention root.
pub fn default_download_dir(app_id: &str) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let safe: String = app_id
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .take(48)
        .collect();
    let dir = if safe.is_empty() {
        "app".to_string()
    } else {
        safe
    };
    Some(home.join(".natives").join("downloads").join(dir))
}

/// Resolve the destination for a download: Host-chosen directory (grant path
/// scope, else the managed per-app dir) plus a sanitized filename.
pub fn safe_download_destination(
    conn: &Connection,
    app_id: &str,
    url: &tauri::Url,
    suggested: &std::path::Path,
) -> Option<PathBuf> {
    download_destination_in(conn, app_id, url, suggested, None)
}

/// [`safe_download_destination`] with an injectable managed-dir base so tests
/// never touch the real convention root (`~/.natives`).
fn download_destination_in(
    conn: &Connection,
    app_id: &str,
    url: &tauri::Url,
    suggested: &std::path::Path,
    managed_base: Option<&std::path::Path>,
) -> Option<PathBuf> {
    let dir = match grant_store::grant_scope(conn, app_id, AppGrant::KIND_DOWNLOAD)
        .ok()
        .flatten()
    {
        Some(scope) if !scope.trim().is_empty() => PathBuf::from(scope),
        _ => managed_base
            .map(std::path::Path::to_path_buf)
            .or_else(|| default_download_dir(app_id))?,
    };
    std::fs::create_dir_all(&dir).ok()?;

    let name = url
        .path_segments()
        .and_then(|segments| segments.last())
        .filter(|s| !s.is_empty())
        .map(sanitize_filename)
        .or_else(|| {
            suggested
                .file_name()
                .and_then(|n| n.to_str())
                .map(sanitize_filename)
        })
        .unwrap_or_else(|| "download".to_string());

    Some(dir.join(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use std::path::Path;

    fn fixture() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        db::create_tables(&conn).unwrap();
        db::apply_migrations(&conn).unwrap();
        conn
    }

    fn ensure_app(conn: &Connection, id: &str) {
        conn.execute(
            "INSERT OR IGNORE INTO applications (id, source, source_id, title, version, created_at, updated_at)
             VALUES (?1, 'local_project', ?1, 'Test', '1', 't', 't')",
            rusqlite::params![id],
        )
        .unwrap();
    }

    #[test]
    fn sanitize_filename_blocks_traversal() {
        assert_eq!(sanitize_filename("report.pdf"), "report.pdf");
        assert_eq!(sanitize_filename("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_filename("..\\..\\evil.txt"), "evil.txt");
        assert_eq!(sanitize_filename("a/b/../c"), "c");
        assert_eq!(sanitize_filename(".."), "download");
        assert_eq!(sanitize_filename("."), "download");
        assert_eq!(sanitize_filename(""), "download");
        assert_eq!(sanitize_filename("note\x00v1"), "notev1");
    }

    #[test]
    fn download_uses_managed_dir_when_no_scope() {
        let conn = fixture();
        ensure_app(&conn, "app-dl");
        let base = tempfile::tempdir().unwrap();
        let url: tauri::Url = "http://127.0.0.1:8080/files/report.pdf".parse().unwrap();
        let dest = download_destination_in(
            &conn,
            "app-dl",
            &url,
            Path::new("report.pdf"),
            Some(base.path()),
        )
        .unwrap();
        assert!(dest.ends_with("report.pdf"));
        assert_eq!(
            dest.parent().unwrap(),
            base.path(),
            "Host-managed dir must be the injected base"
        );
    }

    #[test]
    fn download_respects_grant_scope() {
        let conn = fixture();
        ensure_app(&conn, "app-dl");
        let scope = tempfile::tempdir().unwrap();
        grant_store::set_grant(
            &conn,
            "app-dl",
            AppGrant::KIND_DOWNLOAD,
            AppGrant::POLICY_PERSISTENT,
            scope.path().to_str(),
        )
        .unwrap();
        let url: tauri::Url = "http://127.0.0.1:8080/files/report.pdf".parse().unwrap();
        let dest =
            safe_download_destination(&conn, "app-dl", &url, Path::new("report.pdf")).unwrap();
        assert_eq!(dest.parent().unwrap(), scope.path());
    }

    #[test]
    fn download_sanitizes_url_filename() {
        let conn = fixture();
        ensure_app(&conn, "app-dl");
        let base = tempfile::tempdir().unwrap();
        let url: tauri::Url = "http://127.0.0.1:8080/../../etc/passwd".parse().unwrap();
        let dest = download_destination_in(
            &conn,
            "app-dl",
            &url,
            Path::new("fallback.bin"),
            Some(base.path()),
        )
        .unwrap();
        let name = dest.file_name().unwrap().to_string_lossy().to_string();
        assert_eq!(name, "passwd");
    }
}
