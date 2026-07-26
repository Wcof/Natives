//! 压缩包操作共享模块（W7）
//!
//! - `safe_extract_zip`：带路径穿越 / 绝对路径 / 符号链接防护的 zip 解压。
//!   原实现位于 `creative_app/probe.rs`，上移到此处供 creative_app 与
//!   文件管理器共用（creative_app 行为不变：符号链接条目直接报错）。
//! - `extract_archive`：文件管理器解压命令核心。zip 走共享 safe 实现
//!   （符号链接条目跳过）；tar 家族走系统 `tar` 子进程。
//! - `compress_entries`：多文件/目录打包为 zip。

use crate::{file_manager, Error, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};
use ts_rs::TS;

/// 单次解压总字节数上限（防 zip 炸弹）：4 GiB
const MAX_EXTRACT_TOTAL: u64 = 4 * 1024 * 1024 * 1024;

/// 符号链接条目处理策略
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymlinkPolicy {
    /// 遇到符号链接条目立即报错（creative_app 安装路径的历史行为）
    Reject,
    /// 跳过符号链接条目继续解压（文件管理器解压命令）
    Skip,
}

/// 解压结果（wire 契约）
#[derive(Debug, Serialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ExtractArchiveResult {
    pub ok: bool,
    pub dest_path: String,
    pub entry_count: u32,
}

/// 压缩结果（wire 契约）
#[derive(Debug, Serialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
#[serde(rename_all = "camelCase")]
pub struct CompressEntriesResult {
    pub ok: bool,
    pub zip_path: String,
    pub entry_count: u32,
}

/// Safe ZIP extract under dest; rejects traversal, absolute paths, symlinks.
/// （creative_app 兼容入口：符号链接条目报错，行为与上移前一致）
pub fn safe_extract_zip(zip_path: &Path, dest: &Path, max_total: u64) -> Result<Vec<PathBuf>> {
    safe_extract_zip_with(zip_path, dest, max_total, SymlinkPolicy::Reject)
}

/// safe_extract_zip 的带策略版本：每个条目解析后确认落在 dest 内（防 zip-slip），
/// 符号链接条目按 `symlink_policy` 处理。
pub fn safe_extract_zip_with(
    zip_path: &Path,
    dest: &Path,
    max_total: u64,
    symlink_policy: SymlinkPolicy,
) -> Result<Vec<PathBuf>> {
    use std::io::Read;
    let file = std::fs::File::open(zip_path).map_err(Error::Io)?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| Error::Internal(format!("zip open: {e}")))?;
    std::fs::create_dir_all(dest).map_err(Error::Io)?;
    let dest_canon = dest
        .canonicalize()
        .map_err(|e| Error::Internal(format!("dest canonicalize: {e}")))?;

    let mut written = Vec::new();
    let mut total: u64 = 0;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| Error::Internal(format!("zip entry: {e}")))?;
        let name = entry.name().to_string();
        // 名字级防线：`..` / 绝对路径直接拒绝（先于任何落盘动作）
        if name.contains("..") || name.starts_with('/') || name.starts_with('\\') {
            return Err(Error::InvalidInput(format!("unsafe path in zip: {name}")));
        }
        // 符号链接条目：按策略拒绝或跳过（跟随解压可写到 dest 之外）
        #[allow(deprecated)]
        {
            if entry
                .unix_mode()
                .map(|m| (m & 0o170000) == 0o120000)
                .unwrap_or(false)
            {
                match symlink_policy {
                    SymlinkPolicy::Reject => {
                        return Err(Error::InvalidInput(format!(
                            "symlink in zip rejected: {name}"
                        )));
                    }
                    SymlinkPolicy::Skip => continue,
                }
            }
        }
        let outpath = dest.join(&name);
        // 落盘级防线：父目录 canonicalize 后必须仍在 dest 内
        if let Some(parent) = outpath.parent() {
            std::fs::create_dir_all(parent).map_err(Error::Io)?;
            let parent_c = parent.canonicalize().map_err(Error::Io)?;
            if !parent_c.starts_with(&dest_canon) {
                return Err(Error::InvalidInput(format!(
                    "zip path escapes dest: {name}"
                )));
            }
        }
        if entry.is_dir() {
            std::fs::create_dir_all(&outpath).map_err(Error::Io)?;
        } else {
            let mut outfile = std::fs::File::create(&outpath).map_err(Error::Io)?;
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf).map_err(Error::Io)?;
            total += buf.len() as u64;
            if total > max_total {
                return Err(Error::InvalidInput(format!(
                    "zip total size exceeds limit {max_total}"
                )));
            }
            use std::io::Write;
            outfile.write_all(&buf).map_err(Error::Io)?;
            written.push(outpath);
        }
    }
    Ok(written)
}

