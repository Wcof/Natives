/**
 * Pure workspace reducer — single authority for execution UI state.
 * Run terminal status only from snapshot or terminal events.
 * Sequence: ignore dup/out-of-order; gap → recovering.
 */
import type {
  ConversationSnapshot,
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
import { clearSelectorCachesForConversation } from './selectors';
// Live-message slice (W4): streaming live-bubble projection + lifecycle rank
// helpers moved to `reducer-live` to keep this file a thin composition root.
import { applyEventToLive, runStatusRank, statusFromEventType } from './reducer-live';

const TERMINAL_EVENT_TYPES = new Set(['completed', 'failed', 'cancelled', 'interrupted']);

/**
 * Max number of TERMINAL runs whose event buffers stay resident.
 * Per-run buffers are already capped at 2000 events, but run keys were never
 * evicted, so long sessions grew without bound (P0-13). Active runs are never
 * evicted; `lastSequenceByRun` is kept so late duplicates still dedupe.
 *
 * NOTE(P0-13, messages): `messages` / `messagesByConversation` also only grow.
 * They cannot be LRU-evicted safely here because messagesByConversation order
 * is the timeline source of truth and messagePageInfoByConversation cursors
 * assume a contiguous prefix; dropping mid-list ids would corrupt pagination
 * ("load earlier") and copy paths. Deferred to a follow-up round with a
 * per-conversation windowing design that keeps page cursors consistent.
 */
const TERMINAL_RUN_EVENT_CACHE = 8;

/** Evict event buffers of the least-recently-finished terminal runs (LRU). */
function evictTerminalRunEvents(state: AssistantWorkspaceState): AssistantWorkspaceState {
  const terminalIds = Object.keys(state.eventsByRun).filter((id) => {
    const run = state.runs[id];
    return run ? isTerminalRunStatus(run.status) : false;
  });
  if (terminalIds.length <= TERMINAL_RUN_EVENT_CACHE) return state;
  const newestFirst = terminalIds.sort((a, b) => {
    const fa = state.runs[a]?.finishedAt ?? '';
    const fb = state.runs[b]?.finishedAt ?? '';
    if (fa !== fb) return fb.localeCompare(fa);
    return (state.runs[b]?.lastEventSequence ?? 0) - (state.runs[a]?.lastEventSequence ?? 0);
  });
  const eventsByRun = { ...state.eventsByRun };
  for (const id of newestFirst.slice(TERMINAL_RUN_EVENT_CACHE)) {
    delete eventsByRun[id];
  }
  return { ...state, eventsByRun };
}

function mergeLiveIntoMessages(
  state: AssistantWorkspaceState,
  run: Run,
  live: LiveBubble,
  terminal: boolean,
): AssistantWorkspaceState {
  if (!terminal) {
    return { ...state, liveByRun: { ...state.liveByRun, [run.id]: live } };
  }

  // Promote live bubble to a persisted assistant message once terminal.
  // Completed tools stay in eventsByRun (activity panel), not the answer body.
  const msgId = live.messageId;
  const message: Message = {
    id: msgId,
    conversationId: run.conversationId,
    role: 'assistant',
    status: run.status,
    createdAt: run.startedAt ?? new Date().toISOString(),
    contentBlocks: live.blocks
      .filter((b) => b.type !== 'tool_call' && b.type !== 'tool_result')
      .map((b) => (b.type === 'reasoning' ? { ...b, live: false } : b)),
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

  const last = state.lastSequenceByRun[runId] ?? 0;

  // Out of order (older than last without being the next): ignore if not gap fill
  if (event.sequence <= last) {
    return state;
  }

  // Sequence gap detection (expected last+1)
  if (event.sequence > last + 1 && last > 0) {
    // Run-level only — do not flip global ConnectionBanner to "recovering".
    return {
      ...state,
      recoveringRuns: { ...state.recoveringRuns, [runId]: true },
    };
  }

  const retained = [...(state.eventsByRun[runId] ?? []), event].slice(-2000);
  let next: AssistantWorkspaceState = {
    ...state,
    lastSequenceByRun: { ...state.lastSequenceByRun, [runId]: event.sequence },
    eventsByRun: {
      ...state.eventsByRun,
      [runId]: retained,
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
  // Never invent terminal status without terminal event; never regress lifecycle.
  if (statusPatch) {
    const canApply =
      (isTerminalRunStatus(statusPatch) && !isTerminalRunStatus(run.status)) ||
      (!isTerminalRunStatus(run.status) &&
        !isTerminalRunStatus(statusPatch) &&
        runStatusRank(statusPatch) >= runStatusRank(run.status)) ||
      (isTerminalRunStatus(run.status) && isTerminalRunStatus(statusPatch));
    if (canApply) {
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
    } else {
      run = { ...run, lastEventSequence: event.sequence };
    }
  } else {
    run = { ...run, lastEventSequence: event.sequence };
  }

  next = {
    ...next,
    runs: { ...next.runs, [runId]: run },
    activeRunByConversation: {
      ...next.activeRunByConversation,
      // Terminal runs stay selectable for inspector, but status bar gates on isActive.
      [run.conversationId]: isTerminalRunStatus(run.status)
        ? next.activeRunByConversation[run.conversationId] ?? runId
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

  if (event.type === 'context_usage_updated' && run.conversationId) {
    next = {
      ...next,
      contextUsageByConversation: {
        ...next.contextUsageByConversation,
        [run.conversationId]: {
          conversationId: run.conversationId,
          usedTokens: Number(event.payload.used_tokens ?? event.payload.usedTokens ?? 0),
          maxTokens: Number(event.payload.max_tokens ?? event.payload.maxTokens ?? 128000),
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

  // Run reached a terminal state: pending interactions bound to this run can
  // never be answered anymore — drop them so cards do not linger and lock the
  // composer (P0-5), and evict event buffers of old terminal runs (P0-13).
  if (TERMINAL_EVENT_TYPES.has(String(event.type))) {
    const staleIds = next.interactionOrder.filter(
      (id) => next.interactions[id]?.runId === runId,
    );
    if (staleIds.length > 0) {
      const stale = new Set(staleIds);
      const interactions = { ...next.interactions };
      for (const id of staleIds) delete interactions[id];
      next = {
        ...next,
        interactions,
        interactionOrder: next.interactionOrder.filter((id) => !stale.has(id)),
      };
    }
    next = evictTerminalRunEvents(next);
    // Terminal event: clear run-level errors — the fold shows the real terminal
    // status/error from the daemon now.
    if (next.runErrors[runId]) {
      const runErrors = { ...next.runErrors };
      delete runErrors[runId];
      next = { ...next, runErrors };
    }
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
    messagePageInfoByConversation: snapshot.messagePageInfo
      ? { ...next.messagePageInfoByConversation, [c.id]: { hasMore: snapshot.messagePageInfo.hasMore, nextCursor: snapshot.messagePageInfo.nextCursor } }
      : next.messagePageInfoByConversation,
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
      // Never fall back to runs[0] (often completed).
      [c.id]: snapshot.activeRunId ?? null,
    },
  };

  if (snapshot.eventsByRun) {
    // Reset sequence watermarks and event buffers for runs included in this
    // snapshot so that applyOneEvent never silently drops them.
    // Without this, returning to a conversation re-applies the same events
    // but they are all skipped because sequence <= lastSequenceByRun.
    const lastSequenceByRun = { ...next.lastSequenceByRun };
    const eventsByRun = { ...next.eventsByRun };
    for (const runId of Object.keys(snapshot.eventsByRun)) {
      delete lastSequenceByRun[runId];
      delete eventsByRun[runId];
    }
    next = { ...next, lastSequenceByRun, eventsByRun };

    for (const events of Object.values(snapshot.eventsByRun)) {
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
      // Drop selector memo entries for this conversation + its known runs.
      const runIds = Object.values(state.runs)
        .filter((r) => r.conversationId === action.id)
        .map((r) => r.id);
      clearSelectorCachesForConversation(action.id, runIds);
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
      if (state.activeConversationId === action.id) return state;
      return { ...state, activeConversationId: action.id };

    case 'snapshot/apply':
      return applySnapshot(state, action.snapshot);

    case 'messages/prependPage': {
      const existing = state.messagesByConversation[action.conversationId] ?? [];
      const incoming = action.messages.filter((message) => !state.messages[message.id]);
      const messages = { ...state.messages };
      for (const message of incoming) messages[message.id] = message;
      return {
        ...state,
        messages,
        messagesByConversation: {
          ...state.messagesByConversation,
          [action.conversationId]: [...incoming.map((message) => message.id), ...existing],
        },
        messagePageInfoByConversation: {
          ...state.messagePageInfoByConversation,
          [action.conversationId]: action.pageInfo,
        },
      };
    }

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
      // Run-level only: sequence-gap / soft recovery must not flip the global
      // ConnectionBanner to "recovering" (that looked like a full disconnect).
      const recoveringRuns = { ...state.recoveringRuns };
      if (action.recovering) recoveringRuns[action.runId] = true;
      else delete recoveringRuns[action.runId];
      return {
        ...state,
        recoveringRuns,
        connection:
          Object.keys(recoveringRuns).length === 0 && state.connection === 'recovering'
            ? 'connected'
            : state.connection,
      };
    }

    case 'run/error/set': {
      // Run-level transport/watch error. Stored on the run (ActivityInspector
      // fold), never surfaced verbatim in the global banner (product decision 4).
      const runErrors = { ...state.runErrors };
      if (action.error) runErrors[action.runId] = action.error;
      else delete runErrors[action.runId];
      return { ...state, runErrors };
    }

    case 'projection/recovery':
      return {
        ...state,
        projectionRecoveryByRun: {
          ...state.projectionRecoveryByRun,
          [action.runId]: action.recovery,
        },
      };

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
        attachments: [] as Array<{
          path: string;
          name: string;
          mimeType?: string;
          size?: number;
        }>,
        updatedAt: '',
      };
      const nextText = action.draft.text !== undefined ? action.draft.text : prev.text;
      const nextAttachments =
        action.draft.attachments !== undefined ? action.draft.attachments : prev.attachments;
      // Bail out when content is unchanged — MessageInput can re-dispatch the same
      // draft text after store ticks; always writing updatedAt caused effect storms.
      const textSame = nextText === prev.text;
      const attachmentsSame =
        nextAttachments === prev.attachments ||
        (nextAttachments.length === prev.attachments.length &&
          nextAttachments.every(
            (item, index) =>
              item.path === prev.attachments[index]?.path &&
              item.name === prev.attachments[index]?.name &&
              item.size === prev.attachments[index]?.size &&
              item.mimeType === prev.attachments[index]?.mimeType,
          ));
      if (textSame && attachmentsSame) {
        return state;
      }
      return {
        ...state,
        composerByConversation: {
          ...state.composerByConversation,
          [action.conversationId]: {
            text: nextText,
            attachments: nextAttachments,
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

    case 'capabilitySelection/set': {
      // Idempotent: same reference (incl. null → null) keeps state identity.
      if (state.capabilitySelectionByConversation[action.conversationId] === action.selection) {
        return state;
      }
      return {
        ...state,
        capabilitySelectionByConversation: {
          ...state.capabilitySelectionByConversation,
          [action.conversationId]: action.selection,
        },
      };
    }

    case 'view/patch': {
      // Bail out when every provided field already matches — layout effects
      // dispatch layoutBreakpoint on mount/resize and must not force a new
      // state identity when nothing changed (avoids update-depth cascades).
      const patch = action.patch;
      let changed = false;
      for (const key of Object.keys(patch) as Array<keyof typeof patch>) {
        if (state.view[key] !== patch[key]) {
          changed = true;
          break;
        }
      }
      if (!changed) return state;
      return { ...state, view: { ...state.view, ...patch } };
    }

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

    case 'messages/remove': {
      const order = state.messagesByConversation[action.conversationId] ?? [];
      if (!order.includes(action.id) && !state.messages[action.id]) return state;
      const messages = { ...state.messages };
      delete messages[action.id];
      return {
        ...state,
        messages,
        messagesByConversation: {
          ...state.messagesByConversation,
          [action.conversationId]: order.filter((id) => id !== action.id),
        },
      };
    }

    case 'run/clearActive': {
      const currentId = state.activeRunByConversation[action.conversationId];
      if (action.runId && currentId && currentId !== action.runId) return state;
      const runs = { ...state.runs };
      const liveByRun = { ...state.liveByRun };
      if (action.runId) {
        delete runs[action.runId];
        delete liveByRun[action.runId];
      } else if (currentId) {
        delete runs[currentId];
        delete liveByRun[currentId];
      }
      return {
        ...state,
        runs,
        liveByRun,
        activeRunByConversation: {
          ...state.activeRunByConversation,
          [action.conversationId]: null,
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
