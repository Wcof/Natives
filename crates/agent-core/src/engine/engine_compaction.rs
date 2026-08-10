//! Compaction domain (W9 split from engine_core.rs): model-backed summary
//! round-trips with mechanical truncation fallback, plus the typed-transcript
//! compaction entry point. Every failure path degrades to mechanical
//! compaction — compaction never fails a Run.

use super::conversion::{
    agent_message_id, agent_messages_to_values, values_chars, values_to_agent_messages,
};
use super::engine_core::AgentEngine;
use super::error::EngineError;
use super::provider::{
    EngineProvider, EngineProviderContext, EngineProviderEvent, ProviderTurnRequest,
};
use crate::compaction::{
    apply_model_summary, choose_summary_split, compact_messages as compact_tool_history,
    render_transcript_for_summary, repair_dangling_tool_calls, CompactResult,
    SUMMARY_SYSTEM_PROMPT,
};
use crate::hooks::{HookEvent, HookRegistry, HookRequest};
use crate::context::ContextStats;
use assistant_protocol::v2::RunEventKind;
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::sync::atomic::Ordering as AtomicOrdering;
use std::sync::Mutex;

/// Soft budget for in-engine history characters before tool-output compaction.
const HISTORY_COMPACT_CHARS: usize = 48_000;
const TOOL_OUTPUT_MAX_CHARS: usize = 4_000;

/// Messages kept verbatim at the tail when a model summary replaces the prefix.
pub(crate) const SUMMARY_KEEP_TAIL_MESSAGES: usize = 6;
/// Below this many summarizable messages a provider round trip is not worth it.
const SUMMARY_MIN_PREFIX_MESSAGES: usize = 4;
/// Upper bound on the transcript handed to the summarizer (cost boundary).
const SUMMARY_TRANSCRIPT_MAX_CHARS: usize = 60_000;
/// Per-message truncation inside that transcript.
const SUMMARY_MESSAGE_MAX_CHARS: usize = 2_000;
/// Wall clock ceiling for one summarization round trip.
const SUMMARY_TIMEOUT_MS: u64 = 60_000;
/// Total model summarizations attempted by one engine, successful or not.
const SUMMARY_MAX_ATTEMPTS: u32 = 8;
/// After this many failures the engine stops paying for summarization and
/// stays on mechanical compaction for the rest of the run.
pub(crate) const SUMMARY_MAX_FAILURES: u32 = 2;

impl AgentEngine {
    /// Compact the typed Core transcript at the provider-neutral JSON boundary.
    ///
    /// Over budget the engine first asks the model for a structured summary of
    /// the old prefix (see [`crate::compaction::SUMMARY_SYSTEM_PROMPT`]) and
    /// keeps only `[summary] + recent tail`. Every failure path — provider
    /// error, timeout, cancellation, empty answer, budget exhausted — falls
    /// back to mechanical compaction. Compaction never fails a Run.
    pub(super) async fn maybe_compact_typed_history(
        &self,
        run_id: &str,
        turn_id: &crate::TurnId,
        model: &str,
        provider: &dyn EngineProvider,
        messages: Vec<crate::AgentMessage>,
    ) -> Result<Vec<crate::AgentMessage>, EngineError> {
        let values = agent_messages_to_values(&messages);
        let compacted = self
            .maybe_compact_values(run_id, model, provider, values)
            .await?;
        if compacted != agent_messages_to_values(&messages) {
            let summary_message_id = compacted.iter().find_map(|message| {
                let content = message.get("content").and_then(Value::as_str)?;
                (message.get("role").and_then(Value::as_str) == Some("system")
                    && content.starts_with(crate::compaction::SUMMARY_MARKER))
                .then(|| message.get("message_id").and_then(Value::as_str))
                .flatten()
                .map(str::to_string)
            });
            self.append_critical(
                run_id,
                RunEventKind::ContextSnapshotCommitted {
                    snapshot_id: uuid::Uuid::new_v4().to_string(),
                    turn_id: Some(turn_id.to_string()),
                    source_revision: self.events.last_sequence(run_id),
                    input_message_ids: messages.iter().map(agent_message_id).collect(),
                    summary_message_id,
                    replaced_range: Some(format!("0..{}", messages.len())),
                    algorithm_version: "typed-compaction-v1".into(),
                    provider_context_window: self.provider_context_window,
                    artifact_reference: None,
                    snapshot_json: Value::Array(compacted.clone()),
                },
            )?;
        }
        Ok(values_to_agent_messages(&compacted))
    }