/// 去掉压缩包扩展名得到默认解压目录名（foo.tar.gz → foo）
fn archive_stem(file_name: &str) -> String {
    let lower = file_name.to_lowercase();
    // 复合扩展优先
    for suffix in [".tar.gz", ".tar.bz2", ".tar.xz"] {
        if lower.ends_with(suffix) {
            return file_name[..file_name.len() - suffix.len()].to_string();
        }
    }
    for suffix in [".zip", ".tar", ".tgz", ".tbz2", ".txz"] {
        if lower.ends_with(suffix) {
            return file_name[..file_name.len() - suffix.len()].to_string();
        }
    }
    file_name.to_string()
}

/// 压缩包格式分类
enum ArchiveKind {
    Zip,
    Tar,
}

fn detect_archive_kind(file_name: &str) -> Result<ArchiveKind> {
    let lower = file_name.to_lowercase();
    if lower.ends_with(".zip") {
        return Ok(ArchiveKind::Zip);
    }
    if lower.ends_with(".tar")
        || lower.ends_with(".tar.gz")
        || lower.ends_with(".tgz")
        || lower.ends_with(".tar.bz2")
        || lower.ends_with(".tbz2")
        || lower.ends_with(".tar.xz")
        || lower.ends_with(".txz")
    {
        return Ok(ArchiveKind::Tar);
    }
    Err(Error::InvalidInput(format!(
        "unsupported archive format: {file_name}"
    )))
}

/// 解压压缩包。dest_dir 缺省为压缩包所在目录下的同名文件夹（deduplicate 防覆盖）。
pub fn extract_archive(archive_path: &str, dest_dir: Option<&str>) -> Result<ExtractArchiveResult> {
    let archive = file_manager::expand_tilde(archive_path);
    file_manager::validate_path(&archive)?;
    if !archive.is_file() {
        return Err(Error::NotFound(archive_path.to_string()));
    }
    let file_name = archive
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| Error::InvalidInput("invalid archive file name".into()))?
        .to_string();
    let kind = detect_archive_kind(&file_name)?;

    // 解压目标目录：显式给出则直接用；缺省取同目录同名文件夹并防覆盖
    let dest = match dest_dir {
        Some(d) => {
            let p = file_manager::expand_tilde(d);
            file_manager::validate_path(&p)?;
            p
        }
        None => {
            let parent = archive
                .parent()
                .ok_or_else(|| Error::InvalidInput("archive has no parent dir".into()))?;
            let default = parent.join(archive_stem(&file_name));
            file_manager::deduplicate_path(&default)?
        }
    };
    file_manager::validate_path(&dest)?;

    let entry_count = match kind {
        ArchiveKind::Zip => {
            // zip 走共享 safe 实现；文件管理器语境下符号链接条目跳过
            let written =
                safe_extract_zip_with(&archive, &dest, MAX_EXTRACT_TOTAL, SymlinkPolicy::Skip)?;
            written.len()
        }
        ArchiveKind::Tar => {
            std::fs::create_dir_all(&dest).map_err(Error::Io)?;
            // 系统 tar（bsdtar 自动识别 gz/bz2/xz）。路径全部走旗标参数并以 `--`
            // 结束选项区，防选项注入；bsdtar 默认拒绝绝对路径与 `..` 穿越（未传 -P）。
            let output = std::process::Command::new("tar")
                .arg("-x")
                .arg("-C")
                .arg(&dest)
                .arg("-f")
                .arg(&archive)
                .arg("--")
                .output()
                .map_err(|e| Error::Internal(format!("tar spawn failed: {e}")))?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(Error::Internal(format!(
                    "tar exited with {}: {}",
                    output.status,
                    stderr.chars().take(300).collect::<String>()
                )));
            }
            count_files_recursive(&dest)
        }
    };

    Ok(ExtractArchiveResult {
        ok: true,
        dest_path: dest.to_string_lossy().to_string(),
        entry_count: entry_count as u32,
    })
}

