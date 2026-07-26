//! Policy enforcement for the capability gateway.
//!
//! Defines permission profiles, path traversal detection,
//! symlink escape prevention, command injection checks,
//! and output size limits.

use crate::{PathScope, PermissionClass, ToolError};

/// Policy enforcement result.
#[derive(Debug)]
pub enum PolicyResult {
    Allowed,
    Denied(String),
    NeedsApproval(String),
}

/// Check if a path is within the allowed scope.
///
/// Uses path components (`Path::starts_with`), never raw string prefix matching
/// (which would wrongly allow `/tmp/application` under project `/tmp/app`).
pub fn check_path_scope(path: &str, scope: &PathScope) -> PolicyResult {
    match scope {
        PathScope::None => PolicyResult::Denied("No path access allowed".to_string()),
        PathScope::Any => PolicyResult::Allowed,
        PathScope::Project(project_path) => match resolve_under_project(path, project_path) {
            Ok(true) => PolicyResult::Allowed,
            Ok(false) => PolicyResult::NeedsApproval(format!(
                "Path '{path}' is outside project scope '{project_path}'"
            )),
            Err(msg) => PolicyResult::Denied(msg),
        },
        PathScope::Glob(pattern) => {
            if glob_matches(path, pattern) {
                PolicyResult::Allowed
            } else {
                PolicyResult::Denied(format!(
                    "Path '{}' does not match pattern '{}'",
                    path, pattern
                ))
            }
        }
    }
}

/// Resolve `path` relative to project and return whether it stays under the project root.
fn resolve_under_project(path: &str, project_path: &str) -> Result<bool, String> {
    use std::path::{Component, Path, PathBuf};

    let project = Path::new(project_path);
    let abs_project = project.canonicalize().unwrap_or_else(|_| {
        if project.is_absolute() {
            project.to_path_buf()
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(project)
        }
    });

    let candidate = Path::new(path);
    let joined = if candidate.is_relative() {
        // Lexically join and reject `..` escapes above project root.
        let mut out = abs_project.clone();
        for c in candidate.components() {
            match c {
                Component::CurDir => {}
                Component::ParentDir => {
                    if !out.pop() || !out.starts_with(&abs_project) {
                        return Err(format!("Path '{path}' escapes project root"));
                    }
                }
                Component::Normal(s) => out.push(s),
                Component::RootDir | Component::Prefix(_) => {
                    return Err(format!("Invalid relative path '{path}'"));
                }
            }
        }
        out
    } else {
        candidate.to_path_buf()
    };

    let abs_candidate = joined.canonicalize().unwrap_or(joined);

    let abs_project = abs_project.canonicalize().unwrap_or(abs_project);

    Ok(abs_candidate.starts_with(&abs_project))
}

/// Check for path traversal attempts (e.g., "../" or absolute escape markers).
pub fn check_path_traversal(path: &str) -> Result<(), ToolError> {
    if path.contains("..") {
        return Err(ToolError {
            code: "path_traversal".to_string(),
            message: "Path traversal detected".to_string(),
            retryable: false,
        });
    }
    // Normalize-style absolute system paths that should never be auto-allowed
    // without an explicit project root / elevated permission profile.
    let lowered = path.to_ascii_lowercase();
    if lowered.starts_with("/etc/")
        || lowered.starts_with("/private/etc/")
        || lowered.starts_with("/sys/")
        || lowered.starts_with("/proc/")
    {
        return Err(ToolError {
            code: "path_traversal".to_string(),
            message: "Sensitive system path denied".to_string(),
            retryable: false,
        });
    }
    Ok(())
}

/// Check for command injection in input strings.
pub fn check_command_injection(input: &str) -> Result<(), ToolError> {
    let dangerous = [';', '|', '`', '$', '(', ')', '{', '}', '&', '>', '<'];
    if input.contains(dangerous) {
        return Err(ToolError {
            code: "command_injection".to_string(),
            message: "Potential command injection detected".to_string(),
            retryable: false,
        });
    }
    Ok(())
}

/// Check if output exceeds the size limit.
pub fn check_output_limit(output: &[u8], limit: u64) -> bool {
    output.len() as u64 > limit
}

/// Check permission class for a tool.
///
/// Plan Mode is handled first and on purpose. The gear is a *ceiling*, so it has
/// to be impossible for it to reach the autonomous fast path below no matter how
/// the profile string was produced — an aliasing mistake upstream must degrade
/// into "ask", never into "allow everything".
pub fn check_permission(class: PermissionClass, profile: &str) -> PolicyResult {
    if profile == crate::plan_mode::PLAN_PROFILE {
        // Anything that can change the machine was already refused by
        // `plan_mode::decision` before this call. What reaches here is a read,
        // or a network read the user still gets asked about.
        return match class {
            PermissionClass::AlwaysAllowed | PermissionClass::ProjectRead => PolicyResult::Allowed,
            _ => PolicyResult::NeedsApproval(format!(
                "{class:?} requires explicit approval (Plan Mode)"
            )),
        };
    }
    if profile == "autonomous" {
        return PolicyResult::Allowed;
    }
    match class {
        PermissionClass::AlwaysAllowed => PolicyResult::Allowed,
        PermissionClass::ProjectRead => PolicyResult::Allowed,
        PermissionClass::ProjectWrite => {
            PolicyResult::NeedsApproval("Project write requires approval".to_string())
        }
        PermissionClass::ExternalWrite
        | PermissionClass::Credentials
        | PermissionClass::Elevation
        | PermissionClass::DestructiveCommand
        | PermissionClass::PrivacyResource => {
            PolicyResult::NeedsApproval(format!("{:?} requires explicit approval", class))
        }
    }
}

