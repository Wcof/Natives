//! Compatibility fixes at the local API trust boundary.
//!
//! This intentionally works on JSON before protocol adaptation: the engine and
//! provider adapters retain their native, typed contracts.

use provider_adapters::capabilities::ProviderRequest;
use serde_json::{json, Value};

/// Applies the safe subset at the native engine boundary. Native requests are
/// already typed, so we never discard media speculatively here; image fallback
/// is reserved for an explicit upstream media-compatibility error.
pub fn rectify_provider_request(request: &mut ProviderRequest, enabled: bool) {
    if enabled && request.max_tokens.unwrap_or(0) < 64_000 {
        request.max_tokens = Some(64_000);
    }
}

pub fn rectify(mut request: Value, enabled: bool) -> Value {
    if !enabled {
        return request;
    }
    if let Some(object) = request.as_object_mut() {
        let max_tokens = object
            .get("max_tokens")
            .or_else(|| object.get("max_output_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        if max_tokens < 64_000 {
            object.insert("max_tokens".into(), json!(64_000));
        }
    }
    rectify_value(&mut request, false);
    request
}

fn rectify_value(value: &mut Value, inside_thinking: bool) {
    match value {
        Value::Array(values) => {
            for value in values {
                if is_image(value) {
                    *value = json!({"type": "text", "text": "[Unsupported Image]"});
                } else {
                    rectify_value(value, inside_thinking);
                }
            }
        }
        Value::Object(object) => {
            if inside_thinking {
                // Signatures are provider-specific and commonly make otherwise
                // valid recovered conversations fail after a route switch.
                object.remove("signature");
            }
            if let Some(thinking) = object.get_mut("thinking") {
                *thinking = normalized_thinking(thinking.take());
            }
            for (key, value) in object.iter_mut() {
                if key == "thinking" {
                    continue;
                }
                if is_image(value) {
                    *value = json!({"type": "text", "text": "[Unsupported Image]"});
                } else {
                    rectify_value(value, inside_thinking || key == "thinking");
                }
            }
        }
        _ => {}
    }
}

fn normalized_thinking(_value: Value) -> Value {
    json!({"type": "enabled", "budget_tokens": 32_000})
}

fn is_image(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    matches!(
        object.get("type").and_then(Value::as_str),
        Some("image") | Some("image_url") | Some("input_image")
    ) || object.contains_key("image_url")
        || object.contains_key("image")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enabled_rectifier_normalizes_thinking_budget_and_images() {
        let got = rectify(
            json!({
                "max_tokens": 10,
                "thinking": {"signature": "secret", "budget_tokens": 1},
                "messages": [{"content": [{"type": "image_url", "image_url": {"url": "data:x"}}]}]
            }),
            true,
        );
        assert_eq!(got["max_tokens"], 64_000);
        assert_eq!(got["thinking"]["budget_tokens"], 32_000);
        assert_eq!(
            got["messages"][0]["content"][0]["text"],
            "[Unsupported Image]"
        );
    }

    #[test]
    fn disabled_rectifier_is_identity() {
        let input = json!({"max_tokens": 1, "thinking": {"signature": "keep"}});
        assert_eq!(rectify(input.clone(), false), input);
    }
}
