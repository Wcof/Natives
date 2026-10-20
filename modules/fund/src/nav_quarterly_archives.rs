//! Compare published holdings, without treating undisclosed constituents as zero.
use super::{NavError, QuarterlyHolding, QuarterlyReport};
use crate::fixed::Fixed;
use std::collections::BTreeMap;

#[derive(Clone)]
struct Period {
    date: String,
    rows: Vec<QuarterlyHolding>,
}

fn text(html: &str) -> String {
    let mut out = String::new();
    let mut tag = false;
    for ch in html.chars() {
        match ch {
            '<' => tag = true,
            '>' => tag = false,
            _ if !tag => out.push(ch),
            _ => {}
        }
    }
    out.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .trim()
        .to_string()
}

fn cells<'a>(html: &'a str, tag: &str) -> Vec<&'a str> {
    let prefix = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut rest = html;
    let mut result = Vec::new();
    while let Some(start) = rest.find(&prefix) {
        rest = &rest[start + prefix.len()..];
        if !rest.starts_with('>') && !rest.chars().next().is_some_and(char::is_whitespace) {
            continue;
        }
        let Some(end) = rest.find('>') else {
            break;
        };
        rest = &rest[end + 1..];
        let Some(end) = rest.find(&close) else {
            break;
        };
        result.push(&rest[..end]);
        rest = &rest[end + close.len()..];
    }
    result
}

fn quote(code: &str, cell: &str) -> Option<String> {
    let target = cell
        .split("/unify/r/")
        .nth(1)?
        .split(['\'', '\"', '>'])
        .next()?;
    let market = target.split('.').next()?;
    let prefix = match market {
        "0" if code.starts_with('4') || code.starts_with('8') || code.starts_with("92") => "bj",
        "0" => "sz",
        "1" => "sh",
        "116" => "hk",
        "105" | "106" | "107" => "us",
        _ => return None,
    };
    if !(code.len() <= 15 && code.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.')) {
        return None;
    }
    Some(format!("{prefix}{code}"))
}

fn parse(raw: &str, bond: bool) -> Result<Vec<Period>, NavError> {
    if raw.len() > 1_000_000 {
        return Err(NavError::BadPayload("季度披露响应过大".into()));
    }
    let payload = raw
        .split_once("content:")
        .ok_or_else(|| NavError::BadPayload("季度披露格式无效".into()))?
        .1
        .trim_start();
    let html = serde_json::Deserializer::from_str(payload)
        .into_iter::<String>()
        .next()
        .ok_or_else(|| NavError::BadPayload("季度披露内容缺失".into()))?
        .map_err(|_| NavError::BadPayload("季度披露内容无效".into()))?;
    let mut periods = Vec::new();
    for block in html.split("</table>").take(8) {
        let Some((head, table)) = block.split_once("<table") else {
            continue;
        };
        let label = text(head);
        let Some(date) = label
            .split("截止至：")
            .nth(1)
            .or_else(|| label.split("截止至:").nth(1))
            .map(str::trim)
            .and_then(|s| s.get(..10))
        else {
            continue;
        };
        if !crate::assets::is_valid_calendar_date(date) {
            return Err(NavError::BadPayload("季度报告日期无效".into()));
        }
        let headers = cells(table, "th")
            .iter()
            .map(|s| text(s).replace([' ', '\n', '\r'], ""))
            .collect::<Vec<_>>();
        let column = |needle: &str| {
            headers
                .iter()
                .position(|s| s.contains(needle))
                .ok_or_else(|| NavError::BadPayload("季度持仓列缺失".into()))
        };
        let code_col = column(if bond { "债券代码" } else { "股票代码" })?;
        let name_col = column(if bond { "债券名称" } else { "股票名称" })?;
        let weight_col = column("占净值")?;
        let body = table
            .split_once("<tbody>")
            .map(|(_, s)| s)
            .ok_or_else(|| NavError::BadPayload("季度持仓表缺失".into()))?;
        let mut rows = Vec::new();
        for row in cells(body, "tr").into_iter().take(100) {
            let columns = cells(row, "td");
            if columns.len() <= code_col.max(name_col).max(weight_col) {
                return Err(NavError::BadPayload("季度持仓记录不完整".into()));
            }
            let code = text(columns[code_col]);
            let name = text(columns[name_col]);
            let ratio = text(columns[weight_col]).trim_end_matches('%').to_string();
            if code.is_empty() || name.is_empty() {
                return Err(NavError::BadPayload("季度持仓身份缺失".into()));
            }
            if !ratio.is_empty() && !Fixed::parse(&ratio, 4).is_ok_and(|value| value.raw() >= 0) {
                return Err(NavError::BadPayload("季度持仓占比无效".into()));
            }
            rows.push(QuarterlyHolding {
                quote_symbol: if bond {
                    None
                } else {
                    quote(&code, columns[code_col])
                },
                code,
                name,
                ratio_pct: ratio,
                change_type: String::new(),
                change_pct: String::new(),
                industry: if bond { "债券".into() } else { String::new() },
                previous_ratio_pct: None,
            });
        }
        periods.push(Period {
            date: date.into(),
            rows,
        });
    }
    periods.sort_by(|a, b| b.date.cmp(&a.date));
    periods.dedup_by(|a, b| a.date == b.date);
    Ok(periods)
}

