use crate::{Error, Result};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub path: String,
    pub line: Option<u32>,
    pub text: Option<String>,
    pub score: Option<f64>,
    pub mtime: Option<f64>,
}

// ── Time budgets (Natives2: prevent large directory hangs) ──
const DEADLINE_FILES: std::time::Duration = std::time::Duration::from_secs(4);
const DEADLINE_GREP: std::time::Duration = std::time::Duration::from_secs(3);
const _DEADLINE_MDFIND: std::time::Duration = std::time::Duration::from_secs(6);
const MAX_SPOTLIGHT_PREVIEW: usize = 12;

// ── Fuzzy scoring: subsequence match + streak×8 + word-start+15 + position decay ──
// Ported from FanBox server.js:269-287
fn fuzzy_score(query: &str, target: &str) -> f64 {
    let q: Vec<char> = query.to_lowercase().chars().collect();
    let t: Vec<char> = target.to_lowercase().chars().collect();
    if q.is_empty() {
        return 0.0;
    }
    let (mut qi, mut score, mut last_idx, mut streak) = (0usize, 0.0f64, -1i64, 0u32);
    for (ti, &tc) in t.iter().enumerate() {
        if qi >= q.len() {
            break;
        }
        if tc == q[qi] {
            let mut pts = 10.0f64;
            // Consecutive match bonus
            if ti as i64 == last_idx + 1 {
                streak += 1;
                pts += streak as f64 * 8.0;
            } else {
                streak = 0;
            }
            // Word-start bonus (after / _ - . space or at position 0)
            if ti == 0 || ['/', '_', '-', '.', ' '].contains(&t[ti - 1]) {
                pts += 15.0;
            }
            // Position decay: earlier matches score higher
            pts += (8.0 - ti as f64 * 0.1).max(0.0);
            score += pts;
            last_idx = ti as i64;
            qi += 1;
        }
    }
    if qi < q.len() {
        return -1.0; // Not all query chars matched
    }
    // Length penalty: shorter targets score higher
    score -= (t.len() - q.len()) as f64 * 0.2;
    score
}

