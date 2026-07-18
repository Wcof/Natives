/**
 * Pure workspace reducer — single authority for execution UI state.
 * Run terminal status only from snapshot or terminal events.
 * Sequence: ignore dup/out-of-order; gap → recovering.
 */
import type {
  ContentBlock,
  ConversationSnapshot,
  InteractionRequest,
  Message,
  Run,
  RunEvent,
  RunStatus,
} from '@/lib/assistant-protocol';
import { isTerminalRunStatus } from '@/lib/assistant-protocol';
import {
  createInitialWorkspaceState,
  type AssistantWorkspaceState,
  type LiveBubble,
  type WorkspaceAction,
} from './state';

function statusFromEventType(type: string): RunStatus | null {
  switch (type) {
    case 'queued':
      return 'queued';
    case 'preparing':
      return 'preparing';
    case 'started':
    case 'text_delta':
    case 'reasoning_delta':
    case 'tool_call_requested':
    case 'tool_call_started':
    case 'tool_call_delta':
    case 'tool_call_completed':
    case 'progress':
      // progress.message may signal background_watching from engine
      return null;
    case 'usage_updated':
    case 'file_changed':
    case 'permission_responded':
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
  };
}

function upsertTextBlock(blocks: ContentBlock[], text: string): ContentBlock[] {
  const next = [...blocks];
  const idx = next.findIndex((b) => b.type === 'text');
  if (idx >= 0) {
    const cur = next[idx]!;
    next[idx] = { ...cur, text: `${cur.text ?? ''}${text}` };
  } else {
    next.push({ type: 'text', text });
  }
  return next;
}

function upsertReasoning(blocks: ContentBlock[], text: string): ContentBlock[] {
  const next = [...blocks];
  const idx = next.findIndex((b) => b.type === 'reasoning');
  if (idx >= 0) {
    const cur = next[idx]!;
    next[idx] = { ...cur, reasoning: `${cur.reasoning ?? ''}${text}`, live: true };
  } else {
    next.unshift({ type: 'reasoning', reasoning: text, live: true });
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

function applyEventToLive(
  state: AssistantWorkspaceState,
  event: RunEvent,
  run: Run,
): { live: LiveBubble; interaction?: InteractionRequest; removeInteractionId?: string } {
  const live = { ...ensureLive(state, run), blocks: [...ensureLive(state, run).blocks] };
  const p = event.payload;
  let interaction: InteractionRequest | undefined;
  let removeInteractionId: string | undefined;

  switch (event.type) {
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
    case 'reasoning_delta': {
      const text = String(p.text ?? p.reasoning ?? '');
      live.reasoningStartedAt = live.reasoningStartedAt ?? event.timestamp;
      live.blocks = upsertReasoning(live.blocks, text);
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
      const id = String(p.id ?? p.interaction_id ?? '');
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
          files: Array.isArray(p.files) ? (p.files as ConflictFiles) : [],
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
            options: Array.isArray(p.options) ? (p.options as AskOpts) : undefined,
            multiSelect: Boolean(p.multi_select ?? p.multiSelect),
            freeText: Boolean(p.free_text ?? p.freeText ?? true),
          },
        };
      }
      break;
    }
    case 'interaction_resolved': {
      removeInteractionId = String(p.id ?? p.interaction_id ?? '');
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
      break;
    }
    default:
      break;
  }

  return { live, interaction, removeInteractionId };
}

type ConflictFiles = Array<{ path: string; base?: string; ours?: string; theirs?: string }>;
type AskOpts = Array<{ id: string; label: string; description?: string }>;

