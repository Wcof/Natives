//! Release asset probe — natives.app.json manifest candidates (compose
//! analysis moved to `super::compose`).

use super::compose::analyze_compose_file;
use super::github::{is_compose_asset_name, is_manifest_asset_name, GhAsset, GhRelease};
use super::model::{CreativeAppRuntime, EnvRequirement, InstallCandidate};
use crate::{Error, Result};
use serde::Deserialize;
use std::path::Path;

// Backward-compatible re-exports: compose analysis now lives in `super::compose`.
pub use super::compose::{
    analyze_compose_file, analyze_compose_yaml, collect_host_ports, normalize_compose_to_localhost,
    ComposeAnalysis, ComposeServiceInfo,
};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativesAppManifest {
    #[serde(default)]
    pub schema_version: Option<u32>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub runtime: Option<String>,
    #[serde(default)]
    pub compose_asset: Option<String>,
    #[serde(default)]
    pub service: Option<String>,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub open_path: Option<String>,
    #[serde(default)]
    pub health_path: Option<String>,
    #[serde(default)]
    pub env: Vec<ManifestEnv>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestEnv {
    pub key: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub secret: bool,
}

/// Probe a release (in-memory asset list + optional downloaded files under `release_dir`).
pub fn probe_release(release: &GhRelease, release_dir: Option<&Path>) -> Result<ProbeOutcome> {
    let mut warnings = Vec::new();
    let mut blockers = Vec::new();
    let mut candidates: Vec<InstallCandidate> = Vec::new();

    let manifest_asset = release
        .assets
        .iter()
        .find(|a| is_manifest_asset_name(&a.name));
    let compose_assets: Vec<&GhAsset> = release
        .assets
        .iter()
        .filter(|a| is_compose_asset_name(&a.name))
        .collect();

    let mut manifest: Option<NativesAppManifest> = None;
    if let Some(asset) = manifest_asset {
        if let Some(dir) = release_dir {
            let path = dir.join(&asset.name);
            if path.exists() {
                match load_manifest(&path) {
                    Ok(m) => manifest = Some(m),
                    Err(e) => warnings.push(format!("natives.app.json: {e}")),
                }
            }
        }
    }

    // Compose candidates from assets
    for asset in &compose_assets {
        if asset.name == "natives.compose.zip" {
            candidates.push(InstallCandidate {
                id: format!("compose-zip-{}", asset.id),
                runtime: CreativeAppRuntime::DockerCompose,
                confidence: 85,
                title: "Compose package (natives.compose.zip)".into(),
                description: "Self-contained compose archive from release".into(),
                primary_asset: asset.name.clone(),
                service: manifest.as_ref().and_then(|m| m.service.clone()),
                image: None,
                suggested_host_port: manifest.as_ref().and_then(|m| m.port),
                container_port: manifest.as_ref().and_then(|m| m.port),
                open_path: manifest
                    .as_ref()
                    .and_then(|m| m.open_path.clone())
                    .unwrap_or_else(|| "/".into()),
                health_path: manifest.as_ref().and_then(|m| m.health_path.clone()),
                env_requirements: env_from_manifest(manifest.as_ref()),
                risk_summary: vec![],
                hard_blockers: vec![],
                requires_manual: manifest.as_ref().and_then(|m| m.port).is_none(),
            });
            continue;
        }

        let mut risk = Vec::new();
        let mut hard = Vec::new();
        let mut service = manifest.as_ref().and_then(|m| m.service.clone());
        let mut host_port = manifest.as_ref().and_then(|m| m.port);
        let mut container_port = host_port;
        let mut requires_manual = false;

        if let Some(dir) = release_dir {
            let path = dir.join(&asset.name);
            if path.exists() {
                match analyze_compose_file(&path) {
                    Ok(analysis) => {
                        risk.extend(analysis.risks);
                        hard.extend(analysis.hard_blockers);
                        if service.is_none() {
                            service = analysis.preferred_service;
                        }
                        if host_port.is_none() {
                            host_port = analysis.host_port;
                            container_port = analysis.container_port;
                        }
                        if analysis.web_service_count != 1 || host_port.is_none() {
                            requires_manual = true;
                        }
                        if analysis.build_only {
                            hard.push("service uses build without pullable image".into());
                        }
                    }
                    Err(e) => {
                        warnings.push(format!("{}: {e}", asset.name));
                        requires_manual = true;
                    }
                }
            } else {
                requires_manual = true;
            }
        } else {
            requires_manual = host_port.is_none();
        }

        let confidence = if hard.is_empty() && !requires_manual {
            if manifest
                .as_ref()
                .and_then(|m| m.compose_asset.as_ref())
                .map(|c| c == &asset.name)
                .unwrap_or(false)
            {
                95
            } else {
                80
            }
        } else if hard.is_empty() {
            60
        } else {
            20
        };

        candidates.push(InstallCandidate {
            id: format!("compose-{}-{}", asset.name, asset.id),
            runtime: CreativeAppRuntime::DockerCompose,
            confidence,
            title: format!("Compose ({})", asset.name),
            description: "Docker Compose from release asset".into(),
            primary_asset: asset.name.clone(),
            service,
            image: None,
            suggested_host_port: host_port,
            container_port,
            open_path: manifest
                .as_ref()
                .and_then(|m| m.open_path.clone())
                .unwrap_or_else(|| "/".into()),
            health_path: manifest.as_ref().and_then(|m| m.health_path.clone()),
            env_requirements: env_from_manifest(manifest.as_ref()),
            risk_summary: risk,
            hard_blockers: hard,
            requires_manual,
        });
    }

    // Manifest-declared compose asset not already covered
    if let Some(m) = &manifest {
        if let Some(ca) = &m.compose_asset {
            if !candidates.iter().any(|c| &c.primary_asset == ca) {
                // asset missing from release
                if is_compose_asset_name(ca) || ca.ends_with(".yml") || ca.ends_with(".yaml") {
                    warnings.push(format!(
                        "manifest composeAsset '{ca}' not found in release assets"
                    ));
                }
            } else {
                // boost confidence for declared asset
                for c in &mut candidates {
                    if &c.primary_asset == ca && c.hard_blockers.is_empty() {
                        c.confidence = c.confidence.max(95);
                    }
                }
            }
        }

        // Docker Run candidate from manifest
        let runtime_s = m.runtime.as_deref().unwrap_or("");
        let wants_run = runtime_s == "docker-run"
            || runtime_s == "docker_run"
            || (m.image.is_some() && m.port.is_some());
        if wants_run {
            if let (Some(image), Some(port)) = (&m.image, m.port) {
                candidates.push(InstallCandidate {
                    id: format!("run-{}", release.id),
                    runtime: CreativeAppRuntime::DockerRun,
                    confidence: if runtime_s.contains("run") { 90 } else { 70 },
                    title: m.name.clone().unwrap_or_else(|| "Docker Run".into()),
                    description: format!("Image {image} port {port}"),
                    primary_asset: image.clone(),
                    service: None,
                    image: Some(image.clone()),
                    suggested_host_port: Some(port),
                    container_port: Some(port),
                    open_path: m.open_path.clone().unwrap_or_else(|| "/".into()),
                    health_path: m.health_path.clone(),
                    env_requirements: env_from_manifest(Some(m)),
                    risk_summary: vec![],
                    hard_blockers: vec![],
                    requires_manual: m.env.iter().any(|e| e.required),
                });
            } else {
                blockers.push("docker-run requires image and port in natives.app.json".into());
            }
        }
    }

    if candidates.is_empty() {
        blockers.push(
            "no container install signal (compose asset, natives.compose.zip, or natives.app.json)"
                .into(),
        );
    }

    // Sort by confidence desc
    candidates.sort_by_key(|c| std::cmp::Reverse(c.confidence));

    // One-click eligibility: single top candidate without manual/hard blockers,
    // clear port, no competing equal confidence.
    let mut one_click_eligible = false;
    let mut one_click_id = None;
    if blockers.is_empty() {
        if let Some(top) = candidates.first() {
            let ties = candidates
                .iter()
                .filter(|c| c.confidence == top.confidence)
                .count();
            let env_ok = !top.env_requirements.iter().any(|e| e.required);
            if ties == 1
                && !top.requires_manual
                && top.hard_blockers.is_empty()
                && top.suggested_host_port.is_some()
                && env_ok
            {
                one_click_eligible = true;
                one_click_id = Some(top.id.clone());
            } else if ties > 1 {
                warnings.push(
                    "multiple equally confident candidates; manual selection required".into(),
                );
            } else if top.requires_manual || !top.hard_blockers.is_empty() || !env_ok {
                warnings.push("top candidate requires manual configuration".into());
            }
        }
    }

    Ok(ProbeOutcome {
        candidates,
        warnings,
        blockers,
        one_click_eligible,
        one_click_candidate_id: one_click_id,
    })
}

#[derive(Debug, Clone)]
pub struct ProbeOutcome {
    pub candidates: Vec<InstallCandidate>,
    pub warnings: Vec<String>,
    pub blockers: Vec<String>,
    pub one_click_eligible: bool,
    pub one_click_candidate_id: Option<String>,
}

fn env_from_manifest(m: Option<&NativesAppManifest>) -> Vec<EnvRequirement> {
    m.map(|m| {
        m.env
            .iter()
            .map(|e| EnvRequirement {
                key: e.key.clone(),
                required: e.required,
                secret: e.secret,
            })
            .collect()
    })
    .unwrap_or_default()
}

pub fn load_manifest(path: &Path) -> Result<NativesAppManifest> {
    let text = std::fs::read_to_string(path).map_err(Error::Io)?;
    serde_json::from_str(&text).map_err(|e| Error::InvalidInput(format!("natives.app.json: {e}")))
}

#[cfg(test)]
mod tests {
    use super::super::github::GhRelease;
    use super::*;

    #[test]
    fn probe_no_signal() {
        let release = GhRelease {
            id: 1,
            tag_name: "v1".into(),
            draft: false,
            prerelease: false,
            assets: vec![GhAsset {
                id: 9,
                name: "README.md".into(),
                size: 10,
                content_type: None,
                browser_download_url: None,
            }],
        };
        let out = probe_release(&release, None).unwrap();
        assert!(out.candidates.is_empty());
        assert!(!out.blockers.is_empty());
        assert!(!out.one_click_eligible);
    }

    #[test]
    fn probe_run_manifest() {
        let dir =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/creative_apps/run_manifest");
        let release = GhRelease {
            id: 2,
            tag_name: "v1".into(),
            draft: false,
            prerelease: false,
            assets: vec![GhAsset {
                id: 1,
                name: "natives.app.json".into(),
                size: 100,
                content_type: None,
                browser_download_url: None,
            }],
        };
        // Copy fixture into temp release dir layout
        let tmp = std::env::temp_dir().join(format!(
            "natives-probe-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::copy(dir.join("natives.app.json"), tmp.join("natives.app.json")).unwrap();
        let out = probe_release(&release, Some(&tmp)).unwrap();
        assert!(out
            .candidates
            .iter()
            .any(|c| c.runtime == CreativeAppRuntime::DockerRun));
        // required env → not one-click
        assert!(!out.one_click_eligible);
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
