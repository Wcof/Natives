//! GitHub URL parsing, Release API client, and asset download.

use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

const GITHUB_API: &str = "https://api.github.com";
const USER_AGENT: &str = "Natives-CreativeApp/1.0";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GithubRepoRef {
    pub owner: String,
    pub repo: String,
    /// Optional release tag path fragment (not used for install selection).
    pub path_tag: Option<String>,
}

/// Strictly parse `https://github.com/{owner}/{repo}` (optional `.git`, `/releases/tag/x`).
pub fn parse_github_url(input: &str) -> Result<GithubRepoRef> {
    let s = input.trim();
    if s.is_empty() {
        return Err(Error::InvalidInput("repository URL is empty".into()));
    }
    // Reject obvious injection
    if s.contains("..") || s.contains('\\') || s.contains('\0') {
        return Err(Error::InvalidInput("invalid repository URL".into()));
    }

    let without_scheme = if let Some(rest) = s.strip_prefix("https://") {
        rest
    } else if let Some(rest) = s.strip_prefix("http://") {
        rest
    } else if s.starts_with("github.com/") {
        s
    } else {
        return Err(Error::InvalidInput(
            "repository URL must be https://github.com/{owner}/{repo}".into(),
        ));
    };

    let mut parts: Vec<&str> = without_scheme
        .trim_end_matches('/')
        .split('/')
        .filter(|p| !p.is_empty())
        .collect();

    if parts.is_empty() || !parts[0].eq_ignore_ascii_case("github.com") {
        return Err(Error::InvalidInput(
            "only github.com hosts are allowed".into(),
        ));
    }
    // Drop host
    parts.remove(0);
    if parts.len() < 2 {
        return Err(Error::InvalidInput(
            "repository URL must include owner and repo".into(),
        ));
    }
    let owner = parts[0].to_string();
    let mut repo = parts[1].to_string();
    if let Some(stripped) = repo.strip_suffix(".git") {
        repo = stripped.to_string();
    }
    validate_owner_repo(&owner, &repo)?;

    let path_tag = if parts.len() >= 4 && parts[2] == "releases" && parts[3] == "tag" {
        parts.get(4).map(|s| s.to_string())
    } else {
        None
    };

    Ok(GithubRepoRef {
        owner,
        repo,
        path_tag,
    })
}

fn validate_owner_repo(owner: &str, repo: &str) -> Result<()> {
    if owner.is_empty() || repo.is_empty() {
        return Err(Error::InvalidInput("empty owner or repo".into()));
    }
    for (label, v) in [("owner", owner), ("repo", repo)] {
        if !v
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        {
            return Err(Error::InvalidInput(format!("invalid {label}: {v}")));
        }
        if v.contains("..") {
            return Err(Error::InvalidInput(format!(
                "invalid {label}: path injection"
            )));
        }
    }
    Ok(())
}

pub fn repository_https_url(owner: &str, repo: &str) -> String {
    format!("https://github.com/{owner}/{repo}")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GhRelease {
    pub id: i64,
    pub tag_name: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub assets: Vec<GhAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GhAsset {
    pub id: i64,
    pub name: String,
    pub size: u64,
    #[serde(default)]
    pub content_type: Option<String>,
    /// Never trust this from the frontend for downloads — reconstruct via asset id.
    #[serde(default)]
    pub browser_download_url: Option<String>,
}

fn build_client() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| Error::Internal(format!("http client: {e}")))
}

fn auth_header(token: Option<&str>) -> Option<String> {
    token
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|t| format!("Bearer {t}"))
}