/// File name search with fuzzy scoring + path bonus + dir bonus + recency (Natives2)
/// Ported from FanBox server.js:289-311
pub fn search_files(query: &str, root: &str, max_results: usize) -> Result<Vec<SearchResult>> {
    let root_path = Path::new(root);
    if !root_path.exists() {
        return Err(Error::NotFound(root.to_string()));
    }

    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Ok(Vec::new());
    }

    let mut results = Vec::new();
    let start = std::time::Instant::now();
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(root_path.to_path_buf());

    let ignore_dirs: std::collections::HashSet<&str> = [
        "node_modules",
        ".git",
        ".next",
        ".cache",
        "dist",
        "out",
        "build",
        ".vscode",
        ".idea",
        "Library",
        "Applications",
        ".Trash",
    ]
    .iter()
    .cloned()
    .collect();

    while let Some(dir) = queue.pop_front() {
        if start.elapsed() > DEADLINE_FILES || results.len() >= max_results * 3 {
            break;
        }
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            if start.elapsed() > DEADLINE_FILES {
                break;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if name == ".DS_Store" || name.starts_with('.') || ignore_dirs.contains(name.as_str()) {
                continue;
            }
            let entry_path = entry.path();
            let name_lower = name.to_lowercase();

            // Get metadata for mtime
            let mtime = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.elapsed().ok())
                .map(|e| e.as_secs_f64())
                .unwrap_or(0.0);
            let recency_boost = (20.0 - mtime / 86400.0).max(0.0) * 0.6;

            if entry_path.is_dir() {
                // Directory bonus: +6 (FanBox design: "vibe coding — find project dirs first")
                let dir_score = fuzzy_score(&q, &name_lower);
                if dir_score > 0.0 {
                    let path_bonus =
                        if fuzzy_score(&q, &entry_path.to_string_lossy().to_lowercase()) > 0.0 {
                            3.0
                        } else {
                            0.0
                        };
                    results.push(SearchResult {
                        path: entry_path.to_string_lossy().to_string(),
                        line: None,
                        text: Some(name.clone()),
                        score: Some(dir_score + 6.0 + path_bonus + recency_boost),
                        mtime: Some(mtime),
                    });
                }
                queue.push_back(entry_path);
            } else {
                let name_score = fuzzy_score(&q, &name_lower);
                if name_score > 0.0 {
                    let path_bonus = if fuzzy_score(
                        &q,
                        &entry_path.to_string_lossy().to_lowercase(),
                    ) > 0.0
                    {
                        3.0
                    } else {
                        0.0
                    };
                    results.push(SearchResult {
                        path: entry_path.to_string_lossy().to_string(),
                        line: None,
                        text: Some(name),
                        score: Some(name_score + path_bonus + recency_boost),
                        mtime: Some(mtime),
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

/// Content grep with deadline + mtime-ordered reading + built-in Rust fallback (Natives2)
/// Ported from FanBox server.js:313-343
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

    let q = query.trim();
    if q.is_empty() || q.len() < 2 {
        return Ok(Vec::new());
    }

    // Try rg first, then grep, then built-in Rust fallback
    if which("rg") {
        return grep_with_rg(q, root, max_results, file_pattern);
    }
    if which("grep") {
        return grep_with_system(q, root, max_results, file_pattern);
    }
    // Built-in Rust fallback: walk + read_to_string
    grep_builtin(q, root, max_results)
}

/// grep via ripgrep (rg)
fn grep_with_rg(
    query: &str,
    root: &str,
    max_results: usize,
    file_pattern: Option<&str>,
) -> Result<Vec<SearchResult>> {
    let mut cmd = std::process::Command::new("rg");
    cmd.args([
        "--line-number",
        "--no-heading",
        "--max-count",
        &max_results.to_string(),
        "--max-depth",
        "10",
    ]);
    if let Some(pattern) = file_pattern {
        cmd.args(["--glob", pattern]);
    }
    cmd.arg(query).arg(root);

    let output = cmd
        .output()
        .map_err(|e| Error::Internal(format!("rg failed: {e}")))?;

    parse_grep_output(&output.stdout, max_results, root)
}

/// grep via system grep
fn grep_with_system(
    query: &str,
    root: &str,
    max_results: usize,
    _file_pattern: Option<&str>,
) -> Result<Vec<SearchResult>> {
    let mut cmd = std::process::Command::new("grep");
    cmd.args([
        "-r",
        "-n",
        "--include=*",
        "-m",
        &max_results.to_string(),
    ]);
    cmd.arg(query).arg(root);

    let output = cmd
        .output()
        .map_err(|e| Error::Internal(format!("grep failed: {e}")))?;

    parse_grep_output(&output.stdout, max_results, root)
}

/// Built-in Rust grep fallback: walk files + read_to_string + line match
/// Ported from FanBox grepFiles (server.js:313-343)
fn grep_builtin(query: &str, root: &str, max_results: usize) -> Result<Vec<SearchResult>> {
    let root_path = Path::new(root);
    let lower = query.to_lowercase();
    let start = std::time::Instant::now();

    // Collect text files with mtime
    let mut files: Vec<(std::path::PathBuf, f64)> = Vec::new();
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(root_path.to_path_buf());

    let ignore_dirs: std::collections::HashSet<&str> = [
        "node_modules",
        ".git",
        ".next",
        ".cache",
        "dist",
        "out",
        "build",
        ".vscode",
        ".idea",
    ]
    .iter()
    .cloned()
    .collect();

    let walk_deadline = start + std::time::Duration::from_millis(1800);
    const WALK_LIMIT: usize = 12000;

    while let Some(dir) = queue.pop_front() {
        if std::time::Instant::now() > walk_deadline || files.len() >= WALK_LIMIT {
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
                // Only read small text-like files (< 512KB)
                if let Ok(meta) = entry.metadata() {
                    if meta.len() > 512 * 1024 {
                        continue;
                    }
                    let mtime = meta
                        .modified()
                        .ok()
                        .and_then(|t| t.elapsed().ok())
                        .map(|e| e.as_secs_f64())
                        .unwrap_or(0.0);
                    files.push((entry_path, mtime));
                }
            }
        }
    }

    // Sort by mtime descending — "recently written" files first
    files.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    let grep_deadline = start + DEADLINE_GREP;
    let mut results = Vec::new();

    for (path, mtime) in files {
        if std::time::Instant::now() > grep_deadline || results.len() >= max_results {
            break;
        }
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let mut hits: Vec<(u32, String)> = Vec::new();
        for (i, line) in content.lines().enumerate() {
            if hits.len() >= 4 {
                break;
            }
            if line.to_lowercase().contains(&lower) {
                hits.push(((i + 1) as u32, line.trim().chars().take(200).collect()));
            }
        }
        if !hits.is_empty() {
            let path_str = path.to_string_lossy().to_string();
            for (line_num, text) in hits {
                results.push(SearchResult {
                    path: path_str.clone(),
                    line: Some(line_num),
                    text: Some(text),
                    score: None,
                    mtime: Some(mtime),
                });
            }
        }
    }

    Ok(results)
}

/// Parse rg/grep stdout into SearchResult vec, with mtime-based sorting
fn parse_grep_output(stdout: &[u8], max_results: usize, _root: &str) -> Result<Vec<SearchResult>> {
    let stdout_str = String::from_utf8_lossy(stdout);
    let mut raw: Vec<(String, Option<u32>, String)> = Vec::new();

    for line in stdout_str.lines().take(max_results * 2) {
        let mut parts = line.splitn(3, ':');
        let path = parts.next().unwrap_or("").to_string();
        let line_num = parts.next().and_then(|s| s.parse::<u32>().ok());
        let text = parts.next().unwrap_or("").to_string();
        if !path.is_empty() {
            raw.push((path, line_num, text));
        }
    }

    // Get mtime for each unique path
    let mut mtime_map: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    for (path, _, _) in &raw {
        if !mtime_map.contains_key(path) {
            let mtime = std::fs::metadata(path)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.elapsed().ok())
                .map(|e| e.as_secs_f64())
                .unwrap_or(0.0);
            mtime_map.insert(path.clone(), mtime);
        }
    }

    // Sort by mtime descending (recent files first)
    raw.sort_by(|a, b| {
        let ma = mtime_map.get(&a.0).unwrap_or(&0.0);
        let mb = mtime_map.get(&b.0).unwrap_or(&0.0);
        ma.partial_cmp(mb).unwrap_or(std::cmp::Ordering::Equal)
    });

    let results: Vec<SearchResult> = raw
        .into_iter()
        .take(max_results)
        .map(|(path, line_num, text)| {
            let mtime = mtime_map.get(&path).copied();
            SearchResult {
                path,
                line: line_num,
                text: Some(text),
                score: None,
                mtime,
            }
        })
        .collect();

    Ok(results)
}

/// Spotlight search with kMDItemTextContent query + grep fallback + mtime sort (Natives2)
/// Ported from FanBox server.js:345-392
pub fn search_spotlight(query: &str, root: &str) -> Result<Vec<SearchResult>> {
    let q = query.trim();
    if q.is_empty() || q.len() < 2 {
        return Ok(Vec::new());
    }

    // Escape special chars for mdfind query syntax
    let esc: String = q
        .chars()
        .filter(|c| *c != '\\' && *c != '"' && *c != '*')
        .collect();
    // Use kMDItemTextContent property query — enables PDF/DOCX/OCR full-text search
    let mdfind_query = format!(
        "(kMDItemTextContent == \"*{esc}*\"cd) || (kMDItemDisplayName == \"*{esc}*\"cd)"
    );

    // Run mdfind with timeout
    let output = std::process::Command::new("mdfind")
        .args(["-onlyin", root, &mdfind_query])
        .output()
        .map_err(|e| Error::Internal(format!("mdfind failed: {e}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let paths: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();

    // mdfind found nothing? Fall back to grep for code directories
    if paths.is_empty() {
        if let Ok(grep_results) = search_grep(q, root, 20, None) {
            if !grep_results.is_empty() {
                return Ok(grep_results);
            }
        }
    }

    // Build results with metadata, filter noise paths
    let mut results: Vec<SearchResult> = Vec::new();
    let noise_re = regex::Regex::new(r"/(node_modules|\.git|Library/Caches)/").ok();

    for p in &paths {
        if results.len() >= 60 {
            break;
        }
        // Skip noise paths
        if let Some(re) = &noise_re {
            if re.is_match(p) {
                continue;
            }
        }
        let path = Path::new(p);
        if !path.exists() || !path.is_file() {
            continue;
        }
        let meta = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.elapsed().ok())
            .map(|e| e.as_secs_f64())
            .unwrap_or(0.0);

        results.push(SearchResult {
            path: p.to_string(),
            line: None,
            text: None,
            score: None,
            mtime: Some(mtime),
        });
    }

    // Sort by mtime descending — "recently written" first
    results.sort_by(|a, b| {
        a.mtime
            .unwrap_or(0.0)
            .partial_cmp(&b.mtime.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results.reverse();

    // Add line-level preview for small text files (max 12 files)
    let lower = q.to_lowercase();
    let mut read_count = 0usize;
    for result in results.iter_mut() {
        if read_count >= MAX_SPOTLIGHT_PREVIEW {
            break;
        }
        if result.text.is_some() {
            continue;
        }
        let path = Path::new(&result.path);
        if !path.exists() || !path.is_file() {
            continue;
        }
        // Only preview small text files (< 512KB)
        if let Ok(meta) = std::fs::metadata(path) {
            if meta.len() > 512 * 1024 {
                continue;
            }
        }
        read_count += 1;
        if let Ok(content) = std::fs::read_to_string(path) {
            let mut hits: Vec<(u32, String)> = Vec::new();
            for (i, line) in content.lines().enumerate() {
                if hits.len() >= 3 {
                    break;
                }
                if line.to_lowercase().contains(&lower) {
                    hits.push(((i + 1) as u32, line.trim().chars().take(200).collect()));
                }
            }
            if let Some((first_line, first_text)) = hits.into_iter().next() {
                result.line = Some(first_line);
                result.text = Some(first_text);
            }
        }
    }

    Ok(results)
}

fn which(name: &str) -> bool {
    std::process::Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
