use crate::{Error, Result};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct ArchiveEntry {
    pub name: String,
    pub size: u64,
    #[serde(rename = "isDir")]
    pub is_dir: bool,
}

#[derive(Debug, Serialize)]
pub struct ArchiveListing {
    pub entries: Vec<ArchiveEntry>,
    pub truncated: bool,
    #[serde(rename = "totalSize")]
    pub total_size: u64,
}

const MAX_ENTRIES: usize = 1000;

/// List contents of an archive file
pub fn list_archive(archive_path: &str) -> Result<ArchiveListing> {
    let path = Path::new(archive_path);
    if !path.exists() {
        return Err(Error::NotFound(archive_path.to_string()));
    }

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "zip" => list_zip(path),
        "tar" | "tgz" | "gz" | "bz2" | "xz" => list_tar(path, &ext),
        _ => Err(Error::InvalidInput(format!(
            "unsupported archive format: {ext}"
        ))),
    }
}

fn list_zip(path: &Path) -> Result<ArchiveListing> {
    let file = std::fs::File::open(path).map_err(Error::Io)?;
    let mut zip_archive =
        zip::ZipArchive::new(file).map_err(|e| Error::Internal(format!("zip error: {e}")))?;

    let mut entries = Vec::new();
    let mut total_size = 0u64;
    let truncated = zip_archive.len() > MAX_ENTRIES;

    for i in 0..std::cmp::min(zip_archive.len(), MAX_ENTRIES) {
        let entry = zip_archive
            .by_index(i)
            .map_err(|e| Error::Internal(format!("zip entry error: {e}")))?;
        let name = entry.name().to_string();
        let is_dir = entry.is_dir();
        let size = if is_dir { 0 } else { entry.size() };
        total_size += size;
        entries.push(ArchiveEntry {
            name,
            size,
            is_dir,
        });
    }

    Ok(ArchiveListing {
        entries,
        truncated,
        total_size,
    })
}

fn list_tar(path: &Path, ext: &str) -> Result<ArchiveListing> {
    let flag = match ext {
        "tgz" | "gz" => "tzf",
        "bz2" => "tjf",
        "xz" => "tJf",
        _ => "tf",
    };

    let output = std::process::Command::new("tar")
        .args([flag, &path.to_string_lossy()])
        .output()
        .map_err(|e| Error::Internal(format!("tar failed: {e}")))?;

    if !output.status.success() {
        return Err(Error::Internal(format!(
            "tar exited with {}",
            output.status
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut entries = Vec::new();
    let truncated = false;

    for line in stdout.lines() {
        if entries.len() >= MAX_ENTRIES {
            break;
        }
        let name = line.trim().to_string();
        if name.is_empty() {
            continue;
        }
        let is_dir = name.ends_with('/');
        entries.push(ArchiveEntry {
            name,
            size: 0, // tar listing doesn't report sizes
            is_dir,
        });
    }

    Ok(ArchiveListing {
        entries,
        truncated,
        total_size: 0,
    })
}
