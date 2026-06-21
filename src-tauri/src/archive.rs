use crate::{Error, Result};
use serde::Serialize;
use std::io::{Read, Seek, SeekFrom};
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

    // Handle compound extensions like .tar.gz
    let full_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_lowercase();

    if full_name.ends_with(".tar.gz") || full_name.ends_with(".tar.bz2") || full_name.ends_with(".tar.xz") {
        return list_tar(path, if full_name.ends_with(".gz") { "tgz" } else if full_name.ends_with(".bz2") { "bz2" } else { "xz" });
    }

    match ext.as_str() {
        "zip" => list_zip(path),
        "tar" | "tgz" | "gz" | "bz2" | "xz" => list_tar(path, &ext),
        _ => Err(Error::InvalidInput(format!(
            "unsupported archive format: {ext}"
        ))),
    }
}

// ── ZIP: Direct central directory parsing with UTF-8/GBK detection (Natives2) ──

/// General Purpose Bit Flag bit 11: Language encoding flag
/// If set, filename and comment are UTF-8. If not set, they may be CP437 or local encoding (e.g., GBK for Chinese).
const GPBF_UTF8: u16 = 1 << 11;

fn list_zip(path: &Path) -> Result<ArchiveListing> {
    // First try the zip crate (handles most cases correctly)
    // Then post-process names that contain replacement characters
    let file = std::fs::File::open(path).map_err(Error::Io)?;
    let mut zip_archive =
        zip::ZipArchive::new(file).map_err(|e| Error::Internal(format!("zip error: {e}")))?;

    // Read raw central directory entries for encoding detection
    let raw_names = read_zip_raw_names(path)?;

    let mut entries = Vec::new();
    let mut total_size = 0u64;
    let truncated = zip_archive.len() > MAX_ENTRIES;

    for i in 0..std::cmp::min(zip_archive.len(), MAX_ENTRIES) {
        let entry = zip_archive
            .by_index(i)
            .map_err(|e| Error::Internal(format!("zip entry error: {e}")))?;
        let is_dir = entry.is_dir();
        let size = if is_dir { 0 } else { entry.size() };
        total_size += size;

        // Decode name: try raw bytes with encoding detection
        let name = if let Some((raw_name, gpbf)) = raw_names.get(i) {
            decode_zip_name(raw_name, gpbf)
        } else {
            entry.name().to_string()
        };

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

/// Read raw filename bytes and general purpose bit flags from ZIP central directory.
/// Returns (raw_name_bytes, gpbf) for each entry.
fn read_zip_raw_names(path: &Path) -> Result<Vec<(Vec<u8>, u16)>> {
    use std::io::BufReader;
    let mut f = BufReader::new(std::fs::File::open(path).map_err(Error::Io)?);

    // Find End of Central Directory (EOCD)
    let file_len = f.seek(SeekFrom::End(0)).map_err(Error::Io)?;
    // EOCD is at least 22 bytes, max 65535 + 22 for comment
    let search_start = file_len.saturating_sub(65557);
    f.seek(SeekFrom::Start(search_start)).map_err(Error::Io)?;

    let mut buf = Vec::new();
    f.read_to_end(&mut buf).map_err(Error::Io)?;

    // Find EOCD signature (0x06054b50)
    let eocd_pos = find_signature(&buf, 0x06054b50)
        .ok_or_else(|| Error::Internal("ZIP EOCD not found".into()))?;

    // Parse EOCD
    let eocd = &buf[eocd_pos..];
    if eocd.len() < 22 {
        return Err(Error::Internal("truncated EOCD".into()));
    }
    let cd_size = read_u32(eocd, 12) as u64;
    let cd_offset = read_u32(eocd, 16) as u64;

    // Read central directory
    f.seek(SeekFrom::Start(cd_offset)).map_err(Error::Io)?;
    let mut cd_buf = vec![0u8; cd_size as usize];
    f.read_exact(&mut cd_buf).map_err(Error::Io)?;

    // Parse central directory entries
    let mut results = Vec::new();
    let mut pos = 0;
    while pos + 46 <= cd_buf.len() {
        // Central file header signature: 0x02014b50
        if read_u32(&cd_buf, pos) != 0x02014b50 {
            break;
        }
        let gpbf = read_u16(&cd_buf, pos + 8);
        let name_len = read_u16(&cd_buf, pos + 28) as usize;
        let extra_len = read_u16(&cd_buf, pos + 30) as usize;
        let comment_len = read_u16(&cd_buf, pos + 32) as usize;

        let name_start = pos + 46;
        let name_end = name_start + name_len;
        if name_end > cd_buf.len() {
            break;
        }
        let raw_name = cd_buf[name_start..name_end].to_vec();
        results.push((raw_name, gpbf));

        pos = name_end + extra_len + comment_len;
    }

    Ok(results)
}

/// Decode a ZIP entry name from raw bytes using the general purpose bit flag.
/// Bit 11 = 1 → UTF-8. Bit 11 = 0 → try UTF-8, fallback to GBK (Natives2 approach).
fn decode_zip_name(raw: &[u8], gpbf: &u16) -> String {
    if gpbf & GPBF_UTF8 != 0 {
        // UTF-8 flag set — decode as UTF-8
        return String::from_utf8_lossy(raw).to_string();
    }

    // Try UTF-8 first (many modern tools set names as UTF-8 without the flag)
    if let Ok(s) = std::str::from_utf8(raw) {
        if !s.contains('\u{FFFD}') {
            return s.to_string();
        }
    }

    // Fallback: GBK decoding (Chinese Windows ZIP files)
    let (decoded, _, had_errors) = encoding_rs::GBK.decode(raw);
    if had_errors {
        // GBK also failed, return lossy UTF-8
        String::from_utf8_lossy(raw).to_string()
    } else {
        decoded.into_owned()
    }
}

fn find_signature(data: &[u8], sig: u32) -> Option<usize> {
    let sig_bytes = sig.to_le_bytes();
    data.windows(4)
        .rposition(|w| w == sig_bytes)
}

fn read_u16(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([data[offset], data[offset + 1]])
}

fn read_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

// ── tar/gz: UTF-8 with GBK fallback (Natives2) ──

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

    // Try strict UTF-8 first (Natives2: strict decode, not lossy)
    let stdout_str = match std::str::from_utf8(&output.stdout) {
        Ok(s) => s.to_string(),
        Err(_) => {
            // UTF-8 failed — try GBK fallback
            let (decoded, _, _) = encoding_rs::GBK.decode(&output.stdout);
            decoded.into_owned()
        }
    };

    let mut entries = Vec::new();
    let truncated = false;

    for line in stdout_str.lines() {
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
