use crate::{Error, Result};
use sha2::{Digest, Sha256};
use std::path::Path;

const MAX_CACHE_SIZE: u64 = 400 * 1024 * 1024; // 400 MB
const THUMB_WIDTH_MIN: u32 = 48;
const THUMB_WIDTH_MAX: u32 = 1600;

/// Generate a thumbnail for a file. Returns (jpeg_data, was_cached).
pub fn generate_thumbnail(file_path: &str, width: u32) -> Result<Option<(Vec<u8>, bool)>> {
    let path = Path::new(file_path);
    if !path.exists() || !path.is_file() {
        return Ok(None);
    }

    let width = width.clamp(THUMB_WIDTH_MIN, THUMB_WIDTH_MAX);
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    // Check supported formats
    let supported = matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tiff" | "tif" | "heic" | "avif"
            | "mp4" | "mov" | "avi" | "mkv" | "webm" | "m4v"
            | "pdf"
    );
    if !supported {
        return Ok(None);
    }

    // Check cache
    let cache_dir = get_cache_dir();
    let cache_key = make_cache_key(file_path, width);
    let cache_path = cache_dir.join(format!("{cache_key}.jpg"));

    if cache_path.exists() {
        if let Ok(data) = std::fs::read(&cache_path) {
            return Ok(Some((data, true)));
        }
    }

    // Generate thumbnail using macOS tools
    let jpeg_data = if is_image(&ext) {
        generate_image_thumb(path, width)?
    } else if is_video(&ext) || ext == "pdf" {
        generate_ql_thumb(path, width)?
    } else {
        return Ok(None);
    };

    // Cache the result
    let _ = std::fs::create_dir_all(&cache_dir);
    let _ = std::fs::write(&cache_path, &jpeg_data);

    // LRU eviction
    evict_cache_if_needed(&cache_dir);

    Ok(Some((jpeg_data, false)))
}

fn get_cache_dir() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".natives")
        .join("thumbs")
}

fn make_cache_key(file_path: &str, width: u32) -> String {
    let input = format!("{file_path}:{width}");
    let hash = Sha256::digest(input.as_bytes());
    hex::encode(hash)
}

fn is_image(ext: &str) -> bool {
    matches!(
        ext,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tiff" | "tif" | "heic" | "avif"
    )
}

fn is_video(ext: &str) -> bool {
    matches!(ext, "mp4" | "mov" | "avi" | "mkv" | "webm" | "m4v")
}

/// Generate thumbnail for images using sips (macOS)
fn generate_image_thumb(path: &Path, width: u32) -> Result<Vec<u8>> {
    let tmp_dir = std::env::temp_dir();
    let tmp_file = tmp_dir.join(format!("thumb_{}.jpg", rand_suffix()));

    let output = std::process::Command::new("sips")
        .args([
            "-Z",
            &width.to_string(),
            "-s",
            "format",
            "jpeg",
            &path.to_string_lossy(),
            "--out",
            &tmp_file.to_string_lossy(),
        ])
        .output()
        .map_err(|e| Error::Internal(format!("sips failed: {e}")))?;

    if !output.status.success() {
        return Err(Error::Internal("sips thumbnail generation failed".into()));
    }

    let data = std::fs::read(&tmp_file).map_err(Error::Io)?;
    let _ = std::fs::remove_file(&tmp_file);
    Ok(data)
}

