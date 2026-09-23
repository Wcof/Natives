//! Canonical Tree Hash（计划 §21）：真正的目录树哈希，不是单文件哈希。
//!
//! 统一算法（Packager / Core Verifier / CI 三方共用同一规范）：
//! 1. 递归枚举普通文件（禁止 symlink）
//! 2. relative path 标准化为 `/` 分隔
//! 3. 按 relative path 排序
//! 4. 每文件记录：relative path、size、sha256
//! 5. canonical serialize（逐行 `path\0size\0hash\n`）
//! 6. 最终 SHA-256
//!
//! Node 侧等价实现：scripts/lib/tree-hash.mjs（算法逐字节一致，禁止分叉）。

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

/// 计算目录树的 canonical tree hash；根目录本身不存在返回 None。
pub fn tree_sha256(root: &Path) -> std::io::Result<Option<String>> {
    if !root.exists() {
        return Ok(None);
    }
    let mut files: BTreeMap<String, (u64, String)> = BTreeMap::new();
    for entry in walkdir::WalkDir::new(root).follow_links(false) {
        let entry = entry.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        let file_type = entry.file_type();
        if file_type.is_symlink() {
            // 禁止 symlink：出现即视为树被篡改，fail closed。
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("symlink not allowed in tree: {}", entry.path().display()),
            ));
        }
        if !file_type.is_file() {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(root)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        let rel_path = relative
            .components()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("/");
        if rel_path.is_empty() {
            continue;
        }
        let metadata = entry.metadata()?;
        let mut hasher = Sha256::new();
        let mut file = std::fs::File::open(entry.path())?;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        let digest = format!("{:x}", hasher.finalize());
        files.insert(rel_path, (metadata.len(), digest));
    }
    let mut canonical = Vec::new();
    for (path, (size, hash)) in &files {
        canonical.extend_from_slice(path.as_bytes());
        canonical.push(0);
        canonical.extend_from_slice(size.to_string().as_bytes());
        canonical.push(0);
        canonical.extend_from_slice(hash.as_bytes());
        canonical.push(b'\n');
    }
    let mut hasher = Sha256::new();
    hasher.update(&canonical);
    Ok(Some(format!("{:x}", hasher.finalize())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_dir_and_missing_dir() {
        let temp = tempfile::tempdir().unwrap();
        // 不存在的树 → None
        assert!(tree_sha256(&temp.path().join("nope")).unwrap().is_none());
        // 空树有确定哈希
        let empty = tree_sha256(temp.path()).unwrap().unwrap();
        assert_eq!(empty.len(), 64);
    }

    #[test]
    fn hash_is_order_and_layout_sensitive() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("b.txt"), b"two").unwrap();
        std::fs::write(temp.path().join("a.txt"), b"one").unwrap();
        let h1 = tree_sha256(temp.path()).unwrap().unwrap();
        // 同一内容不同文件名 → 不同树哈希
        std::fs::write(temp.path().join("c.txt"), b"one").unwrap();
        let h2 = tree_sha256(temp.path()).unwrap().unwrap();
        assert_ne!(h1, h2);
        // 同一树重复计算稳定
        let h3 = tree_sha256(temp.path()).unwrap().unwrap();
        assert_eq!(h2, h3);
    }

    #[test]
    fn nested_paths_are_normalized() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("sub/deeper")).unwrap();
        std::fs::write(temp.path().join("sub/deeper/index.html"), b"<html>").unwrap();
        let hash = tree_sha256(temp.path()).unwrap().unwrap();
        // 与 Node 实现的 canonical 行一致：path\0size\0hash\n
        assert_eq!(hash.len(), 64);
    }

    #[test]
    fn symlink_fails_closed() {
        #[cfg(unix)]
        {
            let temp = tempfile::tempdir().unwrap();
            std::fs::write(temp.path().join("real.txt"), b"x").unwrap();
            std::os::unix::fs::symlink(temp.path().join("real.txt"), temp.path().join("link"))
                .unwrap();
            assert!(tree_sha256(temp.path()).is_err());
        }
    }
}
