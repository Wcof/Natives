//! Fixed publisher endpoints and bounded plain-text article extraction.
use super::{
    adapters::{self, FetchError},
    store::MAX_CONTENT_BYTES,
};
use std::time::{Duration, Instant};

pub fn source_url(source: &str, external: &str) -> Option<String> {
    let digits = |s: &str| !s.is_empty() && s.len() <= 32 && s.bytes().all(|b| b.is_ascii_digit());
    match source {
        adapters::SRC_STOCK_ANN | adapters::SRC_FUND_ANN
            if external.strip_prefix("AN").is_some_and(digits) =>
        {
            Some(format!("https://pdf.dfcfw.com/pdf/H2_{external}_1.pdf"))
        }
        adapters::SRC_FINANCE | adapters::SRC_STOCK_NEWS if digits(external) => {
            Some(format!("https://finance.eastmoney.com/a/{external}.html"))
        }
        _ => None,
    }
}

pub fn fetch(
    source: &str,
    external: &str,
    cancelled: impl Fn() -> bool,
) -> Result<(String, &'static str), FetchError> {
    let url = source_url(source, external)
        .ok_or_else(|| FetchError::Parse("unsupported_identity".into()))?;
    let deadline = Instant::now() + Duration::from_secs(20);
    let read = |url: &str| {
        if cancelled() {
            return Err(FetchError::Network("cancelled".into()));
        }
        adapters::get_text_until(url, Some("https://finance.eastmoney.com/"), deadline)
    };
    if matches!(source, adapters::SRC_STOCK_ANN | adapters::SRC_FUND_ANN) {
        announcement_pages(external, |page| {
            read(&format!("https://np-cnotice-stock.eastmoney.com/api/content/ann?art_code={external}&client_source=web&page_index={page}"))
        })
    } else {
        let text = article_text(&read(&url)?)?;
        let completeness = if text.is_empty() {
            "unavailable"
        } else if text.len() > MAX_CONTENT_BYTES {
            "truncated"
        } else {
            "full"
        };
        Ok((text, completeness))
    }
}

fn announcement_pages(
    external: &str,
    mut read: impl FnMut(usize) -> Result<String, FetchError>,
) -> Result<(String, &'static str), FetchError> {
    let mut text = String::new();
    let mut pages = 1;
    for page in 1..=20 {
        let value: serde_json::Value = serde_json::from_str(&read(page)?)
            .map_err(|_| FetchError::Parse("announcement_json".into()))?;
        let data = &value["data"];
        if value["success"] != 1
            || !data.is_object()
            || data["art_code"].as_str().is_some_and(|id| id != external)
        {
            return Err(FetchError::Parse("announcement_identity".into()));
        }
        let count = data["page_size"]
            .as_u64()
            .filter(|n| *n > 0 && *n <= 10000)
            .ok_or_else(|| FetchError::Parse("announcement_pages".into()))?
            as usize;
        if page == 1 {
            pages = count;
        }
        if count != pages {
            return Err(FetchError::Parse("announcement_changed".into()));
        }
        let body = data["notice_content"]
            .as_str()
            .ok_or_else(|| FetchError::Parse("announcement_body".into()))?
            .trim();
        if body.is_empty() {
            return if page == 1 && pages == 1 {
                Ok((String::new(), "unavailable"))
            } else {
                Err(FetchError::Parse("announcement_empty_page".into()))
            };
        }
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(body);
        if text.len() > MAX_CONTENT_BYTES {
            return Ok((text, "truncated"));
        }
        if page == pages {
            return Ok((text, "full"));
        }
    }
    Ok((text, "truncated"))
}

fn article_text(html: &str) -> Result<String, FetchError> {
    let lower = html.to_ascii_lowercase();
    let marker = lower
        .find("id=\"contentbody\"")
        .or_else(|| lower.find("id='contentbody'"))
        .ok_or_else(|| FetchError::Parse("article_body_missing".into()))?;
    let mut cursor = lower[marker..]
        .find('>')
        .map(|n| marker + n + 1)
        .ok_or_else(|| FetchError::Parse("article_body_tag".into()))?;
    let mut depth = 1;
    let mut text = String::new();
    while cursor < html.len() {
        let Some(offset) = lower[cursor..].find('<') else {
            break;
        };
        let start = cursor + offset;
        text.push_str(&html[cursor..start]);
        if lower[start..].starts_with("<!--") {
            cursor = lower[start..]
                .find("-->")
                .map(|n| start + n + 3)
                .ok_or_else(|| FetchError::Parse("article_comment".into()))?;
            continue;
        }
        let end = tag_end(html, start).ok_or_else(|| FetchError::Parse("article_tag".into()))?;
        let tag = lower[start + 1..end].trim();
        let name = tag
            .trim_start_matches('/')
            .split(|c: char| c.is_ascii_whitespace() || c == '/')
            .next()
            .unwrap_or("");
        if matches!(name, "script" | "style" | "noscript" | "iframe") && !tag.starts_with('/') {
            let close = lower[end + 1..]
                .find(&format!("</{name}"))
                .ok_or_else(|| FetchError::Parse("article_hidden_tag".into()))?
                + end
                + 1;
            cursor = tag_end(html, close)
                .ok_or_else(|| FetchError::Parse("article_hidden_end".into()))?
                + 1;
            continue;
        }
        if name == "div" {
            if tag.starts_with('/') {
                depth -= 1;
            } else if !tag.ends_with('/') {
                depth += 1;
            }
            if depth == 0 {
                return Ok(decode_text(&text));
            }
        }
        if matches!(name, "p" | "div" | "br" | "li" | "tr" | "h2" | "h3") {
            text.push('\n');
        }
        if name == "td" {
            text.push(' ');
        }
        cursor = end + 1;
    }
    Err(FetchError::Parse("article_body_incomplete".into()))
}