function mergeLiveIntoMessages(
  state: AssistantWorkspaceState,
  run: Run,
  live: LiveBubble,
  terminal: boolean,
): AssistantWorkspaceState {
  if (!terminal) {
    return { ...state, liveByRun: { ...state.liveByRun, [run.id]: live } };
  }

  // Promote live bubble to a persisted assistant message once terminal
  const msgId = live.messageId;
  const message: Message = {
    id: msgId,
    conversationId: run.conversationId,
    role: 'assistant',
    status: run.status,
    createdAt: run.startedAt ?? new Date().toISOString(),
    contentBlocks: live.blocks.map((b) =>
      b.type === 'reasoning' ? { ...b, live: false } : b,
    ),
    runId: run.id,
  };
  const order = state.messagesByConversation[run.conversationId] ?? [];
  const has = order.includes(msgId);
  const messages = { ...state.messages, [msgId]: message };
  const messagesByConversation = {
    ...state.messagesByConversation,
    [run.conversationId]: has ? order : [...order, msgId],
  };
  const liveByRun = { ...state.liveByRun };
  delete liveByRun[run.id];
  return { ...state, messages, messagesByConversation, liveByRun };
}

function applyOneEvent(
  state: AssistantWorkspaceState,
  event: RunEvent,
): AssistantWorkspaceState {
  const runId = event.runId;
  if (!runId || !Number.isFinite(event.sequence)) return state;

  const seen = state.seenSequencesByRun[runId] ?? {};
  if (seen[event.sequence]) {
    // Idempotent: ignore duplicates
    return state;
  }

  const last = state.lastSequenceByRun[runId] ?? 0;

  // Out of order (older than last without being the next): ignore if not gap fill
  if (event.sequence <= last) {
    return state;
  }

  // Sequence gap detection (expected last+1)
  if (event.sequence > last + 1 && last > 0) {
    return {
      ...state,
      recoveringRuns: { ...state.recoveringRuns, [runId]: true },
      // Gap is always a recover path; keep offline/fatal/incompatible as-is
      connection:
        state.connection === 'offline' ||
        state.connection === 'fatal' ||
        state.connection === 'incompatible'
          ? state.connection
          : 'recovering',
    };
  }

  const nextSeen = { ...seen, [event.sequence]: true as const };
  let next: AssistantWorkspaceState = {
    ...state,
    seenSequencesByRun: { ...state.seenSequencesByRun, [runId]: nextSeen },
    lastSequenceByRun: { ...state.lastSequenceByRun, [runId]: event.sequence },
    eventsByRun: {
      ...state.eventsByRun,
      [runId]: [...(state.eventsByRun[runId] ?? []), event],
    },
  };

  // Ensure run record exists
  let run = next.runs[runId];
  if (!run) {
    run = {
      id: runId,
      conversationId: String(event.payload.conversation_id ?? ''),
      status: 'running',
      providerId: '',
      modelId: '',
      permissionProfile: 'ask',
      lastEventSequence: event.sequence,
    };
  }

  let statusPatch = statusFromEventType(String(event.type));
  // Engine may report background via progress.message
  if (
    event.type === 'progress' &&
    String(event.payload.message ?? '') === 'background_watching'
  ) {
    statusPatch = 'background_watching';
  }
  // Never invent terminal status without terminal event
  if (statusPatch) {
    // Don't regress terminal runs
    if (!isTerminalRunStatus(run.status) || isTerminalRunStatus(statusPatch)) {
      run = {
        ...run,
        status: statusPatch,
        lastEventSequence: event.sequence,
        finishedAt: isTerminalRunStatus(statusPatch)
          ? event.timestamp
          : run.finishedAt,
        background: statusPatch === 'background_watching' ? true : run.background,
        errorCode:
          event.type === 'failed'
            ? String(event.payload.code ?? event.payload.error_code ?? 'failed')
            : run.errorCode,
        errorMessage:
          event.type === 'failed'
            ? String(event.payload.error ?? event.payload.message ?? '')
            : run.errorMessage,
        activity:
          event.type === 'progress'
            ? String(event.payload.message ?? run.activity ?? '')
            : event.type === 'tool_call_started' || event.type === 'tool_started'
              ? String(event.payload.name ?? event.payload.tool_name ?? run.activity ?? '')
              : run.activity,
      };
    }
  } else {
    run = { ...run, lastEventSequence: event.sequence };
  }

  next = {
    ...next,
    runs: { ...next.runs, [runId]: run },
    activeRunByConversation: {
      ...next.activeRunByConversation,
      [run.conversationId]:
        isTerminalRunStatus(run.status)
          ? next.activeRunByConversation[run.conversationId] === runId
            ? runId
            : next.activeRunByConversation[run.conversationId] ?? runId
          : runId,
    },
  };

  // File changes
  if (event.type === 'file_changed') {
    const change = {
      path: String(event.payload.path ?? ''),
      changeType: String(event.payload.change_type ?? event.payload.changeType ?? 'modified'),
      runId,
    };
    next = {
      ...next,
      fileChangesByRun: {
        ...next.fileChangesByRun,
        [runId]: [...(next.fileChangesByRun[runId] ?? []), change],
      },
    };
  }

  // Usage
  if (event.type === 'usage_updated' && run.conversationId) {
    const used =
      Number(event.payload.input_tokens ?? 0) + Number(event.payload.output_tokens ?? 0);
    next = {
      ...next,
      contextUsageByConversation: {
        ...next.contextUsageByConversation,
        [run.conversationId]: {
          conversationId: run.conversationId,
          usedTokens: used,
          maxTokens: Number(event.payload.max_tokens ?? 128000),
        },
      },
    };
  }

  // Subagent
  if (event.type === 'subagent_created') {
    const subId = String(event.payload.sub_run_id ?? event.payload.subRunId ?? '');
    const summary = {
      id: subId,
      parentRunId: runId,
      status: 'running' as RunStatus,
      task: String(event.payload.task ?? ''),
      agentProfileId: event.payload.agent_profile_id != null
        ? String(event.payload.agent_profile_id)
        : null,
    };
    const children = next.childRunsByParent[runId] ?? [];
    next = {
      ...next,
      childRunsByParent: {
        ...next.childRunsByParent,
        [runId]: children.includes(subId) ? children : [...children, subId],
      },
      childSummaries: { ...next.childSummaries, [subId]: summary },
      runs: {
        ...next.runs,
        [subId]: next.runs[subId] ?? {
          id: subId,
          conversationId: run.conversationId,
          status: 'running',
          parentRunId: runId,
          providerId: run.providerId,
          modelId: run.modelId,
          permissionProfile: run.permissionProfile,
          activity: summary.task,
        },
      },
    };
  }
  if (event.type === 'subagent_completed' || event.type === 'subagent_failed') {
    const subId = String(event.payload.sub_run_id ?? event.payload.subRunId ?? '');
    const prev = next.childSummaries[subId];
    if (prev) {
      next = {
        ...next,
        childSummaries: {
          ...next.childSummaries,
          [subId]: {
            ...prev,
            status: event.type === 'subagent_completed' ? 'completed' : 'failed',
          },
        },
        runs: next.runs[subId]
          ? {
              ...next.runs,
              [subId]: {
                ...next.runs[subId]!,
                status: event.type === 'subagent_completed' ? 'completed' : 'failed',
              },
            }
          : next.runs,
      };
    }
  }

  // Artifacts
  if (event.type === 'artifact_created') {
    const art = {
      id: String(event.payload.id ?? event.payload.artifact_id ?? `art-${event.sequence}`),
      runId,
      path: String(event.payload.path ?? ''),
      label: event.payload.label != null ? String(event.payload.label) : undefined,
      kind: String(event.payload.kind ?? 'file'),
      size: Number(event.payload.size ?? 0),
    };
    next = {
      ...next,
      artifactsByRun: {
        ...next.artifactsByRun,
        [runId]: [...(next.artifactsByRun[runId] ?? []), art],
      },
    };
  }

  // Prompt queue server truth
  if (event.type === 'prompt_queue_updated' && run.conversationId) {
    const items = Array.isArray(event.payload.items)
      ? (event.payload.items as AssistantWorkspaceState['promptQueues'][string])
      : next.promptQueues[run.conversationId];
    if (items) {
      next = {
        ...next,
        promptQueues: { ...next.promptQueues, [run.conversationId]: items },
      };
    }
  }

  const { live, interaction, removeInteractionId } = applyEventToLive(next, event, run);

  if (interaction) {
    next = {
      ...next,
      interactions: { ...next.interactions, [interaction.id]: interaction },
      interactionOrder: next.interactionOrder.includes(interaction.id)
        ? next.interactionOrder
        : [...next.interactionOrder, interaction.id],
    };
  }
  if (removeInteractionId) {
    const interactions = { ...next.interactions };
    delete interactions[removeInteractionId];
    next = {
      ...next,
      interactions,
      interactionOrder: next.interactionOrder.filter((id) => id !== removeInteractionId),
    };
  }

  next = mergeLiveIntoMessages(next, next.runs[runId]!, live, isTerminalRunStatus(next.runs[runId]!.status));

  // Clear recovering if we advanced continuously
  if (next.recoveringRuns[runId] && event.sequence === last + 1) {
    const recoveringRuns = { ...next.recoveringRuns };
    delete recoveringRuns[runId];
    next = {
      ...next,
      recoveringRuns,
      connection:
        Object.keys(recoveringRuns).length === 0 && next.connection === 'recovering'
          ? 'connected'
          : next.connection,
    };
  }

  return next;
}

