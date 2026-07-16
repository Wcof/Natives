use crate::usage::{
    mask_home, BreadcrumbKind, SourceCapabilities, UsageBreadcrumb, UsageSourceKind,
    UsageSourceState, UsageSourceStatus,
};
use std::path::Path;

pub fn detected_source_status(id: &str, label: &str, path: &Path) -> Option<UsageSourceStatus> {
    path.exists().then(|| UsageSourceStatus {
        id: id.into(),
        label: label.into(),
        kind: UsageSourceKind::External,
        state: UsageSourceState::Detected,
        breadcrumbs: vec![UsageBreadcrumb {
            kind: BreadcrumbKind::Database,
            label: mask_home(path.to_string_lossy().as_ref()),
        }],
        capabilities: SourceCapabilities {
            total_tokens: false,
            token_breakdown: false,
            cache: false,
            cost: false,
            hourly: false,
            project: false,
            messages: false,
            sessions: false,
            duration: false,
        },
        duration_method: None,
    })
}

pub fn detect_unmeasurable_sources() -> Vec<UsageSourceStatus> {
    let Some(home) = dirs::home_dir() else {
        return vec![];
    };
    let app_support = home.join("Library/Application Support");
    [
        ("cursor", "Cursor", app_support.join("Cursor")),
        (
            "claude-desktop",
            "Claude Desktop",
            app_support.join("Claude"),
        ),
        (
            "chatgpt-desktop",
            "ChatGPT Desktop",
            app_support.join("com.openai.chat"),
        ),
        (
            "antigravity",
            "Antigravity",
            home.join(".gemini/antigravity"),
        ),
    ]
    .into_iter()
    .filter_map(|(id, label, path)| detected_source_status(id, label, &path))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::detected_source_status;
    use crate::usage::UsageSourceState;

    #[test]
    fn existing_app_data_is_detected_without_usage_capabilities() {
        let root =
            std::env::temp_dir().join(format!("natives-detected-source-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let status = detected_source_status("cursor", "Cursor", &root).unwrap();
        std::fs::remove_dir_all(root).unwrap();

        assert!(matches!(status.state, UsageSourceState::Detected));
        assert!(!status.capabilities.total_tokens);
        assert!(!status.capabilities.sessions);
    }
}