fn tag_end(html: &str, start: usize) -> Option<usize> {
    let mut quote = None;
    for (offset, ch) in html[start..].char_indices() {
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            }
        } else if ch == '\'' || ch == '"' {
            quote = Some(ch);
        } else if ch == '>' {
            return Some(start + offset);
        }
    }
    None
}

fn decode_text(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let tail = &rest[start + 1..];
        let entity = tail.find(';').filter(|n| *n <= 12).map(|n| (&tail[..n], n));
        let decoded = entity.and_then(|(name, _)| match name {
            "nbsp" => Some(' '),
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => name
                .strip_prefix("#x")
                .or_else(|| name.strip_prefix("#X"))
                .and_then(|n| u32::from_str_radix(n, 16).ok())
                .or_else(|| name.strip_prefix('#').and_then(|n| n.parse().ok()))
                .and_then(char::from_u32),
        });
        if let (Some(ch), Some((_, length))) = (decoded, entity) {
            out.push(ch);
            rest = &tail[length + 1..];
        } else {
            out.push('&');
            rest = tail;
        }
    }
    out.push_str(rest);
    out.lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "real public publisher probe; run explicitly"]
    fn fixed_publishers_actual_full_text() {
        let finance =
            super::super::finance::fetch(Instant::now() + Duration::from_secs(20)).unwrap();
        let stock = adapters::fetch_stock_news("贵州茅台").unwrap();
        let announcements = adapters::fetch_stock_announcements(&["600519".into()]).unwrap();
        let funds = adapters::fetch_fund_announcements("510300").unwrap();
        for rows in [finance, stock, announcements, funds] {
            let row = rows.first().expect("publisher returned public items");
            let (text, completeness) = fetch(row.source_id, &row.external_id, || false).unwrap();
            assert!(
                text.chars().count() > 100,
                "{}: body unavailable",
                row.source_id
            );
            assert!(matches!(completeness, "full" | "truncated"));
            println!("{}: {} bytes, {completeness}", row.source_id, text.len());
        }
    }
    fn page(text: &str, pages: usize) -> String {
        serde_json::json!({"success":1,"data":{"art_code":"AN1","notice_content":text,"page_size":pages}}).to_string()
    }
    #[test]
    fn announcement_reads_all_pages_and_never_labels_partial_as_full() {
        assert_eq!(
            announcement_pages("AN1", |n| Ok(page(&format!("page {n}"), 2))).unwrap(),
            ("page 1\npage 2".into(), "full")
        );
        assert!(announcement_pages("AN1", |n| if n == 1 {
            Ok(page("first", 2))
        } else {
            Err(FetchError::Blocked)
        })
        .is_err());
        assert_eq!(
            announcement_pages("AN1", |_| Ok(page(&"中".repeat(24000), 25)))
                .unwrap()
                .1,
            "truncated"
        );
        assert_eq!(
            announcement_pages("AN1", |_| Ok(page("", 1))).unwrap().1,
            "unavailable"
        );
        assert!(announcement_pages("AN1", |_| Ok(page("", 2))).is_err());
    }
    #[test]
    fn article_reads_only_body_preserves_paragraphs_and_excludes_active_content() {
        let html = r#"<nav>Navigation</nav><div id="ContentBody"><p>A &amp; B &nbsp; &#20013; &#x6587;</p><div><p>Second <a title="a>b">link</a></p></div><script>bad <div>script</div></script><style>bad</style><!-- bad <div> --><p>Last</p></div><footer>Footer</footer>"#;
        assert_eq!(
            article_text(html).unwrap(),
            "A & B 中 文\nSecond link\nLast"
        );
        for invalid in [
            "<p>Challenge page</p>",
            "<div id='ContentBody'><p>incomplete",
        ] {
            assert!(article_text(invalid).is_err());
        }
        assert_eq!(
            article_text("<div id='ContentBody'><img src='x'></div>").unwrap(),
            ""
        );
    }
    #[test]
    fn endpoints_derive_only_from_valid_saved_identities() {
        for id in ["", "../../x", "AN1?url=http://127.0.0.1", "123#x"] {
            assert!(source_url(adapters::SRC_FINANCE, id).is_none());
            assert!(source_url(adapters::SRC_FUND_ANN, id).is_none());
        }
        assert!(source_url(adapters::SRC_FUND_ANN, "AN123")
            .unwrap()
            .ends_with("H2_AN123_1.pdf"));
        assert!(source_url(adapters::SRC_FINANCE, "123")
            .unwrap()
            .ends_with("/a/123.html"));
    }
}