/// Simple glob matching (supports * and ?).
fn glob_matches(path: &str, pattern: &str) -> bool {
    glob_matches_public(path, pattern)
}

/// Public alias for tool implementations that need glob filtering.
pub fn glob_matches_public(path: &str, pattern: &str) -> bool {
    let regex_pattern = format!(
        "^{}$",
        regex::escape(pattern)
            .replace(r"\*", ".*")
            .replace(r"\?", ".")
    );
    regex::Regex::new(&regex_pattern)
        .map(|re| re.is_match(path))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_traversal_detected() {
        let result = check_path_traversal("/tmp/../etc/passwd");
        assert!(result.is_err());
    }

    #[test]
    fn test_path_traversal_clean() {
        let result = check_path_traversal("/tmp/test.txt");
        assert!(result.is_ok());
    }

    #[test]
    fn test_command_injection_detected() {
        let result = check_command_injection("file.txt; rm -rf /");
        assert!(result.is_err());
    }

    #[test]
    fn test_command_injection_clean() {
        let result = check_command_injection("file.txt");
        assert!(result.is_ok());
    }

    #[test]
    fn test_output_limit_exceeded() {
        let output = vec![0u8; 1000];
        assert!(check_output_limit(&output, 500));
    }

    #[test]
    fn test_output_limit_within() {
        let output = vec![0u8; 100];
        assert!(!check_output_limit(&output, 500));
    }

    #[test]
    fn test_path_scope_allowed() {
        let result = check_path_scope(
            "/project/src/main.rs",
            &PathScope::Project("/project".to_string()),
        );
        assert!(matches!(result, PolicyResult::Allowed));
    }

    #[test]
    fn test_path_scope_denied() {
        let result = check_path_scope("/etc/passwd", &PathScope::Project("/project".to_string()));
        assert!(matches!(result, PolicyResult::NeedsApproval(_)));
    }

    #[test]
    fn test_path_scope_rejects_string_prefix_bypass() {
        // /tmp/app must NOT allow /tmp/application-secret via string prefix.
        let result = check_path_scope(
            "/tmp/application-secret",
            &PathScope::Project("/tmp/app".to_string()),
        );
        assert!(
            matches!(
                result,
                PolicyResult::NeedsApproval(_) | PolicyResult::Denied(_)
            ),
            "prefix bypass must fail, got {result:?}"
        );
    }

    #[test]
    fn test_relative_path_under_project_allowed() {
        let result = check_path_scope("src/main.rs", &PathScope::Project("/project".to_string()));
        assert!(matches!(result, PolicyResult::Allowed));
    }

    #[test]
    fn test_relative_parent_escape_denied() {
        let result = check_path_scope(
            "../../etc/passwd",
            &PathScope::Project("/project".to_string()),
        );
        assert!(matches!(
            result,
            PolicyResult::Denied(_) | PolicyResult::NeedsApproval(_)
        ));
    }

    #[test]
    fn test_permission_autonomous() {
        for class in [
            PermissionClass::ProjectRead,
            PermissionClass::ProjectWrite,
            PermissionClass::ExternalWrite,
            PermissionClass::Credentials,
            PermissionClass::Elevation,
            PermissionClass::DestructiveCommand,
            PermissionClass::PrivacyResource,
        ] {
            let result = check_permission(class, "autonomous");
            assert!(matches!(result, PolicyResult::Allowed), "{class:?}");
        }
    }

    #[test]
    fn test_permission_profiles_match_native_engine_contract() {
        assert!(matches!(
            check_permission(PermissionClass::ProjectRead, "readonly"),
            PolicyResult::Allowed
        ));
        assert!(matches!(
            check_permission(PermissionClass::ProjectRead, "ask"),
            PolicyResult::Allowed
        ));
        assert!(matches!(
            check_permission(PermissionClass::ProjectWrite, "ask"),
            PolicyResult::NeedsApproval(_)
        ));
        let result = check_permission(PermissionClass::ExternalWrite, "confirm_each");
        assert!(matches!(result, PolicyResult::NeedsApproval(_)));
    }

    #[test]
    fn test_plan_profile_never_reaches_the_autonomous_fast_path() {
        for class in [
            PermissionClass::ProjectWrite,
            PermissionClass::ExternalWrite,
            PermissionClass::Credentials,
            PermissionClass::Elevation,
            PermissionClass::DestructiveCommand,
            PermissionClass::PrivacyResource,
        ] {
            assert!(
                matches!(
                    check_permission(class, crate::plan_mode::PLAN_PROFILE),
                    PolicyResult::NeedsApproval(_)
                ),
                "{class:?} must still require approval in Plan Mode"
            );
        }
    }

    #[test]
    fn test_plan_profile_keeps_reads_open() {
        for class in [
            PermissionClass::AlwaysAllowed,
            PermissionClass::ProjectRead,
        ] {
            assert!(
                matches!(
                    check_permission(class, crate::plan_mode::PLAN_PROFILE),
                    PolicyResult::Allowed
                ),
                "{class:?} must stay allowed so the agent can research"
            );
        }
    }

    #[test]
    fn test_glob_matching() {
        assert!(glob_matches("/tmp/test.txt", "/tmp/*.txt"));
        assert!(!glob_matches("/tmp/test.txt", "/tmp/*.md"));
    }
}
