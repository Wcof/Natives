//! Validated upstream history adapter for opportunity refresh.
use serde_json::Value;

pub(super) enum Payload {
    Exchange(String, String),
    Nav(Vec<crate::nav::NavRow>),
}

pub(super) fn fetch(row: &Value) -> Result<Payload, &'static str> {
    let code = row["code"].as_str().ok_or("unsupported")?;
    if code.len() != 6 || !code.bytes().all(|c| c.is_ascii_digit()) {
        return Err("unsupported");
    }
    if row["kind"] == "fund" {
        let rows = crate::nav::EastMoneySource::fetch_analysis_history(code)
            .map_err(|_| "upstream_error")?;
        let today = crate::assets::today_iso();
        if rows.is_empty() {
            return Err("empty_history");
        }
        if rows.len() > 250
            || rows.iter().any(|r| {
                r.fund_code != code
                    || !crate::assets::is_valid_calendar_date(&r.nav_date)
                    || r.nav_date > today
                    || r.unit_nav_raw <= 0
            })
        {
            return Err("invalid_history");
        }
        Ok(Payload::Nav(rows))
    } else {
        let prefix = match row["market"].as_str() {
            Some("SSE") => "sh",
            Some("SZSE") => "sz",
            Some("BSE") => "bj",
            _ => return Err("unsupported"),
        };
        let symbol = format!("{prefix}{code}");
        let body =
            crate::market::fetch_opportunity_history(&symbol).map_err(|_| "upstream_error")?;
        Ok(Payload::Exchange(symbol, body))
    }
}
