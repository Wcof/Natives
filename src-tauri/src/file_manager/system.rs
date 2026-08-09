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
        ("movies", home.join("Movies")),
        ("music", home.join("Music")),
    ];

    let mut roots = Vec::new();
    for (id, path) in candidates {
        if path.is_dir() {
            roots.push(serde_json::json!({
                "id": id,
                "name": match id {
                    "home" => "Home",
                    "desktop" => "Desktop",
                    "documents" => "Documents",
                    "downloads" => "Downloads",
                    "pictures" => "Pictures",
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

/// Open path with a preferred app (fanbox `/api/open`).
/// `with`: "default" | "reveal" | "terminal" | "editor"
pub fn open_with(target: &str, with: &str) -> Result<serde_json::Value> {
    let path = expand_tilde(target);
    if !path.exists() {
        return Err(Error::NotFound(target.to_string()));
    }
    // Enforce allowlist on the resolved target so open/reveal/terminal/editor
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
        "terminal" => {
            let dir = if path.is_dir() {
                path.clone()
            } else {
                path.parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| path.clone())
            };
            #[cfg(target_os = "macos")]
            {
                // Fixed argv — no shell.
                std::process::Command::new("open")
                    .args(["-a", "Terminal", dir.to_string_lossy().as_ref()])
                    .spawn()
                    .map_err(|e| Error::Internal(e.to_string()))?;
            }
            #[cfg(target_os = "windows")]
            {
                // Prefer Windows Terminal with independent argv; fall back to Explorer.
                let dir_s = dir.to_string_lossy().to_string();
                let wt = std::process::Command::new("wt.exe")
                    .args(["-d", &dir_s])
                    .spawn();
                if wt.is_err() {
                    std::process::Command::new("explorer")
                        .arg(&dir_s)
                        .spawn()
                        .map_err(|e| Error::Internal(e.to_string()))?;
                }
            }
            #[cfg(all(unix, not(target_os = "macos")))]
            {
                let dir_s = dir.to_string_lossy().to_string();
                // Prefer x-terminal-emulator with working-directory flag when available.
                let tried = std::process::Command::new("x-terminal-emulator")
                    .args(["--working-directory", &dir_s])
                    .spawn();
                if tried.is_err() {
                    let gnome = std::process::Command::new("gnome-terminal")
                        .args([format!("--working-directory={dir_s}")])
                        .spawn();
                    if gnome.is_err() {
                        open::that(&dir).map_err(|e| Error::Internal(e.to_string()))?;
                    }
                }
            }
            Ok(serde_json::json!({ "ok": true, "with": "terminal" }))
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

/// Copy an image file onto the pasteboard as image data (macOS).
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

    #[cfg(target_os = "macos")]
    {
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
            .arg(path.to_string_lossy().as_ref())
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
                .arg(path.to_string_lossy().as_ref())
                .output()
                .map_err(|e| Error::Internal(format!("osascript failed: {e}")))?;
            if !output2.status.success() {
                let err = String::from_utf8_lossy(&output2.stderr);
                return Err(Error::Internal(format!("copy image failed: {err}")));
            }
        }
        Ok(serde_json::json!({ "ok": true, "path": path.to_string_lossy() }))
    }

    #[cfg(not(target_os = "macos"))]
    {
        // Best-effort: copy path text
        let _ = file_path;
        Err(Error::NotImplemented(
            "clipboard image copy not implemented on this platform".into(),
        ))
    }
}