function applySnapshot(
  state: AssistantWorkspaceState,
  snapshot: ConversationSnapshot,
): AssistantWorkspaceState {
  let next = { ...state };
  const c = snapshot.conversation;
  next = {
    ...next,
    conversations: { ...next.conversations, [c.id]: c },
    conversationOrder: next.conversationOrder.includes(c.id)
      ? next.conversationOrder
      : [c.id, ...next.conversationOrder],
    capabilities: snapshot.capabilities ?? next.capabilities,
  };

  const messages = { ...next.messages };
  const order: string[] = [];
  for (const m of snapshot.messages) {
    messages[m.id] = m;
    order.push(m.id);
  }
  next = {
    ...next,
    messages,
    messagesByConversation: { ...next.messagesByConversation, [c.id]: order },
  };

  const runs = { ...next.runs };
  for (const r of snapshot.runs) {
    runs[r.id] = r;
  }
  next = {
    ...next,
    runs,
    activeRunByConversation: {
      ...next.activeRunByConversation,
      [c.id]: snapshot.activeRunId ?? snapshot.runs[0]?.id ?? null,
    },
  };

  if (snapshot.eventsByRun) {
    for (const [runId, events] of Object.entries(snapshot.eventsByRun)) {
      // Apply via event pipeline for consistency (sorted)
      const sorted = [...events].sort((a, b) => a.sequence - b.sequence);
      for (const e of sorted) {
        next = applyOneEvent(next, e);
      }
    }
  }

  if (snapshot.interactions) {
    const interactions = { ...next.interactions };
    const orderIds = [...next.interactionOrder];
    for (const i of snapshot.interactions) {
      interactions[i.id] = i;
      if (!orderIds.includes(i.id)) orderIds.push(i.id);
    }
    next = { ...next, interactions, interactionOrder: orderIds };
  }

  if (snapshot.promptQueue) {
    next = {
      ...next,
      promptQueues: { ...next.promptQueues, [c.id]: snapshot.promptQueue },
    };
  }

  if (snapshot.artifacts) {
    const byRun = { ...next.artifactsByRun };
    for (const a of snapshot.artifacts) {
      byRun[a.runId] = [...(byRun[a.runId] ?? []).filter((x) => x.id !== a.id), a];
    }
    next = { ...next, artifactsByRun: byRun };
  }

  if (snapshot.children) {
    const childSummaries = { ...next.childSummaries };
    const childRunsByParent = { ...next.childRunsByParent };
    for (const ch of snapshot.children) {
      childSummaries[ch.id] = ch;
      const list = childRunsByParent[ch.parentRunId] ?? [];
      childRunsByParent[ch.parentRunId] = list.includes(ch.id) ? list : [...list, ch.id];
    }
    next = { ...next, childSummaries, childRunsByParent };
  }

  if (snapshot.contextUsage) {
    next = {
      ...next,
      contextUsageByConversation: {
        ...next.contextUsageByConversation,
        [c.id]: snapshot.contextUsage,
      },
    };
  }

  return next;
}

