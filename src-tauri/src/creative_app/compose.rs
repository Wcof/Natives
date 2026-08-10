//! Docker Compose safety analysis + port normalization (moved out of `probe`).
//!
//! Independent business domain: parse compose YAML, apply safety + web service
//! heuristics, and rewrite ports to bind 127.0.0.1 only.

use crate::{Error, Result};
use std::collections::BTreeSet;
use std::path::Path;

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
    let doc: serde_yaml::Value = serde_yaml::from_str(text)
        .map_err(|e| Error::InvalidInput(format!("compose yaml: {e}")))?;
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
        scan_dangerous_yaml(
            v,
            &format!("services.{name}"),
            &mut analysis.hard_blockers,
            &mut analysis.risks,
        );

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
        analysis.host_port = s
            .host_ports
            .first()
            .copied()
            .or_else(|| s.container_ports.first().copied());
        analysis.container_port = s.container_ports.first().copied().or(analysis.host_port);
    } else if web_like.is_empty() {
        // single service without ports → manual
        if analysis.services.len() == 1 {
            analysis.preferred_service = Some(analysis.services[0].name.clone());
        }
    } else {
        analysis.risks.push(format!(
            "{} services publish ports; select web service manually",
            web_like.len()
        ));
        // Prefer name web/frontend/app/ui
        for pref in ["web", "frontend", "app", "ui", "nginx"] {
            if let Some(s) = web_like.iter().find(|s| s.name.eq_ignore_ascii_case(pref)) {
                analysis.preferred_service = Some(s.name.clone());
                analysis.host_port = s
                    .host_ports
                    .first()
                    .copied()
                    .or_else(|| s.container_ports.first().copied());
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
    let mut doc: serde_yaml::Value = serde_yaml::from_str(&text)
        .map_err(|e| Error::InvalidInput(format!("compose yaml: {e}")))?;

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
        assert_eq!(force_localhost_port_str("8080:80"), "127.0.0.1:8080:80");
        assert_eq!(
            force_localhost_port_str("0.0.0.0:8080:80"),
            "127.0.0.1:8080:80"
        );
    }

    // 注：safe_extract_zip 已上移至 crate::archive_ops（zip-slip 等安全测试随之迁移）
}
