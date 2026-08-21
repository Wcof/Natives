//! File index 命令（FIL-005~014 生产接入）。
//!
//! - `file_index_scan`：初始 metadata scan（可取消、可报告进度）。
//! - `file_index_search`：indexed search（FTS ranking + kind 过滤 + 分页）。
//! - `file_index_stats`：索引规模（soak/benchmark 断言用）。

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::file_indexer::{self, IndexedHit, ScanContext};
use crate::AppState;
use crate::Result;

/// 初始扫描请求。
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileIndexScanInput {
    pub root: String,
}

/// 初始扫描结果（processed/total）。
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileIndexScanResult {
    pub processed: usize,
    pub total: usize,
}

/// indexed search 请求。
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileIndexSearchInput {
    pub query: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
}

fn default_limit() -> usize {
    50
}

/// indexed search 结果（FTS ranking；path/kind/snippet 均非敏感）。
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileIndexSearchResult {
    pub hits: Vec<IndexedHitDto>,
    pub total: usize,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexedHitDto {
    pub path: String,
    pub kind: String,
    pub snippet: String,
}

impl From<IndexedHit> for IndexedHitDto {
    fn from(hit: IndexedHit) -> Self {
        Self {
            path: hit.path,
            kind: hit.kind,
            snippet: hit.snippet,
        }
    }
}

/// 初始 metadata scan（FIL-006）。
#[tauri::command]
pub fn file_index_scan(
    state: State<'_, AppState>,
    input: FileIndexScanInput,
) -> Result<FileIndexScanResult> {
    let conn = state
        .db
        .get()
        .map_err(|e| crate::Error::Internal(format!("failed to get DB connection: {e}")))?;
    let (processed, total) =
        file_indexer::scan_initial(&conn, &input.root, &ScanContext::default())?;
    Ok(FileIndexScanResult { processed, total })
}

/// indexed search（FIL-014）。
#[tauri::command]
pub fn file_index_search(
    state: State<'_, AppState>,
    input: FileIndexSearchInput,
) -> Result<FileIndexSearchResult> {
    let conn = state
        .db
        .get()
        .map_err(|e| crate::Error::Internal(format!("failed to get DB connection: {e}")))?;
    let hits = file_indexer::search_indexed(
        &conn,
        &input.query,
        input.kind.as_deref(),
        input.limit,
        input.offset,
    )?;
    let total = hits.len();
    Ok(FileIndexSearchResult {
        hits: hits.into_iter().map(IndexedHitDto::from).collect(),
        total,
    })
}

/// 索引规模（FIL-015/016 soak 断言）。
#[tauri::command]
pub fn file_index_stats(state: State<'_, AppState>) -> Result<usize> {
    let conn = state
        .db
        .get()
        .map_err(|e| crate::Error::Internal(format!("failed to get DB connection: {e}")))?;
    file_indexer::count_indexed(&conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_input_serde_camel_case() {
        let input = FileIndexSearchInput {
            query: "hello".into(),
            kind: Some("txt".into()),
            limit: 10,
            offset: 0,
        };
        let value = serde_json::to_value(&input).unwrap();
        assert_eq!(value["query"], "hello");
        assert_eq!(value["kind"], "txt");
        assert_eq!(value["limit"], 10);
    }
}
