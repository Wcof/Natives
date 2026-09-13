//! Fixed built-in module definitions for the single-product route
//! (ADR-0027/0029, 2026-09-12 convergence).
//!
//! The signed product composition manifest is a packaging-time artifact
//! (implementation plan P1). Until the per-user product-configuration chain
//! lands, the host ships the same fixed module list at compile time so an
//! empty install table still projects the built-in modules honestly:
//! availability comes from the actual payload on disk, never from a catalog,
//! an install record or any remote source. Adding a module requires a new
//! complete Natives version; this list is not a second App Registry.

use serde::Serialize;

pub struct FixedModule {
    pub app_id: &'static str,
    pub name_zh: &'static str,
    pub name_en: &'static str,
    pub description_zh: &'static str,
    pub description_en: &'static str,
    pub entry_route: &'static str,
}

pub const FIXED_MODULES: &[FixedModule] = &[FixedModule {
    app_id: "fund",
    name_zh: "基金",
    name_en: "Fund",
    description_zh: "个人基金记账、持仓与收益",
    description_en: "Personal fund ledger, positions and returns",
    entry_route: "app.html?app=fund",
}];

/// Fixed verified system source (contract §3.2): production installs read
/// `/Library/Application Support/Natives/`; isolated local candidates use
/// `Natives-Local` (ADR-0029 §4.2). Windows/Linux sources are declared only
/// after real platform acceptance and never guessed from macOS results.
pub fn default_system_source() -> std::path::PathBuf {
    if crate::app_signing::is_production_build() {
        std::path::PathBuf::from("/Library/Application Support/Natives")
    } else {
        std::path::PathBuf::from("/Library/Application Support/Natives-Local")
    }
}

/// Projected card for one fixed module: the definition comes from the
/// product manifest, user preferences are overlaid from an App Store record
/// when one exists (defaults are only for users who never chose), and
/// `present` reflects the actual payload directory. `configured` marks an
/// activated module (host registration recorded); it does not imply any
/// install intent and never becomes an "install" button.
#[derive(Serialize)]
pub struct ModuleProjection {
    #[serde(rename = "appId")]
    pub app_id: String,
    pub name: serde_json::Value,
    pub description: serde_json::Value,
    #[serde(rename = "entryRoute")]
    pub entry_route: String,
    pub present: bool,
    pub configured: bool,
    pub enabled: bool,
    #[serde(rename = "showInSidebar")]
    pub show_in_sidebar: bool,
    #[serde(rename = "sidebarOrder")]
    pub sidebar_order: i64,
}

pub fn module_projection(
    module: &FixedModule,
    preference: Option<(bool, bool, i64)>,
    configured: bool,
    apps_root: &std::path::Path,
) -> ModuleProjection {
    let (enabled, show_in_sidebar, sidebar_order) = preference.unwrap_or((true, true, 0));
    let runtime = apps_root.join(module.app_id).join("runtime");
    let present = std::fs::read_dir(&runtime)
        .map(|entries| {
            entries
                .flatten()
                .any(|entry| entry.file_type().map(|t| t.is_dir()).unwrap_or(false))
        })
        .unwrap_or(false);
    ModuleProjection {
        app_id: module.app_id.to_string(),
        name: serde_json::json!({ "zh_CN": module.name_zh, "en": module.name_en }),
        description: serde_json::json!({
            "zh_CN": module.description_zh,
            "en": module.description_en,
        }),
        entry_route: module.entry_route.to_string(),
        present,
        configured,
        enabled,
        show_in_sidebar,
        sidebar_order,
    }
}
