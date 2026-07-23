import type {
  InteractionRequest,
  Message,
  PromptQueueItem,
  Run,
} from '@/lib/assistant-protocol';
import { isActiveRunStatus, isTerminalRunStatus } from '@/lib/assistant-protocol';
import type { AssistantWorkspaceState, LiveBubble } from './state';

const EMPTY: never[] = [];

export function selectActiveConversation(state: AssistantWorkspaceState) {
  const id = state.activeConversationId;
  return id ? state.conversations[id] ?? null : null;
}

export function selectConversationMessages(
  state: AssistantWorkspaceState,
  conversationId: string | null,
): Message[] {
  if (!conversationId) return [];
  const ids = state.messagesByConversation[conversationId] ?? [];
  const base = ids.map((id) => state.messages[id]).filter(Boolean) as Message[];

  // Append live bubble for active non-terminal run if not already promoted.
  // Even with empty blocks, surface a streaming placeholder so the UI can show
  // "正在思考" instead of looking stuck with no assistant row.
  const runId = state.activeRunByConversation[conversationId];
  if (runId) {
    const run = state.runs[runId];
    const live = state.liveByRun[runId];
    if (run && !isTerminalRunStatus(run.status)) {
      const liveId = live?.messageId ?? `live-${runId}`;
      if (!ids.includes(liveId)) {
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
          createdAt: run.startedAt ?? new Date().toISOString(),
          contentBlocks: live?.blocks ?? [],
          runId,
        });
      }
    }
  }
  return base;
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
  return state.interactionOrder
    .map((id) => state.interactions[id])
    .filter((i): i is InteractionRequest => {
      if (!i) return false;
      if (!conversationId) return true;
      return !i.conversationId || i.conversationId === conversationId;
    });
}

/** Global “waiting for me” across all conversations (badges / notifications). */
export function selectAllPendingInteractions(
  state: AssistantWorkspaceState,
): InteractionRequest[] {
  return selectPendingInteractions(state, null);
}

export function selectPromptQueue(
  state: AssistantWorkspaceState,
  conversationId: string | null,
): PromptQueueItem[] {
  if (!conversationId) return [];
  return state.promptQueues[conversationId] ?? [];
}

export function selectComposerDraft(
  state: AssistantWorkspaceState,
  conversationId: string | null,
) {
  if (!conversationId) return { text: '', attachments: [] as const, updatedAt: '' };
  return (
    state.composerByConversation[conversationId] ?? {
      text: '',
      attachments: [],
      updatedAt: '',
    }
  );
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

export function selectChildRuns(state: AssistantWorkspaceState, parentRunId: string | null) {
  if (!parentRunId) return [];
  const ids = state.childRunsByParent[parentRunId] ?? [];
  return ids
    .map((id) => state.childSummaries[id])
    .filter((c): c is NonNullable<typeof c> => Boolean(c));
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

/** Merge artifacts across a main run and its known child runs. */
export function selectArtifactsForRunTree(
  state: AssistantWorkspaceState,
  rootRunId: string | null,
): import('@/lib/assistant-protocol').Artifact[] {
  if (!rootRunId) return EMPTY;
  const ids = [rootRunId, ...(state.childRunsByParent[rootRunId] ?? [])];
  const seen = new Set<string>();
  const out: import('@/lib/assistant-protocol').Artifact[] = [];
  for (const id of ids) {
    for (const a of state.artifactsByRun[id] ?? []) {
      const key = a.id || `${a.runId}:${a.path}`;
      if (seen.has(key)) continue;
      seen.add(key);
      out.push(a);
    }
  }
  return out;
}

/** Merge file changes across a main run and its known child runs. */
export function selectFileChangesForRunTree(
  state: AssistantWorkspaceState,
  rootRunId: string | null,
): import('@/lib/assistant-protocol').FileChange[] {
  if (!rootRunId) return EMPTY;
  const ids = [rootRunId, ...(state.childRunsByParent[rootRunId] ?? [])];
  const seen = new Set<string>();
  const out: import('@/lib/assistant-protocol').FileChange[] = [];
  for (const id of ids) {
    for (const f of state.fileChangesByRun[id] ?? []) {
      const key = `${f.runId ?? id}:${f.path}:${f.changeType}`;
      if (seen.has(key)) continue;
      seen.add(key);
      out.push(f);
    }
  }
  return out;
}

/** Merge events for root + child runs (ordered by timestamp then sequence). */
export function selectEventsForRunTree(
  state: AssistantWorkspaceState,
  rootRunId: string | null,
): import('@/lib/assistant-protocol').RunEvent[] {
  if (!rootRunId) return EMPTY;
  const ids = [rootRunId, ...(state.childRunsByParent[rootRunId] ?? [])];
  const out: import('@/lib/assistant-protocol').RunEvent[] = [];
  for (const id of ids) {
    out.push(...(state.eventsByRun[id] ?? []));
  }
  out.sort((a, b) => {
    const ta = a.timestamp || '';
    const tb = b.timestamp || '';
    if (ta !== tb) return ta.localeCompare(tb);
    if (a.runId !== b.runId) return a.runId.localeCompare(b.runId);
    return a.sequence - b.sequence;
  });
  return out;
}
