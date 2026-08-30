//! File type / project-badge / metadata / natural-sort detection helpers
//! shared by listing, single-file ops, and OS integration. Internal to
//! `file_manager`; re-exported `pub(crate)` for sibling submodules.

use std::path::Path;

/// Detect file kind from extension
pub fn detect_file_kind(name: &str) -> String {
    // Extensionless text files (Dockerfile / Makefile / README …)
    let base = Path::new(name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(name);
    if matches!(
        base,
        "Dockerfile"
            | "Makefile"
            | "Gemfile"
            | "Rakefile"
            | "CHANGELOG"
            | "README"
            | "LICENSE"
            | "VERSION"
            | "Procfile"
            | ".env"
            | ".gitignore"
            | ".dockerignore"
            | ".editorconfig"
    ) {
        return "text".to_string();
    }

    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "txt" | "md" | "mdx" | "markdown" | "json" | "jsonc" | "yaml" | "yml" | "toml" | "xml"
        | "csv" | "log" | "ini" | "cfg" | "conf" | "env" | "gitignore" | "dockerignore"
        | "editorconfig" | "graphql" | "gql" | "sql" | "vue" | "svelte" | "astro" => {
            "text".to_string()
        }
        "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "py" | "pyw" | "rb" | "rs" | "go"
        | "java" | "c" | "cpp" | "h" | "hpp" | "cs" | "swift" | "kt" | "kts" | "sh" | "bash"
        | "zsh" | "fish" | "ps1" | "bat" | "cmd" | "php" | "scala" => "text".to_string(),
        "html" | "htm" | "css" | "scss" | "sass" | "less" => "text".to_string(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "ico" | "bmp" | "tiff" | "tif"
        | "heic" | "heif" | "avif" => "image".to_string(),
        "mp4" | "mov" | "avi" | "mkv" | "webm" | "flv" | "wmv" | "m4v" => "video".to_string(),
        "mp3" | "wav" | "ogg" | "flac" | "aac" | "m4a" | "opus" | "wma" => "audio".to_string(),
        "pdf" => "pdf".to_string(),
        "zip" | "jar" | "tar" | "gz" | "bz2" | "xz" | "zst" | "7z" | "rar" | "tgz" | "tbz2"
        | "tzst" => "archive".to_string(),
        _ => "other".to_string(),
    }
}

/// Infer project type from a set of entry names (fanbox `projectOf`).
pub(crate) fn detect_project_badge(names: &std::collections::HashSet<String>) -> Option<String> {
    let lower: std::collections::HashSet<String> = names.iter().map(|n| n.to_lowercase()).collect();
    if lower.contains("package.json") {
        return Some("node".into());
    }
    if lower.contains("index.html") {
        return Some("web".into());
    }
    if lower.contains("requirements.txt")
        || lower.contains("setup.py")
        || lower.contains("pyproject.toml")
    {
        return Some("python".into());
    }
    if lower.contains("cargo.toml") {
        return Some("rust".into());
    }
    if lower.contains("go.mod") {
        return Some("go".into());
    }
    if names.contains(".git") || lower.contains(".git") {
        return Some("git".into());
    }
    None
}

pub(crate) fn meta_mtime_ms(meta: &std::fs::Metadata) -> f64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

pub(crate) fn meta_btime_ms(meta: &std::fs::Metadata) -> f64 {
    meta.created()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

/// Natural sort comparison (Natives2 localeCompare with numeric: true).
/// Compares character-by-character without heap allocation.
/// - Text segments: case-insensitive comparison
/// - Numeric segments: numeric comparison (file2 < file10)
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let mut a_iter = a.chars();
    let mut b_iter = b.chars();

    loop {
        let a_ch = a_iter.next();
        let b_ch = b_iter.next();

        match (a_ch, b_ch) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(ac), Some(bc)) => {
                if ac.is_ascii_digit() && bc.is_ascii_digit() {
                    // Numeric segment: consume all digits and compare as numbers
                    let mut a_num: u64 = (ac as u8 - b'0') as u64;
                    let mut b_num: u64 = (bc as u8 - b'0') as u64;
                    loop {
                        match a_iter.clone().next() {
                            Some(c) if c.is_ascii_digit() => {
                                a_num = a_num
                                    .saturating_mul(10)
                                    .saturating_add((c as u8 - b'0') as u64);
                                a_iter.next();
                            }
                            _ => break,
                        }
                    }
                    loop {
                        match b_iter.clone().next() {
                            Some(c) if c.is_ascii_digit() => {
                                b_num = b_num
                                    .saturating_mul(10)
                                    .saturating_add((c as u8 - b'0') as u64);
                                b_iter.next();
                            }
                            _ => break,
                        }
                    }
                    match a_num.cmp(&b_num) {
                        std::cmp::Ordering::Equal => continue,
                        other => return other,
                    }
                } else {
                    // Text segment: case-insensitive character comparison
                    let ac_lower = ac.to_lowercase().next().unwrap_or(ac);
                    let bc_lower = bc.to_lowercase().next().unwrap_or(bc);
                    match ac_lower.cmp(&bc_lower) {
                        std::cmp::Ordering::Equal => continue,
                        other => return other,
                    }
                }
            }
        }
    }
}
