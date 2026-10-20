//! M09：移植 PanWatch structured_output 的 JSON／围栏／标签提取，再按 Natives
//! 证据契约验证。通过仅表示结构与引用可核对，不能证明模型判断正确。
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub fn parse(text: &str) -> Result<Value, &'static str> {
    let mut raw = text.trim();
    if let Some(start) = raw.rfind("<!--PANWATCH_JSON-->") {
        let end = raw.rfind("<!--/PANWATCH_JSON-->").ok_or("OUTPUT_JSON")?;
        let content = start + "<!--PANWATCH_JSON-->".len();
        if end < content {
            return Err("OUTPUT_JSON");
        }
        raw = &raw[content..end];
    } else if raw.starts_with("```") {
        let (_, rest) = raw.split_once('\n').ok_or("OUTPUT_JSON")?;
        raw = rest.strip_suffix("```").ok_or("OUTPUT_JSON")?.trim();
    } else if let Some(rest) = raw.strip_prefix("json\n") {
        raw = rest;
    }
    let value: Value = serde_json::from_str(raw).map_err(|_| "OUTPUT_JSON")?;
    if !value.is_object() {
        return Err("OUTPUT_STRUCTURE");
    }
    Ok(value)
}
fn refs(
    value: &Value,
    known: &BTreeMap<String, &Value>,
    required: bool,
) -> Result<BTreeSet<String>, &'static str> {
    let list = value.as_array().ok_or("OUTPUT_REFERENCES")?;
    if list.len() > 30 || (required && list.is_empty()) {
        return Err("OUTPUT_REFERENCES");
    }
    list.iter()
        .map(|v| {
            let id = v.as_str().ok_or("OUTPUT_REFERENCES")?;
            if !known.contains_key(id) {
                return Err("OUTPUT_REFERENCES");
            }
            Ok(id.to_string())
        })
        .collect()
}
fn bounded_text(v: &Value) -> Result<(), &'static str> {
    let text = v.as_str().ok_or("OUTPUT_STRUCTURE")?;
    if text.trim().is_empty() || text.chars().count() > 2000 {
        return Err("OUTPUT_STRUCTURE");
    }
    Ok(())
}
fn texts(v: &Value) -> Result<(), &'static str> {
    let a = v.as_array().ok_or("OUTPUT_STRUCTURE")?;
    if a.len() > 20 {
        return Err("OUTPUT_STRUCTURE");
    }
    for t in a {
        bounded_text(t)?;
    }
    Ok(())
}
fn number_string(v: &Value) -> Option<String> {
    if let Some(n) = v.as_i64() {
        return Some(n.to_string());
    }
    let number = v.as_number().map(ToString::to_string);
    let raw = number.as_deref().or_else(|| v.as_str())?;
    let raw = raw.trim();
    if raw.is_empty()
        || !raw
            .chars()
            .all(|c| c.is_ascii_digit() || c == '-' || c == '.')
    {
        return None;
    }
    let (negative, raw) = raw
        .strip_prefix('-')
        .map(|s| (true, s))
        .unwrap_or((false, raw));
    let mut parts = raw.split('.');
    let integer = parts.next()?;
    if integer.is_empty() || !integer.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let decimal = parts.next().unwrap_or("");
    if parts.next().is_some() || !decimal.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let integer = integer.trim_start_matches('0');
    let integer = if integer.is_empty() { "0" } else { integer };
    let decimal = decimal.trim_end_matches('0');
    Some(format!(
        "{}{}{}",
        if negative && (integer != "0" || !decimal.is_empty()) {
            "-"
        } else {
            ""
        },
        integer,
        if decimal.is_empty() {
            String::new()
        } else {
            format!(".{decimal}")
        }
    ))
}
fn verify(value: &Value, evidence: &Value) -> Result<(), &'static str> {
    if value["task"] != evidence["task"] || value["scope"] != evidence["scope"] {
        return Err("OUTPUT_SCOPE");
    }
    let entries = evidence["evidence"].as_array().ok_or("OUTPUT_EVIDENCE")?;
    let mut known = BTreeMap::new();
    for entry in entries {
        let id = entry["id"].as_str().ok_or("OUTPUT_EVIDENCE")?;
        if known.insert(id.to_string(), entry).is_some() {
            return Err("OUTPUT_EVIDENCE");
        }
    }
    let all = refs(&value["evidenceRefs"], &known, true)?;
    for section in ["facts", "impacts", "counterEvidence"] {
        let items = value[section].as_array().ok_or("OUTPUT_STRUCTURE")?;
        if items.len() > 30 {
            return Err("OUTPUT_STRUCTURE");
        }
        for item in items {
            bounded_text(&item["text"])?;
            let used = refs(&item["evidenceRefs"], &known, true)?;
            if !used.is_subset(&all) {
                return Err("OUTPUT_REFERENCES");
            }
            let mut proven = BTreeSet::new();
            if let Some(claims) = item.get("quantities") {
                let claims = claims.as_array().ok_or("OUTPUT_NUMBER")?;
                if claims.len() > 20 {
                    return Err("OUTPUT_NUMBER");
                }
                for claim in claims {
                    let id = claim["evidenceRef"].as_str().ok_or("OUTPUT_NUMBER")?;
                    if !used.contains(id) {
                        return Err("OUTPUT_REFERENCES");
                    }
                    let path = claim["path"]
                        .as_str()
                        .filter(|p| p.starts_with('/') && p.len() <= 256)
                        .ok_or("OUTPUT_NUMBER")?;
                    let actual = known[id]
                        .pointer(path)
                        .and_then(number_string)
                        .ok_or("OUTPUT_NUMBER")?;
                    if number_string(&claim["value"]).as_deref() != Some(actual.as_str()) {
                        return Err("OUTPUT_NUMBER");
                    }
                    proven.insert(actual);
                }
            }
            // Literal numerical statements need exact evidence, even when the model
            // omits quantities. Structure verification never certifies unit semantics.
            for token in item["text"]
                .as_str()
                .unwrap()
                .split(|c: char| !c.is_ascii_alphanumeric() && c != '.' && c != '-')
            {
                if let Some(number) = number_string(&Value::String(token.into())) {
                    if !proven.contains(&number) {
                        return Err("OUTPUT_NUMBER");
                    }
                }
            }
            if let Some(claims) = item.get("ruleClaims") {
                for claim in claims
                    .as_array()
                    .filter(|a| a.len() <= 20)
                    .ok_or("OUTPUT_RULE")?
                {
                    let id = claim["evidenceRef"].as_str().ok_or("OUTPUT_RULE")?;
                    if !used.contains(id) {
                        return Err("OUTPUT_REFERENCES");
                    }
                    let events = known[id]["events"].as_array().ok_or("OUTPUT_RULE")?;
                    if !events.iter().any(|event| {
                        event["eventId"] == claim["eventId"] && event["status"] == claim["status"]
                    }) {
                        return Err("OUTPUT_RULE");
                    }
                }
            }
        }
    }
    texts(&value["gaps"])?;
    let options = value["options"]
        .as_array()
        .filter(|a| a.len() <= 20)
        .ok_or("OUTPUT_STRUCTURE")?;
    for option in options {
        if !matches!(
            option["action"].as_str(),
            Some("watch" | "hold" | "build" | "add" | "reduce" | "exit" | "review")
        ) {
            return Err("OUTPUT_ACTION");
        }
        texts(&option["conditions"])?;
        texts(&option["invalidConditions"])?;
        texts(&option["gaps"])?;
        let used = refs(&option["evidenceRefs"], &known, true)?;
        if !used.is_subset(&all) {
            return Err("OUTPUT_REFERENCES");
        }
        // 定量交易金额只能来自确定性规划，模型输出不进入交易路径。
        if option.as_object().is_some_and(|m| {
            m.keys().any(|k| {
                ![
                    "action",
                    "conditions",
                    "invalidConditions",
                    "gaps",
                    "evidenceRefs",
                ]
                .contains(&k.as_str())
            })
        }) {
            return Err("OUTPUT_ACTION");
        }
    }
    Ok(())
}
pub fn validate(text: &str, evidence: &Value) -> Value {
    match parse(text).and_then(|v| {
        verify(&v, evidence)?;
        Ok(v)
    }) {
        Ok(result) => {
            json!({"status":"structure_verified","policyVersion":1,"requiresHumanReview":true,"result":result})
        }
        Err(code) => {
            json!({"status":"unverified","policyVersion":1,"requiresHumanReview":true,"error":code})
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_chart_decimal_claims_match_exact_numeric_evidence() {
        let evidence = json!({"task":"diagnosis","scope":{"asset":"research:sh600519"},"evidence":[{"id":"R1","rows":[{"close":10.5}]}]});
        let mut output = json!({"task":"diagnosis","scope":evidence["scope"],"facts":[{"text":"收盘价 10.5","evidenceRefs":["R1"],"quantities":[{"evidenceRef":"R1","path":"/rows/0/close","value":"10.50"}]}],"impacts":[],"counterEvidence":[],"options":[],"gaps":[],"evidenceRefs":["R1"]});
        assert_eq!(
            validate(&output.to_string(), &evidence)["status"],
            "structure_verified"
        );
        output["facts"][0]["quantities"][0]["value"] = json!("10.5000001");
        assert_eq!(
            validate(&output.to_string(), &evidence)["error"],
            "OUTPUT_NUMBER"
        );
    }

    #[test]
    fn output_checks_scope_numbers_references_rules_and_actions() {
        let e = json!({"task":"diagnosis","scope":{"asset":"fund:1:2"},"evidence":[{"id":"E1","amount":"123.40"},{"id":"E3","events":[{"eventId":1,"status":"open"}]}]});
        let mut v = json!({"task":"diagnosis","scope":{"asset":"fund:1:2"},"facts":[{"text":"核对估值","evidenceRefs":["E1"],"quantities":[{"evidenceRef":"E1","path":"/amount","value":"123.4"}]}],"impacts":[],"counterEvidence":[],"options":[{"action":"review","conditions":["核对成本"],"invalidConditions":[],"gaps":[],"evidenceRefs":["E1"]}],"gaps":[],"evidenceRefs":["E1","E3"]});
        assert_eq!(
            validate(&format!("```json\n{v}\n```"), &e)["status"],
            "structure_verified"
        );
        v["facts"][0]["quantities"][0]["value"] = json!(999);
        assert_eq!(validate(&v.to_string(), &e)["error"], "OUTPUT_NUMBER");
        v["facts"][0].as_object_mut().unwrap().remove("quantities");
        v["facts"][0]["text"] = json!("估值 999 元");
        assert_eq!(validate(&v.to_string(), &e)["error"], "OUTPUT_NUMBER");
        v["facts"][0]["text"] = json!("核对估值");
        v["facts"][0]["ruleClaims"] = json!([{"evidenceRef":"E3","eventId":1,"status":"resolved"}]);
        v["facts"][0]["evidenceRefs"] = json!(["E1", "E3"]);
        assert_eq!(validate(&v.to_string(), &e)["error"], "OUTPUT_RULE");
        v["facts"][0].as_object_mut().unwrap().remove("ruleClaims");
        v["facts"][0]["evidenceRefs"] = json!(["E1"]);
        v["scope"] = json!("portfolio");
        assert_eq!(validate(&v.to_string(), &e)["error"], "OUTPUT_SCOPE");
        v["scope"] = e["scope"].clone();
        v["options"][0]["action"] = json!("buy");
        assert_eq!(validate(&v.to_string(), &e)["error"], "OUTPUT_ACTION");
        v["options"][0]["action"] = json!("review");
        v["facts"][0]["evidenceRefs"] = json!(["missing"]);
        assert_eq!(validate(&v.to_string(), &e)["error"], "OUTPUT_REFERENCES");
        assert_eq!(validate("unknown", &e)["status"], "unverified");
    }
}
