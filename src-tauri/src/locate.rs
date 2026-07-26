//! 终端路径定位链（W11，移植 fanbox server.js 的 locatePath/statWithTail）
//!
//! - `verify_paths`：批量 stat，供终端划线前验真（stat 得到才配下划线）。
//! - `locate`：四级兜底把终端里点到的「疑似路径」定位为真实文件：
//!   ① cwd 拼接 + 直接 stat
//!   ② 空格扩展 —— macOS 截屏名「截屏2026-06-10 15.37.43.png」被终端在空格处
//!      截断时，在候选目录下列目录找「以截断段开头、且断点处正好是空格」的真实文件
//!   ③ 多根 basename 模糊搜索（复用 search.rs walk/fuzzy，多根共享 6s 总预算，
//!      同名取 mtime 最新——偏向「我刚生成的」）
//!   ④ macOS `mdfind -name` 兜底（截断路径常指向所有项目根之外）
//!
//! 全部输入路径过 `file_manager::validate_path` 白名单。

use crate::{file_manager, search, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};
use ts_rs::TS;

/// 批量验真上限（与 fanbox termVerify 的 24 同量级，留少量余量）
const MAX_VERIFY_CANDIDATES: usize = 32;
/// 多根搜索共享总预算
const LOCATE_SEARCH_BUDGET: std::time::Duration = std::time::Duration::from_secs(6);
/// mdfind 结果最多检查条数
const MAX_MDFIND_RESULTS: usize = 200;

/// 单条验真结果（wire 契约）
#[derive(Debug, Serialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
#[serde(rename_all = "camelCase")]
pub struct VerifyPathResult {
    pub path: String,
    pub exists: bool,
    pub is_dir: bool,
}

/// 定位结果（wire 契约）。method: "direct" | "spaceExpansion" | "search" | "spotlight"
#[derive(Debug, Serialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
#[serde(rename_all = "camelCase")]
pub struct LocateResult {
    pub found: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub is_dir: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub method: Option<String>,
}

impl LocateResult {
    fn not_found() -> Self {
        Self {
            found: false,
            path: None,
            is_dir: None,
            method: None,
        }
    }

    fn hit(path: &Path, is_dir: bool, method: &str) -> Self {
        Self {
            found: true,
            path: Some(path.to_string_lossy().to_string()),
            is_dir: Some(is_dir),
            method: Some(method.to_string()),
        }
    }
}

/// 白名单内 stat：路径不过 validate_path 或 stat 失败都视为不存在
fn stat_allowed(path: &Path) -> Option<std::fs::Metadata> {
    file_manager::validate_path(path).ok()?;
    std::fs::metadata(path).ok()
}

/// 批量 stat（终端划线前验真）。非法/越权路径按不存在处理，不报错。
pub fn verify_paths(candidates: &[String]) -> Vec<VerifyPathResult> {
    candidates
        .iter()
        .take(MAX_VERIFY_CANDIDATES)
        .map(|c| {
            let p = file_manager::expand_tilde(c);
            match stat_allowed(&p) {
                Some(meta) => VerifyPathResult {
                    path: c.clone(),
                    exists: true,
                    is_dir: meta.is_dir(),
                },
                None => VerifyPathResult {
                    path: c.clone(),
                    exists: false,
                    is_dir: false,
                },
            }
        })
        .collect()
}

/// 把 query 解析为绝对候选路径：绝对/带 ~ 直接展开，相对路径拼到 cwd 下
fn resolve_candidate(query: &str, cwd: Option<&str>) -> Option<PathBuf> {
    let q = query.trim().trim_start_matches("./");
    if q.is_empty() {
        return None;
    }
    if q.starts_with('/') || q.starts_with('~') {
        return Some(file_manager::expand_tilde(q));
    }
    let base = match cwd {
        Some(c) if !c.trim().is_empty() => file_manager::expand_tilde(c.trim()),
        _ => dirs::home_dir()?,
    };
    Some(base.join(q))
}