fn retrieve(code: &str, bond: bool, year: Option<i32>) -> Result<Vec<Period>, NavError> {
    let kind = if bond { "zqcc" } else { "jjcc" };
    let year = year.map(|y| y.to_string()).unwrap_or_default();
    let url=format!("https://fundf10.eastmoney.com/FundArchivesDatas.aspx?type={kind}&code={code}&topline=100&year={year}&month=");
    let raw = crate::network::client()
        .get(&url)
        .set("User-Agent", "Mozilla/5.0")
        .set("Referer", "https://fundf10.eastmoney.com/")
        .call()
        .map_err(|_| NavError::Source("公开季度档案获取失败".into()))?
        .into_string()
        .map_err(|_| NavError::Source("公开季度档案读取失败".into()))?;
    parse(&raw, bond)
}

fn compare(current: &[QuarterlyHolding], previous: &[QuarterlyHolding]) -> Vec<QuarterlyHolding> {
    let map = previous
        .iter()
        .map(|row| ((&row.code, &row.quote_symbol), row))
        .collect::<BTreeMap<_, _>>();
    current
        .iter()
        .cloned()
        .map(|mut row| {
            if let Some(before) = map.get(&(&row.code, &row.quote_symbol)) {
                if let (Ok(a), Ok(b)) = (
                    Fixed::parse(&row.ratio_pct, 4),
                    Fixed::parse(&before.ratio_pct, 4),
                ) {
                    row.previous_ratio_pct = Some(before.ratio_pct.clone());
                    row.change_pct = Fixed::nav(a.raw() - b.raw()).to_string_value();
                }
            }
            row
        })
        .collect()
}

