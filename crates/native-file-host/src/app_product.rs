//! Fixed built-in module definitions for the single-product route
//! (ADR-0031, official module registry).
//!
//! The module list is GENERATED at build time from `modules/registry.json`
//! (the single compile-time authority; see `build.rs`). The host keeps the
//! same fixed module list at compile time so an empty preference table still
//! projects the built-in modules honestly: availability comes from the actual
//! runtime payload, never from page data, a registration record or any remote
//! source. Adding an official module only requires a registry entry — no Core
//! code change. This list is not a second App Registry.

use serde::Serialize;

pub struct FixedModule {
    pub app_id: &'static str,
    pub name_zh: &'static str,
    pub name_en: &'static str,
    pub description_zh: &'static str,
    pub description_en: &'static str,
    pub entry_route: &'static str,
}

include!(concat!(env!("OUT_DIR"), "/fixed_modules.rs"));

/// Fixed verified system source (contract §3.2): production installs read
/// `/Library/Application Support/Natives/`; isolated local candidates use
/// `Natives-Local` (ADR-0029 §4.2). Windows/Linux sources are declared only
/// after real platform acceptance and never guessed from macOS results.
///
/// Dev product-source revision (ADR-0029, 2026-09-17): the root-owned source
/// is a release/installer-candidate requirement only. Non-production builds
/// read the user-writable dev product source under the dev trust-root
/// namespace (`~/.natives-local/product-source/`) so everyday iteration never
/// needs sudo; `NATIVES_DEV_PRODUCT_SOURCE` overrides the location for tests
/// and multi-checkout setups. Verification rules (signature, payload hashes)
/// are identical in either layout.
pub fn default_system_source() -> std::path::PathBuf {
    if crate::app_signing::is_production_build() {
        return std::path::PathBuf::from("/Library/Application Support/Natives");
    }
    if let Some(dir) = std::env::var_os("NATIVES_DEV_PRODUCT_SOURCE") {
        return std::path::PathBuf::from(dir);
    }
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_default();
    home.join(".natives-local").join("product-source")
}

/// Projected card for one fixed module: the definition comes from the
/// product manifest, user preferences are overlaid from the local projection
/// when one exists (defaults are only for users who never chose), and
/// `present` reflects the actual payload directory. `configured` marks an
/// activated module (host registration recorded); it does not imply any
/// download intent and never becomes an installation button.
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