export function workspaceReducer(
  state: AssistantWorkspaceState,
  action: WorkspaceAction,
): AssistantWorkspaceState {
  switch (action.type) {
    case 'connection/set':
      return {
        ...state,
        connection: action.connection,
        connectionError: action.error === undefined ? state.connectionError : action.error,
        reconnectAttempts:
          action.reconnectAttempts === undefined
            ? state.reconnectAttempts
            : action.reconnectAttempts,
      };

    case 'capabilities/set':
      return { ...state, capabilities: action.capabilities };

    case 'conversations/replace': {
      const conversations: Record<string, (typeof action.conversations)[0]> = {};
      const order: string[] = [];
      for (const c of action.conversations) {
        conversations[c.id] = c;
        order.push(c.id);
      }
      return { ...state, conversations, conversationOrder: order };
    }

    case 'conversations/upsert': {
      const exists = Boolean(state.conversations[action.conversation.id]);
      return {
        ...state,
        conversations: {
          ...state.conversations,
          [action.conversation.id]: action.conversation,
        },
        conversationOrder: exists
          ? state.conversationOrder
          : [action.conversation.id, ...state.conversationOrder],
      };
    }

    case 'conversations/remove': {
      const conversations = { ...state.conversations };
      delete conversations[action.id];
      return {
        ...state,
        conversations,
        conversationOrder: state.conversationOrder.filter((id) => id !== action.id),
        activeConversationId:
          state.activeConversationId === action.id ? null : state.activeConversationId,
      };
    }

    case 'conversations/setActive':
      // Switching conversation must NOT clear other runs/messages
      return { ...state, activeConversationId: action.id };

    case 'snapshot/apply':
      return applySnapshot(state, action.snapshot);

    case 'run/upsert': {
      const run = action.run;
      return {
        ...state,
        runs: { ...state.runs, [run.id]: run },
        activeRunByConversation: {
          ...state.activeRunByConversation,
          [run.conversationId]: isTerminalRunStatus(run.status)
            ? state.activeRunByConversation[run.conversationId] ?? run.id
            : run.id,
        },
      };
    }

    case 'event/apply':
      return applyOneEvent(state, action.event);

    case 'event/applyBatch': {
      let next = state;
      const sorted = [...action.events].sort((a, b) => {
        if (a.runId !== b.runId) return a.runId.localeCompare(b.runId);
        return a.sequence - b.sequence;
      });
      for (const e of sorted) next = applyOneEvent(next, e);
      return next;
    }

    case 'event/replay': {
      // Replay fills gaps: temporarily allow any sequence > seen
      let next: AssistantWorkspaceState = {
        ...state,
        recoveringRuns: { ...state.recoveringRuns, [action.runId]: true },
      };
      const sorted = [...action.events].sort((a, b) => a.sequence - b.sequence);
      for (const e of sorted) {
        // Force apply even if gap was detected — mark last so gap check uses update
        next = applyOneEventForReplay(next, e);
      }
      const recoveringRuns = { ...next.recoveringRuns };
      delete recoveringRuns[action.runId];
      return {
        ...next,
        recoveringRuns,
        connection:
          Object.keys(recoveringRuns).length === 0 && next.connection === 'recovering'
            ? 'connected'
            : next.connection,
      };
    }

    case 'recovering/set': {
      const recoveringRuns = { ...state.recoveringRuns };
      if (action.recovering) recoveringRuns[action.runId] = true;
      else delete recoveringRuns[action.runId];
      return {
        ...state,
        recoveringRuns,
        connection: action.recovering
          ? 'recovering'
          : Object.keys(recoveringRuns).length === 0 && state.connection === 'recovering'
            ? 'connected'
            : state.connection,
      };
    }

    case 'interaction/upsert': {
      const id = action.interaction.id;
      return {
        ...state,
        interactions: { ...state.interactions, [id]: action.interaction },
        interactionOrder: state.interactionOrder.includes(id)
          ? state.interactionOrder
          : [...state.interactionOrder, id],
      };
    }

    case 'interaction/remove': {
      const interactions = { ...state.interactions };
      delete interactions[action.id];
      return {
        ...state,
        interactions,
        interactionOrder: state.interactionOrder.filter((id) => id !== action.id),
      };
    }

    case 'promptQueue/set':
      return {
        ...state,
        promptQueues: { ...state.promptQueues, [action.conversationId]: action.items },
      };

    case 'promptQueue/optimisticEnqueue': {
      const cid = action.item.conversationId;
      const items = state.promptQueues[cid] ?? [];
      // Debounce double-enter: same content at end within same temp pattern
      const last = items[items.length - 1];
      if (
        last &&
        last.content === action.item.content &&
        last.clientTempId &&
        action.item.clientTempId &&
        last.clientTempId !== action.item.clientTempId
      ) {
        // still allow if different temp ids from user intent — only block identical temp
      }
      if (last?.clientTempId && last.clientTempId === action.item.clientTempId) {
        return state;
      }
      return {
        ...state,
        promptQueues: { ...state.promptQueues, [cid]: [...items, action.item] },
      };
    }

    case 'promptQueue/reassociate': {
      const items = state.promptQueues[action.conversationId] ?? [];
      return {
        ...state,
        promptQueues: {
          ...state.promptQueues,
          [action.conversationId]: items.map((it) =>
            it.clientTempId === action.clientTempId || it.id === action.clientTempId
              ? { ...action.serverItem, clientTempId: undefined }
              : it,
          ),
        },
      };
    }

    case 'composer/set': {
      const prev = state.composerByConversation[action.conversationId] ?? {
        text: '',
        attachments: [],
        updatedAt: new Date().toISOString(),
      };
      return {
        ...state,
        composerByConversation: {
          ...state.composerByConversation,
          [action.conversationId]: {
            ...prev,
            ...action.draft,
            updatedAt: new Date().toISOString(),
          },
        },
      };
    }

    case 'composer/clear':
      return {
        ...state,
        composerByConversation: {
          ...state.composerByConversation,
          [action.conversationId]: {
            text: '',
            attachments: [],
            updatedAt: new Date().toISOString(),
          },
        },
      };

    case 'view/patch':
      return { ...state, view: { ...state.view, ...action.patch } };

    case 'view/setBlockExpanded':
      return {
        ...state,
        view: {
          ...state.view,
          blockExpanded: {
            ...state.view.blockExpanded,
            [action.key]: action.expanded,
          },
        },
      };

    case 'messages/appendOptimistic': {
      const m = action.message;
      const order = state.messagesByConversation[m.conversationId] ?? [];
      if (order.includes(m.id)) return state;
      return {
        ...state,
        messages: { ...state.messages, [m.id]: m },
        messagesByConversation: {
          ...state.messagesByConversation,
          [m.conversationId]: [...order, m.id],
        },
      };
    }

    case 'disconnect/soft':
      // Keep timeline + drafts; only flip connection
      return {
        ...state,
        connection: 'offline',
        connectionError: state.connectionError ?? 'disconnected',
      };

    default:
      return state;
  }
}

/**
 * Replay apply: allow filling sequences even when gap would block live path.
 * Still idempotent on sequence.
 */
function applyOneEventForReplay(
  state: AssistantWorkspaceState,
  event: RunEvent,
): AssistantWorkspaceState {
  const runId = event.runId;
  const seen = state.seenSequencesByRun[runId] ?? {};
  if (seen[event.sequence]) return state;

  // Pretend last is event.sequence - 1 so gap check passes
  const patched: AssistantWorkspaceState = {
    ...state,
    lastSequenceByRun: {
      ...state.lastSequenceByRun,
      [runId]: Math.max(state.lastSequenceByRun[runId] ?? 0, event.sequence - 1),
    },
  };
  return applyOneEvent(patched, event);
}

export { createInitialWorkspaceState };
