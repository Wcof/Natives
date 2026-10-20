use crate::protocol::Request;
use file_manager_core::file_manager;
use serde_json::{json, Value};
use std::io::Read;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

pub(crate) fn fuzzy_name_score(name: &str, query: &str) -> Option<i64> {
    if name == query {
        return Some(20_000);
    }
    if let Some(prefix) = name.strip_prefix(query) {
        return Some(15_000 - prefix.chars().count() as i64);
    }
    if name.contains(query) {
        return Some(10_000 - name.chars().count() as i64);
    }
    let mut score = 1_000i64;
    let mut characters = name.chars();
    for needle in query.chars() {
        let mut found = false;
        while let Some(character) = characters.next() {
            if character == needle {
                found = true;
                break;
            }
            score -= 1;
        }
        if !found {
            return None;
        }
    }
    Some(score)
}

pub(crate) fn run_locate(
    request: &Request,
    cancelled: Option<&AtomicBool>,
) -> Result<Value, String> {
    const MAX_RESULTS: usize = 50;
    let is_cancelled = || cancelled.is_some_and(|token| token.load(Ordering::Relaxed));
    let query = request
        .params
        .get("query")
        .and_then(Value::as_str)
        .ok_or("query is required")?
        .trim();
    if query.is_empty() || query.len() > 256 || query.contains('\0') {
        return Err("invalid locate query".into());
    }
    if query.starts_with('-') {
        return Ok(json!({"entries": []}));
    }
    if query.contains('/') || query.contains('\\') {
        return Ok(json!({"entries": []}));
    }
    let needle = query.to_lowercase();
    let mut entries = Vec::new();
    'outer: for root in file_manager::default_roots().map_err(|e| e.to_string())? {
        let Some(root_path) = root.get("path").and_then(Value::as_str) else {
            continue;
        };
        let Ok(authorized) = file_manager::FileAccessPolicy::authorize_path(
            root_path,
            file_manager::OperationPolicy::Search,
        ) else {
            continue;
        };
        for item in walkdir::WalkDir::new(authorized.as_path())
            .follow_links(false)
            .max_depth(8)
            .into_iter()
            .filter_entry(|entry| {
                entry.depth() == 0 || !entry.file_name().to_string_lossy().starts_with('.')
            })
            .filter_map(Result::ok)
        {
            if is_cancelled() || entries.len() >= MAX_RESULTS {
                break 'outer;
            }
            let name = item.file_name().to_string_lossy().to_lowercase();
            if item.depth() == 0 || fuzzy_name_score(&name, &needle).is_none() {
                continue;
            }
            if let Ok(stat) = file_manager::stat_path(&item.path().to_string_lossy()) {
                if stat.found {
                    entries.push(serde_json::to_value(stat).unwrap_or_default());
                }
            }
        }
        if entries.len() >= MAX_RESULTS {
            break;
        }
    }
    #[cfg(target_os = "macos")]
    if entries.len() < MAX_RESULTS && !is_cancelled() {
        // Spotlight is only a bounded fallback for misses; every result still
        // passes the same authorization/stat checks before reaching the page.
        for root in file_manager::default_roots().map_err(|e| e.to_string())? {
            if is_cancelled() {
                break;
            }
            let Some(root_path) = root.get("path").and_then(Value::as_str) else {
                continue;
            };
            let Ok(authorized) = file_manager::FileAccessPolicy::authorize_path(
                root_path,
                file_manager::OperationPolicy::Search,
            ) else {
                continue;
            };
            for candidate in stream_bounded_mdfind(authorized.as_path(), query) {
                if is_cancelled() || entries.len() >= MAX_RESULTS {
                    break;
                }
                let Ok(stat) = file_manager::stat_path(&candidate) else {
                    continue;
                };
                if !stat.found
                    || entries
                        .iter()
                        .any(|entry| entry.get("path").and_then(Value::as_str) == Some(&candidate))
                {
                    continue;
                }
                entries.push(serde_json::to_value(stat).unwrap_or_default());
            }
            if entries.len() >= MAX_RESULTS {
                break;
            }
        }
    }
    Ok(json!({"entries": entries, "truncated": entries.len() >= MAX_RESULTS}))
}

