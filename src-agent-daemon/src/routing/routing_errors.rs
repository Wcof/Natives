use agent_core::{EngineError, EngineProviderEvent};

pub(super) fn event_to_error(event: &EngineProviderEvent) -> EngineError {
    match event {
        EngineProviderEvent::Error {
            message,
            code,
            retryable,
            category,
            retry_after_ms,
        } => EngineError::Provider {
            message: message.clone(),
            code: code.clone(),
            retryable: *retryable,
            category: category.clone(),
            retry_after_ms: *retry_after_ms,
        },
        _ => EngineError::Message("routing stream failed".into()),
    }
}

pub(super) fn error_event(error: EngineError) -> EngineProviderEvent {
    match error {
        EngineError::Provider {
            message,
            code,
            retryable,
            category,
            retry_after_ms,
        } => EngineProviderEvent::Error {
            message,
            code,
            retryable,
            category,
            retry_after_ms,
        },
        other => EngineProviderEvent::Error {
            message: other.to_string(),
            code: other.code().into(),
            retryable: other.retryable(),
            category: "Unknown".into(),
            retry_after_ms: None,
        },
    }
}

pub(super) fn timeout_error(message: &str) -> EngineError {
    EngineError::Provider {
        message: message.into(),
        code: "timeout".into(),
        retryable: true,
        category: "Timeout".into(),
        retry_after_ms: None,
    }
}
