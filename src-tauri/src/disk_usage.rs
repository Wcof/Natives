use crate::{Error, Result};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct DiskUsageItem {
    pub name: String,
    pub path: String,
    #[serde(rename = "isDir")]
    pub is_dir: bool,
    pub size: u64,
    #[serde(rename = "sizeFormatted")]
    pub size_formatted: String,
}

/// Get disk usage for a directory
pub fn get_disk_usage(dir_path: &str) -> Result<Vec<DiskUsageItem>> {
    let path = Path::new(dir_path);
    let meta = std::fs::metadata(path).map_err(Error::Io)?;
    if !meta.is_dir() {
        return Err(Error::InvalidInput("not a directory".into()));
    }

    let mut items = Vec::new();

    for entry in std::fs::read_dir(path).map_err(Error::Io)? {
        let entry = entry.map_err(Error::Io)?;
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip dotfiles
        if name.starts_with('.') {
            continue;
        }

        let entry_path = entry.path();
        let entry_meta = std::fs::metadata(&entry_path).map_err(Error::Io)?;

        let size = if entry_meta.is_dir() {
            // Use du for directories
            get_dir_size_du(&entry_path)
        } else {
            entry_meta.len()
        };

        items.push(DiskUsageItem {
            name,
            path: entry_path.to_string_lossy().to_string(),
            is_dir: entry_meta.is_dir(),
            size,
            size_formatted: format_size(size),
        });
    }

    // Sort descending by size
    items.sort_by(|a, b| b.size.cmp(&a.size));

    Ok(items)
}

/// Get directory size using du command (macOS)
fn get_dir_size_du(path: &Path) -> u64 {
    let output = std::process::Command::new("du")
        .args(["-d", "0", "-k", &path.to_string_lossy()])
        .output();
    match output {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            // du output: "1234\t/path"
            stdout
                .split_whitespace()
                .next()
                .and_then(|s| s.parse::<u64>().ok())
                .map(|kb| kb * 1024)
                .unwrap_or(0)
        }
        _ => 0,
    }
}

/// Format bytes to human-readable string
fn format_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_index = 0;
    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }
    if unit_index == 0 {
        format!("{} B", bytes)
    } else {
        format!("{:.1} {}", size, UNITS[unit_index])
    }
}
