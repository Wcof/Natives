//! HEIC/HEIF 等格式的透明转码预览（W8）
//!
//! macOS 用 `sips -s format jpeg` 全尺寸转码到 `~/.natives/convert-cache/`
//! （与 thumbnail 缓存同级）。缓存键含源文件 mtime，源文件更新自动失效；
//! 缓存目录按 mtime 最旧裁剪，上限 200MB。非 macOS 平台明确报错。

use crate::{file_manager, Error, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use ts_rs::TS;

/// 转码缓存目录上限：200 MB
const MAX_CONVERT_CACHE_SIZE: u64 = 200 * 1024 * 1024;
/// 每 8 次写入触发一次裁剪扫描（全尺寸 jpeg 较大，写入频率低，无需每次扫描）
const EVICTION_INTERVAL: usize = 8;
static EVICTION_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// 转码结果（wire 契约）
#[derive(Debug, Serialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ConvertImageResult {
    pub ok: bool,
    pub jpeg_path: String,
    /// 是否命中缓存（前端可用于埋点/调试，不影响展示）
    pub cached: bool,
}

/// 全尺寸转码为 jpeg，返回缓存文件路径。非 macOS 平台报错。
pub fn convert_image_preview(file_path: &str) -> Result<ConvertImageResult> {
    if !cfg!(target_os = "macos") {
        return Err(Error::NotImplemented(
            "not supported on this platform".into(),
        ));
    }

    let path = file_manager::expand_tilde(file_path);
    file_manager::validate_path(&path)?;
    if !path.is_file() {
        return Err(Error::NotFound(file_path.to_string()));
    }

    // 缓存键含 mtime：源文件被改写后旧缓存自动失效
    let mtime = std::fs::metadata(&path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let cache_dir = get_cache_dir();
    let key = make_cache_key(&path.to_string_lossy(), mtime);
    let cache_path = cache_dir.join(format!("{key}.jpg"));

    if cache_path.is_file() {
        return Ok(ConvertImageResult {
            ok: true,
            jpeg_path: cache_path.to_string_lossy().to_string(),
            cached: true,
        });
    }

    std::fs::create_dir_all(&cache_dir).map_err(Error::Io)?;
    // 先写临时名再改名，避免并发读到半成品
    let tmp_path = cache_dir.join(format!(".tmp-{key}-{}.jpg", rand::random::<u32>()));

    let output = std::process::Command::new("sips")
        .args([
            "-s",
            "format",
            "jpeg",
            &path.to_string_lossy(),
            "--out",
            &tmp_path.to_string_lossy(),
        ])
        .output()
        .map_err(|e| Error::Internal(format!("sips spawn failed: {e}")))?;

    if !output.status.success() || !tmp_path.is_file() {
        let _ = std::fs::remove_file(&tmp_path);
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Internal(format!(
            "sips convert failed: {}",
            stderr.chars().take(200).collect::<String>()
        )));
    }
    std::fs::rename(&tmp_path, &cache_path).map_err(Error::Io)?;

    // 周期性裁剪（参考 thumbnail.rs：避免每次写入都付 O(n) 目录扫描）
    if EVICTION_COUNTER.fetch_add(1, Ordering::Relaxed) % EVICTION_INTERVAL == 0 {
        evict_cache_if_needed(&cache_dir);
    }

    Ok(ConvertImageResult {
        ok: true,
        jpeg_path: cache_path.to_string_lossy().to_string(),
        cached: false,
    })
}

/// 转码缓存目录（与 thumbnail 缓存 `~/.natives/thumbs` 同级）
fn get_cache_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".natives")
        .join("convert-cache")
}

/// 缓存键：sha256(源路径 + mtime)。纯函数，单测覆盖。
fn make_cache_key(file_path: &str, mtime: u128) -> String {
    let input = format!("{file_path}:{mtime}");
    hex::encode(Sha256::digest(input.as_bytes()))
}

/// 从缓存条目里挑出应删除的路径：按 mtime 最旧优先，删到总量 ≤ max_total。
/// 纯函数，单测覆盖。
fn select_evictions(
    entries: &[(PathBuf, u64, std::time::SystemTime)],
    max_total: u64,
) -> Vec<PathBuf> {
    let total: u64 = entries.iter().map(|e| e.1).sum();
    if total <= max_total {
        return Vec::new();
    }
    let mut sorted: Vec<_> = entries.to_vec();
    // mtime 最旧在前
    sorted.sort_by_key(|e| e.2);
    let mut remaining = total;
    let mut victims = Vec::new();
    for (path, size, _) in sorted {
        if remaining <= max_total {
            break;
        }
        remaining -= size;
        victims.push(path);
    }
    victims
}

/// 超过上限时按 mtime 最旧裁剪缓存目录
fn evict_cache_if_needed(cache_dir: &Path) {
    let mut entries: Vec<(PathBuf, u64, std::time::SystemTime)> = Vec::new();
    if let Ok(dir) = std::fs::read_dir(cache_dir) {
        for entry in dir.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("jpg") {
                if let Ok(meta) = std::fs::metadata(&path) {
                    let mtime = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
                    entries.push((path, meta.len(), mtime));
                }
            }
        }
    }
    for path in select_evictions(&entries, MAX_CONVERT_CACHE_SIZE) {
        let _ = std::fs::remove_file(&path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn cache_key_changes_with_mtime_and_path() {
        let a = make_cache_key("/a/b.heic", 100);
        let same = make_cache_key("/a/b.heic", 100);
        let diff_mtime = make_cache_key("/a/b.heic", 101);
        let diff_path = make_cache_key("/a/c.heic", 100);
        assert_eq!(a, same, "同路径同 mtime 键必须稳定");
        assert_ne!(a, diff_mtime, "mtime 变化必须换键");
        assert_ne!(a, diff_path, "路径变化必须换键");
        assert_eq!(a.len(), 64, "sha256 hex 长度");
    }

    #[test]
    fn select_evictions_under_limit_is_noop() {
        let entries = vec![
            (PathBuf::from("/c/1.jpg"), 10, UNIX_EPOCH),
            (PathBuf::from("/c/2.jpg"), 10, UNIX_EPOCH + Duration::from_secs(1)),
        ];
        assert!(select_evictions(&entries, 100).is_empty());
    }

    #[test]
    fn select_evictions_removes_oldest_first() {
        let entries = vec![
            (PathBuf::from("/c/new.jpg"), 60, UNIX_EPOCH + Duration::from_secs(30)),
            (PathBuf::from("/c/oldest.jpg"), 60, UNIX_EPOCH),
            (PathBuf::from("/c/mid.jpg"), 60, UNIX_EPOCH + Duration::from_secs(10)),
        ];
        // 总量 180，上限 120 → 只需删掉最旧的一个
        let victims = select_evictions(&entries, 120);
        assert_eq!(victims, vec![PathBuf::from("/c/oldest.jpg")]);
        // 上限 60 → 删最旧两个
        let victims = select_evictions(&entries, 60);
        assert_eq!(
            victims,
            vec![PathBuf::from("/c/oldest.jpg"), PathBuf::from("/c/mid.jpg")]
        );
    }
}
