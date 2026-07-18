#![allow(dead_code, unused_imports, unused_variables)]
use crate::{Error, Result};
use serde::Serialize;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex, OnceLock, Weak},
};

type RepositoryLock = Arc<Mutex<()>>;
static REPOSITORY_LOCKS: OnceLock<Mutex<HashMap<PathBuf, Weak<Mutex<()>>>>> = OnceLock::new();

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

pub fn git_checkout_branch(dir_path: &str, branch: &str) -> Result<GitStatus> {
    let path = Path::new(dir_path);
    with_repository_mutation_lock(path, || {
        validate_mutation_target(dir_path, branch)?;
        reject_dirty_worktree(path)?;

        if !local_branch_exists(path, branch)? {
            return Err(Error::InvalidInput(
                "branch_not_found: local branch does not exist".into(),
            ));
        }

        if branch_in_other_worktree(path, branch)? {
            return Err(Error::InvalidInput(
                "branch_in_use: branch is checked out in another worktree".into(),
            ));
        }

        run_branch_mutation(path, &["switch", branch])?;
        mutation_status_result(git_status(dir_path))
    })
}

pub fn git_create_branch(dir_path: &str, branch: &str) -> Result<GitStatus> {
    let path = Path::new(dir_path);
    with_repository_mutation_lock(path, || {
        validate_mutation_target(dir_path, branch)?;
        reject_dirty_worktree(path)?;

        if local_branch_exists(path, branch)? {
            return Err(Error::InvalidInput(
                "branch_exists: local branch already exists".into(),
            ));
        }

        run_branch_mutation(path, &["switch", "-c", branch])?;
        mutation_status_result(git_status(dir_path))
    })
}

fn with_repository_mutation_lock<T>(
    path: &Path,
    operation: impl FnOnce() -> Result<T>,
) -> Result<T> {
    let identity = repository_identity(path)?;
    let lock = repository_lock(identity)?;
    let _guard = lock.lock().map_err(|_| {
        Error::Internal("repository_lock_failed: branch mutation lock is unavailable".into())
    })?;
    operation()
}

fn repository_identity(path: &Path) -> Result<PathBuf> {
    if !path.is_dir() {
        return Err(Error::InvalidInput(
            "not_repository: directory is not a Git repository".into(),
        ));
    }
    let output = Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(path)
        .output()
        .map_err(|_| Error::Internal("git_unavailable: could not run Git".into()))?;
    if !output.status.success() {
        return Err(Error::InvalidInput(
            "not_repository: directory is not a Git repository".into(),
        ));
    }
    let common_dir = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    common_dir.canonicalize().map_err(|_| {
        Error::Internal("repository_identity_failed: could not resolve Git repository".into())
    })
}

fn repository_lock(identity: PathBuf) -> Result<RepositoryLock> {
    let locks = REPOSITORY_LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut locks = locks.lock().map_err(|_| {
        Error::Internal(
            "repository_lock_failed: branch mutation lock registry is unavailable".into(),
        )
    })?;
    locks.retain(|_, lock| lock.strong_count() > 0);
    if let Some(lock) = locks.get(&identity).and_then(Weak::upgrade) {
        return Ok(lock);
    }
    let lock = Arc::new(Mutex::new(()));
    locks.insert(identity, Arc::downgrade(&lock));
    Ok(lock)
}

fn mutation_status_result(status: Result<GitStatus>) -> Result<GitStatus> {
    status.map_err(|_| {
        Error::Internal(
            "mutation_succeeded_status_refresh_failed: branch changed; refresh repository status"
                .into(),
        )
    })
}

fn validate_mutation_target(dir_path: &str, branch: &str) -> Result<()> {
    validate_branch_name(branch).map_err(|_| {
        Error::InvalidInput("branch_invalid: enter a valid local branch name".into())
    })?;

    let path = Path::new(dir_path);
    if !path.is_dir() {
        return Err(Error::InvalidInput(
            "not_repository: directory is not a Git repository".into(),
        ));
    }

    let output = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(path)
        .output()
        .map_err(|_| Error::Internal("git_unavailable: could not run Git".into()))?;
    if !output.status.success() || String::from_utf8_lossy(&output.stdout).trim() != "true" {
        return Err(Error::InvalidInput(
            "not_repository: directory is not a Git repository".into(),
        ));
    }

    Ok(())
}

fn reject_dirty_worktree(path: &Path) -> Result<()> {
    let output = Command::new("git")
        .args(["status", "--porcelain=v1", "-u"])
        .current_dir(path)
        .output()
        .map_err(|_| Error::Internal("git_unavailable: could not run Git".into()))?;
    if !output.status.success() {
        return Err(Error::Internal(
            "git_status_failed: could not inspect the worktree".into(),
        ));
    }
    if !output.stdout.is_empty() {
        return Err(Error::InvalidInput(
            "dirty_worktree: commit or discard changes before switching branches".into(),
        ));
    }
    Ok(())
}

