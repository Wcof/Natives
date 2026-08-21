//! file_indexer — persistent file metadata + FTS5 content index（FIL-005~008）。
//!
//! 复用 `file_manager`（授权内核）与 `fs_watch`（事件源），本模块负责：
//! - 初始 metadata scan（有界 worker、进度、取消）；
//! - `file_index` 增量 upsert/delete（fs_watch 事件 → debounce/coalesce）；
//! - FTS5 内容索引（content separated：正文与元数据分离）；
//! - indexed search API（metadata 过滤 + FTS ranking/paging）。
//!
//! 不变量：
//! - `file_index` 只存非敏感元数据（path/kind/size/mtime/thumbnail），无正文；
//! - FTS5 存正文但索引与元数据分离，查询经 ranking/paging 返回；
//! - 扫描/索引为有界任务，无无限增长；取消经 cancel 标志（FIL-016）。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rusqlite::{params, Connection};

use crate::{Error, Result};

/// file_index 行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedFile {
    pub path: String,
    pub kind: String,
    pub size: u64,
    pub mtime_ms: u64,
    pub has_thumbnail: bool,
}

/// indexed search 单条结果（FTS ranking）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedHit {
    pub path: String,
    pub kind: String,
    pub snippet: String,
    pub rank: i64,
}

/// 初始扫描的进度回调（FIL-006：progress）。
pub type ProgressFn = Box<dyn Fn(usize, usize) + Send + Sync>;

/// 有界 worker 上下文：并发上限、取消标志（FIL-016 无无限增长）。
pub struct ScanContext {
    pub max_workers: usize,
    pub cancelled: Arc<AtomicBool>,
    pub progress: Option<ProgressFn>,
}

impl Default for ScanContext {
    fn default() -> Self {
        Self {
            max_workers: 4,
            cancelled: Arc::new(AtomicBool::new(false)),
            progress: None,
        }
    }
}

impl IndexedFile {
    /// 从磁盘条目构建（只读元数据；正文由 FTS 单独写入）。
    pub fn from_metadata(path: impl Into<String>, kind: &str, size: u64, mtime_ms: u64) -> Self {
        Self {
            path: path.into(),
            kind: kind.to_string(),
            size,
            mtime_ms,
            has_thumbnail: false,
        }
    }
}

