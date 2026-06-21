use crate::{Error, Result};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Get the macOS Screenshots directory dynamically using defaults.
/// Falls back to ~/Desktop if defaults fails or does not exist.
fn screenshots_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    
    let output = std::process::Command::new("defaults")
        .arg("read")
        .arg("com.apple.screencapture")
        .arg("location")
        .output();
        
    if let Ok(out) = output {
        if out.status.success() {
            let path_str = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !path_str.is_empty() {
                let resolved_path = if path_str.starts_with('~') {
                    home.join(path_str.trim_start_matches('~').trim_start_matches('/'))
                } else {
                    PathBuf::from(path_str)
                };
                if resolved_path.exists() {
                    return resolved_path;
                }
            }
        }
    }
    
    home.join("Desktop")
}

fn is_screenshot_file(filename: &str) -> bool {
    let name = filename.to_lowercase();
    if !(name.ends_with(".png") || name.ends_with(".jpg") || name.ends_with(".jpeg")) {
        return false;
    }
    
    let prefixes = [
        "screenshot", "screen shot", "屏幕截图", "图片", "截图", "截圖", "截屏", ".截屏", 
        "snipaste", "微信图片", "qq截图", "cleanshot", "scr-"
    ];
    
    for prefix in &prefixes {
        if name.starts_with(prefix) {
            return true;
        }
    }
    
    false
}

/// Start watching for new screenshots.
/// The loop runs until `stop_flag` is set to true.
pub fn start_watching(app_handle: tauri::AppHandle, stop_flag: Arc<AtomicBool>) -> Result<()> {
    use tauri::Emitter;

    let watch_dir = screenshots_dir();
    if !watch_dir.exists() {
        return Err(Error::NotFound("screenshots directory".into()));
    }

    std::thread::spawn(move || {
        let mut last_count = count_screenshots(&watch_dir);
        loop {
            // Check stop flag before sleeping
            if stop_flag.load(Ordering::Relaxed) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_secs(2));
            // Check again after sleep so stop is responsive (~2s max latency)
            if stop_flag.load(Ordering::Relaxed) {
                break;
            }
            let current_count = count_screenshots(&watch_dir);
            if current_count > last_count {
                // New screenshot detected
                if let Some(newest) = find_newest_screenshot(&watch_dir) {
                    // Stable size check (wait up to 3 seconds, checking every 250ms)
                    let mut stable_size = 0u64;
                    let mut stable_check_count = 0;
                    loop {
                        if let Ok(meta) = std::fs::metadata(&newest) {
                            let size = meta.len();
                            if size >= 1000 && size == stable_size {
                                break;
                            }
                            stable_size = size;
                        }
                        stable_check_count += 1;
                        if stable_check_count > 12 {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(250));
                    }

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
                .filter(|e| is_screenshot_file(&e.file_name().to_string_lossy()))
                .count()
        })
        .unwrap_or(0)
}

fn find_newest_screenshot(dir: &PathBuf) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| is_screenshot_file(&e.file_name().to_string_lossy()))
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
