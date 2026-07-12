use crate::{Error, Result};
use serde::Serialize;
use std::{collections::HashMap, path::Path, process::Command};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GitBranch {
    pub name: String,
    pub current: bool,
    pub remote: bool,
    pub worktree_path: Option<String>,
}

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

pub fn validate_branch_name(name: &str) -> Result<()> {
    let deterministically_invalid = name.is_empty()
        || name.trim() != name
        || name.starts_with('-')
        || name.starts_with("refs/")
        || name == "HEAD"
        || name.chars().any(|character| {
            character.is_whitespace()
                || character.is_control()
                || matches!(character, '\\' | '~' | '^' | ':' | '?' | '*' | '[')
        })
        || name.contains("..")
        || name.contains("@{")
        || name.ends_with('/')
        || name.ends_with('.')
        || name.ends_with(".lock");

    if deterministically_invalid {
        return Err(Error::InvalidInput(format!(
            "invalid Git branch name: {name}"
        )));
    }

    let output = Command::new("git")
        .args(["check-ref-format", "--branch", name])
        .output()
        .map_err(|error| Error::Internal(format!("git check-ref-format failed: {error}")))?;
    if !output.status.success() {
        return Err(Error::InvalidInput(format!(
            "invalid Git branch name: {name}"
        )));
    }

    Ok(())
}