// stream_bounded_mdfind 启动 mdfind 并以流式限额读取输出：总输出上限 64 KiB、
// 进程等待上限 2s（超时 kill 后仍 wait/reap），不先完整缓冲整个输出再检查
// 长度（F6）。返回路径行（读取到上限即止）。
#[cfg(target_os = "macos")]
fn stream_bounded_mdfind(root: &std::path::Path, query: &str) -> Vec<String> {
    use std::io::BufRead;
    use std::process::Stdio;
    const MAX_OUTPUT: usize = 64 * 1024;
    const MDFIND_DEADLINE: Duration = Duration::from_secs(2);

    let Ok(mut child) = Command::new("mdfind")
        .args(["-onlyin", &root.to_string_lossy(), query])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return Vec::new();
    };
    let stdout = match child.stdout.take() {
        Some(pipe) => pipe,
        None => {
            let _ = child.wait();
            return Vec::new();
        }
    };
    let (sender, receiver) = std::sync::mpsc::channel::<Vec<String>>();
    let reader = thread::spawn(move || {
        let mut lines = Vec::new();
        let mut total = 0usize;
        let mut overflow = false;
        for line in std::io::BufReader::new(stdout).lines() {
            if overflow || total > MAX_OUTPUT {
                overflow = true;
                break;
            }
            let Ok(line) = line else { break };
            total += line.len() + 1;
            lines.push(line);
            if lines.len() >= 2000 {
                break; // 行数上限：响应构建侧还会做授权与去重过滤
            }
        }
        let _ = sender.send(lines);
        overflow
    });
    let mut output = Vec::new();
    let deadline = Instant::now() + MDFIND_DEADLINE;
    loop {
        match receiver.recv_timeout(Duration::from_millis(50)) {
            Ok(lines) => {
                output = lines;
                break;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    break;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    if !matches!(child.try_wait(), Ok(Some(_))) {
        let _ = child.kill();
    }
    // kill 后仍必须 wait/reap（有界：kill 后 stdout 关闭，读线程随即结束）。
    let _ = child.wait();
    let _ = reader.join();
    output
}

pub(crate) fn run_search(
    request: &Request,
    cancelled: Option<&AtomicBool>,
) -> Result<Value, String> {
    let root = request
        .params
        .get("path")
        .and_then(Value::as_str)
        .ok_or("path is required")?;
    let query = request
        .params
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_lowercase();
    if query.is_empty() {
        return Ok(json!({"entries": [], "hasMore": false, "truncated": false}));
    }
    let authorized =
        file_manager::FileAccessPolicy::authorize_path(root, file_manager::OperationPolicy::Search)
            .map_err(|e| e.to_string())?;
    let offset = request
        .params
        .get("offset")
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;
    let limit = request
        .params
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(500)
        .min(2_000) as usize;
    let recursive = request
        .params
        .get("recursive")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let show_hidden = request
        .params
        .get("showHidden")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let content_search = request
        .params
        .get("content")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    const MAX_SEARCH_RESULTS: usize = 100_000;
    const MAX_SEARCH_DURATION: Duration = Duration::from_secs(4);
    let mut results = Vec::new();
    let mut scan_truncated = false;
    let mut content_unavailable = false;
    let search_started = Instant::now();
    let walker = walkdir::WalkDir::new(authorized.as_path())
        .follow_links(false)
        .max_depth(if recursive { 8 } else { 1 });
    for entry in walker
        .into_iter()
        .filter_entry(|entry| {
            entry.depth() == 0
                || ((!entry.file_name().to_string_lossy().starts_with('.') || show_hidden)
                    && !matches!(
                        entry.file_name().to_string_lossy().as_ref(),
                        "node_modules" | "target" | "dist" | "build" | "out" | ".git"
                    ))
        })
        .filter_map(Result::ok)
    {
        if search_started.elapsed() >= MAX_SEARCH_DURATION {
            scan_truncated = true;
            break;
        }
        if cancelled.is_some_and(|token| token.load(Ordering::Relaxed)) {
            return Err("search cancelled".into());
        }
        let name = entry.file_name().to_string_lossy();
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        if file_manager::FileAccessPolicy::authorize_path_buf(
            entry.path(),
            file_manager::OperationPolicy::Search,
        )
        .is_err()
        {
            continue;
        }
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        let is_dir = metadata.is_dir();
        let lower_name = name.to_lowercase();
        let name_score = fuzzy_name_score(&lower_name, &query);
        let name_match = name_score.is_some();
        let mut match_count = 0u64;
        let mut match_lines = Vec::new();
        let content_excerpt = if content_search && !is_dir {
            let text_match = (metadata.len() <= 2 * 1024 * 1024)
                .then(|| std::fs::read_to_string(entry.path()).ok())
                .flatten()
                .and_then(|text| {
                    let mut first = None;
                    for (line_number, line) in text.lines().enumerate() {
                        if line.to_lowercase().contains(&query) {
                            match_count += 1;
                            if match_lines.len() < 5 {
                                match_lines.push(format!(
                                    "{}: {}",
                                    line_number + 1,
                                    line.chars().take(240).collect::<String>()
                                ));
                            }
                            if first.is_none() {
                                first = Some(line.chars().take(240).collect::<String>());
                            }
                        }
                    }
                    first
                });
            text_match.or_else(|| {
                (metadata.len() <= 20 * 1024 * 1024
                    && entry
                        .path()
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf")))
                .then(|| match pdf_text_match(entry.path(), &query) {
                    Ok(value) => value,
                    Err(()) => {
                        content_unavailable = true;
                        None
                    }
                })
                .flatten()
            })
        } else {
            None
        };
        let content_match = content_excerpt.is_some();
        if (content_search && !content_match) || (!content_search && !name_match) {
            continue;
        }
        let recency_score = if content_match {
            metadata
                .modified()
                .ok()
                .and_then(|mtime| mtime.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|mtime| {
                    let age_days = (std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs()
                        .saturating_sub(mtime.as_secs()))
                        / 86_400;
                    if age_days < 7 {
                        500 - age_days as i64 * 70
                    } else {
                        0
                    }
                })
                .unwrap_or(0)
        } else {
            0
        };
        if results.len() >= MAX_SEARCH_RESULTS {
            scan_truncated = true;
            break;
        }
        results.push(json!({
            "name": name,
            "path": entry.path().to_string_lossy(),
            "isDir": is_dir,
            "kind": if is_dir { "dir".to_string() } else { file_manager::detect_file_kind(&name) },
            "hidden": false,
            "size": if is_dir { 4096 } else { metadata.len() },
            "mtime": metadata.modified().ok().and_then(|v| v.duration_since(std::time::UNIX_EPOCH).ok()).map(|v| v.as_millis() as f64).unwrap_or(0.0),
            "btime": metadata.created().ok().and_then(|v| v.duration_since(std::time::UNIX_EPOCH).ok()).map(|v| v.as_millis() as f64).unwrap_or(0.0),
            "match": content_excerpt,
            "matchCount": match_count,
            "matchLines": match_lines,
            // Content matches should not all tie at zero: bounded frequency
            // keeps the strongest textual hit ahead of a one-line match.
            "searchScore": name_score.unwrap_or_else(|| 5_000 + match_count.min(100) as i64 * 10) + recency_score
        }));
    }
    results.sort_by(|a, b| {
        let a_dir = a.get("isDir").and_then(Value::as_bool).unwrap_or(false);
        let b_dir = b.get("isDir").and_then(Value::as_bool).unwrap_or(false);
        if a_dir != b_dir {
            return b_dir.cmp(&a_dir);
        }
        let a_score = a.get("searchScore").and_then(Value::as_i64).unwrap_or(0);
        let b_score = b.get("searchScore").and_then(Value::as_i64).unwrap_or(0);
        if a_score != b_score {
            return b_score.cmp(&a_score);
        }
        let a_name = a.get("name").and_then(Value::as_str).unwrap_or("");
        let b_name = b.get("name").and_then(Value::as_str).unwrap_or("");
        file_manager::natural_cmp(a_name, b_name)
    });
    let has_more = scan_truncated || results.len() > offset.saturating_add(limit);
    let page = results
        .into_iter()
        .skip(offset)
        .take(limit)
        .collect::<Vec<_>>();
    Ok(
        json!({"entries": page, "hasMore": has_more, "truncated": scan_truncated, "contentUnavailable": content_unavailable}),
    )
}

pub(crate) fn pdf_text_match(path: &std::path::Path, query: &str) -> Result<Option<String>, ()> {
    use std::process::Stdio;
    const MAX_OUTPUT: usize = 512 * 1024;
    const PDF_WAIT_LIMIT: Duration = Duration::from_secs(2);
    let mut child = Command::new("pdftotext")
        .args(["-f", "1", "-l", "20", "-layout"])
        .arg(path)
        .arg("-")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| ())?;
    let stdout = child.stdout.take().ok_or(())?;
    let (sender, receiver) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let mut output = Vec::new();
        let _ = stdout
            .take((MAX_OUTPUT + 1) as u64)
            .read_to_end(&mut output);
        let _ = sender.send(output);
    });
    let output = receiver.recv_timeout(PDF_WAIT_LIMIT).map_err(|_| {
        let _ = child.kill();
        // kill 后仍 wait/reap；读线程在 stdout EOF 后自行结束，
        // 发送结果到已无人接收的 channel（无阻塞、无泄漏）。
        let _ = child.wait();
    })?;
    if output.len() > MAX_OUTPUT {
        let _ = child.kill();
    }
    // 非 kill 路径的 child.wait 也必须有界：子进程写出部分输出后挂起时，
    // 不能无期限阻塞调用线程（F6）。
    let wait_deadline = Instant::now() + PDF_WAIT_LIMIT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < wait_deadline => {
                thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
            Err(_) => break,
        }
    }
    let text = String::from_utf8_lossy(&output);
    let mut first = None;
    for line in text.lines() {
        if line.to_lowercase().contains(query) {
            first = Some(line.chars().take(240).collect::<String>());
            break;
        }
    }
    Ok(first)
}
