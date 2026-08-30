//! OS integration: default quick-access roots, open-with, and clipboard
//! copy (macOS / Linux / Windows).

use super::*;
use crate::{Error, Result};
use std::path::PathBuf;

/// Default quick-access roots (fanbox `/api/roots` equivalent).
pub fn default_roots() -> Result<Vec<serde_json::Value>> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    let candidates: Vec<(&str, PathBuf)> = vec![
        ("home", home.clone()),
        ("desktop", home.join("Desktop")),
        ("documents", home.join("Documents")),
        ("downloads", home.join("Downloads")),
        ("pictures", home.join("Pictures")),
        ("projects", home.join("Projects")),
        ("movies", home.join("Movies")),
        ("music", home.join("Music")),
    ];

    let mut roots = Vec::new();
    for (id, path) in candidates {
        // macOS asks for TCC access even for an existence probe. Show standard
        // home roots now and let the user's first navigation request access.
        if is_macos_privacy_protected_home_child(&path) || path.is_dir() {
            roots.push(serde_json::json!({
                "id": id,
                "name": match id {
                    "home" => "Home",
                    "desktop" => "Desktop",
                    "documents" => "Documents",
                    "downloads" => "Downloads",
                    "pictures" => "Pictures",
                    "projects" => "Projects",
                    "movies" => "Movies",
                    "music" => "Music",
                    _ => id,
                },
                "path": path.to_string_lossy(),
            }));
        }
    }
    // Always include /tmp if present
    for tmp in ["/tmp", "/private/tmp"] {
        let p = PathBuf::from(tmp);
        if p.is_dir() {
            roots.push(serde_json::json!({
                "id": "tmp",
                "name": "tmp",
                "path": p.to_string_lossy(),
            }));
            break;
        }
    }
    Ok(roots)
}

/// Open the platform's own Trash/Recycle Bin without exposing its contents to the page.
pub fn open_trash() -> Result<serde_json::Value> {
    #[cfg(target_os = "macos")]
    {
        let trash = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("/"))
            .join(".Trash");
        std::process::Command::new("open")
            .arg(trash)
            .spawn()
            .map_err(|e| Error::Internal(e.to_string()))?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer.exe")
            .arg("shell:RecycleBinFolder")
            .spawn()
            .map_err(|e| Error::Internal(e.to_string()))?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if std::process::Command::new("gio")
            .args(["open", "trash:///"])
            .spawn()
            .is_err()
        {
            open::that("trash:///").map_err(|e| Error::Internal(e.to_string()))?;
        }
    }
    Ok(serde_json::json!({ "ok": true, "with": "trash" }))
}

/// Open path with a preferred app (fanbox `/api/open`).
/// `with`: "default" | "reveal" | "editor"
pub fn open_with(target: &str, with: &str) -> Result<serde_json::Value> {
    let path = expand_tilde(target);
    if !path.exists() {
        return Err(Error::NotFound(target.to_string()));
    }
    // Enforce allowlist on the resolved target so open/reveal/editor
    // can't act on blocklisted/out-of-scope paths.
    let canon = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
    validate_path(&canon)?;

    match with {
        "reveal" => {
            #[cfg(target_os = "macos")]
            {
                std::process::Command::new("open")
                    .args(["-R", path.to_string_lossy().as_ref()])
                    .spawn()
                    .map_err(|e| Error::Internal(e.to_string()))?;
            }
            #[cfg(target_os = "windows")]
            {
                std::process::Command::new("explorer")
                    .args(["/select,", path.to_string_lossy().as_ref()])
                    .spawn()
                    .map_err(|e| Error::Internal(e.to_string()))?;
            }
            #[cfg(all(unix, not(target_os = "macos")))]
            {
                let parent = path
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| path.clone());
                open::that(parent).map_err(|e| Error::Internal(e.to_string()))?;
            }
            Ok(serde_json::json!({ "ok": true, "with": "reveal" }))
        }
        "editor" => {
            // Prefer VS Code CLI, fall back to default opener.
            let path_s = path.to_string_lossy().to_string();
            match std::process::Command::new("code").arg(&path_s).spawn() {
                Ok(_child) => {
                    // Detach: don't wait. If spawn succeeded we're good.
                    Ok(serde_json::json!({ "ok": true, "with": "editor" }))
                }
                Err(_) => {
                    open::that(&path).map_err(|e| Error::Internal(e.to_string()))?;
                    Ok(serde_json::json!({ "ok": true, "with": "default" }))
                }
            }
        }
        _ => {
            open::that(&path).map_err(|e| Error::Internal(e.to_string()))?;
            Ok(serde_json::json!({ "ok": true, "with": "default" }))
        }
    }
}

