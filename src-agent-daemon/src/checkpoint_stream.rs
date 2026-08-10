//! Bounded streaming file reads and the in-memory per-run checkpoint state.
//!
//! These are private building blocks: [`LiveCheckpoint`] is the process-local
//! map entry behind [`crate::checkpoint::CheckpointManager`], and
//! [`stream_read_capped`] is the bounded-memory file reader used by capture and
//! rewind hashing. Memory use is O(chunk), never O(file).

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::checkpoint::FileSnapshot;

/// Streaming-hash chunk size (memory stays bounded for arbitrarily large files).
const STREAM_CHUNK_BYTES: usize = 64 * 1024;

/// Process-local in-memory checkpoint state for one run.
#[derive(Debug, Default)]
pub(crate) struct LiveCheckpoint {
    pub(crate) id: String,
    pub(crate) conversation_id: String,
    pub(crate) run_id: String,
    pub(crate) project_root: PathBuf,
    pub(crate) files: HashMap<String, FileSnapshot>,
    /// Total content bytes persisted for this run (drives the byte quota).
    pub(crate) captured_bytes: u64,
}

/// Result of a bounded, streaming file read.
pub(crate) struct StreamedRead {
    pub(crate) hash: String,
    pub(crate) size: u64,
    /// Content only when the file fits the content cap AND is valid UTF-8.
    pub(crate) content: Option<String>,
    /// True when the file exceeded the per-file content cap (content dropped).
    pub(crate) over_cap: bool,
}

/// Stream a file in bounded chunks, hashing the whole content (so the hash
/// stays a stable identity for conflict detection) while capping how much
/// content is retained. Memory use is O(chunk), never O(file).
pub(crate) fn stream_read_capped(path: &Path, content_cap: u64) -> Result<StreamedRead, String> {
    let mut file = std::fs::File::open(path).map_err(|e| format!("checkpoint read failed: {e}"))?;
    let size = file.metadata().map_err(|e| e.to_string())?.len();
    let mut hasher = Sha256::new();
    let mut content = Vec::new();
    let mut over_cap = false;
    let mut buf = vec![0u8; STREAM_CHUNK_BYTES];
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        if !over_cap {
            let remaining = (content_cap as usize).saturating_sub(content.len());
            let take = n.min(remaining);
            content.extend_from_slice(&buf[..take]);
            if take < n {
                over_cap = true;
            }
        }
    }
    let content = if over_cap {
        None
    } else {
        String::from_utf8(content).ok()
    };
    Ok(StreamedRead {
        hash: format!("{:x}", hasher.finalize()),
        size,
        content,
        over_cap,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checkpoint::MAX_CAPTURE_FILE_CONTENT_BYTES;
    use uuid::Uuid;

    /// A 1GB sparse file must not be read into memory: `stream_read_capped`
    /// hashes it in bounded chunks and stores no content. The streaming helper
    /// itself is the memory-bounded primitive; the manager applies it.
    #[test]
    fn one_gigabyte_file_streams_with_bounded_memory() {
        let dir = std::env::temp_dir().join(format!("cp-gb-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("giant.bin");
        // Sparse file: instant to create, 1 GiB of zeros on disk.
        let f = std::fs::File::create(&path).unwrap();
        f.set_len(1024 * 1024 * 1024).unwrap();
        drop(f);
        let read = stream_read_capped(&path, MAX_CAPTURE_FILE_CONTENT_BYTES).unwrap();
        assert_eq!(read.size, 1024 * 1024 * 1024);
        assert!(read.over_cap, "1GiB file must exceed the content cap");
        assert!(read.content.is_none(), "1GiB content must not be buffered");
        assert_eq!(read.hash.len(), 64, "full SHA-256 hex digest");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
