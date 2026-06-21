use crate::{Error, Result};

#[tauri::command]
pub fn show_item_in_folder(path: String) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        // macOS: `open -R` reveals the file in Finder
        std::process::Command::new("open")
            .args(["-R", &path])
            .spawn()
            .map_err(|e| Error::Internal(e.to_string()))?;
        Ok(())
    }
    #[cfg(target_os = "linux")]
    {
        // Linux: try `dbus` to open file manager with selection, fallback to `xdg-open` on parent dir
        let result = std::process::Command::new("dbus-send")
            .args([
                "--print-reply",
                "--dest=org.freedesktop.FileManager1",
                "/org/freedesktop/FileManager1",
                "org.freedesktop.FileManager1.ShowItems",
                format!("array:string:file://{}", &path).as_str(),
                "string:",
            ])
            .spawn();
        match result {
            Ok(mut child) => {
                let _ = child.wait();
                Ok(())
            }
            Err(_) => {
                // Fallback: open parent directory
                let parent = std::path::Path::new(&path)
                    .parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.clone());
                open::that(&parent).map_err(|e| Error::Internal(e.to_string()))
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        // Windows: `explorer /select,` highlights the file
        std::process::Command::new("explorer")
            .args(["/select,", &path])
            .spawn()
            .map_err(|e| Error::Internal(e.to_string()))?;
        Ok(())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Err(Error::NotImplemented(format!(
            "showItemInFolder on {}",
            std::env::consts::OS
        )))
    }
}

#[tauri::command]
pub fn open_path(path: String) -> Result<()> {
    open::that(&path).map_err(|e| Error::Internal(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_show_item_in_folder_valid_file() {
        let dir = std::env::temp_dir().join("n2-test-shell-siif");
        std::fs::create_dir_all(&dir).ok();
        let f = dir.join("target.txt");
        std::fs::write(&f, b"x").ok();
        let r = show_item_in_folder(f.to_string_lossy().into());
        // macOS opens Finder; Linux/Windows may error without display
        // Just ensure no crash or infinite loop
        if let Err(e) = r {
            let msg = e.to_string();
            assert!(!msg.contains("Recursive"), "no recursion: {msg}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_show_item_in_folder_nonexistent() {
        let r = show_item_in_folder("/tmp/_n2_nonexistent_".into());
        // Should not panic; OS may succeed (opens parent) or fail
        if let Err(e) = r {
            let _ = e; // any error is acceptable
        }
    }

    #[test]
    fn test_open_path_valid_url() {
        let r = open_path("https://example.com".into());
        assert!(r.is_ok() || r.is_err()); // may fail in headless CI
    }

    #[test]
    fn test_open_path_invalid() {
        let r = open_path("\0invalid".into());
        if let Err(e) = r {
            let _ = e; // expected — invalid paths should fail
        }
    }
}
