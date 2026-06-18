use crate::{Error, Result};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct GitStatusEntry {
    pub path: String,
    pub status: String, // "modified", "added", "deleted", "renamed", "untracked"
    pub staged: bool,
}

#[derive(Debug, Serialize)]
pub struct GitStatus {
    pub branch: String,
    pub entries: Vec<GitStatusEntry>,
    pub dirty: bool,
}

/// Get git status for a directory
pub fn git_status(dir_path: &str) -> Result<GitStatus> {
    let path = Path::new(dir_path);
    if !path.exists() {
        return Err(Error::NotFound(dir_path.to_string()));
    }

    // Get current branch
    let branch_output = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(path)
        .output()
        .map_err(|e| Error::Internal(format!("git failed: {e}")))?;

    let branch = if branch_output.status.success() {
        String::from_utf8_lossy(&branch_output.stdout).trim().to_string()
    } else {
        "unknown".to_string()
    };

    // Get status
    let status_output = std::process::Command::new("git")
        .args(["status", "--porcelain=v1", "-u"])
        .current_dir(path)
        .output()
        .map_err(|e| Error::Internal(format!("git failed: {e}")))?;

    if !status_output.status.success() {
        return Err(Error::Internal("git status failed".into()));
    }

    let stdout = String::from_utf8_lossy(&status_output.stdout);
    let mut entries = Vec::new();

    for line in stdout.lines() {
        if line.len() < 3 {
            continue;
        }
        let index_status = line.chars().nth(0).unwrap_or(' ');
        let worktree_status = line.chars().nth(1).unwrap_or(' ');
        let file_path = line[3..].trim().to_string();

        let staged = index_status != ' ' && index_status != '?';
        let status = match worktree_status {
            'M' => "modified",
            'A' => "added",
            'D' => "deleted",
            'R' => "renamed",
            '?' => "untracked",
            _ => match index_status {
                'M' => "modified",
                'A' => "added",
                'D' => "deleted",
                'R' => "renamed",
                _ => "unknown",
            },
        };

        entries.push(GitStatusEntry {
            path: file_path,
            status: status.to_string(),
            staged,
        });
    }

    let dirty = !entries.is_empty();

    Ok(GitStatus {
        branch,
        entries,
        dirty,
    })
}

/// Get git diff for a file
pub fn git_diff(file_path: &str) -> Result<String> {
    let path = Path::new(file_path);
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

    let output = std::process::Command::new("git")
        .args(["diff", "--", file_name])
        .current_dir(dir)
        .output()
        .map_err(|e| Error::Internal(format!("git diff failed: {e}")))?;

    if !output.status.success() {
        return Err(Error::Internal("git diff failed".into()));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