    pub(super) async fn maybe_compact_values(
        &self,
        run_id: &str,
        model: &str,
        provider: &dyn EngineProvider,
        messages: Vec<Value>,
    ) -> Result<Vec<Value>, EngineError> {
        let history_limit = self.history_compact_chars.unwrap_or(HISTORY_COMPACT_CHARS);
        let tool_limit = self.tool_output_max_chars.unwrap_or(TOOL_OUTPUT_MAX_CHARS);
        // PERF-001: observe the transcript with a cheap structured char count
        // (a bounded `Value` tree walk — no string allocation) and append ONLY
        // this round's growth to the running budget. The pre-fix path
        // re-serialized the whole transcript to a JSON string every
        // provider/tool round AND re-accumulated that full size into the
        // estimate, so an uncompacted multi-turn run drifted arbitrarily large
        // while still paying O(n) serialization per round.
        let before_chars = values_chars(&messages);
        self.context_stats
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .observe_transcript(before_chars);
        if before_chars < history_limit {
            // Still repair dangling pairs cheaply.
            let (fixed, repaired) = repair_dangling_tool_calls(&messages);
            if repaired == 0 {
                return Ok(messages);
            }
            return Ok(fixed);
        }

        let pre_compact = self
            .hooks
            .dispatch(HookRequest {
                event: HookEvent::PreCompact,
                run_id: run_id.to_string(),
                tool_name: None,
                input: json!({
                    "before_chars": before_chars,
                    "messages": messages.len(),
                    "model_compaction": self.model_compaction,
                }),
            })
            .await;
        if HookRegistry::aggregate_allow(&pre_compact).is_err() {
            // PreCompact is a pre-event: a Deny aborts compaction (the
            // transcript is returned untouched) — a deterministic, honest
            // outcome, unlike the post-event observation points.
            return Ok(messages);
        }

        let result = match self
            .try_model_summary(run_id, model, provider, &messages, tool_limit)
            .await
        {
            Some(summarized) => summarized,
            None => compact_tool_history(&messages, tool_limit),
        };
        let mode = if result.summarized_messages > 0 {
            "model"
        } else {
            "mechanical"
        };
        // Same cheap structured measure as the observe above, so the reset
        // baseline and the next observation use one consistent char metric.
        let after_chars = values_chars(&result.messages);
        // P1-05/PERF-001: a real compaction rewrote the transcript — bump the
        // revision and re-seed the running counters from the kept size so
        // stale bytes do not accumulate (P1-06 invariant).
        {
            let mut stats = self.context_stats.lock().unwrap_or_else(|e| e.into_inner());
            stats.note_compaction();
            stats.reset(after_chars);
        }

        self.events.append(
            run_id,
            RunEventKind::ContextCompressed {
                before_tokens: (before_chars as u64 / 4).max(1),
                after_tokens: (after_chars as u64 / 4).max(1),
                summary: result.summary.clone(),
            },
        );

        // PostCompact is an observation point: the compaction already applied
        // and a hook cannot rewind it. A hook that tries to alter the outcome
        // (Deny/Modify/Inject) is refused loudly rather than silently ignored.
        self.hooks
            .observe(HookRequest {
                event: HookEvent::PostCompact,
                run_id: run_id.to_string(),
                tool_name: None,
                input: json!({
                    "mode": mode,
                    "before_chars": before_chars,
                    "after_chars": after_chars,
                    "dropped_tool_outputs": result.dropped_tool_outputs,
                    "repaired_dangling": result.repaired_dangling,
                    "summarized_messages": result.summarized_messages,
                    "summary": result.summary,
                }),
            })
            .await
            .map_err(|refusal| match refusal {
                crate::hooks::ObserveResult::Failed { reason } => EngineError::HookRefused(reason),
                crate::hooks::ObserveResult::Observe => {
                    EngineError::HookRefused("post-hook observation failed".into())
                }
            })?;

        Ok(result.messages)
    }

