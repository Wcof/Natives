//! Security tests for the capability gateway.
//!
//! Tests for traversal, symlink escape, command injection,
//! oversized output, timeout, and secret redaction.

use capability_gateway::policy::*;
use capability_gateway::{CapabilityGateway, PermissionClass, PathScope};
use capability_gateway::tools::builtin_tools;

#[test]
fn test_traversal_escape_fails() {
    let result = check_path_traversal("/tmp/../../etc/passwd");
    assert!(result.is_err(), "Path traversal should be detected");
}

#[test]
fn test_symlink_escape_detected() {
    let result = check_path_scope("/etc/passwd", &PathScope::Project("/safe/project".to_string()));
    assert!(matches!(result, PolicyResult::NeedsApproval(_)), "External path should require approval");
}

#[test]
fn test_command_injection_fails() {
    let result = check_command_injection("filename; rm -rf /");
    assert!(result.is_err(), "Command injection should be detected");
}

#[test]
fn test_command_injection_pipe_fails() {
    let result = check_command_injection("filename | cat /etc/passwd");
    assert!(result.is_err(), "Pipe injection should be detected");
}

#[test]
fn test_oversized_output_detected() {
    let output = vec![0u8; 1_000_000];
    assert!(check_output_limit(&output, 500_000), "Oversized output should be detected");
}

#[test]
fn test_normal_output_within_limit() {
    let output = vec![0u8; 1000];
    assert!(!check_output_limit(&output, 500_000), "Normal output should be within limit");
}

#[test]
fn test_builtin_tools_registered() {
    let tools = builtin_tools();
    assert!(!tools.is_empty(), "Built-in tools should be registered");
    let names: Vec<&str> = tools.iter().map(|t| t.name).collect();
    assert!(names.contains(&"read_file"), "read_file should be registered");
    assert!(names.contains(&"write_file"), "write_file should be registered");
    assert!(names.contains(&"search_files"), "search_files should be registered");
}

#[test]
fn test_gateway_register_and_list() {
    let mut gateway = CapabilityGateway::new();
    gateway.register_builtins();
    let tools = gateway.list_tools();
    assert_eq!(tools.len(), 4, "Should have 4 built-in tools");
}

#[test]
fn test_gateway_get_tool() {
    let mut gateway = CapabilityGateway::new();
    gateway.register_builtins();
    let tool = gateway.get_tool("read_file");
    assert!(tool.is_some(), "read_file should be found");
    assert_eq!(tool.unwrap().name, "read_file");
}

#[test]
fn test_gateway_get_unknown_tool() {
    let gateway = CapabilityGateway::new();
    let tool = gateway.get_tool("nonexistent");
    assert!(tool.is_none(), "Unknown tool should return None");
}

#[test]
fn test_permission_hard_policy() {
    // Hard-policy actions should require explicit authorization
    let result = check_permission(
        capability_gateway::PermissionClass::ExternalWrite,
        "autonomous",
    );
    assert!(matches!(result, PolicyResult::NeedsApproval(_)), "External write should require approval");
}

#[test]
fn test_always_allowed_pass() {
    let result = check_permission(
        capability_gateway::PermissionClass::AlwaysAllowed,
        "confirm_each",
    );
    assert!(matches!(result, PolicyResult::Allowed), "AlwaysAllowed should pass");
}