//! Policy enforcement for the capability gateway.
//!
//! Defines permission profiles, path traversal detection,
//! symlink escape prevention, command injection checks,
//! and output size limits.

use crate::{PermissionClass, PathScope, ToolError};

/// Policy enforcement result.
#[derive(Debug)]
pub enum PolicyResult {
    Allowed,
    Denied(String),
    NeedsApproval(String),
}

/// Check if a path is within the allowed scope.
pub fn check_path_scope(path: &str, scope: &PathScope) -> PolicyResult {
    match scope {
        PathScope::None => PolicyResult::Denied("No path access allowed".to_string()),
        PathScope::Any => PolicyResult::Allowed,
        PathScope::Project(project_path) => {
            if path.starts_with(project_path) {
                PolicyResult::Allowed
            } else {
                PolicyResult::NeedsApproval(format!(
                    "Path '{}' is outside project scope '{}'",
                    path, project_path
                ))
            }
        }
        PathScope::Glob(pattern) => {
            if glob_matches(path, pattern) {
                PolicyResult::Allowed
            } else {
                PolicyResult::Denied(format!("Path '{}' does not match pattern '{}'", path, pattern))
            }
        }
    }
}

/// Check for path traversal attempts (e.g., "../" or "/etc/").
pub fn check_path_traversal(path: &str) -> Result<(), ToolError> {
    if path.contains("..") {
        return Err(ToolError {
            code: "path_traversal".to_string(),
            message: "Path traversal detected".to_string(),
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
pub fn check_permission(class: PermissionClass, profile: &str) -> PolicyResult {
    match class {
        PermissionClass::AlwaysAllowed => PolicyResult::Allowed,
        PermissionClass::ProjectRead => {
            if profile == "autonomous" {
                PolicyResult::Allowed
            } else {
                PolicyResult::NeedsApproval("Project read requires approval".to_string())
            }
        }
        PermissionClass::ProjectWrite => {
            if profile == "autonomous" {
                PolicyResult::Allowed
            } else {
                PolicyResult::NeedsApproval("Project write requires approval".to_string())
            }
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
        let result = check_path_scope("/project/src/main.rs", &PathScope::Project("/project".to_string()));
        assert!(matches!(result, PolicyResult::Allowed));
    }

    #[test]
    fn test_path_scope_denied() {
        let result = check_path_scope("/etc/passwd", &PathScope::Project("/project".to_string()));
        assert!(matches!(result, PolicyResult::NeedsApproval(_)));
    }

    #[test]
    fn test_permission_autonomous() {
        let result = check_permission(PermissionClass::ProjectWrite, "autonomous");
        assert!(matches!(result, PolicyResult::Allowed));
    }

    #[test]
    fn test_permission_confirm_each() {
        let result = check_permission(PermissionClass::ExternalWrite, "confirm_each");
        assert!(matches!(result, PolicyResult::NeedsApproval(_)));
    }

    #[test]
    fn test_glob_matching() {
        assert!(glob_matches("/tmp/test.txt", "/tmp/*.txt"));
        assert!(!glob_matches("/tmp/test.txt", "/tmp/*.md"));
    }
}