fn local_branch_exists(path: &Path, branch: &str) -> Result<bool> {
    let ref_name = format!("refs/heads/{branch}");
    let output = Command::new("git")
        .args(["show-ref", "--verify", "--quiet", &ref_name])
        .current_dir(path)
        .output()
        .map_err(|_| Error::Internal("git_unavailable: could not run Git".into()))?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(Error::Internal(
            "git_ref_check_failed: could not inspect local branches".into(),
        )),
    }
}

fn branch_in_other_worktree(path: &Path, branch: &str) -> Result<bool> {
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(path)
        .output()
        .map_err(|_| Error::Internal("git_unavailable: could not run Git".into()))?;
    if !output.status.success() {
        return Err(Error::Internal(
            "git_worktree_check_failed: could not inspect linked worktrees".into(),
        ));
    }

    let ref_name = format!("refs/heads/{branch}");
    let current_branch = git_status(path.to_str().ok_or_else(|| {
        Error::InvalidInput("not_repository: repository path is not valid UTF-8".into())
    })?)?
    .branch;
    Ok(current_branch != branch
        && parse_worktree_paths(&String::from_utf8_lossy(&output.stdout)).contains_key(&ref_name))
}

fn run_branch_mutation(path: &Path, args: &[&str]) -> Result<()> {
    let output = Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .map_err(|_| Error::Internal("git_unavailable: could not run Git".into()))?;
    if !output.status.success() {
        let detail = sanitized_git_stderr(&output.stderr);
        let message = if detail.is_empty() {
            "branch_mutation_failed: Git could not change branches".into()
        } else {
            format!("branch_mutation_failed: Git could not change branches: {detail}")
        };
        return Err(Error::Internal(message));
    }
    Ok(())
}

