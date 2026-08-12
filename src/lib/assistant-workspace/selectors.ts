import type {
  InteractionRequest,
  Message,
  PromptQueueItem,
  Run,
} from '@/lib/assistant-protocol';
import { isActiveRunStatus, isTerminalRunStatus } from '@/lib/assistant-protocol';
import type { AssistantWorkspaceState, LiveBubble } from './state';

const EMPTY: never[] = [];
const EMPTY_DRAFT = { text: '', attachments: [] as const, updatedAt: '' };
const EMPTY_BLOCKS: Message['contentBlocks'] = [];

/**
 * Per-conversation memo for selectConversationMessages.
 * Composer / view / unrelated-run ticks must not allocate a fresh messages[]
 * (timeline MessageRow memo + scroll effects depend on array + row identity).
 */
interface MessagesSelectorCacheEntry {
  ids: string[];
  messages: AssistantWorkspaceState['messages'];
  runId: string | null;
  runStatus: string | null;
  runStartedAt: string | null;
  live: LiveBubble | null | undefined;
  includeLive: boolean;
  result: Message[];
}
const messagesSelectorCache = new Map<string, MessagesSelectorCacheEntry>();

/** Max entries for each global selector cache (conversation / run keyed). */
const SELECTOR_CACHE_MAX = 128;

function cacheSet<K, V>(map: Map<K, V>, key: K, value: V): void {
  if (map.has(key)) {
    map.set(key, value);
    return;
  }
  if (map.size >= SELECTOR_CACHE_MAX) {
    const oldest = map.keys().next().value;
    if (oldest !== undefined) map.delete(oldest);
  }
  map.set(key, value);
}

interface PendingInteractionsCacheEntry {
  order: string[];
  interactions: AssistantWorkspaceState['interactions'];
  result: InteractionRequest[];
}
const pendingInteractionsCache = new Map<string, PendingInteractionsCacheEntry>();

/**
 * Drop selector cache entries for a conversation (and optional run ids).
 * Call when a conversation is deleted so the 128-cap does not retain stale rows.
 */
export function clearSelectorCachesForConversation(
  conversationId: string,
  runIds: string[] = [],
): void {
  messagesSelectorCache.delete(conversationId);
  pendingInteractionsCache.delete(conversationId);
  pendingInteractionsCache.delete('__all__');
  for (const runId of runIds) {
    childRunsCache.delete(runId);
    artifactsTreeCache.delete(runId);
    fileChangesTreeCache.delete(runId);
    eventsTreeCache.delete(runId);
  }
}

export function selectActiveConversation(state: AssistantWorkspaceState) {
  const id = state.activeConversationId;
  return id ? state.conversations[id] ?? null : null;
}

export function selectConversationMessages(
  state: AssistantWorkspaceState,
  conversationId: string | null,
): Message[] {
  if (!conversationId) return EMPTY;
  const ids = state.messagesByConversation[conversationId] ?? EMPTY;
  const runId = state.activeRunByConversation[conversationId] ?? null;
  const run = runId ? state.runs[runId] ?? null : null;
  const live = runId ? state.liveByRun[runId] : undefined;
  const runActive = Boolean(run && !isTerminalRunStatus(run.status));
  const liveId = runActive ? (live?.messageId ?? `live-${runId}`) : null;
  const includeLive = Boolean(liveId && !ids.includes(liveId));

  const prev = messagesSelectorCache.get(conversationId);
  if (
    prev &&
    prev.ids === ids &&
    prev.messages === state.messages &&
    prev.runId === runId &&
    prev.runStatus === (run?.status ?? null) &&
    prev.runStartedAt === (run?.startedAt ?? null) &&
    prev.live === live &&
    prev.includeLive === includeLive
  ) {
    return prev.result;
  }

  // No messages and no streaming placeholder → shared EMPTY (effect-dep stable).
  if (ids.length === 0 && !includeLive) {
    cacheSet(messagesSelectorCache, conversationId, {
      ids,
      messages: state.messages,
      runId,
      runStatus: run?.status ?? null,
      runStartedAt: run?.startedAt ?? null,
      live,
      includeLive: false,
      result: EMPTY,
    });
    return EMPTY;
  }

  const base = ids.map((id) => state.messages[id]).filter(Boolean) as Message[];

  // Append live bubble for active non-terminal run if not already promoted.
  // Even with empty blocks, surface a streaming placeholder so the UI can show
  // "正在思考" instead of looking stuck with no assistant row.
  if (includeLive && run && runId && liveId) {
    base.push({
      id: liveId,
      conversationId,
      role: 'assistant',
      status:
        run.status === 'running' ||
        run.status === 'reasoning' ||
        run.status === 'preparing' ||
        run.status === 'queued'
          ? 'streaming'
          : run.status,
      // Prefer run.startedAt so createdAt does not churn every selector call.
      // Fall back to a fixed epoch rather than Date.now() — churn here forces
      // timeline message identity changes and effect storms.
      createdAt: run.startedAt ?? '1970-01-01T00:00:00.000Z',
      contentBlocks: live?.blocks ?? EMPTY_BLOCKS,
      runId,
    });
  }

  const result = base.length === 0 ? EMPTY : base;
  cacheSet(messagesSelectorCache, conversationId, {
    ids,
    messages: state.messages,
    runId,
    runStatus: run?.status ?? null,
    runStartedAt: run?.startedAt ?? null,
    live,
    includeLive,
    result,
  });
  return result;
}

