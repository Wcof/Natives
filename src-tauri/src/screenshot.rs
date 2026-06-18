use crate::{Error, Result};
use std::path::PathBuf;

/// Get the macOS Screenshots directory
fn screenshots_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Desktop")
}

/// Start watching for new screenshots
pub fn start_watching(app_handle: tauri::AppHandle) -> Result<()> {
    use tauri::Emitter;

    let watch_dir = screenshots_dir();
    if !watch_dir.exists() {
        return Err(Error::NotFound("screenshots directory".into()));
    }

    std::thread::spawn(move || {
        let mut last_count = count_screenshots(&watch_dir);
        loop {
            std::thread::sleep(std::time::Duration::from_secs(2));
            let current_count = count_screenshots(&watch_dir);
            if current_count > last_count {
                // New screenshot detected
                if let Some(newest) = find_newest_screenshot(&watch_dir) {
                    let _ = app_handle.emit("screenshot:detected", newest.to_string_lossy());
                }
            }
            last_count = current_count;
        }
    });

    Ok(())
}

fn count_screenshots(dir: &PathBuf) -> usize {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    let name = e.file_name().to_string_lossy().to_lowercase();
                    name.starts_with("screenshot") && (name.ends_with(".png") || name.ends_with(".jpg"))
                })
                .count()
        })
        .unwrap_or(0)
}

fn find_newest_screenshot(dir: &PathBuf) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_lowercase();
            name.starts_with("screenshot") && (name.ends_with(".png") || name.ends_with(".jpg"))
        })
        .max_by_key(|e| e.metadata().ok().and_then(|m| m.modified().ok()).unwrap_or(std::time::UNIX_EPOCH))
        .map(|e| e.path())
}

/// Save annotated screenshot from data URL
pub fn save_annotated(data_url: &str, target_path: Option<&str>) -> Result<String> {
    use base64::Engine;

    // Parse data URL: data:image/png;base64,...
    let base64_data = data_url
        .split(',')
        .nth(1)
        .ok_or_else(|| Error::InvalidInput("invalid data URL".into()))?;

    let image_data = base64::engine::general_purpose::STANDARD
        .decode(base64_data)
        .map_err(|e| Error::Internal(format!("base64 decode failed: {e}")))?;

    let save_path = match target_path {
        Some(p) => PathBuf::from(p),
        None => {
            let dir = screenshots_dir();
            let timestamp = chrono::Local::now().format("%Y-%m-%d at %H.%M.%S");
            dir.join(format!("Annotated {timestamp}.png"))
        }
    };

    std::fs::write(&save_path, &image_data).map_err(Error::Io)?;
    Ok(save_path.to_string_lossy().to_string())
}
