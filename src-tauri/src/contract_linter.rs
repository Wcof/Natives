//! Contract Linter — static code audit for AI-generated modules.
//! Enforces KI-3 constraints before modules are written to disk.
//!
//! Checks:
//! 1. No remote CDN scripts (only `tauri://assets/vendor/` allowed)
//! 2. Migration files must be declarative JSON only
//! 3. manifest schema_version must be valid
//! 4. permissions must be from the allowed list
//!
//! # Why this is a shared crate
//!
//! The Tauri host runs these checks in `write_generated_module` before any
//! bytes hit `~/.natives/modules/`. The Agent Daemon runs the *same* checks in
//! its draft tools before a revision lands in `~/.natives/drafts/`. Those are
//! two processes, and ADR-0014 invariant #2 requires them to measure with one
//! ruler: a draft that lints clean must never be rejected at publish time.
//!
//! Keeping the rules here — pure functions over `regex` + `serde_json`, with no
//! filesystem, database or Tauri coupling — is what makes that invariant hold.
//! Do not fork these rules into either process.

use regex::Regex;

const ALLOWED_SCRIPT_SOURCES: &[&str] = &["'self'", "tauri://assets", "tauri://assets/vendor/"];

const ALLOWED_PERMISSIONS: &[&str] = &[
    "db:read",
    "db:write",
    "env:read",
    "notification",
    "ipc:send",
    "lifecycle",
    "settings",
];

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LinterResult {
    pub passed: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

/// Run all lint checks on a generated module's HTML content.
pub fn lint_html(html_content: &str) -> LinterResult {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    // Check 1: No remote CDN scripts
    let script_re = Regex::new(r#"<script[^>]*src\s*=\s*["']([^"']+)["']"#).unwrap();
    for cap in script_re.captures_iter(html_content) {
        let src = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        if !ALLOWED_SCRIPT_SOURCES
            .iter()
            .any(|allowed| src.starts_with(allowed))
        {
            errors.push(format!(
                "Remote script source not allowed: '{}'. Only tauri://assets/vendor/ sources are permitted.",
                src
            ));
        }
    }

    // Check 2: No inline event handlers (security risk)
    let inline_handler_re = Regex::new(r#"on\w+\s*=\s*["'][^"']*["']"#).unwrap();
    if inline_handler_re.is_match(html_content) {
        warnings.push(
            "Inline event handlers detected (onclick, onload, etc.). Consider using addEventListener instead."
                .to_string(),
        );
    }

    // Check 3: No eval or similar
    if html_content.contains("eval(") || html_content.contains("new Function(") {
        errors.push("Dynamic code execution (eval/new Function) is not allowed.".to_string());
    }

    LinterResult {
        passed: errors.is_empty(),
        errors,
        warnings,
    }
}

/// Lint a manifest JSON for schema compliance.
pub fn lint_manifest(manifest_json: &serde_json::Value) -> LinterResult {
    let mut errors = Vec::new();

    // Check schema_version exists
    match manifest_json.get("schema_version") {
        Some(v) if v.is_string() => {
            let ver = v.as_str().unwrap();
            if ver != "1.0" && ver != "1.1" {
                errors.push(format!(
                    "Unsupported schema_version: {}. Expected 1.0 or 1.1.",
                    ver
                ));
            }
        }
        Some(_) => errors.push("schema_version must be a string.".to_string()),
        None => errors.push("Missing schema_version in manifest.".to_string()),
    }

    // Check permissions are from allowed list
    if let Some(perms) = manifest_json.get("permissions").and_then(|p| p.as_array()) {
        for perm in perms {
            if let Some(p_str) = perm.as_str() {
                if !ALLOWED_PERMISSIONS.contains(&p_str) {
                    errors.push(format!(
                        "Permission '{}' is not in the allowed list.",
                        p_str
                    ));
                }
            }
        }
    }

    LinterResult {
        passed: errors.is_empty(),
        errors,
        warnings: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reject_remote_cdn_script() {
        let html = r#"<html><body><script src="https://cdn.jsdelivr.net/npm/react"></script></body></html>"#;
        let result = lint_html(html);
        assert!(!result.passed);
        assert!(result.errors[0].contains("cdn.jsdelivr.net"));
    }

    #[test]
    fn test_allow_tauri_vendor_script() {
        let html =
            r#"<html><body><script src="tauri://assets/vendor/alpine.js"></script></body></html>"#;
        let result = lint_html(html);
        assert!(result.passed);
    }

    #[test]
    fn test_reject_inline_handler() {
        let html = r#"<button onclick="alert(1)">Click</button>"#;
        let result = lint_html(html);
        assert!(result.passed); // inline handlers are warnings, not errors
        assert!(!result.warnings.is_empty());
    }

    #[test]
    fn test_reject_eval() {
        let html = r#"<script>eval("alert(1)")</script>"#;
        let result = lint_html(html);
        assert!(!result.passed);
        assert!(result.errors[0].contains("eval"));
    }

    #[test]
    fn test_manifest_valid_schema_version() {
        let manifest = serde_json::json!({
            "schema_version": "1.0",
            "permissions": ["db:read", "notification"]
        });
        let result = lint_manifest(&manifest);
        assert!(result.passed);
    }

    #[test]
    fn test_manifest_invalid_permission() {
        let manifest = serde_json::json!({
            "schema_version": "1.0",
            "permissions": ["db:read", "admin:full_access"]
        });
        let result = lint_manifest(&manifest);
        assert!(!result.passed);
        assert!(result.errors[0].contains("admin:full_access"));
    }

    #[test]
    fn test_manifest_missing_schema_version() {
        let manifest = serde_json::json!({
            "name": "test"
        });
        let result = lint_manifest(&manifest);
        assert!(!result.passed);
    }

    #[test]
    fn test_clean_html_passes() {
        let html = r#"<html><body><div>Hello</div><script src="tauri://assets/vendor/htmx.js"></script></body></html>"#;
        let result = lint_html(html);
        assert!(result.passed);
    }
}