/// 第②级：空格扩展。候选目录下找「以截断段开头、断点处是空格」的真实条目，
/// 多个命中取 mtime 最新（偏向刚生成的截屏）。
fn expand_by_space(candidate: &Path) -> Option<LocateResult> {
    let parent = candidate.parent()?;
    let prefix = candidate.file_name()?.to_str()?;
    if prefix.is_empty() {
        return None;
    }
    // 父目录必须在白名单内才允许列目录
    file_manager::validate_path(parent).ok()?;
    let entries = std::fs::read_dir(parent).ok()?;

    let mut best: Option<(PathBuf, bool, std::time::SystemTime)> = None;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        // 真实名必须比截断段长，且断点处正好是空格（终端就是在空格处截断的）
        if !name.starts_with(prefix) || name.len() <= prefix.len() {
            continue;
        }
        if !name[prefix.len()..].starts_with(' ') {
            continue;
        }
        let p = entry.path();
        let Ok(meta) = std::fs::metadata(&p) else {
            continue;
        };
        let mtime = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
        let is_dir = meta.is_dir();
        match &best {
            Some((_, _, t)) if *t >= mtime => {}
            _ => best = Some((p, is_dir, mtime)),
        }
    }
    best.map(|(p, is_dir, _)| LocateResult::hit(&p, is_dir, "spaceExpansion"))
}