/// Generate thumbnail for videos/PDF using qlmanage (macOS)
fn generate_ql_thumb(path: &Path, width: u32) -> Result<Vec<u8>> {
    let tmp_dir = std::env::temp_dir().join(format!("ql_{}", rand_suffix()));
    std::fs::create_dir_all(&tmp_dir).map_err(Error::Io)?;

    let output = std::process::Command::new("qlmanage")
        .args([
            "-t",
            "-s",
            &width.to_string(),
            "-o",
            &tmp_dir.to_string_lossy(),
            &path.to_string_lossy(),
        ])
        .output()
        .map_err(|e| Error::Internal(format!("qlmanage failed: {e}")))?;

    if !output.status.success() {
        let _ = std::fs::remove_dir_all(&tmp_dir);
        return Err(Error::Internal("qlmanage thumbnail generation failed".into()));
    }

    // qlmanage outputs a PNG file
    let png_path = tmp_dir.join(format!(
        "{}.png",
        path.file_stem().and_then(|s| s.to_str()).unwrap_or("thumb")
    ));

    // Convert PNG to JPEG using sips
    let jpeg_path = tmp_dir.join("thumb.jpg");
    let sips_output = std::process::Command::new("sips")
        .args([
            "-s",
            "format",
            "jpeg",
            &png_path.to_string_lossy(),
            "--out",
            &jpeg_path.to_string_lossy(),
        ])
        .output();

    let data = match sips_output {
        Ok(out) if out.status.success() && jpeg_path.exists() => {
            std::fs::read(&jpeg_path).map_err(Error::Io)?
        }
        _ => {
            // Fallback: read the PNG directly
            std::fs::read(&png_path).map_err(Error::Io)?
        }
    };

    let _ = std::fs::remove_dir_all(&tmp_dir);
    Ok(data)
}

/// LRU cache eviction when cache exceeds MAX_CACHE_SIZE
fn evict_cache_if_needed(cache_dir: &Path) {
    let mut entries: Vec<(std::path::PathBuf, u64, std::time::SystemTime)> = Vec::new();
    let mut total_size = 0u64;

    if let Ok(dir) = std::fs::read_dir(cache_dir) {
        for entry in dir.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("jpg") {
                if let Ok(meta) = std::fs::metadata(&path) {
                    let size = meta.len();
                    let accessed = meta.accessed().unwrap_or(std::time::UNIX_EPOCH);
                    total_size += size;
                    entries.push((path, size, accessed));
                }
            }
        }
    }

    if total_size <= MAX_CACHE_SIZE {
        return;
    }

    // Sort by access time (oldest first)
    entries.sort_by_key(|e| e.2);

    // Delete oldest entries until under limit
    for (path, size, _) in entries {
        if total_size <= MAX_CACHE_SIZE {
            break;
        }
        let _ = std::fs::remove_file(&path);
        total_size -= size;
    }
}

fn rand_suffix() -> u32 {
    rand::random::<u32>()
}

// ── HEIC/HEIF full-size transcode (Natives2: transparent markdown preview) ──

/// Full-size HEIC → JPEG transcode using macOS sips.
/// Returns (jpeg_data, was_cached). Used by HTTP server for transparent HEIC preview.
#[allow(dead_code)]
pub fn transcode_heic(file_path: &str) -> Result<Option<(Vec<u8>, bool)>> {
    let path = Path::new(file_path);
    if !path.exists() || !path.is_file() {
        return Ok(None);
    }

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    // Only handle HEIC/HEIF
    if !matches!(ext.as_str(), "heic" | "heif") {
        return Ok(None);
    }

    // Check cache (keyed on file path + mtime)
    let cache_dir = get_cache_dir().join("heic");
    let mtime = std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let cache_key = make_cache_key(&format!("{file_path}:heic:{mtime}"), 0);
    let cache_path = cache_dir.join(format!("{cache_key}.jpg"));

    if cache_path.exists() {
        if let Ok(data) = std::fs::read(&cache_path) {
            return Ok(Some((data, true)));
        }
    }

    // Full-size transcode using sips (no resize, just format conversion)
    let tmp_dir = std::env::temp_dir();
    let tmp_file = tmp_dir.join(format!("heic_{}.jpg", rand_suffix()));

    let output = std::process::Command::new("sips")
        .args([
            "-s",
            "format",
            "jpeg",
            &path.to_string_lossy(),
            "--out",
            &tmp_file.to_string_lossy(),
        ])
        .output()
        .map_err(|e| Error::Internal(format!("sips HEIC transcode failed: {e}")))?;

    if !output.status.success() {
        return Err(Error::Internal("sips HEIC transcode failed".into()));
    }

    let data = std::fs::read(&tmp_file).map_err(Error::Io)?;
    let _ = std::fs::remove_file(&tmp_file);

    // Cache the result
    let _ = std::fs::create_dir_all(&cache_dir);
    let _ = std::fs::write(&cache_path, &data);

    // LRU eviction for HEIC cache
    evict_cache_if_needed(&cache_dir);

    Ok(Some((data, false)))
}
