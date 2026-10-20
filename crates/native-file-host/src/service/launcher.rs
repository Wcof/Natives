//! Short-lived launcher handoff to the authenticated local Web surface.
use std::io;
use std::process::Command;

fn route(target: &str) -> io::Result<&'static str> {
    match target {
        "space" => Ok("space.html"),
        "files" => Ok("files.html"),
        "tokenusage" => Ok("app.html?app=tokenusage"),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unknown launcher target",
        )),
    }
}

pub fn open_web(target: &str) -> io::Result<()> {
    let path = route(target)?;
    let port = super::ensure_service(None)?;
    let body = serde_json::to_vec(
        &serde_json::json!({"id":"launcher-bootstrap","method":"native:bootstrap","params":{}}),
    )?;
    let response = super::relay::send_relay_http(port, &body)?;
    let response: serde_json::Value = serde_json::from_slice(&response)?;
    let token = response["result"]["bootstrapToken"]
        .as_str()
        .filter(|token| {
            token.len() == 43
                && token
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
        .ok_or_else(|| io::Error::other("Web session preparation failed"))?;
    let url = format!("http://127.0.0.1:{port}/{path}#token={token}");
    #[cfg(target_os = "macos")]
    let status = Command::new("/usr/bin/open")
        .args(["-a", "Google Chrome", &url])
        .status()?;
    #[cfg(target_os = "windows")]
    let status = Command::new("rundll32.exe")
        .args(["url.dll,FileProtocolHandler", &url])
        .status()?;
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let status = Command::new("xdg-open").arg(&url).status()?;
    if !status.success() {
        return Err(io::Error::other("browser handoff failed"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn launcher_only_opens_registered_product_routes() {
        assert_eq!(route("tokenusage").unwrap(), "app.html?app=tokenusage");
        assert_eq!(route("files").unwrap(), "files.html");
        assert_eq!(route("space").unwrap(), "space.html");
        for invalid in ["https://example.com", "../files.html", "space#token=x", ""] {
            assert!(route(invalid).is_err());
        }
    }
}