/// 递归统计目录下文件数（tar 解压后计数用）
fn count_files_recursive(dir: &Path) -> usize {
    let mut count = 0usize;
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(dir.to_path_buf());
    while let Some(d) = queue.pop_front() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            // 用 symlink_metadata 避免跟随链接
            let Ok(meta) = std::fs::symlink_metadata(&p) else {
                continue;
            };
            if meta.is_dir() {
                queue.push_back(p);
            } else {
                count += 1;
            }
        }
    }
    count
}

/// 把多个文件/目录打包为 zip。dest_zip_path 缺省为第一项同目录下 `<名称>.zip`（防覆盖）。
pub fn compress_entries(paths: &[String], dest_zip_path: Option<&str>) -> Result<CompressEntriesResult> {
    if paths.is_empty() {
        return Err(Error::InvalidInput("no paths to compress".into()));
    }
    // 全部输入先过白名单并确认存在
    let mut inputs: Vec<PathBuf> = Vec::with_capacity(paths.len());
    for p in paths {
        let expanded = file_manager::expand_tilde(p);
        file_manager::validate_path(&expanded)?;
        if !expanded.exists() {
            return Err(Error::NotFound(p.clone()));
        }
        inputs.push(expanded);
    }

    // 目标 zip 路径：缺省取第一项的名称（文件去扩展名，目录取原名）
    let zip_path = match dest_zip_path {
        Some(d) => {
            let p = file_manager::expand_tilde(d);
            file_manager::validate_path(&p)?;
            file_manager::deduplicate_path(&p)?
        }
        None => {
            let first = &inputs[0];
            let parent = first
                .parent()
                .ok_or_else(|| Error::InvalidInput("path has no parent dir".into()))?;
            let base = if first.is_dir() {
                first
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("archive")
                    .to_string()
            } else {
                first
                    .file_stem()
                    .and_then(|n| n.to_str())
                    .unwrap_or("archive")
                    .to_string()
            };
            file_manager::deduplicate_path(&parent.join(format!("{base}.zip")))?
        }
    };

    let file = std::fs::File::create(&zip_path).map_err(Error::Io)?;
    let mut writer = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default();
    let mut entry_count = 0u32;

    for input in &inputs {
        let name = input
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| Error::InvalidInput("invalid entry name".into()))?;
        let meta = std::fs::symlink_metadata(input).map_err(Error::Io)?;
        if meta.file_type().is_symlink() {
            // 符号链接不入包：避免把 zip 变成指向包外的引用
            continue;
        }
        if meta.is_dir() {
            entry_count += add_dir_to_zip(&mut writer, input, name, opts)?;
        } else {
            add_file_to_zip(&mut writer, input, name, opts)?;
            entry_count += 1;
        }
    }

    writer
        .finish()
        .map_err(|e| Error::Internal(format!("zip finish: {e}")))?;

    Ok(CompressEntriesResult {
        ok: true,
        zip_path: zip_path.to_string_lossy().to_string(),
        entry_count,
    })
}

