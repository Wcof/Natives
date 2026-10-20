//! 内嵌 UI 资源（构建期嵌入，CSP 兼容，无外联）。

const INDEX_HTML: &str = include_str!("../ui/dist/index.html");
const APP_JS: &str = include_str!("../ui/dist/app.js");

pub fn index_html() -> String {
    INDEX_HTML.to_string()
}

pub fn static_js(path: &str) -> Option<String> {
    match path {
        "/app.js" => Some(APP_JS.to_string()),
        "/brands.js" => Some(include_str!("../ui/dist/brands.js").to_string()),
        "/locale.js" => Some(include_str!("../ui/dist/locale.js").to_string()),
        _ => None,
    }
}

pub fn static_css(path: &str) -> Option<&'static str> {
    match path {
        "/styles/theme.css" => Some(include_str!("../ui/dist/styles/theme.css")),
        "/styles/layout.css" => Some(include_str!("../ui/dist/styles/layout.css")),
        _ => None,
    }
}
