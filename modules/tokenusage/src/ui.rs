//! 内嵌 UI 资源（构建期嵌入，CSP 兼容，无外联）。

const INDEX_HTML: &str = include_str!("../ui/dist/index.html");
const APP_JS: &str = include_str!("../ui/dist/app.js");

pub fn index_html() -> String {
    INDEX_HTML.to_string()
}

pub fn static_js(path: &str) -> Option<String> {
    match path {
        "/app.js" => Some(APP_JS.to_string()),
        _ => None,
    }
}