fn add_file_to_zip(
    writer: &mut zip::ZipWriter<std::fs::File>,
    file_path: &Path,
    entry_name: &str,
    opts: zip::write::SimpleFileOptions,
) -> Result<()> {
    use std::io::Write;
    writer
        .start_file(entry_name, opts)
        .map_err(|e| Error::Internal(format!("zip start_file: {e}")))?;
    let data = std::fs::read(file_path).map_err(Error::Io)?;
    writer.write_all(&data).map_err(Error::Io)?;
    Ok(())
}

/// 目录递归入包（条目名以顶层目录名为前缀）；返回写入的文件条目数
fn add_dir_to_zip(
    writer: &mut zip::ZipWriter<std::fs::File>,
    dir: &Path,
    prefix: &str,
    opts: zip::write::SimpleFileOptions,
) -> Result<u32> {
    let mut count = 0u32;
    writer
        .add_directory(format!("{prefix}/"), opts)
        .map_err(|e| Error::Internal(format!("zip add_directory: {e}")))?;
    for entry in std::fs::read_dir(dir).map_err(Error::Io)? {
        let entry = entry.map_err(Error::Io)?;
        let p = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let child_name = format!("{prefix}/{name}");
        let meta = std::fs::symlink_metadata(&p).map_err(Error::Io)?;
        if meta.file_type().is_symlink() {
            continue; // 符号链接跳过
        }
        if meta.is_dir() {
            count += add_dir_to_zip(writer, &p, &child_name, opts)?;
        } else {
            add_file_to_zip(writer, &p, &child_name, opts)?;
            count += 1;
        }
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir()
            .join("natives-archive-ops-tests")
            .join(format!("{name}-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&d).unwrap();
        // 走 canonicalize，规避 /tmp → /private/tmp 别名导致 starts_with 判断失真
        d.canonicalize().unwrap()
    }

    /// 恶意 zip：`../` 穿越条目必须被拦
    #[test]
    fn zip_slip_traversal_rejected() {
        let dir = test_dir("slip");
        let zip_path = dir.join("bad.zip");
        {
            let f = std::fs::File::create(&zip_path).unwrap();
            let mut zip = zip::ZipWriter::new(f);
            let opts = zip::write::SimpleFileOptions::default();
            zip.start_file("../evil.txt", opts).unwrap();
            use std::io::Write;
            zip.write_all(b"x").unwrap();
            zip.finish().unwrap();
        }
        let dest = dir.join("out");
        let err = safe_extract_zip(&zip_path, &dest, 1024 * 1024).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unsafe") || msg.contains("escapes"), "{msg}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 恶意 zip：绝对路径条目必须被拦
    #[test]
    fn zip_absolute_path_rejected() {
        let dir = test_dir("abs");
        let zip_path = dir.join("abs.zip");
        {
            let f = std::fs::File::create(&zip_path).unwrap();
            let mut zip = zip::ZipWriter::new(f);
            let opts = zip::write::SimpleFileOptions::default();
            zip.start_file("/tmp/natives-abs-evil.txt", opts).unwrap();
            use std::io::Write;
            zip.write_all(b"x").unwrap();
            zip.finish().unwrap();
        }
        let dest = dir.join("out");
        let err = safe_extract_zip(&zip_path, &dest, 1024 * 1024).unwrap_err();
        assert!(err.to_string().contains("unsafe"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 符号链接条目：Reject 策略报错，Skip 策略跳过且不落盘
    #[test]
    fn zip_symlink_reject_and_skip() {
        let dir = test_dir("symlink");
        let zip_path = dir.join("link.zip");
        {
            let f = std::fs::File::create(&zip_path).unwrap();
            let mut zip = zip::ZipWriter::new(f);
            let opts = zip::write::SimpleFileOptions::default();
            zip.add_symlink("evil-link", "/etc/passwd", opts).unwrap();
            zip.start_file("normal.txt", opts).unwrap();
            use std::io::Write;
            zip.write_all(b"hello").unwrap();
            zip.finish().unwrap();
        }
        // Reject（creative_app 历史行为）
        let dest1 = dir.join("out-reject");
        let err = safe_extract_zip(&zip_path, &dest1, 1024 * 1024).unwrap_err();
        assert!(err.to_string().contains("symlink"), "{err}");
        // Skip（文件管理器行为）：链接不落盘，普通文件照常解出
        let dest2 = dir.join("out-skip");
        let written =
            safe_extract_zip_with(&zip_path, &dest2, 1024 * 1024, SymlinkPolicy::Skip).unwrap();
        assert_eq!(written.len(), 1);
        assert!(dest2.join("normal.txt").is_file());
        assert!(std::fs::symlink_metadata(dest2.join("evil-link")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 正常往返：compress → extract → 内容一致
    #[test]
    fn compress_extract_roundtrip() {
        let dir = test_dir("roundtrip");
        let sub = dir.join("proj");
        std::fs::create_dir_all(sub.join("nested")).unwrap();
        std::fs::write(sub.join("a.txt"), b"alpha").unwrap();
        std::fs::write(sub.join("nested").join("b.txt"), b"beta").unwrap();

        let res = compress_entries(&[sub.to_string_lossy().to_string()], None).unwrap();
        assert!(res.ok);
        assert_eq!(res.entry_count, 2);
        assert!(res.zip_path.ends_with("proj.zip"), "{}", res.zip_path);

        let out = extract_archive(&res.zip_path, None).unwrap();
        assert!(out.ok);
        assert_eq!(out.entry_count, 2);
        let out_dir = PathBuf::from(&out.dest_path);
        assert_eq!(
            std::fs::read(out_dir.join("proj").join("a.txt")).unwrap(),
            b"alpha"
        );
        assert_eq!(
            std::fs::read(out_dir.join("proj").join("nested").join("b.txt")).unwrap(),
            b"beta"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 缺省解压目录重名时 deduplicate（不覆盖既有目录）
    #[test]
    fn extract_default_dest_deduplicates() {
        let dir = test_dir("dedupe");
        std::fs::write(dir.join("data.txt"), b"d").unwrap();
        let res = compress_entries(&[dir.join("data.txt").to_string_lossy().to_string()], None)
            .unwrap();
        // 预先占用同名目录
        std::fs::create_dir_all(dir.join("data")).unwrap();
        let out = extract_archive(&res.zip_path, None).unwrap();
        assert!(out.dest_path.ends_with("data (1)"), "{}", out.dest_path);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// tar 解压一例（系统 tar 打包 → extract_archive 解压）
    #[test]
    fn extract_tar_gz() {
        let dir = test_dir("tar");
        let src = dir.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("hello.txt"), b"tar content").unwrap();
        let tar_path = dir.join("bundle.tar.gz");
        let status = std::process::Command::new("tar")
            .arg("-c")
            .arg("-z")
            .arg("-C")
            .arg(&dir)
            .arg("-f")
            .arg(&tar_path)
            .arg("src")
            .status()
            .unwrap();
        assert!(status.success());

        let out = extract_archive(&tar_path.to_string_lossy(), None).unwrap();
        assert!(out.ok);
        assert_eq!(out.entry_count, 1);
        let out_dir = PathBuf::from(&out.dest_path);
        assert!(out_dir.ends_with("bundle"), "{}", out.dest_path);
        assert_eq!(
            std::fs::read(out_dir.join("src").join("hello.txt")).unwrap(),
            b"tar content"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn archive_stem_strips_compound_extensions() {
        assert_eq!(archive_stem("foo.zip"), "foo");
        assert_eq!(archive_stem("foo.tar.gz"), "foo");
        assert_eq!(archive_stem("foo.tgz"), "foo");
        assert_eq!(archive_stem("foo.tar.bz2"), "foo");
        assert_eq!(archive_stem("foo.tar.xz"), "foo");
        assert_eq!(archive_stem("noext"), "noext");
    }
}
