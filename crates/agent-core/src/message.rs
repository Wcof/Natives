//! Typed, provider-neutral messages used by the deepening path.

use serde::{Deserialize, Serialize};

macro_rules! id_type {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            pub fn new() -> Self {
                Self(uuid::Uuid::new_v4().to_string())
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }
        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_string())
            }
        }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

id_type!(EngineRunId);
id_type!(TurnId);
id_type!(MessageId);
id_type!(ToolCallId);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserMessage {
    pub message_id: MessageId,
    pub content: Vec<ContentBlock>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssistantMessage {
    pub message_id: MessageId,
    pub content: Vec<ContentBlock>,
    pub stop_reason: Option<StopReason>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemMessage {
    pub message_id: MessageId,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomMessage {
    pub message_id: MessageId,
    pub kind: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentMessage {
    User(UserMessage),
    Assistant(AssistantMessage),
    ToolResult(ToolResultMessage),
    System(SystemMessage),
    Custom(CustomMessage),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContentBlock {
    Text {
        text: String,
    },
    Thinking {
        text: String,
        signature: Option<String>,
    },
    Image {
        source: ImageSource,
    },
    ToolCall(ToolCall),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageSource {
    pub url: String,
    pub media_type: Option<String>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub tool_call_id: ToolCallId,
    pub name: String,
    pub arguments_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolResultMessage {
    pub message_id: MessageId,
    pub tool_call_id: ToolCallId,
    pub tool_name: String,
    pub content: Vec<ToolResultBlock>,
    pub is_error: bool,
    pub code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolResultBlock {
    Text {
        text: String,
    },
    Json {
        value: serde_json::Value,
    },
    Artifact {
        artifact_id: String,
        preview: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StopReason {
    Stop,
    ToolUse,
    Length,
    Cancelled,
    Error,
    Provider(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ids_are_opaque_and_round_trip() {
        let id = ToolCallId::from("call-1");
        let encoded = serde_json::to_string(&id).unwrap();
        assert_eq!(encoded, "\"call-1\"");
        assert_eq!(serde_json::from_str::<ToolCallId>(&encoded).unwrap(), id);
    }
}