export function selectActiveRun(
  state: AssistantWorkspaceState,
  conversationId: string | null,
): Run | null {
  if (!conversationId) return null;
  const runId = state.activeRunByConversation[conversationId];
  return runId ? state.runs[runId] ?? null : null;
}

export function selectLiveBubble(
  state: AssistantWorkspaceState,
  runId: string | null,
): LiveBubble | null {
  if (!runId) return null;
  return state.liveByRun[runId] ?? null;
}

export function selectPendingInteractions(
  state: AssistantWorkspaceState,
  conversationId?: string | null,
): InteractionRequest[] {
  // 问题3：pending interaction 必须显式绑定到会话才投影到该会话。
  // 无会话上下文（新项目临时会话）时返回空——旧消息/旧工具授权/旧 run
  // 的 interaction 绝不串入任意会话。全局徽章用 selectAllPendingInteractions。
  if (!conversationId) return EMPTY;
  // Cache by filter key so Workbench permission lookups stay referentially stable
  // across composer / stream ticks that do not touch interactions.
  const prev = pendingInteractionsCache.get(conversationId);
  if (
    prev &&
    prev.order === state.interactionOrder &&
    prev.interactions === state.interactions
  ) {
    return prev.result;
  }
  const result = state.interactionOrder
    .map((id) => state.interactions[id])
    .filter((i): i is InteractionRequest => {
      if (!i) return false;
      return i.conversationId === conversationId;
    });
  const stable = result.length === 0 ? (EMPTY as InteractionRequest[]) : result;
  cacheSet(pendingInteractionsCache, conversationId, {
    order: state.interactionOrder,
    interactions: state.interactions,
    result: stable,
  });
  return stable;
}

/**
 * Global "waiting for me" across all conversations (badges / notifications).
 * 独立实现：包含未绑定会话的 interaction，供徽章计数；不经过会话投影过滤。
 */
export function selectAllPendingInteractions(
  state: AssistantWorkspaceState,
): InteractionRequest[] {
  return state.interactionOrder
    .map((id) => state.interactions[id])
    .filter((i): i is InteractionRequest => Boolean(i));
}

export function selectPromptQueue(
  state: AssistantWorkspaceState,
  conversationId: string | null,
): PromptQueueItem[] {
  if (!conversationId) return EMPTY;
  return state.promptQueues[conversationId] ?? EMPTY;
}

export function selectComposerDraft(
  state: AssistantWorkspaceState,
  conversationId: string | null,
) {
  if (!conversationId) return EMPTY_DRAFT;
  return state.composerByConversation[conversationId] ?? EMPTY_DRAFT;
}

export function selectIsRunActive(
  state: AssistantWorkspaceState,
  conversationId: string | null,
): boolean {
  const run = selectActiveRun(state, conversationId);
  return Boolean(run && isActiveRunStatus(run.status));
}

export function selectConversationBadge(
  state: AssistantWorkspaceState,
  conversationId: string,
): 'running' | 'waiting_permission' | 'failed' | 'completed' | 'offline' | null {
  if (state.connection === 'offline' || state.connection === 'fatal') return 'offline';
  const runId = state.activeRunByConversation[conversationId];
  const run = runId ? state.runs[runId] : null;
  if (!run) return null;
  if (run.status === 'waiting_permission' || run.status === 'waiting_user') {
    return 'waiting_permission';
  }
  if (isActiveRunStatus(run.status)) return 'running';
  if (run.status === 'failed') return 'failed';
  if (run.status === 'completed') return 'completed';
  return null;
}

