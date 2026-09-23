//! Read-only access to the extension manifest installed by the product.

use std::fs;
use std::path::Path;

pub fn read_managed_manifest_version(dir: &Path) -> Option<String> {
    let text = fs::read_to_string(dir.join("manifest.json")).ok()?;
    let manifest: serde_json::Value = serde_json::from_str(&text).ok()?;
    manifest.get("version")?.as_str().map(String::from)
}
