use crate::file_manager::{deduplicate_path, FileAccessPolicy, OperationPolicy};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const PNG_DATA_URL_PREFIX: &str = "data:image/png;base64,";
/// Canvas exports are bounded before decoding to keep Renderer-controlled IPC
/// payloads from causing unbounded Host allocations.
pub const MAX_ANNOTATED_PNG_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveAnnotatedRequest {
    pub source_path: String,
    pub data_url: String,
}

#[derive(Debug, Serialize)]
pub struct SaveAnnotatedResult {
    pub path: String,
}

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
        "screenshot",
        "screen shot",
        "屏幕截图",
        "图片",
        "截图",
        "截圖",
        "截屏",
        ".截屏",
        "snipaste",
        "微信图片",
        "qq截图",
        "cleanshot",
        "scr-",
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
        .max_by_key(|e| {
            e.metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or(std::time::UNIX_EPOCH)
        })
        .map(|e| e.path())
}

/// Save an annotated PNG beside an authorized source screenshot without ever
/// allowing Renderer to select or overwrite a destination.
pub fn save_annotated(request: SaveAnnotatedRequest) -> Result<SaveAnnotatedResult> {
    let image_data = decode_png_data_url(&request.data_url)?;

    let source = FileAccessPolicy::authorize_path(&request.source_path, OperationPolicy::Read)?;
    let source_path = source.as_path();
    let metadata = std::fs::metadata(source_path).map_err(Error::Io)?;
    if !metadata.is_file() {
        return Err(Error::InvalidInput(
            "screenshot source must be a file".into(),
        ));
    }
    let parent = source_path
        .parent()
        .ok_or_else(|| Error::InvalidInput("screenshot source has no parent directory".into()))?;
    FileAccessPolicy::authorize_path_buf(parent, OperationPolicy::Write)?;

    let stem = source_path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("screenshot");
    let requested = parent.join(format!("{stem}-annotated.png"));

    // deduplicate_path handles existing files; create-new publication closes
    // the race between that check and the final write.
    for _ in 0..100 {
        let destination = deduplicate_path(&requested)?;
        match write_new_file_atomically(&destination, &image_data) {
            Ok(()) => {
                return Ok(SaveAnnotatedResult {
                    path: destination.to_string_lossy().to_string(),
                })
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(Error::Io(error)),
        }
    }

    Err(Error::InvalidInput(
        "too many annotated screenshot name collisions".into(),
    ))
}

fn decode_png_data_url(data_url: &str) -> Result<Vec<u8>> {
    use base64::Engine;

    let base64_data = data_url
        .strip_prefix(PNG_DATA_URL_PREFIX)
        .ok_or_else(|| Error::InvalidInput("expected an exact PNG data URL".into()))?;
    let max_encoded_len = MAX_ANNOTATED_PNG_BYTES.div_ceil(3) * 4;
    if base64_data.len() > max_encoded_len {
        return Err(Error::InvalidInput(format!(
            "annotated PNG exceeds {} byte limit",
            MAX_ANNOTATED_PNG_BYTES
        )));
    }

    let image_data = base64::engine::general_purpose::STANDARD
        .decode(base64_data)
        .map_err(|_| Error::InvalidInput("malformed PNG base64 payload".into()))?;
    if image_data.len() > MAX_ANNOTATED_PNG_BYTES {
        return Err(Error::InvalidInput(format!(
            "annotated PNG exceeds {} byte limit",
            MAX_ANNOTATED_PNG_BYTES
        )));
    }
    if !image_data.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(Error::InvalidInput("payload is not a PNG image".into()));
    }
    Ok(image_data)
}

fn write_new_file_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;

    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("destination has no parent"))?;
    let basename = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("annotated.png");
    let temp_path = parent.join(format!(
        ".tmp-{basename}-{}-{}",
        std::process::id(),
        rand::random::<u64>()
    ));

    let result = (|| {
        let mut temp = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)?;
        temp.write_all(bytes)?;
        temp.sync_all()?;
        drop(temp);

        // hard_link publishes the fully-synced bytes with create-new semantics:
        // an existing destination yields AlreadyExists and is never replaced.
        std::fs::hard_link(&temp_path, path)
    })();
    let _ = std::fs::remove_file(&temp_path);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use std::time::{SystemTime, UNIX_EPOCH};

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nfixture";

    fn fixture_dir(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "natives-screenshot-{label}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn data_url(bytes: &[u8]) -> String {
        format!(
            "{PNG_DATA_URL_PREFIX}{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        )
    }

    fn save(source: &Path) -> Result<SaveAnnotatedResult> {
        save_annotated(SaveAnnotatedRequest {
            source_path: source.to_string_lossy().to_string(),
            data_url: data_url(PNG),
        })
    }

    #[test]
    fn derives_case_insensitive_and_suffixless_sibling_names_without_overwrite() {
        for (source_name, expected_name) in [
            ("Screenshot.PNG", "Screenshot-annotated.png"),
            ("Screenshot", "Screenshot-annotated.png"),
        ] {
            let dir = fixture_dir("names");
            let source = dir.join(source_name);
            let original = b"original source bytes";
            std::fs::write(&source, original).unwrap();

            let first = save(&source).unwrap();
            let second = save(&source).unwrap();
            assert_eq!(Path::new(&first.path).file_name().unwrap(), expected_name);
            assert_eq!(
                Path::new(&second.path).file_name().unwrap(),
                "Screenshot-annotated (1).png"
            );
            assert_eq!(std::fs::read(&source).unwrap(), original);
            assert_eq!(std::fs::read(first.path).unwrap(), PNG);
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn rejects_invalid_or_unbounded_renderer_payloads() {
        for invalid in [
            "data:image/jpeg;base64,AAAA".to_string(),
            "data:image/png;base64,%%%".to_string(),
            data_url(b"not png"),
            format!(
                "{PNG_DATA_URL_PREFIX}{}",
                "A".repeat(MAX_ANNOTATED_PNG_BYTES.div_ceil(3) * 4 + 1)
            ),
        ] {
            assert!(decode_png_data_url(&invalid).is_err());
        }
    }

    #[test]
    fn rejects_out_of_scope_source_before_writing() {
        let result = save_annotated(SaveAnnotatedRequest {
            source_path: "/etc/passwd".into(),
            data_url: data_url(PNG),
        });
        assert!(result.is_err());
        assert!(!Path::new("/etc/passwd-annotated.png").exists());
    }

    #[test]
    fn atomic_publish_preserves_existing_destination_on_collision() {
        let dir = fixture_dir("atomic");
        let destination = dir.join("Screenshot-annotated.png");
        std::fs::write(&destination, b"existing").unwrap();

        let error = write_new_file_atomically(&destination, PNG).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&destination).unwrap(), b"existing");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
