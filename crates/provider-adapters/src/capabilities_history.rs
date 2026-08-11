//! History-message → provider-message conversion (W9 split from
//! capabilities.rs). Owns the role/content/tool-call/tool-result/image
//! encoding rules shared by every provider adapter.

use super::{HistoryMessage, ImageSource, ProviderContentBlock, ProviderMessage};

/// Map a single history message into a provider message.
pub fn history_message_to_provider(msg: HistoryMessage) -> ProviderMessage {
    fn push_images(content: &mut Vec<ProviderContentBlock>, images: Vec<ImageSource>) {
        for image_url in images {
            content.push(ProviderContentBlock::Image { image_url });
        }
    }
    let images = msg.images;
    if let Some(calls) = msg.tool_calls {
        if !calls.is_empty() {
            let mut content = Vec::new();
            if !msg.content.is_empty() {
                content.push(ProviderContentBlock::Text { text: msg.content });
            }
            push_images(&mut content, images);
            for call in calls {
                let input = parse_tool_arguments(&call.arguments);
                content.push(ProviderContentBlock::ToolCall {
                    id: call.id,
                    name: call.name,
                    input,
                });
            }
            return ProviderMessage {
                role: if msg.role.is_empty() {
                    "assistant".into()
                } else {
                    msg.role
                },
                content,
            };
        }
    }

    if msg.role == "tool" || msg.tool_call_id.is_some() {
        let tool_call_id = msg
            .tool_call_id
            .unwrap_or_else(|| "unknown_tool_call".into());
        let mut content = vec![ProviderContentBlock::ToolResult {
            tool_call_id,
            content: msg.content,
            name: msg.tool_name,
        }];
        push_images(&mut content, images);
        return ProviderMessage {
            role: "tool".into(),
            content,
        };
    }

    let mut content = vec![ProviderContentBlock::Text { text: msg.content }];
    push_images(&mut content, images);
    ProviderMessage {
        role: msg.role,
        content,
    }
}

/// Map a batch of history messages.
pub fn history_messages_to_provider(
    messages: impl IntoIterator<Item = HistoryMessage>,
) -> Vec<ProviderMessage> {
    messages
        .into_iter()
        .map(history_message_to_provider)
        .collect()
}

fn parse_tool_arguments(arguments: &str) -> serde_json::Value {
    let trimmed = arguments.trim();
    if trimmed.is_empty() {
        return serde_json::json!({});
    }
    serde_json::from_str(trimmed)
        .unwrap_or_else(|_| serde_json::Value::String(arguments.to_string()))
}

#[cfg(test)]
mod history_message_tests {
    use super::*;

    #[test]
    fn preserves_assistant_tool_calls_and_tool_results() {
        let assistant = history_message_to_provider(HistoryMessage {
            role: "assistant".into(),
            content: "calling".into(),
            tool_call_id: None,
            tool_name: None,
            tool_calls: Some(vec![
                HistoryToolCall {
                    id: "call_1".into(),
                    name: "read_file".into(),
                    arguments: r#"{"path":"a.txt"}"#.into(),
                },
                HistoryToolCall {
                    id: "call_2".into(),
                    name: "echo".into(),
                    arguments: r#"{"x":1}"#.into(),
                },
            ]),
            images: Vec::new(),
        });
        assert_eq!(assistant.role, "assistant");
        assert_eq!(assistant.content.len(), 3);
        assert!(matches!(
            &assistant.content[0],
            ProviderContentBlock::Text { text } if text == "calling"
        ));
        assert!(matches!(
            &assistant.content[1],
            ProviderContentBlock::ToolCall { id, name, .. }
                if id == "call_1" && name == "read_file"
        ));
        assert!(matches!(
            &assistant.content[2],
            ProviderContentBlock::ToolCall { id, name, .. }
                if id == "call_2" && name == "echo"
        ));

        let tool = history_message_to_provider(HistoryMessage {
            role: "tool".into(),
            content: r#"{"ok":true}"#.into(),
            tool_call_id: Some("call_1".into()),
            tool_name: Some("read_file".into()),
            tool_calls: None,
            images: Vec::new(),
        });
        assert_eq!(tool.role, "tool");
        assert!(matches!(
            &tool.content[0],
            ProviderContentBlock::ToolResult {
                tool_call_id,
                name: Some(n),
                ..
            } if tool_call_id == "call_1" && n == "read_file"
        ));
    }

    #[test]
    fn plain_user_stays_text() {
        let msg = history_message_to_provider(HistoryMessage {
            role: "user".into(),
            content: "hi".into(),
            tool_call_id: None,
            tool_name: None,
            tool_calls: None,
            images: Vec::new(),
        });
        assert_eq!(msg.content.len(), 1);
        assert!(matches!(
            &msg.content[0],
            ProviderContentBlock::Text { text } if text == "hi"
        ));
    }

    #[test]
    fn images_reach_the_wire_in_every_branch() {
        let image = ImageSource::new("data:image/png;base64,AAAB");

        let user = history_message_to_provider(HistoryMessage {
            role: "user".into(),
            content: "look".into(),
            images: vec![image.clone()],
            ..Default::default()
        });
        assert_eq!(user.content.len(), 2);
        assert!(matches!(
            &user.content[1],
            ProviderContentBlock::Image { image_url } if image_url == &image
        ));

        let assistant = history_message_to_provider(HistoryMessage {
            role: "assistant".into(),
            content: "here".into(),
            tool_calls: Some(vec![HistoryToolCall {
                id: "c1".into(),
                name: "echo".into(),
                arguments: "{}".into(),
            }]),
            images: vec![image.clone()],
            ..Default::default()
        });
        // text, image, tool_call — the image must not be swallowed by the
        // tool-call branch.
        assert_eq!(assistant.content.len(), 3);
        assert!(matches!(
            &assistant.content[1],
            ProviderContentBlock::Image { .. }
        ));

        let tool = history_message_to_provider(HistoryMessage {
            role: "tool".into(),
            content: "{}".into(),
            tool_call_id: Some("c1".into()),
            images: vec![image],
            ..Default::default()
        });
        assert_eq!(tool.content.len(), 2);
        assert!(matches!(
            &tool.content[1],
            ProviderContentBlock::Image { .. }
        ));
    }
}
