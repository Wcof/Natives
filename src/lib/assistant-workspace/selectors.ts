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