fn sanitized_git_stderr(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
        .chars()
        .filter(|character| !character.is_control())
        .take(240)
        .collect()
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
    use super::{
        git_branches, git_checkout_branch, git_create_branch, git_status, mutation_status_result,
        sanitized_git_stderr, validate_branch_name, with_repository_mutation_lock,
    };
    use std::{
        fs,
        path::{Path, PathBuf},
        process::Command,
        sync::{
            atomic::{AtomicU64, Ordering},
            mpsc, Arc, Barrier,
        },
        thread,
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
            run_git(&path, &["config", "commit.gpgSign", "false"]);
            let hooks = root.join("disabled-hooks");
            fs::create_dir(&hooks).expect("empty hooks directory should be created");
            run_git(&path, &["config", "core.hooksPath", path_str(&hooks)]);
            fs::write(path.join("README.md"), "test repository\n")
                .expect("fixture file should be written");
            run_git(&path, &["add", "README.md"]);
            run_git(&path, &["commit", "-m", "initial commit"]);
            run_git(&path, &["branch", "-M", "main"]);
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

    #[test]
    fn git_branch_mutation_creates_and_checks_out_branch() {
        let repo = TestRepo::new();

        let status = git_create_branch(path_str(&repo.path), "feature/new-ui")
            .expect("branch should be created");

        assert_eq!(status.branch, "feature/new-ui");
        assert_eq!(
            git_status(path_str(&repo.path)).unwrap().branch,
            "feature/new-ui"
        );
    }

    #[test]
    fn git_branch_mutation_checks_out_existing_local_branch() {
        let repo = TestRepo::new();
        git_create_branch(path_str(&repo.path), "feature/new-ui").unwrap();

        let status =
            git_checkout_branch(path_str(&repo.path), "main").expect("main should be checked out");

        assert_eq!(status.branch, "main");
        assert_eq!(git_status(path_str(&repo.path)).unwrap().branch, "main");
    }

    #[test]
    fn git_branch_mutation_rejects_dirty_worktree_without_changing_head() {
        let repo = TestRepo::new();
        fs::write(repo.path.join("README.md"), "modified\n").unwrap();
        let head = current_branch(&repo.path);

        let error = git_checkout_branch(path_str(&repo.path), "feature/existing").unwrap_err();

        assert!(error.to_string().contains("dirty_worktree"));
        assert_eq!(current_branch(&repo.path), head);
    }

    #[test]
    fn git_branch_mutation_rejects_staged_changes_without_changing_head() {
        let repo = TestRepo::new();
        fs::write(repo.path.join("README.md"), "staged\n").unwrap();
        run_git(&repo.path, &["add", "README.md"]);
        let head = current_branch(&repo.path);

        let error = git_checkout_branch(path_str(&repo.path), "feature/existing").unwrap_err();

        assert!(error.to_string().contains("dirty_worktree"));
        assert_eq!(current_branch(&repo.path), head);
    }

    #[test]
    fn git_branch_mutation_rejects_untracked_files_without_changing_head() {
        let repo = TestRepo::new();
        fs::write(repo.path.join("untracked.txt"), "untracked\n").unwrap();
        let head = current_branch(&repo.path);

        let error = git_checkout_branch(path_str(&repo.path), "feature/existing").unwrap_err();

        assert!(error.to_string().contains("dirty_worktree"));
        assert_eq!(current_branch(&repo.path), head);
    }

    #[test]
    fn git_branch_mutation_serializes_linked_worktrees_for_same_repository() {
        let repo = TestRepo::new();
        let linked_path = repo.root.join("linked-worktree");
        run_git(
            &repo.path,
            &[
                "worktree",
                "add",
                path_str(&linked_path),
                "feature/existing",
            ],
        );
        let barrier = Arc::new(Barrier::new(2));
        let (entered_tx, entered_rx) = mpsc::channel();
        let repo_path = repo.path.clone();
        let first_barrier = Arc::clone(&barrier);
        let first = thread::spawn(move || {
            with_repository_mutation_lock(&repo_path, || {
                entered_tx.send(()).unwrap();
                first_barrier.wait();
                Ok(())
            })
            .unwrap();
        });
        entered_rx.recv().unwrap();

        let (started_tx, started_rx) = mpsc::channel();
        let (second_tx, second_rx) = mpsc::channel();
        let second = thread::spawn(move || {
            started_tx.send(()).unwrap();
            with_repository_mutation_lock(&linked_path, || {
                second_tx.send(()).unwrap();
                Ok(())
            })
            .unwrap();
        });
        started_rx.recv().unwrap();
        assert!(second_rx.try_recv().is_err());
        barrier.wait();
        first.join().unwrap();
        second_rx.recv().unwrap();
        second.join().unwrap();
    }

    #[test]
    fn git_branch_mutation_reports_when_status_refresh_fails_after_success() {
        let error = mutation_status_result(Err(crate::Error::Internal("status failed".into())))
            .unwrap_err();

        assert!(error
            .to_string()
            .contains("mutation_succeeded_status_refresh_failed"));
        assert!(!error.to_string().contains("status failed"));
    }

    #[test]
    fn git_branch_mutation_sanitizes_and_bounds_git_error_detail() {
        let detail = format!(
            "\n fatal: useful detail\r\nsecret second line\n{}",
            "x".repeat(300)
        );

        let sanitized = sanitized_git_stderr(detail.as_bytes());

        assert_eq!(sanitized, "fatal: useful detail");
        assert!(sanitized.len() <= 240);
    }

    #[test]
    fn git_branch_mutation_rejects_invalid_and_missing_branches_without_changing_head() {
        let repo = TestRepo::new();
        let head = current_branch(&repo.path);

        for error in [
            git_create_branch(path_str(&repo.path), "feature/existing").unwrap_err(),
            git_checkout_branch(path_str(&repo.path), "feature/missing").unwrap_err(),
            git_create_branch(path_str(&repo.path), "bad name").unwrap_err(),
            git_checkout_branch(path_str(&repo.path), "bad name").unwrap_err(),
        ] {
            assert!(error.to_string().contains("branch_"));
            assert_eq!(current_branch(&repo.path), head);
        }
    }

    #[test]
    fn git_branch_mutation_rejects_non_repository_without_changing_head() {
        let repo = TestRepo::new();
        let non_repo = repo.root.join("not-a-repository");
        fs::create_dir(&non_repo).unwrap();
        let head = current_branch(&repo.path);

        for error in [
            git_create_branch(path_str(&non_repo), "feature/new-ui").unwrap_err(),
            git_checkout_branch(path_str(&non_repo), "main").unwrap_err(),
        ] {
            assert!(error.to_string().contains("not_repository"));
            assert_eq!(current_branch(&repo.path), head);
        }
    }

    #[test]
    fn git_branch_mutation_rejects_branch_checked_out_in_linked_worktree() {
        let repo = TestRepo::new();
        let linked_path = repo.root.join("linked-worktree");
        run_git(
            &repo.path,
            &[
                "worktree",
                "add",
                path_str(&linked_path),
                "feature/existing",
            ],
        );
        let head = current_branch(&repo.path);

        let error = git_checkout_branch(path_str(&repo.path), "feature/existing").unwrap_err();

        assert!(error.to_string().contains("branch_in_use"));
        assert_eq!(current_branch(&repo.path), head);
    }

    fn current_branch(repo: &Path) -> String {
        let output = Command::new("git")
            .args(["symbolic-ref", "--short", "HEAD"])
            .current_dir(repo)
            .output()
            .expect("git should be available");
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    fn path_str(path: &Path) -> &str {
        path.to_str().expect("temporary path should be UTF-8")
    }
}
