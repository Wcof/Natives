//! Release asset probe — compose / zip / natives.app.json candidates.

use super::github::{is_compose_asset_name, is_manifest_asset_name, GhAsset, GhRelease};
use super::model::{
    CreativeAppRuntime, EnvRequirement, InstallCandidate,
};
use crate::{Error, Result};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::path::Path;

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
pub fn probe_release(
    release: &GhRelease,
    release_dir: Option<&Path>,
) -> Result<ProbeOutcome> {
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
        let wants_run = runtime_s == "docker-run" || runtime_s == "docker_run" || (m.image.is_some() && m.port.is_some());
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
    candidates.sort_by(|a, b| b.confidence.cmp(&a.confidence));

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
                warnings.push("multiple equally confident candidates; manual selection required".into());
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

#[derive(Debug, Default)]
pub struct ComposeAnalysis {
    pub preferred_service: Option<String>,
    pub host_port: Option<u16>,
    pub container_port: Option<u16>,
    pub web_service_count: usize,
    pub build_only: bool,
    pub risks: Vec<String>,
    pub hard_blockers: Vec<String>,
    pub services: Vec<ComposeServiceInfo>,
}

#[derive(Debug, Clone)]
pub struct ComposeServiceInfo {
    pub name: String,
    pub image: Option<String>,
    pub has_build: bool,
    pub host_ports: Vec<u16>,
    pub container_ports: Vec<u16>,
}

/// Parse compose YAML and apply safety + web service heuristics.
pub fn analyze_compose_file(path: &Path) -> Result<ComposeAnalysis> {
    let text = std::fs::read_to_string(path).map_err(Error::Io)?;
    analyze_compose_yaml(&text)
}

pub fn analyze_compose_yaml(text: &str) -> Result<ComposeAnalysis> {
    let doc: serde_yaml::Value =
        serde_yaml::from_str(text).map_err(|e| Error::InvalidInput(format!("compose yaml: {e}")))?;
    let mut analysis = ComposeAnalysis::default();

    // Hard blockers at top-level
    scan_dangerous_yaml(&doc, "", &mut analysis.hard_blockers, &mut analysis.risks);

    let services = doc
        .get("services")
        .and_then(|s| s.as_mapping())
        .ok_or_else(|| Error::InvalidInput("compose has no services".into()))?;

    for (k, v) in services {
        let name = k.as_str().unwrap_or("").to_string();
        if name.is_empty() {
            continue;
        }
        let image = v
            .get("image")
            .and_then(|i| i.as_str())
            .map(|s| s.to_string());
        let has_build = v.get("build").is_some();
        if has_build && image.is_none() {
            analysis.build_only = true;
            analysis
                .hard_blockers
                .push(format!("service '{name}' is build-only (no image)"));
        }

        let (host_ports, container_ports) = parse_ports(v.get("ports"));
        // privileged etc. on service
        scan_dangerous_yaml(v, &format!("services.{name}"), &mut analysis.hard_blockers, &mut analysis.risks);

        analysis.services.push(ComposeServiceInfo {
            name: name.clone(),
            image,
            has_build,
            host_ports: host_ports.clone(),
            container_ports: container_ports.clone(),
        });
    }

    // Prefer services that publish ports
    let web_like: Vec<&ComposeServiceInfo> = analysis
        .services
        .iter()
        .filter(|s| !s.host_ports.is_empty() || !s.container_ports.is_empty())
        .collect();
    analysis.web_service_count = web_like.len();
    if web_like.len() == 1 {
        let s = web_like[0];
        analysis.preferred_service = Some(s.name.clone());
        analysis.host_port = s.host_ports.first().copied().or_else(|| s.container_ports.first().copied());
        analysis.container_port = s.container_ports.first().copied().or(analysis.host_port);
    } else if web_like.is_empty() {
        // single service without ports → manual
        if analysis.services.len() == 1 {
            analysis.preferred_service = Some(analysis.services[0].name.clone());
        }
    } else {
        analysis
            .risks
            .push(format!("{} services publish ports; select web service manually", web_like.len()));
        // Prefer name web/frontend/app/ui
        for pref in ["web", "frontend", "app", "ui", "nginx"] {
            if let Some(s) = web_like.iter().find(|s| s.name.eq_ignore_ascii_case(pref)) {
                analysis.preferred_service = Some(s.name.clone());
                analysis.host_port = s.host_ports.first().copied().or_else(|| s.container_ports.first().copied());
                analysis.container_port = s.container_ports.first().copied().or(analysis.host_port);
                break;
            }
        }
    }

    Ok(analysis)
}

fn parse_ports(ports: Option<&serde_yaml::Value>) -> (Vec<u16>, Vec<u16>) {
    let mut host = Vec::new();
    let mut container = Vec::new();
    let Some(ports) = ports else {
        return (host, container);
    };
    let seq = match ports.as_sequence() {
        Some(s) => s,
        None => return (host, container),
    };
    for p in seq {
        if let Some(s) = p.as_str() {
            // formats: "8080:80", "127.0.0.1:8080:80", "80", "8080:80/tcp"
            let s = s.split('/').next().unwrap_or(s);
            let parts: Vec<&str> = s.split(':').collect();
            match parts.len() {
                1 => {
                    if let Ok(c) = parts[0].parse::<u16>() {
                        container.push(c);
                    }
                }
                2 => {
                    if let Ok(h) = parts[0].parse::<u16>() {
                        host.push(h);
                    }
                    if let Ok(c) = parts[1].parse::<u16>() {
                        container.push(c);
                    }
                }
                3 => {
                    // host_ip:host:container
                    if let Ok(h) = parts[1].parse::<u16>() {
                        host.push(h);
                    }
                    if let Ok(c) = parts[2].parse::<u16>() {
                        container.push(c);
                    }
                }
                _ => {}
            }
        } else if let Some(n) = p.as_u64() {
            if n <= u16::MAX as u64 {
                container.push(n as u16);
            }
        } else if let Some(map) = p.as_mapping() {
            let tp = map
                .get(serde_yaml::Value::String("target".into()))
                .and_then(|v| v.as_u64());
            let pubp = map
                .get(serde_yaml::Value::String("published".into()))
                .and_then(|v| {
                    v.as_u64()
                        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                });
            if let Some(c) = tp {
                if c <= u16::MAX as u64 {
                    container.push(c as u16);
                }
            }
            if let Some(h) = pubp {
                if h <= u16::MAX as u64 {
                    host.push(h as u16);
                }
            }
        }
    }
    (host, container)
}

fn scan_dangerous_yaml(
    value: &serde_yaml::Value,
    path: &str,
    hard: &mut Vec<String>,
    risks: &mut Vec<String>,
) {
    let Some(map) = value.as_mapping() else {
        return;
    };
    for (k, v) in map {
        let key = k.as_str().unwrap_or("");
        let p = if path.is_empty() {
            key.to_string()
        } else {
            format!("{path}.{key}")
        };
        match key {
            "privileged" if v.as_bool() == Some(true) => {
                hard.push(format!("{p}: privileged mode is blocked"));
            }
            "network_mode" => {
                if let Some(s) = v.as_str() {
                    if s == "host" {
                        hard.push(format!("{p}: host network is blocked"));
                    }
                }
            }
            "pid" | "ipc" => {
                if v.as_str() == Some("host") {
                    hard.push(format!("{p}: host {key} namespace is blocked"));
                }
            }
            "devices" => {
                hard.push(format!("{p}: device mapping is blocked in v1"));
            }
            "volumes" => {
                inspect_volumes(v, &p, hard, risks);
            }
            _ => {
                if v.is_mapping() || v.is_sequence() {
                    if v.is_mapping() {
                        scan_dangerous_yaml(v, &p, hard, risks);
                    } else if let Some(seq) = v.as_sequence() {
                        for (i, item) in seq.iter().enumerate() {
                            scan_dangerous_yaml(item, &format!("{p}[{i}]"), hard, risks);
                        }
                    }
                }
            }
        }
    }
}

fn inspect_volumes(
    value: &serde_yaml::Value,
    path: &str,
    hard: &mut Vec<String>,
    risks: &mut Vec<String>,
) {
    let Some(seq) = value.as_sequence() else {
        return;
    };
    for (i, item) in seq.iter().enumerate() {
        let p = format!("{path}[{i}]");
        if let Some(s) = item.as_str() {
            classify_volume_str(s, &p, hard, risks);
        } else if let Some(map) = item.as_mapping() {
            let src = map
                .get(serde_yaml::Value::String("source".into()))
                .or_else(|| map.get(serde_yaml::Value::String("Source".into())))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let read_only = map
                .get(serde_yaml::Value::String("read_only".into()))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if src.starts_with('/') || src.starts_with('~') {
                classify_abs_mount(src, read_only, &p, hard, risks);
            }
            if src.contains("docker.sock") {
                hard.push(format!("{p}: Docker socket mount is blocked"));
            }
        }
    }
}

fn classify_volume_str(s: &str, path: &str, hard: &mut Vec<String>, risks: &mut Vec<String>) {
    // host:container[:mode]
    let parts: Vec<&str> = s.split(':').collect();
    if parts.is_empty() {
        return;
    }
    let src = parts[0];
    if src.contains("docker.sock") {
        hard.push(format!("{path}: Docker socket mount is blocked"));
        return;
    }
    let mode = parts.get(2).copied().unwrap_or("rw");
    let read_only = mode.contains("ro");
    if src.starts_with('/') || src.starts_with('~') {
        classify_abs_mount(src, read_only, path, hard, risks);
    }
}

fn classify_abs_mount(
    src: &str,
    read_only: bool,
    path: &str,
    hard: &mut Vec<String>,
    risks: &mut Vec<String>,
) {
    let home = dirs::home_dir()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_default();
    if src == "/" || src == "/etc" || src == "/root" || src.starts_with("/etc/") {
        hard.push(format!("{path}: host root/system mount '{src}' is blocked"));
        return;
    }
    if !home.is_empty() && (src == home || src == "~" || src.starts_with("~/")) && !read_only {
        hard.push(format!(
            "{path}: writable home directory mount '{src}' is blocked"
        ));
        return;
    }
    if !read_only {
        risks.push(format!(
            "{path}: absolute host mount '{src}' (rw) requires manual confirmation"
        ));
    } else {
        risks.push(format!(
            "{path}: absolute host mount '{src}' (ro) requires manual confirmation"
        ));
    }
}

/// Rewrite compose ports to bind 127.0.0.1 only; write to dest.
pub fn normalize_compose_to_localhost(
    source: &Path,
    dest: &Path,
    service: &str,
    host_port: u16,
    container_port: u16,
) -> Result<()> {
    let text = std::fs::read_to_string(source).map_err(Error::Io)?;
    let mut doc: serde_yaml::Value =
        serde_yaml::from_str(&text).map_err(|e| Error::InvalidInput(format!("compose yaml: {e}")))?;

    let services = doc
        .get_mut("services")
        .and_then(|s| s.as_mapping_mut())
        .ok_or_else(|| Error::InvalidInput("compose has no services".into()))?;

    let key = serde_yaml::Value::String(service.to_string());
    let svc = services
        .get_mut(&key)
        .ok_or_else(|| Error::InvalidInput(format!("service '{service}' not found")))?;

    // Force published port for the web service
    let port_entry = serde_yaml::Value::String(format!("127.0.0.1:{host_port}:{container_port}"));
    if let Some(map) = svc.as_mapping_mut() {
        map.insert(
            serde_yaml::Value::String("ports".into()),
            serde_yaml::Value::Sequence(vec![port_entry]),
        );
        // Ensure image present if only build — already blocked earlier
    }

    // Rewrite other services' host bindings to 127.0.0.1 when they publish ports
    for (k, v) in services.iter_mut() {
        let name = k.as_str().unwrap_or("");
        if name == service {
            continue;
        }
        if let Some(ports) = v.get_mut("ports").and_then(|p| p.as_sequence_mut()) {
            for p in ports.iter_mut() {
                if let Some(s) = p.as_str() {
                    *p = serde_yaml::Value::String(force_localhost_port_str(s));
                }
            }
        }
    }

    let out = serde_yaml::to_string(&doc)
        .map_err(|e| Error::Internal(format!("serialize compose: {e}")))?;
    super::paths::atomic_write(dest, out.as_bytes()).map_err(Error::Io)?;
    Ok(())
}

fn force_localhost_port_str(s: &str) -> String {
    let (core, suffix) = match s.split_once('/') {
        Some((c, rest)) => (c, format!("/{rest}")),
        None => (s, String::new()),
    };
    let parts: Vec<&str> = core.split(':').collect();
    match parts.len() {
        1 => format!("127.0.0.1:{}:{}{}", parts[0], parts[0], suffix),
        2 => format!("127.0.0.1:{}:{}{}", parts[0], parts[1], suffix),
        3 => format!("127.0.0.1:{}:{}{}", parts[1], parts[2], suffix),
        _ => format!("127.0.0.1:{s}"),
    }
}

/// Collect unique host ports claimed by a compose file (for conflict checks).
pub fn collect_host_ports(text: &str) -> Result<BTreeSet<u16>> {
    let analysis = analyze_compose_yaml(text)?;
    let mut set = BTreeSet::new();
    for s in analysis.services {
        for p in s.host_ports {
            set.insert(p);
        }
    }
    Ok(set)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::github::GhRelease;

    #[test]
    fn compose_simple_fixture() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/creative_apps/compose_simple/docker-compose.yml");
        let a = analyze_compose_file(&path).unwrap();
        assert_eq!(a.preferred_service.as_deref(), Some("web"));
        assert_eq!(a.host_port, Some(8080));
        assert!(!a.build_only);
        assert!(a.hard_blockers.is_empty());
    }

    #[test]
    fn build_only_blocked() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/creative_apps/compose_build_only/docker-compose.yml");
        let a = analyze_compose_file(&path).unwrap();
        assert!(a.build_only);
        assert!(!a.hard_blockers.is_empty());
    }

    #[test]
    fn multi_service_requires_choice() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/creative_apps/compose_multi_service/compose.yaml");
        let a = analyze_compose_file(&path).unwrap();
        assert!(a.web_service_count >= 2);
        assert_eq!(a.preferred_service.as_deref(), Some("web"));
    }

    #[test]
    fn port_normalize_localhost() {
        assert_eq!(
            force_localhost_port_str("8080:80"),
            "127.0.0.1:8080:80"
        );
        assert_eq!(
            force_localhost_port_str("0.0.0.0:8080:80"),
            "127.0.0.1:8080:80"
        );
    }

    // 注：safe_extract_zip 已上移至 crate::archive_ops（zip-slip 等安全测试随之迁移）

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
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/creative_apps/run_manifest");
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
        assert!(out.candidates.iter().any(|c| c.runtime == CreativeAppRuntime::DockerRun));
        // required env → not one-click
        assert!(!out.one_click_eligible);
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