export function selectRunEvents(state: AssistantWorkspaceState, runId: string | null) {
  if (!runId) return EMPTY;
  return state.eventsByRun[runId] ?? EMPTY;
}

interface ChildRunsCacheEntry {
  ids: string[];
  summaries: AssistantWorkspaceState['childSummaries'];
  result: NonNullable<AssistantWorkspaceState['childSummaries'][string]>[];
}
const childRunsCache = new Map<string, ChildRunsCacheEntry>();

export function selectChildRuns(state: AssistantWorkspaceState, parentRunId: string | null) {
  if (!parentRunId) return EMPTY;
  const ids = state.childRunsByParent[parentRunId] ?? EMPTY;
  if (ids.length === 0) return EMPTY;
  const prev = childRunsCache.get(parentRunId);
  if (prev && prev.ids === ids && prev.summaries === state.childSummaries) {
    return prev.result;
  }
  const result = ids
    .map((id) => state.childSummaries[id])
    .filter((c): c is NonNullable<typeof c> => Boolean(c));
  const stable = result.length === 0 ? EMPTY : result;
  cacheSet(childRunsCache, parentRunId, {
    ids,
    summaries: state.childSummaries,
    result: stable as ChildRunsCacheEntry['result'],
  });
  return stable;
}

export function selectArtifacts(state: AssistantWorkspaceState, runId: string | null) {
  if (!runId) return EMPTY;
  return state.artifactsByRun[runId] ?? EMPTY;
}

export function selectFileChanges(state: AssistantWorkspaceState, runId: string | null) {
  if (!runId) return EMPTY;
  return state.fileChangesByRun[runId] ?? EMPTY;
}

/**
 * Surface conversation: child session when selected, otherwise the root.
 * Project list / navigation keep root; timeline/input/permission use surface.
 */
export function selectSurfaceConversationId(
  selectedRootConversationId: string | null | undefined,
  selectedChildConversationId: string | null | undefined,
): string | null {
  const child = selectedChildConversationId?.trim() || null;
  if (child) return child;
  const root = selectedRootConversationId?.trim() || null;
  return root;
}

/**
 * 审计收口 #3：store activeConversationId 是唯一 root authority。
 *
 * 本地 selectedRootConversationId 只应作为 fallback（例如 subagent child 视图
 * 保持树根），绝不能优先于 store active——否则新建项目后第一帧仍会投影旧
 * timeline/授权/run。新项目创建必须通过 reducer 原子切换 store active。
 */
export function resolveSurfaceRoot(
  storeActiveId: string | null | undefined,
  selectedRootConversationId: string | null | undefined,
): string | null {
  const active = storeActiveId?.trim() || null;
  if (active) return active;
  return selectedRootConversationId?.trim() || null;
}

interface ArtifactsTreeCacheEntry {
  childIds: string[];
  arrays: Array<import('@/lib/assistant-protocol').Artifact[] | undefined>;
  result: import('@/lib/assistant-protocol').Artifact[];
}
const artifactsTreeCache = new Map<string, ArtifactsTreeCacheEntry>();

/** Merge artifacts across a main run and its known child runs. */
export function selectArtifactsForRunTree(
  state: AssistantWorkspaceState,
  rootRunId: string | null,
): import('@/lib/assistant-protocol').Artifact[] {
  if (!rootRunId) return EMPTY;
  const childIds = state.childRunsByParent[rootRunId] ?? EMPTY;
  // No children → reuse the root run's stored array (referential stability for effects).
  if (childIds.length === 0) return state.artifactsByRun[rootRunId] ?? EMPTY;

  const arrays: Array<import('@/lib/assistant-protocol').Artifact[] | undefined> = [
    state.artifactsByRun[rootRunId],
    ...childIds.map((id) => state.artifactsByRun[id]),
  ];
  const prev = artifactsTreeCache.get(rootRunId);
  if (
    prev &&
    prev.childIds === childIds &&
    prev.arrays.length === arrays.length &&
    prev.arrays.every((arr, i) => arr === arrays[i])
  ) {
    return prev.result;
  }

  const ids = [rootRunId, ...childIds];
  const seen = new Set<string>();
  const out: import('@/lib/assistant-protocol').Artifact[] = [];
  for (const id of ids) {
    for (const a of state.artifactsByRun[id] ?? EMPTY) {
      const key = a.id || `${a.runId}:${a.path}`;
      if (seen.has(key)) continue;
      seen.add(key);
      out.push(a);
    }
  }
  const result = out.length === 0 ? EMPTY : out;
  cacheSet(artifactsTreeCache, rootRunId, { childIds, arrays, result });
  return result;
}

