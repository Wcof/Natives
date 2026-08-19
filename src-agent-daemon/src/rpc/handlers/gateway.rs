//! `model.gateway.stream` handler (ADR-0019 P2) — unified model access over the
//! daemon UDS, reusing `RoutedProvider` (the single routing executor) without a
//! full Agent Run state machine. This is the native-Rust Model Data Plane.

use agent_core::{
    EngineImage, EngineMessage, EngineProvider, EngineProviderEvent, EngineToolCall,
    ProviderStopReason, ToolSchema,
};
use assistant_protocol::error::{error_codes, DaemonError, ErrorCategory};
use assistant_protocol::v2::gateway::{
    gateway_stream_ack, ModelGatewayMessage, ModelGatewayStreamEvent, ModelGatewayStreamRequest,
};
use futures_util::StreamExt;

use crate::routing::{load_plan, RoutedProvider};

fn to_engine_message(message: &ModelGatewayMessage) -> EngineMessage {
    EngineMessage {
        role: message.role.clone(),
        content: message.content.clone(),
        tool_call_id: message.tool_call_id.clone(),
        tool_name: message.tool_name.clone(),
        tool_calls: message.tool_calls.as_ref().map(|calls| {
            calls
                .iter()
                .map(|call| EngineToolCall {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    arguments: call.arguments.clone(),
                })
                .collect()
        }),
        images: message
            .images
            .iter()
            .map(|image| EngineImage {
                url: image.url.clone(),
                media_type: image.media_type.clone(),
                detail: image.detail.clone(),
            })
            .collect(),
    }
}

fn stop_reason_str(reason: &ProviderStopReason) -> &'static str {
    match reason {
        ProviderStopReason::Stop => "stop",
        ProviderStopReason::ToolUse => "tool_use",
        ProviderStopReason::Length => "length",
        ProviderStopReason::Cancelled => "cancelled",
        ProviderStopReason::Error => "error",
        ProviderStopReason::Unknown(_) => "unknown",
    }
}

fn to_stream_event(event: EngineProviderEvent) -> ModelGatewayStreamEvent {
    match event {
        EngineProviderEvent::TextDelta(text) => ModelGatewayStreamEvent::TextDelta { text },
        EngineProviderEvent::ReasoningDelta(text) => {
            ModelGatewayStreamEvent::ReasoningDelta { text }
        }
        EngineProviderEvent::ToolCallDelta {
            index,
            id,
            name,
            arguments_delta,
        } => ModelGatewayStreamEvent::ToolCallDelta {
            index,
            id,
            name,
            arguments_delta,
        },
        EngineProviderEvent::Usage {
            input_tokens,
            output_tokens,
            reasoning_tokens,
            cache_creation_tokens,
            cache_read_tokens,
        } => ModelGatewayStreamEvent::Usage {
            input_tokens,
            output_tokens,
            reasoning_tokens,
            cache_creation_tokens,
            cache_read_tokens,
        },
        EngineProviderEvent::Completed => ModelGatewayStreamEvent::Completed {
            reason: "stop".into(),
        },
        EngineProviderEvent::CompletedWithReason { reason } => ModelGatewayStreamEvent::Completed {
            reason: stop_reason_str(&reason).to_string(),
        },
        EngineProviderEvent::Error {
            message,
            code,
            retryable,
            category,
            retry_after_ms,
        } => ModelGatewayStreamEvent::Error {
            message,
            code,
            retryable,
            category,
            retry_after_ms,
        },
    }
}

pub(crate) async fn dispatch_gateway(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    request: &assistant_protocol::v2::V2Request,
) {
    use crate::rpc::{send_error, send_success, write_json_line};

    let req: ModelGatewayStreamRequest = match serde_json::from_value(request.params.clone()) {
        Ok(req) => req,
        Err(error) => {
            send_error(
                writer,
                &request.request_id,
                &DaemonError::new(
                    error_codes::INVALID_INPUT,
                    ErrorCategory::Validation,
                    false,
                    format!("model.gateway.stream: {error}"),
                ),
            )
            .await;
            return;
        }
    };

    let model = req.model.trim();
    if model.is_empty() {
        send_error(
            writer,
            &request.request_id,
            &DaemonError::new(
                error_codes::INVALID_INPUT,
                ErrorCategory::Validation,
                false,
                "model is required".to_string(),
            ),
        )
        .await;
        return;
    }
    let provider_id = req.provider_id.trim();
    if provider_id.is_empty() {
        send_error(
            writer,
            &request.request_id,
            &DaemonError::new(
                error_codes::INVALID_INPUT,
                ErrorCategory::Validation,
                false,
                "providerId is required".to_string(),
            ),
        )
        .await;
        return;
    }

    let messages: Vec<EngineMessage> = req.messages.iter().map(to_engine_message).collect();
    let tools: Vec<ToolSchema> = req
        .tools
        .iter()
        .map(|tool| ToolSchema {
            name: tool.name.clone(),
            description: tool.description.clone(),
            input_schema: tool.input_schema.clone(),
        })
        .collect();
    let system = req.system.as_deref();

    let provider = RoutedProvider::new(load_plan(
        provider_id.to_string(),
        req.key_id.clone(),
        model.to_string(),
    ));
    let cancel = tokio_util::sync::CancellationToken::new();
    let mut events = match provider
        .stream(model, messages, &tools, system, cancel)
        .await
    {
        Ok(stream) => stream,
        Err(error) => {
            send_error(
                writer,
                &request.request_id,
                &DaemonError::new(
                    error_codes::PROVIDER_ERROR,
                    ErrorCategory::Provider,
                    false,
                    format!("failed to start model stream: {error}"),
                ),
            )
            .await;
            return;
        }
    };

    // ACK first (normal RPC response), then newline-delimited event frames —
    // the same contract shape as `run.watch`.
    send_success(
        writer,
        &request.request_id,
        &request.client_id,
        &request.session_token,
        gateway_stream_ack(),
    )
    .await;

    while let Some(event) = events.next().await {
        if write_json_line(writer, &to_stream_event(event))
            .await
            .is_err()
        {
            return; // client dropped — clean exit, no orphan task
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_conversion_carries_tool_calls_and_images() {
        let msg = ModelGatewayMessage {
            role: "assistant".into(),
            content: "hi".into(),
            tool_call_id: None,
            tool_name: None,
            tool_calls: Some(vec![
                assistant_protocol::v2::gateway::ModelGatewayToolCall {
                    id: "call-1".into(),
                    name: "lookup".into(),
                    arguments: "{\"q\":1}".into(),
                },
            ]),
            images: vec![assistant_protocol::v2::gateway::ModelGatewayImage {
                url: "data:image/png;base64,AA==".into(),
                media_type: None,
                detail: Some("high".into()),
            }],
        };
        let engine = to_engine_message(&msg);
        assert_eq!(engine.role, "assistant");
        assert_eq!(engine.content, "hi");
        assert_eq!(engine.tool_calls.as_ref().unwrap()[0].name, "lookup");
        assert_eq!(engine.images[0].detail.as_deref(), Some("high"));
    }

    #[test]
    fn event_conversion_maps_stop_reason_snake_case() {
        let completed = to_stream_event(EngineProviderEvent::CompletedWithReason {
            reason: ProviderStopReason::ToolUse,
        });
        assert!(matches!(
            completed,
            ModelGatewayStreamEvent::Completed { reason } if reason == "tool_use"
        ));
    }

    #[test]
    fn stream_event_serializes_to_tagged_shape() {
        let event = ModelGatewayStreamEvent::TextDelta {
            text: "hello".into(),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "text_delta");
        assert_eq!(json["text"], "hello");
    }
}
