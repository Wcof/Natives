//! Streaming and batch file import operations into authorized directories.

use super::*;
use crate::{Error, Result};
use std::io::Write;
use std::path::{Path, PathBuf};

/// The browser sends one bounded chunk at a time. Keeping this limit in the
/// core makes it impossible for a future adapter to accidentally turn import
/// into an unbounded in-memory write.
pub const MAX_IMPORT_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub const MAX_IMPORT_CHUNK_BYTES: usize = 512 * 1024;

/// A streaming import writer. The temporary file lives beside its final
/// target, so commit can use a same-filesystem, no-overwrite hard link.
/// Dropping an unfinished writer removes the temporary file.
pub struct ImportWriter {
    file: Option<std::fs::File>,
    temp_path: PathBuf,
    target_path: PathBuf,
    expected_size: u64,
    received: u64,
}

#[derive(Debug)]
pub struct ImportResult {
    pub path: String,
    pub size: u64,
    pub mtime: f64,
}

impl ImportWriter {
    pub fn target_path(&self) -> &Path {
        &self.target_path
    }

    pub fn expected_size(&self) -> u64 {
        self.expected_size
    }

    pub fn received(&self) -> u64 {
        self.received
    }

    pub fn write_chunk(&mut self, offset: u64, bytes: &[u8]) -> Result<()> {
        if bytes.len() > MAX_IMPORT_CHUNK_BYTES {
            return Err(Error::InvalidInput("import chunk is too large".into()));
        }
        if offset != self.received {
            return Err(Error::InvalidInput("import chunk offset is invalid".into()));
        }
        let next = self
            .received
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| Error::InvalidInput("import size is invalid".into()))?;
        if next > self.expected_size {
            return Err(Error::InvalidInput("import exceeds declared size".into()));
        }
        self.file
            .as_mut()
            .ok_or_else(|| Error::InvalidInput("import is closed".into()))?
            .write_all(bytes)
            .map_err(Error::Io)?;
        self.received = next;
        Ok(())
    }

    pub fn finish(mut self) -> Result<ImportResult> {
        if self.received != self.expected_size {
            return Err(Error::InvalidInput("import is incomplete".into()));
        }
        let file = self
            .file
            .take()
            .ok_or_else(|| Error::InvalidInput("import is closed".into()))?;
        file.sync_all().map_err(Error::Io)?;
        drop(file);

        // hard_link is the portable no-overwrite commit primitive available
        // here: both paths are in the same authorized parent. It also refuses
        // a race where another process creates the target after begin_import.
        if std::fs::symlink_metadata(&self.target_path).is_ok() {
            return Err(Error::Message("destination exists".into()));
        }
        std::fs::hard_link(&self.temp_path, &self.target_path).map_err(Error::Io)?;
        std::fs::remove_file(&self.temp_path).map_err(Error::Io)?;
        let mtime = std::fs::metadata(&self.target_path)
            .ok()
            .map(|meta| meta_mtime_ms(&meta))
            .unwrap_or(0.0);
        Ok(ImportResult {
            path: self.target_path.to_string_lossy().to_string(),
            size: self.expected_size,
            mtime,
        })
    }
}

impl Drop for ImportWriter {
    fn drop(&mut self) {
        let _ = self.file.take();
        let _ = std::fs::remove_file(&self.temp_path);
    }
}

/// Start a streaming import into `parent`. `conflict = "rename"` chooses a
/// numbered sibling; `"skip"` rejects an existing destination. No overwrite
/// mode exists for imports, preventing an accidental data loss path.
pub fn begin_import(parent: &str, name: &str, size: u64, conflict: &str) -> Result<ImportWriter> {
    if size > MAX_IMPORT_BYTES {
        return Err(Error::InvalidInput("import is too large".into()));
    }
    if !valid_name(name) {
        return Err(Error::InvalidInput("invalid import name".into()));
    }
    if !matches!(conflict, "rename" | "skip") {
        return Err(Error::InvalidInput("invalid import conflict policy".into()));
    }
    let authorized_parent = FileAccessPolicy::authorize_path(parent, OperationPolicy::Write)?;
    if !authorized_parent.as_path().is_dir() {
        return Err(Error::InvalidInput(
            "import parent is not a directory".into(),
        ));
    }
    let requested = authorized_parent.as_path().join(name);
    let target = if std::fs::symlink_metadata(&requested).is_ok() {
        if conflict == "skip" {
            return Err(Error::Message("destination exists".into()));
        }
        deduplicate_path(&requested)?
    } else {
        requested
    };
    let authorized_target = FileAccessPolicy::authorize_path_buf(&target, OperationPolicy::Write)?;
    let target = authorized_target.as_path().to_path_buf();
    let temp_path = target
        .parent()
        .unwrap_or_else(|| Path::new("/"))
        .join(format!(
            ".natives-import-{}-{}",
            std::process::id(),
            rand_suffix()
        ));
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)
        .map_err(Error::Io)?;
    Ok(ImportWriter {
        file: Some(file),
        temp_path,
        target_path: target,
        expected_size: size,
        received: 0,
    })
}

/// Import files (copy from external paths)
pub fn import_files(source_paths: &[String], dest_dir: &str) -> Result<Vec<String>> {
    let dest = expand_tilde(dest_dir);
    validate_path(&dest)?;

    let mut result = Vec::new();
    for src_str in source_paths {
        let src = PathBuf::from(src_str);
        if !src.exists() {
            // Frontend contract is Vec<String> of imported paths; keep the shape
            // but surface skipped sources in the log instead of silently dropping.
            eprintln!("import_files: skipping missing source: {src_str}");
            continue;
        }
        // Drag/drop sources are still caller-controlled. Require the source
        // itself to cross the same authorization boundary before reading it.
        let src = FileAccessPolicy::authorize_path_buf(&src, OperationPolicy::Read)?;
        let src = src.as_path();
        let file_name = src.file_name().and_then(|n| n.to_str()).unwrap_or("file");
        let target = deduplicate_path(&dest.join(file_name))?;

        if src.is_dir() {
            copy_dir_recursive(&src, &target)?;
        } else {
            std::fs::copy(&src, &target).map_err(Error::Io)?;
        }
        result.push(target.to_string_lossy().to_string());
    }
    Ok(result)
}
