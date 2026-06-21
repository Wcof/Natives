use crate::{disk_usage, Error, Result};
use serde::Serialize;
use serde_json::Value as JsonValue;
use sysinfo::Disks;

#[derive(Serialize)]
pub struct DiskSystemInfo {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
}

#[tauri::command]
pub fn disk_usage(dir_path: String) -> Result<Vec<JsonValue>> {
    let items = disk_usage::get_disk_usage(&dir_path)?;
    serde_json::to_value(items)
        .map(|v| {
            if let JsonValue::Array(arr) = v {
                arr
            } else {
                vec![]
            }
        })
        .map_err(|e| Error::Internal(e.to_string()))
}

#[tauri::command]
pub fn disk_system_info() -> Result<DiskSystemInfo> {
    let disks = Disks::new_with_refreshed_list();
    let mut total: u64 = 0;
    let mut available: u64 = 0;

    for disk in &disks {
        total += disk.total_space();
        available += disk.available_space();
    }

    let used = total.saturating_sub(available);

    Ok(DiskSystemInfo {
        total_bytes: total,
        used_bytes: used,
        available_bytes: available,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join("n2-test-disk").join(name);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn test_disk_usage_basic() {
        let dir = test_dir("du_basic");
        std::fs::write(dir.join("a.txt"), b"a".repeat(100).as_slice()).unwrap();
        std::fs::write(dir.join("b.txt"), b"b".repeat(200).as_slice()).unwrap();
        let items = disk_usage(dir.to_string_lossy().into()).expect("disk_usage");
        assert!(!items.is_empty(), "should have entries");
        let total: u64 = items.iter().filter_map(|i| i["size"].as_u64()).sum();
        assert!(total >= 300, "total >= 300, got {total}");
    }

    #[test]
    fn test_disk_usage_empty_dir() {
        let dir = test_dir("du_empty");
        let items = disk_usage(dir.to_string_lossy().into()).expect("disk_usage empty");
        // Should return empty list, not error
        assert!(items.is_empty() || items.len() == 1); // may include "." entry
    }

    #[test]
    fn test_disk_system_info() {
        let info = disk_system_info().expect("system_info");
        assert!(info.total_bytes > 0, "total > 0");
        assert!(info.available_bytes > 0, "available > 0");
        assert_eq!(info.total_bytes, info.used_bytes + info.available_bytes);
    }

    #[test]
    fn test_disk_usage_nonexistent() {
        let r = disk_usage("/tmp/_n2_nonexistent_".into());
        assert!(r.is_err());
    }
}