pub fn git_branches(dir_path: &str) -> Result<Vec<GitBranch>> {
    let path = Path::new(dir_path);
    if !path.exists() {
        return Err(Error::NotFound(dir_path.to_string()));
    }

    let refs_output = Command::new("git")
        .args([
            "for-each-ref",
            "--format=%(refname)%00%(HEAD)%00%(symref)",
            "refs/heads",
            "refs/remotes",
        ])
        .current_dir(path)
        .output()
        .map_err(|error| Error::Internal(format!("git for-each-ref failed: {error}")))?;
    if !refs_output.status.success() {
        return Err(git_command_error("git for-each-ref", &refs_output.stderr));
    }

    let worktree_output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(path)
        .output()
        .map_err(|error| Error::Internal(format!("git worktree list failed: {error}")))?;
    if !worktree_output.status.success() {
        return Err(git_command_error(
            "git worktree list",
            &worktree_output.stderr,
        ));
    }

    let worktree_paths = parse_worktree_paths(&String::from_utf8_lossy(&worktree_output.stdout));
    let mut branches = Vec::new();
    for line in String::from_utf8_lossy(&refs_output.stdout).lines() {
        let mut fields = line.splitn(3, '\0');
        let (Some(ref_name), Some(head_marker), Some(symref)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        if !symref.is_empty() {
            continue;
        }
        let (name, remote) = if let Some(name) = ref_name.strip_prefix("refs/heads/") {
            (name, false)
        } else if let Some(name) = ref_name.strip_prefix("refs/remotes/") {
            (name, true)
        } else {
            continue;
        };

        branches.push(GitBranch {
            name: name.to_string(),
            current: head_marker == "*",
            remote,
            worktree_path: worktree_paths.get(ref_name).cloned(),
        });
    }

    Ok(branches)
}

fn parse_worktree_paths(output: &str) -> HashMap<String, String> {
    let mut paths = HashMap::new();
    let mut worktree_path = None;

    for line in output.lines().chain(std::iter::once("")) {
        if let Some(path) = line.strip_prefix("worktree ") {
            worktree_path = Some(path.to_string());
        } else if let Some(branch) = line.strip_prefix("branch ") {
            if let Some(path) = worktree_path.as_ref() {
                paths.insert(branch.to_string(), path.clone());
            }
        } else if line.is_empty() {
            worktree_path = None;
        }
    }

    paths
}

fn git_command_error(command: &str, stderr: &[u8]) -> Error {
    let detail = String::from_utf8_lossy(stderr);
    Error::Internal(format!("{command} failed: {}", detail.trim()))
}

/// Get git status for a directory
pub fn git_status(dir_path: &str) -> Result<GitStatus> {
    let path = Path::new(dir_path);
    if !path.exists() {
        return Err(Error::NotFound(dir_path.to_string()));
    }

    // Get current branch
    let branch_output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(path)
        .output()
        .map_err(|e| Error::Internal(format!("git failed: {e}")))?;

    let branch = if branch_output.status.success() {
        String::from_utf8_lossy(&branch_output.stdout)
            .trim()
            .to_string()
    } else {
        "unknown".to_string()
    };

    // Get status
    let status_output = Command::new("git")
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

    let output = Command::new("git")
        .args(["diff", "--", file_name])
        .current_dir(dir)
        .output()
        .map_err(|e| Error::Internal(format!("git diff failed: {e}")))?;

    if !output.status.success() {
        return Err(Error::Internal("git diff failed".into()));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(test)]
mod git_branch_tests {
    use super::{git_branches, validate_branch_name};
    use std::{
        fs,
        path::{Path, PathBuf},
        process::Command,
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    static REPO_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct TestRepo {
        root: PathBuf,
        path: PathBuf,
    }

    impl TestRepo {
        fn new() -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock should be after Unix epoch")
                .as_nanos();
            let sequence = REPO_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "natives-git-branch-test-{}-{unique}-{sequence}",
                std::process::id()
            ));
            let path = root.join("repo");
            fs::create_dir_all(&path).expect("temporary repository should be created");

            run_git(&path, &["init"]);
            run_git(&path, &["config", "user.name", "Natives Test"]);
            run_git(&path, &["config", "user.email", "natives-test@example.com"]);
            fs::write(path.join("README.md"), "test repository\n")
                .expect("fixture file should be written");
            run_git(&path, &["add", "README.md"]);
            run_git(&path, &["commit", "-m", "initial commit"]);
            run_git(&path, &["branch", "feature/existing"]);

            Self { root, path }
        }
    }

    impl Drop for TestRepo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn run_git(repo: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .expect("git should be available");
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn git_branch_name_validation_enforces_git_ref_rules() {
        assert!(validate_branch_name("feature/new-ui").is_ok());

        for invalid in [
            "",
            " feature/new-ui",
            "-danger",
            "refs/heads/main",
            "HEAD",
            "bad name",
            "bad\nname",
            "bad..name",
            "bad@{name",
            "bad\\name",
            "bad~name",
            "bad^name",
            "bad:name",
            "bad?name",
            "bad*name",
            "bad[name",
            "bad/",
            "bad.",
            "bad.lock",
            "foo//bar",
            ".hidden",
            "foo/.hidden",
            "foo.lock/bar",
            "foo/./bar",
        ] {
            assert!(
                validate_branch_name(invalid).is_err(),
                "expected {invalid:?} to be rejected"
            );
        }
    }

    #[test]
    fn git_branch_listing_includes_local_branches_and_one_current_branch() {
        let repo = TestRepo::new();
        let branches = git_branches(repo.path.to_str().expect("temporary path should be UTF-8"))
            .expect("branches should be listed");

        assert!(branches
            .iter()
            .any(|branch| branch.name == "feature/existing"));
        assert_eq!(branches.iter().filter(|branch| branch.current).count(), 1);
        assert!(branches
            .iter()
            .find(|branch| branch.current)
            .and_then(|branch| branch.worktree_path.as_ref())
            .is_some());
    }

    #[test]
    fn git_branch_listing_excludes_symbolic_remote_refs() {
        let repo = TestRepo::new();
        let remote = repo.root.join("remote.git");
        run_git(&repo.root, &["init", "--bare", path_str(&remote)]);
        run_git(&repo.path, &["remote", "add", "origin", path_str(&remote)]);
        run_git(&repo.path, &["push", "origin", "HEAD"]);
        run_git(&repo.path, &["remote", "set-head", "origin", "--auto"]);

        let branches = git_branches(path_str(&repo.path)).expect("branches should be listed");

        assert!(!branches.iter().any(|branch| branch.name == "origin/HEAD"));
    }

    #[test]
    fn git_branch_listing_marks_fetched_remote_branches() {
        let repo = TestRepo::new();
        let remote = repo.root.join("remote.git");
        run_git(&repo.root, &["init", "--bare", path_str(&remote)]);
        run_git(&repo.path, &["remote", "add", "origin", path_str(&remote)]);
        run_git(&repo.path, &["push", "origin", "feature/existing"]);
        run_git(&repo.path, &["fetch", "origin"]);

        let branches = git_branches(path_str(&repo.path)).expect("branches should be listed");
        let branch = branches
            .iter()
            .find(|branch| branch.name == "origin/feature/existing")
            .expect("fetched remote branch should be listed");

        assert!(branch.remote);
        assert_eq!(branch.name, "origin/feature/existing");
        assert_eq!(branch.worktree_path, None);
    }

    #[test]
    fn git_branch_listing_reports_linked_worktree_path() {
        let repo = TestRepo::new();
        let linked_path = repo.root.join("linked-worktree");
        run_git(&repo.path, &["branch", "feature/linked"]);
        run_git(
            &repo.path,
            &["worktree", "add", path_str(&linked_path), "feature/linked"],
        );
        let linked_path = linked_path
            .canonicalize()
            .expect("linked worktree path should be canonicalized");

        let branches = git_branches(path_str(&repo.path)).expect("branches should be listed");
        let branch = branches
            .iter()
            .find(|branch| branch.name == "feature/linked")
            .expect("linked branch should be listed");

        assert!(!branch.remote);
        assert_eq!(branch.name, "feature/linked");
        assert_eq!(
            branch.worktree_path.as_deref(),
            Some(path_str(&linked_path))
        );
    }

    fn path_str(path: &Path) -> &str {
        path.to_str().expect("temporary path should be UTF-8")
    }
}
