//! 本机独立 Web 静态资源与模板分发（ADR-0042）。

use std::fs;
use std::path::{Path, PathBuf};

pub fn resolve_static_root() -> PathBuf {
    if let Ok(root) = std::env::var("NATIVES_WEB_ROOT") {
        let p = PathBuf::from(root);
        if p.is_dir() {
            return p;
        }
    }
    let product_web = crate::app_product::default_system_source().join("ChromeExtension");
    if product_web.is_dir() || !cfg!(debug_assertions) {
        return product_web;
    }
    // 未安装产品源的本地开发环境仍可使用当前 checkout 的页面。
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .unwrap_or(manifest_dir);
    let dist_web = repo_root.join("dist").join("web");
    if dist_web.exists() {
        dist_web
    } else {
        repo_root.join("extension")
    }
}

pub fn mime_type_for(path: &str) -> &'static str {
    if path.ends_with(".html") {
        "text/html; charset=utf-8"
    } else if path.ends_with(".js") || path.ends_with(".mjs") {
        "application/javascript; charset=utf-8"
    } else if path.ends_with(".css") {
        "text/css; charset=utf-8"
    } else if path.ends_with(".json") {
        "application/json; charset=utf-8"
    } else if path.ends_with(".svg") {
        "image/svg+xml"
    } else if path.ends_with(".png") {
        "image/png"
    } else if path.ends_with(".jpg") || path.ends_with(".jpeg") {
        "image/jpeg"
    } else if path.ends_with(".ico") {
        "image/x-icon"
    } else {
        "application/octet-stream"
    }
}

// Web-only assets are compiled into the signed Core binary. The product source
// does not copy them into the Chrome extension tree (ADR-0042/WEB_ONLY), so the
// Web entry remains usable without shipping pairing/session code in Chrome.
const WEB_ENTRY_JS: &str = include_str!("../../../../extension/web-entry.js");
const WEB_SESSION_JS: &str = include_str!("../../../../extension/platform/session.js");
const WEB_APP_JS: &str = include_str!("../../../../extension/app.js");
const WEB_TRANSPORT_JS: &str = include_str!("../../../../extension/platform/transport.js");
const WEB_SURFACE_JS: &str = include_str!("../../../../extension/platform/surface.js");
const WEB_EVENTS_JS: &str = include_str!("../../../../extension/platform/events.js");
const WEB_STORAGE_JS: &str = include_str!("../../../../extension/platform/storage.js");
const WEB_SESSION_ADAPTER_JS: &str = include_str!("../../../../extension/platform/web-session.js");

fn embedded_web_asset(path: &str) -> Option<(Vec<u8>, &'static str)> {
    let bytes = match path {
        "web-entry.js" => WEB_ENTRY_JS,
        "app.js" => WEB_APP_JS,
        "platform/session.js" => WEB_SESSION_JS,
        "platform/transport.js" => WEB_TRANSPORT_JS,
        "platform/surface.js" => WEB_SURFACE_JS,
        "platform/events.js" => WEB_EVENTS_JS,
        "platform/storage.js" => WEB_STORAGE_JS,
        "platform/web-session.js" => WEB_SESSION_ADAPTER_JS,
        _ => return None,
    };
    Some((
        bytes.as_bytes().to_vec(),
        "application/javascript; charset=utf-8",
    ))
}

pub fn serve_static_file(req_path: &str) -> Option<(Vec<u8>, &'static str)> {
    let clean_path = req_path
        .split('?')
        .next()
        .unwrap_or(req_path)
        .trim_start_matches('/');
    let target_file = if clean_path.is_empty() || clean_path == "index.html" {
        "space.html"
    } else {
        clean_path
    };

    // 路径越界防御：严禁含 ".." 或非法分隔符
    if target_file.contains("..") || target_file.contains('\\') {
        return None;
    }
    if let Some(asset) = embedded_web_asset(target_file) {
        return Some(asset);
    }

    let root = resolve_static_root();
    let file_path = root.join(target_file);

    // 二次安全核验：规范化路径必须以静态根目录开头
    let Ok(canonical) = file_path.canonicalize() else {
        return None;
    };
    let Ok(canonical_root) = root.canonicalize() else {
        return None;
    };
    if !canonical.starts_with(&canonical_root) {
        return None;
    }

    let Ok(bytes) = fs::read(&canonical) else {
        return None;
    };

    let mime = mime_type_for(target_file);
    Some((bytes, mime))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_only_auth_assets_are_served_from_the_signed_core() {
        let (entry, mime) = serve_static_file("/web-entry.js").expect("web-entry embedded");
        assert_eq!(mime, "application/javascript; charset=utf-8");
        assert!(String::from_utf8_lossy(&entry).contains("natives-pairing-modal"));

        let (session, mime) = serve_static_file("/platform/session.js").expect("session embedded");
        assert_eq!(mime, "application/javascript; charset=utf-8");
        assert!(String::from_utf8_lossy(&session).contains("initWebSession"));

        let (surface, _) = serve_static_file("/platform/surface.js").expect("surface embedded");
        assert!(String::from_utf8_lossy(&surface).contains("request('/web/surfaces'"));
        assert!(String::from_utf8_lossy(&surface).contains("20_000"));
        let (events, _) = serve_static_file("/platform/events.js").expect("events embedded");
        assert!(String::from_utf8_lossy(&events).contains("new EventSource"));
        assert!(String::from_utf8_lossy(&events)
            .contains("reason:epoch !== payload.epoch ? 'epoch' : 'sequence'"));
        let (storage, _) = serve_static_file("/platform/storage.js").expect("storage embedded");
        assert!(String::from_utf8_lossy(&storage).contains("localStorage.setItem"));
        let (adapter, _) =
            serve_static_file("/platform/web-session.js").expect("session adapter embedded");
        assert!(String::from_utf8_lossy(&adapter).contains("import('./session.js')"));
    }
}