/// Put file paths on the system pasteboard so Finder/Explorer can paste them.
/// macOS: AppleScript `set the clipboard to … as «class furl»` via osascript.
pub fn clipboard_copy_files(paths: &[String]) -> Result<serde_json::Value> {
    if paths.is_empty() {
        return Err(Error::InvalidInput("no paths".into()));
    }
    // Validate all paths exist & allowed
    let mut abs: Vec<PathBuf> = Vec::new();
    for p in paths {
        let path = expand_tilde(p);
        if !path.exists() {
            return Err(Error::NotFound(p.clone()));
        }
        validate_path(&path)?;
        abs.push(path);
    }

    #[cfg(target_os = "macos")]
    {
        // Build AppleScript list of POSIX files.
        // Use argv-style osascript to avoid quote injection.
        // osascript -e 'on run argv' -e 'set the clipboard to (POSIX file (item 1 of argv) as alias)' ...
        // Multi-file: set the clipboard to {POSIX file a as alias, POSIX file b as alias}
        let mut script = String::from("on run argv\nset fileList to {}\n");
        script.push_str("repeat with a in argv\n");
        script.push_str("set end of fileList to (POSIX file a as alias)\n");
        script.push_str("end repeat\n");
        script.push_str("set the clipboard to fileList\nend run\n");

        let mut cmd = std::process::Command::new("osascript");
        cmd.arg("-e").arg(&script);
        for p in &abs {
            cmd.arg(p.to_string_lossy().as_ref());
        }
        let output = cmd
            .output()
            .map_err(|e| Error::Internal(format!("osascript failed: {e}")))?;
        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            return Err(Error::Internal(format!(
                "clipboard copy files failed: {err}"
            )));
        }
        Ok(serde_json::json!({ "ok": true, "count": abs.len(), "platform": "macos" }))
    }

    #[cfg(not(target_os = "macos"))]
    {
        // Fallback: put newline-joined paths as text (better than nothing)
        let text = abs
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("\n");
        // Reuse pbcopy/xclip via std process when available
        #[cfg(target_os = "linux")]
        {
            use std::io::Write;
            let mut child = std::process::Command::new("xclip")
                .args(["-selection", "clipboard"])
                .stdin(std::process::Stdio::piped())
                .spawn()
                .or_else(|_| {
                    std::process::Command::new("xsel")
                        .args(["--clipboard", "--input"])
                        .stdin(std::process::Stdio::piped())
                        .spawn()
                })
                .map_err(|e| Error::Internal(format!("clipboard tool failed: {e}")))?;
            if let Some(stdin) = child.stdin.as_mut() {
                let _ = stdin.write_all(text.as_bytes());
            }
            let _ = child.wait();
        }
        #[cfg(target_os = "windows")]
        {
            // PowerShell Set-Clipboard
            let ps = format!("Set-Clipboard -Value @'\n{text}\n'@");
            std::process::Command::new("powershell")
                .args(["-NoProfile", "-Command", &ps])
                .spawn()
                .map_err(|e| Error::Internal(e.to_string()))?;
        }
        Ok(serde_json::json!({
            "ok": true,
            "count": abs.len(),
            "platform": std::env::consts::OS,
            "mode": "text-paths"
        }))
    }
}

