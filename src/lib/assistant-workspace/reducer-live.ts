/**
 * Live-message slice of the workspace reducer (W4 split).
 *
 * Owns the streaming "live bubble" projection: applying a single `RunEvent` to
 * the ephemeral live message blocks (reasoning / text / tool cards / notices).
 * The parent reducer file composes this slice; the external `workspaceReducer`
 * interface is unchanged.
 */
import type { ContentBlock, InteractionRequest, Run, RunEvent, RunStatus } from '@/lib/assistant-protocol';
import type { AssistantWorkspaceState, LiveBubble } from './state';

/** Lifecycle rank — higher means further along; used to prevent status regression. */
export function runStatusRank(status: RunStatus | string): number {
  switch (status) {
    case 'created':
      return 0;
    case 'queued':
      return 1;
    case 'preparing':
      return 2;
    case 'reasoning':
    case 'running':
    case 'waiting_permission':
    case 'waiting_user':
    case 'waiting_subagent':
    case 'cancelling':
    case 'background_watching':
      return 3;
    case 'completed':
    case 'failed':
    case 'cancelled':
    case 'interrupted':
      return 100;
    default:
      return 0;
  }
}

export function statusFromEventType(type: string): RunStatus | null {
  switch (type) {
    case 'queued':
      return 'queued';
    case 'preparing':
    case 'generation_attempt_started':
      return 'preparing';
    case 'started':
      return 'running';
    case 'reasoning_delta':
      return 'reasoning';
    case 'text_delta':
    case 'assistant_delta':
    case 'tool_call_requested':
    case 'tool_call_started':
    case 'tool_call_delta':
    case 'tool_call_completed':
    case 'tool_output_delta':
    case 'task_started':
    case 'task_updated':
    case 'task_completed':
    case 'progress':
      // progress.message may signal background_watching from engine
      return 'running';
    case 'usage_updated':
    case 'file_changed':
    case 'permission_responded':
    case 'interaction_resolved':
    case 'interaction_responded':
      return 'running';
    case 'permission_requested':
      return 'waiting_permission';
    case 'interaction_requested':
      return 'waiting_user';
    case 'subagent_created':
      return 'waiting_subagent';
    case 'subagent_completed':
    case 'subagent_failed':
      return 'running';
    case 'context_compressed':
      return null;
    case 'completed':
      return 'completed';
    case 'failed':
      return 'failed';
    case 'cancelled':
      return 'cancelled';
    case 'interrupted':
      return 'interrupted';
    default:
      return null;
  }
}

function ensureLive(state: AssistantWorkspaceState, run: Run): LiveBubble {
  const existing = state.liveByRun[run.id];
  if (existing) return existing;
  return {
    runId: run.id,
    conversationId: run.conversationId,
    messageId: `live-${run.id}`,
    blocks: [],
    reasoningStartedAt: null,
    reasoningFinishedAt: null,
    attemptSnapshots: {},
  };
}

function upsertTextBlock(blocks: ContentBlock[], text: string): ContentBlock[] {
  const next = [...blocks];
  // Stream-order merge (mirrors upsertReasoning): only append to the text block
  // when it is the LAST block. Anything in between (tool card, notice, …) starts
  // a new text block, so "explain A → tool → explain B" keeps its order instead
  // of gluing B onto A.
  const lastIdx = next.length - 1;
  if (lastIdx >= 0 && next[lastIdx]!.type === 'text') {
    const cur = next[lastIdx]!;
    next[lastIdx] = { ...cur, text: `${cur.text ?? ''}${text}` };
  } else {
    next.push({ type: 'text', text });
  }
  return next;
}

function upsertReasoning(
  blocks: ContentBlock[],
  text: string,
  segmentId?: string,
): ContentBlock[] {
  const next = [...blocks];
  let idx = segmentId
    ? next.findIndex((b) => b.type === 'reasoning' && b.segmentId === segmentId)
    : -1;
  if (idx < 0 && next.length > 0 && next[next.length - 1]!.type === 'reasoning') {
    idx = next.length - 1;
  }
  if (idx >= 0) {
    const cur = next[idx]!;
    next[idx] = {
      ...cur,
      reasoning: `${cur.reasoning ?? ''}${text}`,
      segmentId: segmentId ?? cur.segmentId,
      live: true,
    };
  } else {
    // Append in stream order (supports Thinking -> Tool -> Thinking interleaving)
    next.push({ type: 'reasoning', reasoning: text, segmentId, live: true });
  }
  return next;
}