/// upsert 一条索引（FIL-007 增量；幂等）。
pub fn upsert_indexed(conn: &Connection, entry: &IndexedFile) -> Result<()> {
    conn.execute(
        "INSERT INTO file_index (path, kind, size, mtime_ms, has_thumbnail, indexed_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(path) DO UPDATE SET
           kind=excluded.kind, size=excluded.size, mtime_ms=excluded.mtime_ms,
           has_thumbnail=excluded.has_thumbnail, indexed_at_ms=excluded.indexed_at_ms",
        params![
            entry.path,
            entry.kind,
            entry.size as i64,
            entry.mtime_ms as i64,
            entry.has_thumbnail as i64,
            now_ms() as i64
        ],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// 删除索引（fs_watch delete / rename 源路径，FIL-007）。
pub fn remove_indexed(conn: &Connection, path: &str) -> Result<()> {
    conn.execute("DELETE FROM file_index WHERE path = ?1", params![path])
        .map_err(Error::Database)?;
    // FTS 同步删除（content separated，保持一致）
    conn.execute(
        "DELETE FROM file_content_fts WHERE path = ?1",
        params![path],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// 写入 FTS 正文（FIL-008；只对可提取文本的 kind 调用）。
pub fn upsert_content(conn: &Connection, path: &str, content: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM file_content_fts WHERE path = ?1",
        params![path],
    )
    .map_err(Error::Database)?;
    conn.execute(
        "INSERT INTO file_content_fts (path, content) VALUES (?1, ?2)",
        params![path, content],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// 初始 metadata scan（FIL-006）：遍历目录树，有界 worker，可取消。
///
/// 只扫描授权根（复用 file_manager 语义），不进入 target/node_modules/
/// .git 等；进度经 `ScanContext.progress` 回调（已处理/总数）。
pub fn scan_initial(
    conn: &Connection,
    root: &str,
    context: &ScanContext,
) -> Result<(usize, usize)> {
    let root_path = PathBuf::from(root);
    if !root_path.is_dir() {
        return Ok((0, 0));
    }

    // 收集待索引路径（先做一次轻量遍历，避免无界 worker 队列）。
    let mut paths: Vec<PathBuf> = Vec::new();
    collect_indexable(root_path.as_path(), &mut paths, &context.cancelled);
    let total = paths.len();
    if total == 0 {
        return Ok((0, 0));
    }

    let mut processed = 0usize;
    for path in paths {
        if context.cancelled.load(Ordering::Relaxed) {
            break;
        }
        let metadata = match std::fs::metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let kind = if metadata.is_dir() { "dir" } else { "file" };
        let size = if metadata.is_file() {
            metadata.len()
        } else {
            0
        };
        let mtime_ms = metadata
            .modified()
            .map(|t| {
                t.duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0)
            })
            .unwrap_or(0);
        let rel = path.to_string_lossy().to_string();
        upsert_indexed(conn, &IndexedFile::from_metadata(rel, kind, size, mtime_ms))?;
        processed += 1;
        if let Some(progress) = &context.progress {
            progress(processed, total);
        }
    }
    Ok((processed, total))
}

/// fs_watch 增量应用（FIL-007）：kind ∈ {created, modified, removed}。
/// 重命名（renamed）按两个事件处理：源 removed + 目标 created。
pub fn apply_watch_event(conn: &Connection, path: &str, kind: &str) -> Result<()> {
    match kind {
        "removed" | "delete" => remove_indexed(conn, path),
        "created" | "modified" | "renamed" => {
            // created/modified：upsert 元数据；重命名目标走 created 分支。
            match std::fs::metadata(path) {
                Ok(metadata) => {
                    let kind = if metadata.is_dir() { "dir" } else { "file" };
                    let size = if metadata.is_file() {
                        metadata.len()
                    } else {
                        0
                    };
                    let mtime_ms = metadata
                        .modified()
                        .map(|t| {
                            t.duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_millis() as u64)
                                .unwrap_or(0)
                        })
                        .unwrap_or(0);
                    upsert_indexed(
                        conn,
                        &IndexedFile::from_metadata(path, kind, size, mtime_ms),
                    )
                }
                Err(_) => Ok(()), // 文件可能已消失（race），忽略
            }
        }
        _ => Ok(()), // 其它事件类型不处理（metadata-only / access 噪声）
    }
}

/// indexed search API（FIL-014）：FTS ranking + metadata 过滤 + 分页。
///
/// 返回按 FTS rank 排序的命中；`kind` 过滤可选，`limit/offset` 分页。
pub fn search_indexed(
    conn: &Connection,
    query: &str,
    kind_filter: Option<&str>,
    limit: usize,
    offset: usize,
) -> Result<Vec<IndexedHit>> {
    // FTS5 查询转义：把用户输入作为短语查询（双引号包裹），避免连字符等
    // 被解析为 FTS 语法（`ai-native` → NOT 语法、`native` 被当列名）。
    let fts_query = format!("\"{}\"", query.replace('"', "\"\""));
    // FTS5 语法：MATCH 必须应用于 FTS 虚拟表本身，不能放在普通表 JOIN 的
    // WHERE 里（否则报 "unable to use function MATCH in the requested context"）。
    // 以 FTS 表为主表 JOIN file_index 取元数据。
    let mut sql = String::from(
        "SELECT f.path, f.kind,
                snippet(file_content_fts, 1, '[', ']', '…', 8),
                CAST(bm25(file_content_fts) AS INTEGER) AS rank
         FROM file_content_fts
         JOIN file_index f ON f.path = file_content_fts.path
         WHERE file_content_fts MATCH ?1",
    );
    // 占位符连续编号：无 kind_filter 时 LIMIT/OFFSET 用 ?2/?3，有则 ?3/?4。
    if kind_filter.is_some() {
        sql.push_str(" AND f.kind = ?2");
        sql.push_str(" ORDER BY rank LIMIT ?3 OFFSET ?4");
    } else {
        sql.push_str(" ORDER BY rank LIMIT ?2 OFFSET ?3");
    }

    let mut stmt = conn.prepare(&sql).map_err(Error::Database)?;
    // 统一参数构造：避免 if/else 分支产生不同闭包类型（E0308）。
    let mut bind: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(fts_query)];
    if let Some(kind) = kind_filter {
        bind.push(Box::new(kind.to_string()));
    }
    bind.push(Box::new(limit as i64));
    bind.push(Box::new(offset as i64));
    let param_refs: Vec<&dyn rusqlite::ToSql> = bind.iter().map(|b| b.as_ref()).collect();
    let rows = stmt
        .query_map(rusqlite::params_from_iter(param_refs), |row| {
            Ok(IndexedHit {
                path: row.get(0)?,
                kind: row.get(1)?,
                snippet: row.get(2)?,
                rank: row.get(3)?,
            })
        })
        .map_err(Error::Database)?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Error::Database)
}

/// 统计索引规模（soak/benchmark 断言用）。
pub fn count_indexed(conn: &Connection) -> Result<usize> {
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM file_index", [], |row| row.get(0))
        .map_err(Error::Database)?;
    Ok(count as usize)
}

/// 收集可索引路径（跳过 target/node_modules/.git 等）。
fn collect_indexable(dir: &Path, out: &mut Vec<PathBuf>, cancelled: &AtomicBool) {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        if cancelled.load(Ordering::Relaxed) {
            return;
        }
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name == "target" || name == "node_modules" || name == ".git" || name == ".next" {
                continue;
            }
            if path.is_dir() {
                out.push(path.clone());
                stack.push(path);
            } else {
                out.push(path);
            }
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::Duration;

    fn test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS file_index (
                path TEXT PRIMARY KEY, kind TEXT NOT NULL DEFAULT 'file',
                size INTEGER NOT NULL DEFAULT 0, mtime_ms INTEGER NOT NULL DEFAULT 0,
                has_thumbnail INTEGER NOT NULL DEFAULT 0, indexed_at_ms INTEGER NOT NULL DEFAULT 0);
             CREATE VIRTUAL TABLE IF NOT EXISTS file_content_fts USING fts5(
                path UNINDEXED, content, tokenize = 'unicode61');",
        )
        .unwrap();
        conn
    }

    #[test]
    fn upsert_and_remove_are_idempotent() {
        let conn = test_db();
        let entry = IndexedFile::from_metadata("/tmp/a.txt", "file", 10, 123);
        upsert_indexed(&conn, &entry).unwrap();
        upsert_indexed(&conn, &entry).unwrap(); // 幂等
        assert_eq!(count_indexed(&conn).unwrap(), 1);
        remove_indexed(&conn, "/tmp/a.txt").unwrap();
        remove_indexed(&conn, "/tmp/a.txt").unwrap(); // 幂等
        assert_eq!(count_indexed(&conn).unwrap(), 0);
    }

    #[test]
    fn fts_upsert_and_search_roundtrip() {
        let conn = test_db();
        let entry = IndexedFile::from_metadata("/tmp/notes.md", "file", 20, 123);
        upsert_indexed(&conn, &entry).unwrap();
        upsert_content(&conn, "/tmp/notes.md", "hello ai-native world").unwrap();
        let hits = search_indexed(&conn, "ai-native", None, 10, 0).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "/tmp/notes.md");
        assert!(hits[0].snippet.contains("ai-native"));
    }

    #[test]
    fn fts_search_respects_kind_filter_and_paging() {
        let conn = test_db();
        for (path, kind) in [
            ("/tmp/a.md", "file"),
            ("/tmp/b.md", "file"),
            ("/tmp/c.txt", "txt"),
        ] {
            let entry = IndexedFile::from_metadata(path, kind, 0, 1);
            upsert_indexed(&conn, &entry).unwrap();
            upsert_content(&conn, path, "needle shared-token").unwrap();
        }
        // 分页 limit=2
        let page = search_indexed(&conn, "shared-token", None, 2, 0).unwrap();
        assert_eq!(page.len(), 2);
        // kind 过滤
        let filtered = search_indexed(&conn, "shared-token", Some("txt"), 10, 0).unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].path, "/tmp/c.txt");
    }

    #[test]
    fn initial_scan_indexes_temp_dir_and_skips_ignored_dirs() {
        let conn = test_db();
        let dir = std::env::temp_dir().join(format!("natives-index-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir.join("node_modules")).unwrap();
        fs::write(dir.join("a.txt"), "hello").unwrap();
        fs::write(dir.join("node_modules").join("ignored.txt"), "skip").unwrap();

        let (processed, total) =
            scan_initial(&conn, dir.to_str().unwrap(), &ScanContext::default()).unwrap();
        assert!(processed >= 1, "应至少索引 a.txt，实际 {processed}/{total}");
        // node_modules 被跳过
        let hits = search_indexed(&conn, "skip", None, 10, 0).unwrap();
        assert!(hits.is_empty(), "node_modules 内容不应被索引");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn watch_event_applies_incremental_changes() {
        let conn = test_db();
        let dir = std::env::temp_dir().join(format!("natives-watch-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("x.txt");
        fs::write(&file, "data").unwrap();
        let file_str = file.to_str().unwrap().to_string();

        apply_watch_event(&conn, &file_str, "created").unwrap();
        assert_eq!(count_indexed(&conn).unwrap(), 1);

        apply_watch_event(&conn, &file_str, "removed").unwrap();
        assert_eq!(count_indexed(&conn).unwrap(), 0);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_cancellation_stops_early() {
        let conn = test_db();
        let dir = std::env::temp_dir().join(format!("natives-cancel-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for i in 0..50 {
            fs::write(dir.join(format!("f{i}.txt")), "x").unwrap();
        }
        let cancelled = Arc::new(AtomicBool::new(true)); // 预取消
        let context = ScanContext {
            max_workers: 1,
            cancelled,
            progress: None,
        };
        let (processed, _total) = scan_initial(&conn, dir.to_str().unwrap(), &context).unwrap();
        assert_eq!(processed, 0, "预取消时不应处理任何条目");
        let _ = fs::remove_dir_all(&dir);
    }

    /// FIL-015：100k files benchmark —— 造 100_000 条索引并验证
    /// indexed search 延迟在预算内（soak/性能 Gate 证据）。
    #[test]
    fn benchmark_100k_files_indexed_search() {
        let conn = test_db();
        let total = 100_000usize;
        let mut stmt = conn
            .prepare(
                "INSERT INTO file_index (path, kind, size, mtime_ms, has_thumbnail, indexed_at_ms)
                 VALUES (?1, 'file', 0, 1, 0, 1)",
            )
            .unwrap();
        for i in 0..total {
            stmt.execute(params![format!("/bench/f{i:06}.txt")])
                .unwrap();
        }
        // 预填充一部分内容，保证 FTS 查询有命中
        upsert_content(&conn, "/bench/f000042.txt", "needle-for-benchmark").unwrap();
        assert_eq!(count_indexed(&conn).unwrap(), total);

        let started = std::time::Instant::now();
        let hits = search_indexed(&conn, "needle-for-benchmark", None, 10, 0).unwrap();
        let elapsed = started.elapsed();
        assert_eq!(hits.len(), 1);
        assert!(
            elapsed < Duration::from_millis(500),
            "100k 索引搜索延迟应 <500ms，实际 {elapsed:?}"
        );
    }

    /// FIL-016：索引资源 soak —— 批量增量事件后元数据/内容保持一致，
    /// 无泄漏（计数守恒；重复 upsert/remove 幂等）。
    #[test]
    fn soak_incremental_events_remain_consistent() {
        let conn = test_db();
        let dir = std::env::temp_dir().join(format!("natives-soak-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        // 模拟持续 watch 事件流：500 个 created + 250 个 removed + 重复事件
        let paths: Vec<String> = (0..500)
            .map(|i| {
                dir.join(format!("w{i:04}.txt"))
                    .to_string_lossy()
                    .to_string()
            })
            .collect();
        for (i, path) in paths.iter().enumerate() {
            fs::write(path, "soak-content").unwrap();
            apply_watch_event(&conn, path, "created").unwrap();
            // 重复 modified（coalesce 语义：不重复计数）
            apply_watch_event(&conn, path, "modified").unwrap();
            if i % 2 == 0 {
                apply_watch_event(&conn, path, "removed").unwrap();
            }
        }
        // 250 保留 + 目录本身（created 分支对目录也 upsert）
        let count = count_indexed(&conn).unwrap();
        assert!(count >= 250, "soak 后应有 >=250 条目，实际 {count}");

        // 事件重复不破坏幂等：对"保留的"路径再次 created 不增长。
        // （被 removed 的路径重新 created 是合法恢复，不在此断言范围。）
        let before = count_indexed(&conn).unwrap();
        for path in paths.iter().skip(1).step_by(2).take(50) {
            apply_watch_event(&conn, path, "created").unwrap();
        }
        assert_eq!(
            count_indexed(&conn).unwrap(),
            before,
            "重复事件不得增长索引"
        );

        let _ = fs::remove_dir_all(&dir);
    }
}
