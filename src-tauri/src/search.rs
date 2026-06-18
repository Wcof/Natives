use crate::{Error, Result};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub path: String,
    pub line: Option<u32>,
    pub text: Option<String>,
    pub score: Option<f64>,
}

/// File name search (glob pattern matching)
pub fn search_files(query: &str, root: &str, max_results: usize) -> Result<Vec<SearchResult>> {
    let root_path = Path::new(root);
    if !root_path.exists() {
        return Err(Error::NotFound(root.to_string()));
    }

    let query_lower = query.to_lowercase();
    let mut results = Vec::new();
    let start = std::time::Instant::now();
    let deadline = std::time::Duration::from_secs(5);

    let mut queue = std::collections::VecDeque::new();
    queue.push_back(root_path.to_path_buf());

    let ignore_dirs: std::collections::HashSet<&str> = [
        "node_modules", ".git", ".next", ".cache", "dist", "out", "build", ".vscode", ".idea",
    ]
    .iter()
    .cloned()
    .collect();

    while let Some(dir) = queue.pop_front() {
        if start.elapsed() > deadline || results.len() >= max_results {
            break;
        }
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || ignore_dirs.contains(name.as_str()) {
                continue;
            }
            let entry_path = entry.path();
            if entry_path.is_dir() {
                queue.push_back(entry_path);
            } else {
                let name_lower = name.to_lowercase();
                if name_lower.contains(&query_lower) {
                    // Simple scoring: exact match > starts_with > contains
                    let score = if name_lower == query_lower {
                        1.0
                    } else if name_lower.starts_with(&query_lower) {
                        0.8
                    } else {
                        0.5
                    };
                    results.push(SearchResult {
                        path: entry_path.to_string_lossy().to_string(),
                        line: None,
                        text: Some(name),
                        score: Some(score),
                    });
                }
            }
        }
    }

    // Sort by score descending
    results.sort_by(|a, b| {
        b.score
            .unwrap_or(0.0)
            .partial_cmp(&a.score.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results.truncate(max_results);
    Ok(results)
}

/// Content grep (search file contents)
pub fn search_grep(
    query: &str,
    root: &str,
    max_results: usize,
    file_pattern: Option<&str>,
) -> Result<Vec<SearchResult>> {
    let root_path = Path::new(root);
    if !root_path.exists() {
        return Err(Error::NotFound(root.to_string()));
    }

    // Use ripgrep if available, fall back to grep
    let mut cmd = if which("rg") {
        let mut c = std::process::Command::new("rg");
        c.args([
            "--line-number",
            "--no-heading",
            "--max-count",
            &max_results.to_string(),
            "--max-depth",
            "10",
        ]);
        if let Some(pattern) = file_pattern {
            c.args(["--glob", pattern]);
        }
        c.arg(query);
        c.arg(root);
        c
    } else {
        let mut c = std::process::Command::new("grep");
        c.args(["-r", "-n", "--include=*", "-m", &max_results.to_string()]);
        c.arg(query);
        c.arg(root);
        c
    };

    let output = cmd.output().map_err(|e| Error::Internal(format!("grep failed: {e}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut results = Vec::new();

    for line in stdout.lines().take(max_results) {
        // Parse: path:line:text
        let mut parts = line.splitn(3, ':');
        let path = parts.next().unwrap_or("");
        let line_num = parts.next().and_then(|s| s.parse::<u32>().ok());
        let text = parts.next().map(|s| s.to_string());

        if !path.is_empty() {
            results.push(SearchResult {
                path: path.to_string(),
                line: line_num,
                text,
                score: None,
            });
        }
    }

    Ok(results)
}

/// Spotlight search (macOS mdfind)
pub fn search_spotlight(query: &str, root: &str) -> Result<Vec<SearchResult>> {
    let output = std::process::Command::new("mdfind")
        .args(["-onlyin", root, query])
        .output()
        .map_err(|e| Error::Internal(format!("mdfind failed: {e}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let results: Vec<SearchResult> = stdout
        .lines()
        .take(50)
        .map(|path| SearchResult {
            path: path.to_string(),
            line: None,
            text: None,
            score: None,
        })
        .collect();

    Ok(results)
}

fn which(name: &str) -> bool {
    std::process::Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