/// List releases (paginated first page is enough for v1 manual tag pick).
pub fn list_releases(owner: &str, repo: &str, token: Option<&str>) -> Result<Vec<GhRelease>> {
    let client = build_client()?;
    let url = format!("{GITHUB_API}/repos/{owner}/{repo}/releases?per_page=30");
    let mut req = client
        .get(&url)
        .header("Accept", "application/vnd.github+json");
    if let Some(a) = auth_header(token) {
        req = req.header("Authorization", a);
    }
    let resp = req
        .send()
        .map_err(|e| Error::Internal(format!("GitHub API list releases: {e}")))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        return Err(Error::Internal(format!(
            "GitHub API list releases failed: {status} {body}"
        )));
    }
    let releases: Vec<GhRelease> = resp
        .json()
        .map_err(|e| Error::Internal(format!("parse releases: {e}")))?;
    Ok(releases)
}

/// Latest non-draft non-prerelease release.
pub fn latest_stable_release(owner: &str, repo: &str, token: Option<&str>) -> Result<GhRelease> {
    let client = build_client()?;
    let url = format!("{GITHUB_API}/repos/{owner}/{repo}/releases/latest");
    let mut req = client
        .get(&url)
        .header("Accept", "application/vnd.github+json");
    if let Some(a) = auth_header(token) {
        req = req.header("Authorization", a);
    }
    let resp = req
        .send()
        .map_err(|e| Error::Internal(format!("GitHub API latest: {e}")))?;
    if resp.status().as_u16() == 404 {
        // Fallback: scan list for first stable
        let list = list_releases(owner, repo, token)?;
        return list
            .into_iter()
            .find(|r| !r.draft && !r.prerelease)
            .ok_or_else(|| Error::NotFound("no stable GitHub release found".into()));
    }
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        return Err(Error::Internal(format!(
            "GitHub API latest failed: {status} {body}"
        )));
    }
    let release: GhRelease = resp
        .json()
        .map_err(|e| Error::Internal(format!("parse latest release: {e}")))?;
    if release.draft || release.prerelease {
        return Err(Error::NotFound("latest release is draft/prerelease".into()));
    }
    Ok(release)
}

pub fn get_release_by_tag(
    owner: &str,
    repo: &str,
    tag: &str,
    token: Option<&str>,
) -> Result<GhRelease> {
    let client = build_client()?;
    let url = format!("{GITHUB_API}/repos/{owner}/{repo}/releases/tags/{tag}");
    let mut req = client
        .get(&url)
        .header("Accept", "application/vnd.github+json");
    if let Some(a) = auth_header(token) {
        req = req.header("Authorization", a);
    }
    let resp = req
        .send()
        .map_err(|e| Error::Internal(format!("GitHub API release by tag: {e}")))?;
    if resp.status().as_u16() == 404 {
        return Err(Error::NotFound(format!("release tag not found: {tag}")));
    }
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        return Err(Error::Internal(format!(
            "GitHub API release by tag failed: {status} {body}"
        )));
    }
    let release: GhRelease = resp
        .json()
        .map_err(|e| Error::Internal(format!("parse release: {e}")))?;
    if release.draft {
        return Err(Error::InvalidInput(
            "draft releases cannot be installed".into(),
        ));
    }
    Ok(release)
}

