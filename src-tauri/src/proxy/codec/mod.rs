//! 3×3 协议双向编解码矩阵（CDC-001..005）。

pub mod decoders;
pub mod encoders;
pub mod response;
pub mod types;

pub use decoders::*;
pub use encoders::*;
pub use response::*;
pub use types::*;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_3x3_matrix_roundtrips() {
        let sample_req = CanonicalRequest {
            model: "test-model".into(),
            messages: vec![CanonicalMessage {
                role: "user".into(),
                content: "Hello AI".into(),
                tool_call_id: None,
                tool_name: None,
                tool_calls: None,
                images: Vec::new(),
            }],
            system: Some("You are helpful".into()),
            tools: vec![CanonicalTool {
                name: "get_weather".into(),
                description: "Get weather".into(),
                parameters: json!({"type": "object", "properties": {"loc": {"type": "string"}}}),
            }],
            tool_choice: None,
            temperature: Some(0.7),
            max_tokens: Some(1024),
            stream: false,
            reasoning_effort: None,
            thinking_budget: None,
            structured_output: None,
        };

        // 1. Chat -> 3 upstreams
        let c_chat = encode_to_chat_completions(&sample_req, "gpt-4o").unwrap();
        assert_eq!(c_chat["model"], "gpt-4o");
        let decoded_chat = decode_chat_completions_request(&c_chat).unwrap();
        assert_eq!(decoded_chat.system.as_deref(), Some("You are helpful"));
        assert_eq!(decoded_chat.messages[0].content, "Hello AI");

        let c_resp = encode_to_responses(&sample_req, "o3-mini").unwrap();
        assert_eq!(c_resp["model"], "o3-mini");
        let decoded_resp = decode_responses_request(&c_resp).unwrap();
        assert_eq!(decoded_resp.system.as_deref(), Some("You are helpful"));

        let c_msg = encode_to_messages(&sample_req, "claude-3-7-sonnet").unwrap();
        assert_eq!(c_msg["model"], "claude-3-7-sonnet");
        let decoded_msg = decode_messages_request(&c_msg).unwrap();
        assert_eq!(decoded_msg.system.as_deref(), Some("You are helpful"));

        // 2. Completion Encoders
        for proto in [
            ProtocolKind::ChatCompletions,
            ProtocolKind::Responses,
            ProtocolKind::Messages,
        ] {
            let mut enc = InboundCompletionEncoder::new(proto, "m".into());
            enc.push_event(CanonicalEvent::TextDelta("Hello world".into()));
            enc.push_event(CanonicalEvent::Usage(CanonicalUsage {
                prompt_tokens: 10,
                completion_tokens: 2,
                reasoning_tokens: None,
                cached_tokens: None,
            }));
            enc.push_event(CanonicalEvent::Completed {
                finish_reason: "stop".into(),
            });
            let val = enc.finish().unwrap();
            assert!(val.to_string().contains("Hello world"));
        }
    }
}