/// 第③级：多根 basename 搜索（共享总预算；精确同名取 mtime 最新，否则模糊最高分）
fn search_roots(name: &str, roots: &[PathBuf]) -> Option<LocateResult> {
    let deadline = std::time::Instant::now() + LOCATE_SEARCH_BUDGET;
    let mut seen: Vec<PathBuf> = Vec::new();
    let mut fuzzy_best: Option<String> = None;

    for root in roots {
        if std::time::Instant::now() > deadline {
            break;
        }
        // 白名单 + 嵌套根去重（子根不重复走）
        if file_manager::validate_path(root).is_err() {
            continue;
        }
        let canon = std::fs::canonicalize(root).unwrap_or_else(|_| root.clone());
        if seen.iter().any(|d| canon == *d || canon.starts_with(d)) {
            continue;
        }
        seen.push(canon.clone());

        let Ok(results) =
            search::search_files_with_deadline(name, &canon.to_string_lossy(), 30, deadline)
        else {
            continue;
        };
        // 精确同名：SearchResult.mtime 是「距今秒数」，越小越新
        let exact = results
            .iter()
            .filter(|r| {
                Path::new(&r.path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n == name)
                    .unwrap_or(false)
            })
            .min_by(|a, b| {
                a.mtime
                    .unwrap_or(f64::MAX)
                    .partial_cmp(&b.mtime.unwrap_or(f64::MAX))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        if let Some(hit) = exact {
            let p = PathBuf::from(&hit.path);
            let is_dir = p.is_dir();
            return Some(LocateResult::hit(&p, is_dir, "search"));
        }
        if fuzzy_best.is_none() {
            fuzzy_best = results.first().map(|r| r.path.clone());
        }
    }

    fuzzy_best.map(|p| {
        let path = PathBuf::from(&p);
        let is_dir = path.is_dir();
        LocateResult::hit(&path, is_dir, "search")
    })
}

/// 第④级：macOS Spotlight 按文件名兜底；精确同名里取 mtime 最新
fn spotlight_by_name(name: &str) -> Option<LocateResult> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    // `-name` 的值作为旗标参数被整体消费，无选项注入风险（mdfind 不识别 `--`）
    let output = std::process::Command::new("mdfind")
        .args(["-name", name])
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut best: Option<(PathBuf, bool, std::time::SystemTime)> = None;
    for line in stdout.lines().take(MAX_MDFIND_RESULTS) {
        let p = PathBuf::from(line);
        if p.file_name().and_then(|n| n.to_str()) != Some(name) {
            continue;
        }
        // 兜底结果也必须落在白名单内
        let Some(meta) = stat_allowed(&p) else {
            continue;
        };
        let mtime = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
        let is_dir = meta.is_dir();
        match &best {
            Some((_, _, t)) if *t >= mtime => {}
            _ => best = Some((p, is_dir, mtime)),
        }
    }
    best.map(|(p, is_dir, _)| LocateResult::hit(&p, is_dir, "spotlight"))
}

/// 四级兜底定位。query 可以是绝对路径、~ 路径、相对 cwd 的路径或裸文件名。
pub fn locate(query: &str, cwd: Option<&str>, roots: &[String]) -> Result<LocateResult> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(LocateResult::not_found());
    }

    // ① cwd 拼接 + 直接 stat
    let candidate = resolve_candidate(q, cwd);
    if let Some(cand) = &candidate {
        if let Some(meta) = stat_allowed(cand) {
            return Ok(LocateResult::hit(cand, meta.is_dir(), "direct"));
        }
        // ② 空格扩展
        if let Some(hit) = expand_by_space(cand) {
            return Ok(hit);
        }
    }

    // ③ 多根 basename 搜索：cwd 优先，其后前端传来的活跃根
    let name = Path::new(q)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(q);
    let mut search_bases: Vec<PathBuf> = Vec::new();
    if let Some(c) = cwd {
        if !c.trim().is_empty() {
            search_bases.push(file_manager::expand_tilde(c.trim()));
        }
    }
    for r in roots {
        search_bases.push(file_manager::expand_tilde(r));
    }
    if let Some(hit) = search_roots(name, &search_bases) {
        return Ok(hit);
    }

    // ④ Spotlight 兜底
    if let Some(hit) = spotlight_by_name(name) {
        return Ok(hit);
    }

    Ok(LocateResult::not_found())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir()
            .join("natives-locate-tests")
            .join(format!("{name}-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&d).unwrap();
        d.canonicalize().unwrap()
    }

    #[test]
    fn verify_paths_reports_exists_and_kind() {
        let dir = test_dir("verify");
        std::fs::write(dir.join("f.txt"), b"x").unwrap();
        let cands = vec![
            dir.join("f.txt").to_string_lossy().to_string(),
            dir.to_string_lossy().to_string(),
            dir.join("missing.txt").to_string_lossy().to_string(),
        ];
        let out = verify_paths(&cands);
        assert_eq!(out.len(), 3);
        assert!(out[0].exists && !out[0].is_dir);
        assert!(out[1].exists && out[1].is_dir);
        assert!(!out[2].exists);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn verify_paths_denies_blocked_paths() {
        // 白名单外（/etc）按不存在处理，不报错
        let out = verify_paths(&["/etc/hosts".to_string()]);
        assert_eq!(out.len(), 1);
        assert!(!out[0].exists);
    }

    #[test]
    fn locate_direct_via_cwd_join() {
        let dir = test_dir("direct");
        std::fs::write(dir.join("hello.txt"), b"x").unwrap();
        let r = locate("hello.txt", Some(&dir.to_string_lossy()), &[]).unwrap();
        assert!(r.found);
        assert_eq!(r.method.as_deref(), Some("direct"));
        assert!(r.path.unwrap().ends_with("hello.txt"));
        assert_eq!(r.is_dir, Some(false));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn locate_space_expansion_screenshot_name() {
        let dir = test_dir("space");
        // macOS 截屏名：终端在空格处截断，只点到「截屏2026-06-10」
        std::fs::write(dir.join("截屏2026-06-10 15.37.43.png"), b"png").unwrap();
        let r = locate("截屏2026-06-10", Some(&dir.to_string_lossy()), &[]).unwrap();
        assert!(r.found, "空格扩展必须命中");
        assert_eq!(r.method.as_deref(), Some("spaceExpansion"));
        assert!(r.path.unwrap().ends_with("截屏2026-06-10 15.37.43.png"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn locate_space_expansion_prefers_newest() {
        let dir = test_dir("space-newest");
        std::fs::write(dir.join("shot 1.png"), b"a").unwrap();
        std::fs::write(dir.join("shot 2.png"), b"b").unwrap();
        // 让第二个文件 mtime 更新
        let newer = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        let f = std::fs::File::options()
            .append(true)
            .open(dir.join("shot 2.png"))
            .unwrap();
        f.set_modified(newer).unwrap();
        let r = locate("shot", Some(&dir.to_string_lossy()), &[]).unwrap();
        assert!(r.found);
        assert!(r.path.unwrap().ends_with("shot 2.png"), "同前缀取 mtime 最新");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn locate_space_expansion_requires_space_boundary() {
        let dir = test_dir("boundary");
        // 「file123」不该被「file1」命中（断点处不是空格）
        std::fs::write(dir.join("file123"), b"x").unwrap();
        let r = locate("file1", Some(&dir.to_string_lossy()), &[]).unwrap();
        assert!(
            r.method.as_deref() != Some("spaceExpansion"),
            "非空格边界不得走空格扩展: {:?}",
            r.method
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn locate_via_multi_root_search() {
        let dir = test_dir("roots");
        let proj = dir.join("proj");
        std::fs::create_dir_all(proj.join("deep")).unwrap();
        std::fs::write(proj.join("deep").join("unique_locate_target.md"), b"x").unwrap();
        // cwd 指向别处，靠 roots 搜到
        let elsewhere = test_dir("roots-cwd");
        let r = locate(
            "unique_locate_target.md",
            Some(&elsewhere.to_string_lossy()),
            &[proj.to_string_lossy().to_string()],
        )
        .unwrap();
        assert!(r.found);
        assert_eq!(r.method.as_deref(), Some("search"));
        assert!(r.path.unwrap().ends_with("unique_locate_target.md"));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&elsewhere);
    }

    #[test]
    fn locate_empty_query_not_found() {
        let r = locate("  ", None, &[]).unwrap();
        assert!(!r.found);
    }
}