interface FileChangesTreeCacheEntry {
  childIds: string[];
  arrays: Array<import('@/lib/assistant-protocol').FileChange[] | undefined>;
  result: import('@/lib/assistant-protocol').FileChange[];
}
const fileChangesTreeCache = new Map<string, FileChangesTreeCacheEntry>();

/** Merge file changes across a main run and its known child runs. */
export function selectFileChangesForRunTree(
  state: AssistantWorkspaceState,
  rootRunId: string | null,
): import('@/lib/assistant-protocol').FileChange[] {
  if (!rootRunId) return EMPTY;
  const childIds = state.childRunsByParent[rootRunId] ?? EMPTY;
  if (childIds.length === 0) return state.fileChangesByRun[rootRunId] ?? EMPTY;

  const arrays: Array<import('@/lib/assistant-protocol').FileChange[] | undefined> = [
    state.fileChangesByRun[rootRunId],
    ...childIds.map((id) => state.fileChangesByRun[id]),
  ];
  const prev = fileChangesTreeCache.get(rootRunId);
  if (
    prev &&
    prev.childIds === childIds &&
    prev.arrays.length === arrays.length &&
    prev.arrays.every((arr, i) => arr === arrays[i])
  ) {
    return prev.result;
  }

  const ids = [rootRunId, ...childIds];
  const seen = new Set<string>();
  const out: import('@/lib/assistant-protocol').FileChange[] = [];
  for (const id of ids) {
    for (const f of state.fileChangesByRun[id] ?? EMPTY) {
      const key = `${f.runId ?? id}:${f.path}:${f.changeType}`;
      if (seen.has(key)) continue;
      seen.add(key);
      out.push(f);
    }
  }
  const result = out.length === 0 ? EMPTY : out;
  cacheSet(fileChangesTreeCache, rootRunId, { childIds, arrays, result });
  return result;
}

interface EventsTreeCacheEntry {
  childIds: string[];
  /** Per-run events array identity (root first, then children in childIds order). */
  eventArrays: Array<import('@/lib/assistant-protocol').RunEvent[] | undefined>;
  result: import('@/lib/assistant-protocol').RunEvent[];
}
const eventsTreeCache = new Map<string, EventsTreeCacheEntry>();

/** Merge events for root + child runs (ordered by timestamp then sequence). */
export function selectEventsForRunTree(
  state: AssistantWorkspaceState,
  rootRunId: string | null,
): import('@/lib/assistant-protocol').RunEvent[] {
  if (!rootRunId) return EMPTY;
  const childIds = state.childRunsByParent[rootRunId] ?? EMPTY;
  // Single-run tree: return the stored events array so effect deps stay stable
  // across unrelated store ticks (composer, view, connection banners, …).
  if (childIds.length === 0) return state.eventsByRun[rootRunId] ?? EMPTY;

  const eventArrays: Array<import('@/lib/assistant-protocol').RunEvent[] | undefined> = [
    state.eventsByRun[rootRunId],
    ...childIds.map((id) => state.eventsByRun[id]),
  ];
  const prev = eventsTreeCache.get(rootRunId);
  if (
    prev &&
    prev.childIds === childIds &&
    prev.eventArrays.length === eventArrays.length &&
    prev.eventArrays.every((arr, i) => arr === eventArrays[i])
  ) {
    return prev.result;
  }

  const ids = [rootRunId, ...childIds];
  const out: import('@/lib/assistant-protocol').RunEvent[] = [];
  for (const id of ids) {
    out.push(...(state.eventsByRun[id] ?? EMPTY));
  }
  if (out.length === 0) {
    cacheSet(eventsTreeCache, rootRunId, { childIds, eventArrays, result: EMPTY });
    return EMPTY;
  }
  out.sort((a, b) => {
    const ta = a.timestamp || '';
    const tb = b.timestamp || '';
    if (ta !== tb) return ta.localeCompare(tb);
    if (a.runId !== b.runId) return a.runId.localeCompare(b.runId);
    return a.sequence - b.sequence;
  });
  cacheSet(eventsTreeCache, rootRunId, { childIds, eventArrays, result: out });
  return out;
}