/// Download asset by **asset id** (never trust browser_download_url from client).
pub fn download_asset_by_id(
    owner: &str,
    repo: &str,
    asset_id: i64,
    dest: &Path,
    token: Option<&str>,
    max_bytes: u64,
) -> Result<u64> {
    let client = build_client()?;
    let url = format!("{GITHUB_API}/repos/{owner}/{repo}/releases/assets/{asset_id}");
    let mut req = client
        .get(&url)
        .header("Accept", "application/octet-stream");
    if let Some(a) = auth_header(token) {
        req = req.header("Authorization", a);
    }
    let mut resp = req
        .send()
        .map_err(|e| Error::Internal(format!("download asset: {e}")))?;
    if !resp.status().is_success() {
        let status = resp.status();
        return Err(Error::Internal(format!(
            "download asset {asset_id} failed: {status}"
        )));
    }
    if let Some(len) = resp.content_length() {
        if len > max_bytes {
            return Err(Error::InvalidInput(format!(
                "asset exceeds size limit ({len} > {max_bytes})"
            )));
        }
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(Error::Io)?;
    }
    let tmp = dest.with_extension("partial");
    {
        use std::io::{Read, Write};
        let mut file = std::fs::File::create(&tmp).map_err(Error::Io)?;
        let mut buf = [0u8; 64 * 1024];
        let mut total: u64 = 0;
        loop {
            let n = resp.read(&mut buf).map_err(Error::Io)?;
            if n == 0 {
                break;
            }
            total += n as u64;
            if total > max_bytes {
                let _ = std::fs::remove_file(&tmp);
                return Err(Error::InvalidInput(format!(
                    "asset exceeds size limit while downloading (>{max_bytes})"
                )));
            }
            file.write_all(&buf[..n]).map_err(Error::Io)?;
        }
        file.sync_all().map_err(Error::Io)?;
    }
    std::fs::rename(&tmp, dest).map_err(Error::Io)?;
    let meta = std::fs::metadata(dest).map_err(Error::Io)?;
    Ok(meta.len())
}

/// Filter releases for manual mode: non-draft (prerelease allowed).
pub fn manual_release_tags(releases: &[GhRelease]) -> Vec<super::model::ReleaseTagInfo> {
    releases
        .iter()
        .filter(|r| !r.draft)
        .map(|r| super::model::ReleaseTagInfo {
            tag: r.tag_name.clone(),
            release_id: r.id,
            is_prerelease: r.prerelease,
        })
        .collect()
}

/// Known container asset names (v1).
pub fn is_compose_asset_name(name: &str) -> bool {
    matches!(
        name,
        "docker-compose.yml"
            | "docker-compose.yaml"
            | "compose.yml"
            | "compose.yaml"
            | "natives.compose.zip"
    )
}

pub fn is_manifest_asset_name(name: &str) -> bool {
    name == "natives.app.json"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_standard_url() {
        let r = parse_github_url("https://github.com/Acme/my-app").unwrap();
        assert_eq!(r.owner, "Acme");
        assert_eq!(r.repo, "my-app");
    }

    #[test]
    fn parse_git_suffix_and_tag_path() {
        let r = parse_github_url("https://github.com/Acme/my-app.git").unwrap();
        assert_eq!(r.repo, "my-app");
        let r = parse_github_url("https://github.com/Acme/my-app/releases/tag/v1.0.0").unwrap();
        assert_eq!(r.path_tag.as_deref(), Some("v1.0.0"));
    }

    #[test]
    fn reject_non_github_and_injection() {
        assert!(parse_github_url("https://gitlab.com/a/b").is_err());
        assert!(parse_github_url("https://github.com/../etc/passwd").is_err());
        assert!(parse_github_url("https://github.com/a").is_err());
        assert!(parse_github_url("").is_err());
    }

    #[test]
    fn latest_excludes_draft_prerelease_logic() {
        // Pure filter unit: manual keeps prerelease, latest helper rejects draft/prerelease flags
        let releases = vec![
            GhRelease {
                id: 1,
                tag_name: "v2-rc".into(),
                draft: false,
                prerelease: true,
                assets: vec![],
            },
            GhRelease {
                id: 2,
                tag_name: "v1".into(),
                draft: false,
                prerelease: false,
                assets: vec![],
            },
            GhRelease {
                id: 3,
                tag_name: "draft".into(),
                draft: true,
                prerelease: false,
                assets: vec![],
            },
        ];
        let tags = manual_release_tags(&releases);
        assert_eq!(tags.len(), 2);
        assert!(tags.iter().any(|t| t.is_prerelease));
        assert!(!tags.iter().any(|t| t.tag == "draft"));
        let stable = releases.iter().find(|r| !r.draft && !r.prerelease).unwrap();
        assert_eq!(stable.tag_name, "v1");
    }
}
