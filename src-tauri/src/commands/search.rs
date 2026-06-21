use crate::{search, Error, Result};
use serde_json::Value as JsonValue;

#[tauri::command]
pub fn search_grep(
    query: String,
    root: String,
    options: Option<JsonValue>,
) -> Result<Vec<JsonValue>> {
    let max_results = options
        .as_ref()
        .and_then(|o| o.get("maxResults"))
        .and_then(|v| v.as_u64())
        .unwrap_or(50) as usize;
    let file_pattern = options
        .as_ref()
        .and_then(|o| o.get("pattern"))
        .and_then(|v| v.as_str());

    let results = search::search_grep(&query, &root, max_results, file_pattern)?;
    serde_json::to_value(results)
        .map(|v| {
            if let JsonValue::Array(arr) = v {
                arr
            } else {
                vec![]
            }
        })
        .map_err(|e| Error::Internal(e.to_string()))
}

#[tauri::command]
pub fn search_files(
    query: String,
    root: String,
    options: Option<JsonValue>,
) -> Result<Vec<JsonValue>> {
    let max_results = options
        .as_ref()
        .and_then(|o| o.get("maxResults"))
        .and_then(|v| v.as_u64())
        .unwrap_or(50) as usize;

    let results = search::search_files(&query, &root, max_results)?;
    serde_json::to_value(results)
        .map(|v| {
            if let JsonValue::Array(arr) = v {
                arr
            } else {
                vec![]
            }
        })
        .map_err(|e| Error::Internal(e.to_string()))
}

#[tauri::command]
pub fn search_spotlight(query: String, root: String) -> Result<Vec<JsonValue>> {
    let results = search::search_spotlight(&query, &root)?;
    serde_json::to_value(results)
        .map(|v| {
            if let JsonValue::Array(arr) = v {
                arr
            } else {
                vec![]
            }
        })
        .map_err(|e| Error::Internal(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join("n2-test-search").join(name);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn test_search_files_finds_file_by_name() {
        let dir = test_dir("sf_find");
        std::fs::write(dir.join("unique_s3arch_file.txt"), b"content here").unwrap();
        let r = search_files("unique_s3arch".into(), dir.to_string_lossy().into(), None)
            .expect("search_files");
        assert!(!r.is_empty(), "must find file by name");
        let paths: Vec<&str> = r.iter().filter_map(|v| v["path"].as_str()).collect();
        assert!(paths.iter().any(|p| p.contains("unique_s3arch_file.txt")));
    }

    #[test]
    fn test_search_files_no_match() {
        let dir = test_dir("sf_none");
        std::fs::write(dir.join("f.txt"), b"hello").unwrap();
        let r = search_files("ZZZ_NO_MATCH".into(), dir.to_string_lossy().into(), None)
            .expect("search_files empty");
        assert!(r.is_empty());
    }

    #[test]
    fn test_search_grep_finds_content() {
        let dir = test_dir("sg_find");
        std::fs::write(dir.join("code.rs"), b"fn grep_target() {}\n").unwrap();
        let r = search_grep("grep_target".into(), dir.to_string_lossy().into(), None)
            .expect("search_grep");
        assert!(!r.is_empty(), "must find match");
        // Should have at least one result with a line number
        assert!(r.iter().any(|v| v.get("line").and_then(|n| n.as_u64()).is_some()));
    }

    #[test]
    fn test_search_grep_no_match() {
        let dir = test_dir("sg_none");
        std::fs::write(dir.join("f.txt"), b"hello world").unwrap();
        let r = search_grep("ZZZ_GREP_NO".into(), dir.to_string_lossy().into(), None)
            .expect("search_grep empty");
        assert!(r.is_empty());
    }

    #[test]
    fn test_search_files_nonexistent_dir() {
        let r = search_files("test".into(), "/tmp/_n2_no_dir_".into(), None);
        assert!(r.is_err());
    }

    #[test]
    fn test_search_grep_binary_file() {
        let dir = test_dir("sg_bin");
        std::fs::write(dir.join("data.bin"), &[0u8, 159, 0x00, 0xFF]).unwrap();
        // Binary content should not crash grep — should return empty or error gracefully
        let r = search_grep("test".into(), dir.to_string_lossy().into(), None);
        match r {
            Ok(results) => assert!(results.is_empty(), "binary file should match nothing"),
            Err(_) => {}  // binary grep error is acceptable
        }
    }
}