/// Copy an image file onto the system pasteboard as image data.
pub fn clipboard_copy_image(file_path: &str) -> Result<serde_json::Value> {
    let path = expand_tilde(file_path);
    validate_path(&path)?;
    if !path.exists() {
        return Err(Error::NotFound(file_path.to_string()));
    }
    let kind = detect_file_kind(path.file_name().and_then(|n| n.to_str()).unwrap_or(""));
    if kind != "image" {
        return Err(Error::InvalidInput("not an image file".into()));
    }

    // Reuse the bounded preview path so clipboard writes inherit the same
    // size, format and magic-byte checks as in-page previews.
    let preview = super::read_image_preview(file_path)?;

    #[cfg(target_os = "macos")]
    {
        let needs_conversion = matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some(ext) if ext.eq_ignore_ascii_case("heic") || ext.eq_ignore_ascii_case("heif")
        );
        let temporary = if needs_conversion {
            let target = std::env::temp_dir()
                .join(format!("natives-clipboard-{}.jpg", rand::random::<u64>()));
            std::fs::write(&target, &preview.bytes).map_err(Error::Io)?;
            Some(target)
        } else {
            None
        };
        let clipboard_path = temporary.as_ref().unwrap_or(&path);
        // osascript: set the clipboard to (read POSIX file "..." as «class PNGf»/JPEG)
        // Use generic picture data via Finder/System Events is fragile; use `osascript` + `read … as TIFF picture`
        let script = r#"
on run argv
  set p to item 1 of argv
  set the clipboard to (read (POSIX file p) as «class PNGf»)
end run
"#;
        let output = std::process::Command::new("osascript")
            .arg("-e")
            .arg(script)
            .arg(clipboard_path.to_string_lossy().as_ref())
            .output()
            .map_err(|e| Error::Internal(format!("osascript failed: {e}")))?;
        if !output.status.success() {
            // Fallback: try TIFF
            let script2 = r#"
on run argv
  set p to item 1 of argv
  set the clipboard to (read (POSIX file p) as TIFF picture)
end run
"#;
            let output2 = std::process::Command::new("osascript")
                .arg("-e")
                .arg(script2)
                .arg(clipboard_path.to_string_lossy().as_ref())
                .output()
                .map_err(|e| Error::Internal(format!("osascript failed: {e}")))?;
            if !output2.status.success() {
                if let Some(path) = temporary {
                    let _ = std::fs::remove_file(path);
                }
                let err = String::from_utf8_lossy(&output2.stderr);
                return Err(Error::Internal(format!("copy image failed: {err}")));
            }
        }
        if let Some(path) = temporary {
            let _ = std::fs::remove_file(path);
        }
        Ok(
            serde_json::json!({ "ok": true, "path": path.to_string_lossy(), "mimeType": preview.mime_type }),
        )
    }

    #[cfg(target_os = "linux")]
    {
        use std::io::Write;
        let mime = preview.mime_type.as_str();
        let mut child = std::process::Command::new("wl-copy")
            .args(["--type", mime])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .or_else(|_| {
                std::process::Command::new("xclip")
                    .args(["-selection", "clipboard", "-t", mime, "-i"])
                    .stdin(std::process::Stdio::piped())
                    .spawn()
            })
            .map_err(|e| Error::NotImplemented(format!("image clipboard unavailable: {e}")))?;
        child
            .stdin
            .as_mut()
            .ok_or_else(|| Error::Internal("image clipboard stdin unavailable".into()))?
            .write_all(&preview.bytes)
            .map_err(Error::Io)?;
        let output = child
            .wait_with_output()
            .map_err(|e| Error::Internal(format!("image clipboard failed: {e}")))?;
        if !output.status.success() {
            return Err(Error::Internal(format!(
                "image clipboard failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        Ok(
            serde_json::json!({ "ok": true, "path": path.to_string_lossy(), "mimeType": preview.mime_type }),
        )
    }

    #[cfg(target_os = "windows")]
    {
        let ps = "Add-Type -AssemblyName System.Windows.Forms,System.Drawing; $i=[Drawing.Image]::FromFile($args[0]); [Windows.Forms.Clipboard]::SetImage($i); $i.Dispose()";
        let output = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                ps,
                path.to_string_lossy().as_ref(),
            ])
            .output()
            .map_err(|e| Error::NotImplemented(format!("image clipboard unavailable: {e}")))?;
        if !output.status.success() {
            return Err(Error::Internal(format!(
                "image clipboard failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        Ok(
            serde_json::json!({ "ok": true, "path": path.to_string_lossy(), "mimeType": preview.mime_type }),
        )
    }
}
