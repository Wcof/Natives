//! Bounded binary preview generation (images, PDFs, audio/video).

use super::*;
use crate::{Error, Result};
use std::path::Path;

// Keep Base64-encoded responses below the Native Messaging 1 MiB envelope.
pub const MAX_IMAGE_PREVIEW_BYTES: u64 = 512 * 1024;
pub const MAX_PDF_PREVIEW_BYTES: u64 = 512 * 1024;
pub const MAX_MEDIA_PREVIEW_BYTES: u64 = 512 * 1024;

#[derive(Debug)]
pub struct ImagePreviewResult {
    pub bytes: Vec<u8>,
    pub mime_type: String,
    pub size: u64,
    pub mtime: f64,
}

pub fn read_image_preview(file_path: &str) -> Result<ImagePreviewResult> {
    let path = expand_tilde(file_path);
    validate_path(&path)?;
    let meta = std::fs::metadata(&path).map_err(Error::Io)?;
    if meta.is_dir() {
        return Err(Error::InvalidInput("is a directory".into()));
    }
    if meta.len() > MAX_IMAGE_PREVIEW_BYTES {
        return Err(Error::InvalidInput("image preview is too large".into()));
    }
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    if matches!(
        path.extension().and_then(|ext| ext.to_str()).map(|ext| {
            ext.eq_ignore_ascii_case("heic")
                || ext.eq_ignore_ascii_case("heif")
                || ext.eq_ignore_ascii_case("tif")
                || ext.eq_ignore_ascii_case("tiff")
        }),
        Some(true)
    ) {
        return read_sips_image_preview(&path, &meta);
    }
    let mime_type = match name
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        Some("avif") => "image/avif",
        _ => {
            return Err(Error::InvalidInput(
                "image preview format unsupported".into(),
            ))
        }
    };
    let bytes = std::fs::read(&path).map_err(Error::Io)?;
    let valid_header = match mime_type {
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        "image/gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        "image/webp" => bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"),
        "image/bmp" => bytes.starts_with(b"BM"),
        "image/avif" => bytes
            .get(4..12)
            .is_some_and(|value| value == b"ftypavif" || value == b"ftypavis"),
        _ => false,
    };
    if !valid_header {
        return Err(Error::InvalidInput("image header is invalid".into()));
    }
    Ok(ImagePreviewResult {
        bytes,
        mime_type: mime_type.into(),
        size: meta.len(),
        mtime: meta_mtime_ms(&meta),
    })
}

fn read_sips_image_preview(
    path: &Path,
    source_meta: &std::fs::Metadata,
) -> Result<ImagePreviewResult> {
    #[cfg(target_os = "macos")]
    {
        // ponytail: use the platform decoder instead of adding a large image
        // crate; upgrade only if non-macOS camera-format support is required.
        let output =
            std::env::temp_dir().join(format!("natives-image-{}.jpg", rand::random::<u64>()));
        let result = std::process::Command::new("sips")
            .args([
                "-s",
                "format",
                "jpeg",
                path.to_string_lossy().as_ref(),
                "--out",
            ])
            .arg(&output)
            .output()
            .map_err(|error| Error::Internal(format!("image decoder unavailable: {error}")))?;
        if !result.status.success() {
            let _ = std::fs::remove_file(&output);
            return Err(Error::InvalidInput("image preview unsupported".into()));
        }
        let converted = std::fs::metadata(&output).map_err(Error::Io)?;
        if converted.len() > MAX_IMAGE_PREVIEW_BYTES {
            let _ = std::fs::remove_file(&output);
            return Err(Error::InvalidInput("image preview is too large".into()));
        }
        let bytes = std::fs::read(&output).map_err(Error::Io)?;
        let _ = std::fs::remove_file(&output);
        if !bytes.starts_with(&[0xff, 0xd8, 0xff]) {
            return Err(Error::InvalidInput(
                "converted image header is invalid".into(),
            ));
        }
        return Ok(ImagePreviewResult {
            size: source_meta.len(),
            mtime: meta_mtime_ms(source_meta),
            bytes,
            mime_type: "image/jpeg".into(),
        });
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (path, source_meta);
        Err(Error::InvalidInput(
            "HEIC/HEIF/TIFF preview unsupported on this platform".into(),
        ))
    }
}

pub fn read_pdf_preview(file_path: &str) -> Result<ImagePreviewResult> {
    let path = expand_tilde(file_path);
    validate_path(&path)?;
    let meta = std::fs::metadata(&path).map_err(Error::Io)?;
    if meta.is_dir() {
        return Err(Error::InvalidInput("is a directory".into()));
    }
    if meta.len() > MAX_PDF_PREVIEW_BYTES {
        return Err(Error::InvalidInput("pdf preview is too large".into()));
    }
    let bytes = std::fs::read(&path).map_err(Error::Io)?;
    if !bytes.starts_with(b"%PDF-") {
        return Err(Error::InvalidInput("pdf header is invalid".into()));
    }
    Ok(ImagePreviewResult {
        bytes,
        mime_type: "application/pdf".into(),
        size: meta.len(),
        mtime: meta_mtime_ms(&meta),
    })
}

pub fn read_media_preview(file_path: &str) -> Result<ImagePreviewResult> {
    let path = expand_tilde(file_path);
    validate_path(&path)?;
    let meta = std::fs::metadata(&path).map_err(Error::Io)?;
    if meta.is_dir() {
        return Err(Error::InvalidInput("is a directory".into()));
    }
    if meta.len() > MAX_MEDIA_PREVIEW_BYTES {
        return Err(Error::InvalidInput("media preview is too large".into()));
    }
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let mime_type = match name
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("ogg") => "audio/ogg",
        Some("m4a") => "audio/mp4",
        Some("mp4") | Some("m4v") => "video/mp4",
        Some("webm") => "video/webm",
        Some("mov") => "video/quicktime",
        _ => {
            return Err(Error::InvalidInput(
                "media preview format unsupported".into(),
            ))
        }
    };
    let bytes = std::fs::read(&path).map_err(Error::Io)?;
    let valid_header = match mime_type {
        "audio/mpeg" => {
            bytes.starts_with(b"ID3") || bytes.first().is_some_and(|byte| byte & 0xe0 == 0xe0)
        }
        "audio/wav" => bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE"),
        "audio/ogg" => bytes.starts_with(b"OggS"),
        "audio/mp4" | "video/mp4" | "video/quicktime" => bytes.get(4..8) == Some(b"ftyp"),
        "video/webm" => bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]),
        _ => false,
    };
    if !valid_header {
        return Err(Error::InvalidInput("media header is invalid".into()));
    }
    Ok(ImagePreviewResult {
        bytes,
        mime_type: mime_type.into(),
        size: meta.len(),
        mtime: meta_mtime_ms(&meta),
    })
}