function upsertTool(
  blocks: ContentBlock[],
  toolCallId: string,
  patch: Partial<ContentBlock>,
): ContentBlock[] {
  const next = [...blocks];
  const idx = next.findIndex((b) => b.type === 'tool_call' && b.toolCallId === toolCallId);
  if (idx >= 0) {
    next[idx] = { ...next[idx]!, ...patch, type: 'tool_call', toolCallId };
  } else {
    next.push({
      type: 'tool_call',
      toolCallId,
      toolName: patch.toolName ?? 'tool',
      toolInput: patch.toolInput ?? {},
      toolStatus: patch.toolStatus ?? 'pending',
      ...patch,
    });
  }
  return next;
}

export type LiveEventResult = {
  live: LiveBubble;
  interaction?: InteractionRequest;
  removeInteractionId?: string;
};

export function applyEventToLive(
  state: AssistantWorkspaceState,
  event: RunEvent,
  run: Run,
): LiveEventResult {
  const live = { ...ensureLive(state, run), blocks: [...ensureLive(state, run).blocks] };
  const p = event.payload;
  let interaction: InteractionRequest | undefined;
  let removeInteractionId: string | undefined;

  switch (event.type) {
    case 'generation_attempt_started': {
      const attempt = Number(p.attempt ?? 0);
      if (attempt > 0) {
        live.attemptSnapshots = {
          ...(live.attemptSnapshots ?? {}),
          [attempt]: live.blocks.map((b) => ({ ...b })),
        };
      }
      break;
    }
    case 'generation_attempt_discarded': {
      const attempt = Number(p.attempt ?? 0);
      const snapshot = attempt > 0 ? live.attemptSnapshots?.[attempt] : undefined;
      if (snapshot) {
        // Text/tool deltas from a failed provider attempt are speculative, but
        // reasoning is useful context and must not vanish when the engine
        // retries or reports the attempt failure.
        const reasoning = live.blocks
          .filter((b) => b.type === 'reasoning')
          .map((b) => ({ ...b, live: false }));
        live.blocks = [
          ...snapshot.filter((b) => b.type !== 'reasoning').map((b) => ({ ...b })),
          ...reasoning,
        ];
        live.attemptSnapshots = { ...(live.attemptSnapshots ?? {}) };
        delete live.attemptSnapshots[attempt];
      }
      break;
    }
    case 'generation_attempt_committed': {
      const attempt = Number(p.attempt ?? 0);
      if (attempt > 0 && live.attemptSnapshots?.[attempt]) {
        live.attemptSnapshots = { ...live.attemptSnapshots };
        delete live.attemptSnapshots[attempt];
      }
      break;
    }
    case 'generation_attempt_failed': {
      // Upstream retry visibility: while the engine retries (503/EMPTY_RESPONSE…)
      // the user otherwise only sees "thinking". Surface a lightweight notice.
      // Non-retrying failures are followed by a `failed` event (error block),
      // so only the retrying case emits a notice. Structured data only — the
      // rendering layer formats/localizes it.
      const retrying = Boolean(p.retrying);
      if (retrying) {
        live.blocks = [
          ...live.blocks,
          {
            type: 'system_notice',
            noticeKind: 'generation_retry',
            noticeData: {
              attempt: Number(p.attempt ?? 0),
              maxAttempts:
                p.max_attempts != null || p.maxAttempts != null
                  ? Number(p.max_attempts ?? p.maxAttempts)
                  : undefined,
              code: String(p.code ?? ''),
              retryable: Boolean(p.retryable),
              retrying,
            },
          },
        ];
      }
      break;
    }
    case 'context_compressed': {
      // Engine compacted the conversation context; make it visible as a
      // compaction divider block (before/after tokens + summary).
      live.blocks = [
        ...live.blocks,
        {
          type: 'compaction',
          beforeTokens: Number(p.before_tokens ?? p.beforeTokens ?? 0),
          afterTokens: Number(p.after_tokens ?? p.afterTokens ?? 0),
          summary: String(p.summary ?? ''),
        },
      ];
      break;
    }
    case 'checkpoint_created': {
      live.blocks = [
        ...live.blocks,
        {
          type: 'system_notice',
          noticeKind: 'checkpoint_created',
          noticeData: {
            checkpointId: String(p.checkpoint_id ?? p.checkpointId ?? ''),
            label: p.label != null ? String(p.label) : undefined,
          },
        },
      ];
      break;
    }
    case 'checkpoint_rewound': {
      const paths = Array.isArray(p.paths) ? p.paths.map(String) : [];
      live.blocks = [
        ...live.blocks,
        {
          type: 'system_notice',
          noticeKind: 'checkpoint_rewound',
          noticeData: {
            checkpointId: String(p.checkpoint_id ?? p.checkpointId ?? ''),
            paths,
            count: paths.length,
            conflictPolicy:
              p.conflict_policy != null || p.conflictPolicy != null
                ? String(p.conflict_policy ?? p.conflictPolicy)
                : undefined,
          },
        },
      ];
      break;
    }
    case 'subagent_created': {
      // State-level tracking (childRunsByParent…) happens in the parent reducer;
      // here we add a timeline block so the spawn is visible in the answer body.
      live.blocks = [
        ...live.blocks,
        {
          type: 'subagent',
          subRunId: String(p.sub_run_id ?? p.subRunId ?? ''),
          noticeKind: 'subagent_created',
          noticeData: {
            task: String(p.task ?? ''),
            agentProfileId:
              p.agent_profile_id != null || p.agentProfileId != null
                ? String(p.agent_profile_id ?? p.agentProfileId)
                : undefined,
          },
        },
      ];
      break;
    }
    case 'text_delta':
    case 'assistant_delta': {
      const text = String(p.text ?? p.delta ?? '');
      if (live.reasoningStartedAt && !live.reasoningFinishedAt) {
        live.reasoningFinishedAt = event.timestamp;
        live.blocks = live.blocks.map((b) =>
          b.type === 'reasoning' ? { ...b, live: false } : b,
        );
      }
      live.blocks = upsertTextBlock(live.blocks, text);
      break;
    }
    case 'reasoning_segment_start':
    case 'reasoning_start': {
      const segmentId = p.segment_id != null ? String(p.segment_id) : p.segmentId != null ? String(p.segmentId) : undefined;
      live.reasoningStartedAt = live.reasoningStartedAt ?? event.timestamp;
      if (segmentId && !live.blocks.some((b) => b.type === 'reasoning' && b.segmentId === segmentId)) {
        live.blocks = [...live.blocks, { type: 'reasoning', reasoning: '', segmentId, live: true }];
      }
      break;
    }
    case 'reasoning_segment_end': {
      const segmentId = p.segment_id != null ? String(p.segment_id) : p.segmentId != null ? String(p.segmentId) : undefined;
      const durationMs = typeof p.duration_ms === 'number' ? p.duration_ms : typeof p.durationMs === 'number' ? p.durationMs : undefined;
      const summary = p.summary != null ? String(p.summary) : undefined;
      const summaryStatus = (p.summary_status ?? p.summaryStatus) as ContentBlock['summaryStatus'];
      live.blocks = live.blocks.map((b) => {
        if (b.type === 'reasoning' && (!segmentId || b.segmentId === segmentId)) {
          return {
            ...b,
            live: false,
            durationMs: durationMs ?? b.durationMs,
            summary: summary ?? b.summary,
            summaryStatus: summaryStatus ?? b.summaryStatus,
          };
        }
        return b;
      });
      break;
    }
    case 'reasoning_delta': {
      const text = String(p.text ?? p.reasoning ?? '');
      const segmentId = p.segment_id != null ? String(p.segment_id) : p.segmentId != null ? String(p.segmentId) : undefined;
      live.reasoningStartedAt = live.reasoningStartedAt ?? event.timestamp;
      live.blocks = upsertReasoning(live.blocks, text, segmentId);
      break;
    }
    case 'tool_call_requested':
    case 'tool_call_started':
    case 'tool_started': {
      const id = String(p.id ?? p.tool_call_id ?? p.toolCallId ?? '');
      const name = String(p.name ?? p.tool_name ?? p.toolName ?? 'tool');
      const input = (p.input ?? p.args ?? {}) as Record<string, unknown>;
      live.blocks = upsertTool(live.blocks, id, {
        toolName: name,
        toolInput: input,
        toolStatus: event.type === 'tool_call_requested' ? 'pending' : 'running',
      });
      break;
    }
    case 'tool_call_delta': {
      // Streamed argument fragments: merge by tool id, or by index placeholder
      // until the real id arrives on requested/started.
      const id = String(p.id ?? p.tool_call_id ?? p.toolCallId ?? '');
      const index =
        typeof p.index === 'number'
          ? p.index
          : typeof p.index === 'string'
            ? Number(p.index)
            : NaN;
      const placeholderId =
        id || (Number.isFinite(index) ? `tool-index-${index}` : '');
      if (!placeholderId) break;
      const name =
        p.name != null || p.tool_name != null || p.toolName != null
          ? String(p.name ?? p.tool_name ?? p.toolName)
          : undefined;
      const delta = String(p.arguments_delta ?? p.argumentsDelta ?? p.delta ?? '');
      const existing = live.blocks.find(
        (b) => b.type === 'tool_call' && b.toolCallId === placeholderId,
      );
      const prevPartial =
        existing && typeof (existing as { toolPartialArgs?: string }).toolPartialArgs === 'string'
          ? (existing as { toolPartialArgs?: string }).toolPartialArgs!
          : existing && typeof existing.toolInput === 'object' && existing.toolInput
            ? JSON.stringify(existing.toolInput)
            : '';
      const merged = `${prevPartial}${delta}`;
      let parsed: Record<string, unknown> = {};
      try {
        const t = JSON.parse(merged);
        if (t && typeof t === 'object' && !Array.isArray(t)) {
          parsed = t as Record<string, unknown>;
        }
      } catch {
        // keep partial as raw under _partial until complete JSON arrives
        parsed = { _partial: merged };
      }
      live.blocks = upsertTool(live.blocks, placeholderId, {
        toolName: name,
        toolInput: parsed,
        toolStatus: 'running',
        toolPartialArgs: merged,
      } as Partial<ContentBlock>);
      // If we later get a real id that differs from the placeholder, requested/started
      // will upsert under the real id; index placeholder remains until then.
      break;
    }
    case 'tool_call_completed':
    case 'tool_completed':
    case 'tool_rejected': {
      const id = String(p.id ?? p.tool_call_id ?? p.toolCallId ?? '');
      const isError =
        event.type === 'tool_rejected' ||
        Boolean(p.is_error ?? p.isError) ||
        p.status === 'error';
      live.blocks = upsertTool(live.blocks, id, {
        toolName: p.name != null || p.tool_name != null ? String(p.name ?? p.tool_name) : undefined,
        toolStatus: isError ? 'failed' : 'completed',
        toolOutput: p.output ?? p.result,
        isError,
        durationMs: typeof p.duration_ms === 'number' ? p.duration_ms : typeof p.durationMs === 'number' ? p.durationMs : undefined,
      });
      break;
    }
    case 'tool_output_delta': {
      // Terminal / long-tool stdout streaming: append delta under the tool card.
      // Replays must not double-append: callers dedupe by sequence; here we only
      // merge when the event carries a non-empty delta for a known tool_call_id.
      const id = String(p.tool_call_id ?? p.toolCallId ?? p.id ?? '');
      if (!id) break;
      const delta = String(p.delta ?? p.chunk ?? p.text ?? p.output ?? '');
      if (!delta) break;
      const existing = live.blocks.find(
        (b) => b.type === 'tool_call' && b.toolCallId === id,
      );
      const prev =
        existing && typeof existing.toolOutput === 'string'
          ? existing.toolOutput
          : existing && existing.toolOutput != null
            ? JSON.stringify(existing.toolOutput)
            : '';
      // Cap live card text (~1MB plan limit is for persisted events; UI keeps a
      // smaller window so re-renders stay cheap).
      const merged = `${prev}${delta}`;
      const capped =
        merged.length > 256_000 ? `…[truncated]\n${merged.slice(-256_000)}` : merged;
      live.blocks = upsertTool(live.blocks, id, {
        toolStatus: 'running',
        toolOutput: capped,
        toolStreamTruncated: merged.length > 256_000 || Boolean(p.truncated),
      } as Partial<ContentBlock>);
      break;
    }
    case 'task_started':
    case 'task_updated':
    case 'task_completed': {
      // Activity panel derives tasks from events; keep run live while tasks move.
      break;
    }
    case 'permission_requested': {
      const id = String(p.permission_id ?? p.permissionId ?? p.tool_call_id ?? p.id ?? '');
      interaction = {
        kind: 'permission',
        id,
        runId: event.runId,
        conversationId: run.conversationId,
        toolCallId: String(p.tool_call_id ?? p.toolCallId ?? ''),
        toolName: String(p.tool_name ?? p.toolName ?? 'tool'),
        reason: String(p.reason ?? ''),
        input: (p.input ?? p.args ?? {}) as Record<string, unknown>,
        createdAt: event.timestamp,
      };
      break;
    }
    case 'permission_responded': {
      removeInteractionId = String(p.permission_id ?? p.permissionId ?? p.id ?? '');
      break;
    }
    case 'interaction_requested': {
      const kind = String(p.kind ?? 'ask_user');
      const id = String(p.id ?? p.interaction_id ?? p.interactionId ?? '');
      if (kind === 'plan_approval') {
        interaction = {
          kind: 'plan_approval',
          id,
          runId: event.runId,
          conversationId: run.conversationId,
          createdAt: event.timestamp,
          title: String(p.title ?? 'Plan'),
          planMarkdown: String(p.plan_markdown ?? p.planMarkdown ?? p.plan ?? ''),
        };
      } else if (kind === 'conflict_resolution') {
        interaction = {
          kind: 'conflict_resolution',
          id,
          runId: event.runId,
          conversationId: run.conversationId,
          createdAt: event.timestamp,
          files: Array.isArray(p.files)
            ? (p.files as Array<{ path: string; base?: string; ours?: string; theirs?: string }>)
            : [],
        };
      } else if (kind === 'subagent_assignment') {
        const nested =
          p.payload && typeof p.payload === 'object'
            ? (p.payload as Record<string, unknown>)
            : p;
        const defaultRaw =
          nested.default_binding && typeof nested.default_binding === 'object'
            ? (nested.default_binding as Record<string, unknown>)
            : nested.defaultBinding && typeof nested.defaultBinding === 'object'
              ? (nested.defaultBinding as Record<string, unknown>)
              : null;
        const defaultBinding = defaultRaw
          ? {
              providerId: String(defaultRaw.provider_id ?? defaultRaw.providerId ?? ''),
              keyId: String(defaultRaw.key_id ?? defaultRaw.keyId ?? ''),
              modelId: String(defaultRaw.model_id ?? defaultRaw.modelId ?? ''),
            }
          : null;
        const tasks = Array.isArray(nested.tasks)
          ? nested.tasks
              .filter((item): item is Record<string, unknown> => Boolean(item) && typeof item === 'object')
              .map((task, index) => ({
                callId: String(task.call_id ?? task.callId ?? `task-${index}`),
                name: String(task.name ?? task.prompt ?? task.task ?? `Task ${index + 1}`),
                prompt:
                  task.prompt != null
                    ? String(task.prompt)
                    : task.task != null
                      ? String(task.task)
                      : null,
              }))
          : undefined;
        interaction = {
          kind: 'subagent_assignment',
          id,
          runId: event.runId,
          conversationId: String(
            nested.parent_conversation_id ??
              nested.parentConversationId ??
              nested.conversation_id ??
              nested.conversationId ??
              run.conversationId ??
              '',
          ),
          createdAt: event.timestamp,
          reason: nested.reason != null ? String(nested.reason) : undefined,
          batchId:
            nested.batch_id != null || nested.batchId != null
              ? String(nested.batch_id ?? nested.batchId)
              : undefined,
          parentConversationId:
            nested.parent_conversation_id != null || nested.parentConversationId != null
              ? String(nested.parent_conversation_id ?? nested.parentConversationId)
              : undefined,
          parentRunId:
            nested.parent_run_id != null || nested.parentRunId != null
              ? String(nested.parent_run_id ?? nested.parentRunId)
              : undefined,
          defaultBinding: defaultBinding ? defaultBinding : null,
          tasks,
        };
      } else {
        interaction = {
          kind: 'ask_user',
          id,
          runId: event.runId,
          conversationId: run.conversationId,
          createdAt: event.timestamp,
          question: {
            prompt: String(p.prompt ?? p.question ?? ''),
            options: Array.isArray(p.options)
              ? (p.options as Array<{ id: string; label: string; description?: string }>)
              : undefined,
            multiSelect: Boolean(p.multi_select ?? p.multiSelect),
            freeText: Boolean(p.free_text ?? p.freeText ?? true),
          },
        };
      }
      break;
    }
    // The engine emits `interaction_responded` (Rust RunEventKind::InteractionResponded);
    // older paths used `interaction_resolved`. Accept both so cross-device /
    // timeout resolves actually dismiss the pending card and unlock the composer.
    case 'interaction_resolved':
    case 'interaction_responded': {
      removeInteractionId = String(p.id ?? p.interaction_id ?? p.interactionId ?? '');
      break;
    }
    case 'artifact_created': {
      // Handled at state level via artifactsByRun
      break;
    }
    case 'completed':
    case 'failed':
    case 'cancelled':
    case 'interrupted': {
      live.blocks = live.blocks.map((b) =>
        b.type === 'reasoning' ? { ...b, live: false } : b,
      );
      if (live.reasoningStartedAt && !live.reasoningFinishedAt) {
        live.reasoningFinishedAt = event.timestamp;
      }
      if (event.type === 'failed') {
        const code = String(p.code ?? p.error_code ?? 'failed');
        const message = String(p.error ?? p.message ?? 'Run failed');
        const hasError = live.blocks.some((b) => b.type === 'error');
        if (!hasError) {
          live.blocks = [
            ...live.blocks,
            {
              type: 'error',
              errorCode: code,
              errorMessage: message,
              text: message,
            },
          ];
        }
      }
      break;
    }
    default:
      break;
  }

  return { live, interaction, removeInteractionId };
}