pub(super) fn fetch(
    code: &str,
    mobile: Option<&QuarterlyReport>,
) -> Result<QuarterlyReport, NavError> {
    let mut stocks = retrieve(code, false, None)?;
    let mut bonds = retrieve(code, true, None)?;
    let dates = |stocks: &[Period], bonds: &[Period]| {
        let mut values = stocks
            .iter()
            .chain(bonds)
            .map(|p| p.date.clone())
            .collect::<Vec<_>>();
        values.sort();
        values.dedup();
        values.reverse();
        values
    };
    let mut available = dates(&stocks, &bonds);
    if available.len() == 1 {
        let year = available[0][..4]
            .parse::<i32>()
            .map_err(|_| NavError::BadPayload("季度年份无效".into()))?
            - 1;
        stocks.extend(retrieve(code, false, Some(year)).unwrap_or_default());
        bonds.extend(retrieve(code, true, Some(year)).unwrap_or_default());
        available = dates(&stocks, &bonds);
    }
    let Some(latest) = available.first() else {
        return Err(NavError::Source("尚无可读取的季度档案".into()));
    };
    let previous = available.get(1);
    let rows = |periods: &[Period], date: Option<&String>| {
        periods
            .iter()
            .find(|p| Some(&p.date) == date)
            .map(|p| p.rows.clone())
            .unwrap_or_default()
    };
    let mut current = compare(&rows(&stocks, Some(latest)), &rows(&stocks, previous));
    let current_bonds = compare(&rows(&bonds, Some(latest)), &rows(&bonds, previous));
    if let Some(mobile) = mobile.filter(|m| m.report_date == *latest) {
        for row in &mut current {
            if let Some(extra) = mobile.stocks.iter().find(|s| s.code == row.code) {
                row.industry = extra.industry.clone();
            }
        }
    }
    Ok(QuarterlyReport {
        fund_code: code.into(),
        report_date: latest.clone(),
        previous_report_date: previous.cloned(),
        source: "eastmoney-archives".into(),
        stocks: current,
        bonds: current_bonds,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn response(date: &str, weight: &str) -> String {
        let html=format!("<h4>截止至：<font>{date}</font></h4><table><thead><tr><th>股票代码</th><th>股票名称</th><th>占净值<br />比例</th></tr></thead><tbody><tr><td><a href='//quote.eastmoney.com/unify/r/0.000001'>000001</a></td><td>平安银行</td><td>{weight}%</td></tr></tbody></table>");
        format!(
            "var apidata={{ content:{},arryear:[2026]}};",
            serde_json::to_string(&html).unwrap()
        )
    }
    #[test]
    fn archive_identity_weight_comparison_and_missing_previous_are_exact() {
        let latest = parse(&response("2026-06-30", "6.45"), false).unwrap();
        let previous = parse(&response("2026-03-31", "4.31"), false).unwrap();
        let row = &compare(&latest[0].rows, &previous[0].rows)[0];
        assert_eq!(row.quote_symbol.as_deref(), Some("sz000001"));
        assert_eq!(row.change_pct, "2.14");
        assert!(
            row.change_type.is_empty(),
            "weight change does not establish trading activity"
        );
        assert_eq!(row.previous_ratio_pct.as_deref(), Some("4.31"));
        assert!(compare(&latest[0].rows, &[])[0].change_pct.is_empty());
        assert_eq!(
            compare(&previous[0].rows, &latest[0].rows)[0].change_pct,
            "-2.14"
        );
        assert!(parse(&response("2026-02-30", "6.45"), false).is_err());
        assert!(parse(&response("2026-06-30", "-1"), false).is_err());
        assert!(parse(&response("2026-06-30", "NaN"), false).is_err());
        let beijing = response("2026-06-30", "1").replace("000001", "920001");
        assert_eq!(
            parse(&beijing, false).unwrap()[0].rows[0]
                .quote_symbol
                .as_deref(),
            Some("bj920001")
        );
        assert!(parse("var apidata={content:42}", false).is_err());
    }

    #[test]
    #[ignore = "requires real public fund archive source"]
    fn live_archives_return_two_actual_periods() {
        let report = super::fetch("000001", None).unwrap();
        assert!(!report.stocks.is_empty());
        assert!(report
            .previous_report_date
            .as_ref()
            .is_some_and(|date| date < &report.report_date));
        let comparable = report
            .stocks
            .iter()
            .filter(|row| row.previous_ratio_pct.is_some())
            .collect::<Vec<_>>();
        assert!(!comparable.is_empty());
        for row in &comparable {
            let current = Fixed::parse(&row.ratio_pct, 4).unwrap();
            let previous = Fixed::parse(row.previous_ratio_pct.as_ref().unwrap(), 4).unwrap();
            assert_eq!(
                row.change_pct,
                Fixed::nav(current.raw() - previous.raw()).to_string_value()
            );
        }
        println!(
            "archive live: current={}, previous={}, stocks={}, bonds={}, compared={}",
            report.report_date,
            report.previous_report_date.unwrap(),
            report.stocks.len(),
            report.bonds.len(),
            comparable.len()
        );
    }
}