    /// Model-backed compaction: `Some` only when a usable summary came back.
    ///
    /// Returning `None` is the documented degradation path and the caller
    /// answers it with mechanical compaction.
    pub(super) async fn try_model_summary(
        &self,
        run_id: &str,
        model: &str,
        provider: &dyn EngineProvider,
        values: &[Value],
        tool_limit: usize,
    ) -> Option<CompactResult> {
        if !self.model_compaction || self.cancel.is_cancelled() {
            return None;
        }
        // Cost boundary: bounded attempts per engine, and a failure streak
        // permanently drops the run back to mechanical compaction.
        if self.summary_attempts.load(AtomicOrdering::SeqCst) >= SUMMARY_MAX_ATTEMPTS
            || self.summary_failures.load(AtomicOrdering::SeqCst) >= SUMMARY_MAX_FAILURES
        {
            return None;
        }
        let split = choose_summary_split(values, SUMMARY_KEEP_TAIL_MESSAGES);
        if split < SUMMARY_MIN_PREFIX_MESSAGES {
            // Too little history to be worth a round trip.
            return None;
        }

        self.summary_attempts.fetch_add(1, AtomicOrdering::SeqCst);
        let transcript = render_transcript_for_summary(
            &values[..split],
            SUMMARY_TRANSCRIPT_MAX_CHARS,
            SUMMARY_MESSAGE_MAX_CHARS,
        );
        let request = vec![crate::AgentMessage::User(crate::UserMessage {
            message_id: crate::MessageId::new(),
            content: vec![crate::ContentBlock::Text { text: transcript }],
        })];

        match self
            .stream_summary_text(run_id, model, provider, request)
            .await
        {
            Ok(summary) if !summary.trim().is_empty() => {
                Some(apply_model_summary(values, split, &summary, tool_limit))
            }
            _ => {
                self.summary_failures.fetch_add(1, AtomicOrdering::SeqCst);
                None
            }
        }
    }

    /// One isolated provider round trip that yields plain summary text.
    ///
    /// Isolated in three ways: no tools (so it cannot start a tool loop), a
    /// throwaway message vector (so it never touches the run history), and no
    /// event emission (so the summary does not surface as assistant output).
    /// Bounded by the run cancel token and a wall-clock timeout.
    pub(super) async fn stream_summary_text(
        &self,
        run_id: &str,
        model: &str,
        provider: &dyn EngineProvider,
        messages: Vec<crate::AgentMessage>,
    ) -> Result<String, String> {
        let cancel = self.cancel.clone();
        let collect = async {
            let mut stream = provider
                .stream_turn(
                    ProviderTurnRequest {
                        context: EngineProviderContext {
                            run_id: run_id.to_string(),
                            attempt: 0,
                        },
                        model: model.to_string(),
                        system_prompt: Some(SUMMARY_SYSTEM_PROMPT.to_string()),
                        messages,
                        tools: Vec::new(),
                    },
                    cancel.clone(),
                )
                .await
                .map_err(|e| e.to_string())?;
            let mut text = String::new();
            while let Some(event) = stream.next().await {
                if cancel.is_cancelled() {
                    return Err("cancelled".to_string());
                }
                match event {
                    EngineProviderEvent::TextDelta(delta) => text.push_str(&delta),
                    EngineProviderEvent::Error { message, .. } => return Err(message),
                    _ => {}
                }
            }
            Ok(text)
        };

        tokio::select! {
            biased;
            _ = self.cancel.cancelled() => Err("cancelled".to_string()),
            outcome = tokio::time::timeout(
                std::time::Duration::from_millis(SUMMARY_TIMEOUT_MS),
                collect,
            ) => outcome.unwrap_or_else(|_| Err("summary request timed out".to_string())),
        }
    }
}

#[allow(unused)]
fn _assert_sync<T: Send + Sync>() {}
fn _assert_context_stats(_: &ContextStats) {}
fn _assert_mutex(_: &Mutex<()>) {}
