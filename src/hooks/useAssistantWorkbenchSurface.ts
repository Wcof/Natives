'use client';

/**
 * Surface derivation for the assistant workbench.
 *
 * Computes all conversation/run/event surface values the timeline, composer,
 * and panels read. Root selection (project list / sidebar) and surface
 * selection (timeline / input) are kept separate so inspecting a subagent
 * session never flips the main Todo or root event stream.
 *
 * The orchestrator retains single state authority — setters are passed in,
 * not recreated here.
 */

import { useCallback, useMemo, useState } from 'react';
import {
  selectActiveRun,
  selectArtifacts,
  selectArtifactsForRunTree,
  selectChildRuns,
  selectConversationMessages,
  selectEventsForRunTree,
  selectFileChanges,
  selectFileChangesForRunTree,
  selectIsRunActive,
  selectPendingInteractions,
  selectPromptQueue,
  selectRunEvents,
  selectSurfaceConversationId,
  resolveSurfaceRoot,
  useAssistantDispatch,
  useAssistantGateway,
  useAssistantStore,
} from '@/lib/assistant-workspace';
import {
  resolveModelSelection,
} from '@/lib/provider-model-selection';
import { mapWireMessage } from '@/lib/assistant-protocol';
import type {
  Conversation,
  RunEvent,
  SubagentAssignmentInteraction,
} from '@/lib/assistant-protocol';
import { messagePlainText } from '@/lib/assistant-message-view';
import { extractTodosFromEvents } from '@/lib/assistant-activity-view';
import { summarizeConversationChanges } from '@/lib/assistant-timeline';
import type { ProviderWithModels } from '@/lib/assistant-ui-types';

/** Shared empty child-event list — avoid `[]` literal thrashing useMemo deps. */
const EMPTY_CHILD_EVENTS: RunEvent[] = [];

export interface UseAssistantWorkbenchSurfaceOptions {
  providers: ProviderWithModels[];
  storeActiveId: string | null;
  selectedRootConversationId: string | null;
  selectedChildConversationId: string | null;
}

export interface AssistantSurfaceDerivation {
  surfaceConversationId: string;
  rootConversationId: string | null;
  activeId: string;
  rootConversation: Conversation | null;
  activeConversation: Conversation | null;
  modelSelection: ReturnType<typeof resolveModelSelection>;
  messages: ReturnType<typeof selectConversationMessages>;
  messagePageInfo:
    | { hasMore: boolean; nextCursor: { createdAt: string; id: string } | null }
    | undefined;
  loadingOlderMessages: boolean;
  loadOlderMessages: () => Promise<void>;
  rootRun: ReturnType<typeof selectActiveRun>;
  surfaceRun: ReturnType<typeof selectActiveRun>;
  activeRun: ReturnType<typeof selectActiveRun>;
  activeRunId: string | undefined;
  activeRunStatus: string | undefined;
  isStreaming: boolean;
  interactions: ReturnType<typeof selectPendingInteractions>;
  promptQueue: ReturnType<typeof selectPromptQueue>;
  rootRunId: string | null;
  rootEvents: RunEvent[];
  selectedChildEvents: RunEvent[];
  events: RunEvent[];
  artifacts: ReturnType<typeof selectArtifacts>;
  children: ReturnType<typeof selectChildRuns>;
  fileChanges: ReturnType<typeof selectFileChangesForRunTree>;
  mainTodos: ReturnType<typeof extractTodosFromEvents>;
  selectedChildTodos: ReturnType<typeof extractTodosFromEvents>;
  contextUsage: { usedTokens: number; remaining: number } | null;
  planApproval: ReturnType<typeof selectPendingInteractions>[number] | undefined;
  subagentAssignment: SubagentAssignmentInteraction | undefined;
  conversationChangeSummary: string;
  fileEvents: Array<{
    path: string;
    changeType: string;
    at: number;
    runId: string | null;
  }>;
  isGoalMode: boolean;
  goalInstruction: string | null;
  goalCanResume: boolean;
  timelineMessages: Array<Record<string, unknown>>;
}

export function useAssistantWorkbenchSurface({
  providers,
  storeActiveId,
  selectedRootConversationId,
  selectedChildConversationId,
}: UseAssistantWorkbenchSurfaceOptions) {
  const state = useAssistantStore();
  const dispatch = useAssistantDispatch();
  const gateway = useAssistantGateway();

  const [loadingOlderMessages, setLoadingOlderMessages] = useState(false);

  // 审计收口 #3：store activeConversationId 是唯一 root authority——新建项目
  // 原子切换后第一帧 root 即为新 temp 会话，绝不沿用旧 local root 串旧投影。
  const rootConversationId = resolveSurfaceRoot(storeActiveId, selectedRootConversationId);
  const surfaceConversationId = selectSurfaceConversationId(
    rootConversationId,
    selectedChildConversationId,
  );
  const activeId = surfaceConversationId;
  const rootConversation = rootConversationId
    ? state.conversations[rootConversationId] ?? null
    : null;
  const activeConversation = activeId ? state.conversations[activeId] ?? rootConversation : null;
  // Picker selection is resolved against the live provider list so collapsed
  // /stale provider ids still highlight and empty wire fields still show a model.
  const modelSelection = useMemo(
    () =>
      resolveModelSelection(providers, {
        providerId: activeConversation?.providerId ?? rootConversation?.providerId,
        modelId: activeConversation?.modelId ?? rootConversation?.modelId,
      }),
    [
      providers,
      activeConversation?.providerId,
      activeConversation?.modelId,
      rootConversation?.providerId,
      rootConversation?.modelId,
    ],
  );
  // Slice deps (not whole `state`) so composer/view ticks recompute only when
  // the underlying maps change. Selectors themselves are also identity-cached.
  const messages = useMemo(
    () => selectConversationMessages(state, activeId),
    [
      activeId,
      state.messagesByConversation,
      state.messages,
      state.activeRunByConversation,
      state.runs,
      state.liveByRun,
    ],
  );
  const messagePageInfo = activeId ? state.messagePageInfoByConversation[activeId] : undefined;
  const loadOlderMessages = useCallback(async () => {
    if (!activeId || !messagePageInfo?.hasMore || loadingOlderMessages) return;
    setLoadingOlderMessages(true);
    try {
      const raw = await gateway.request<unknown>('conversation.getMessagesPage', {
        conversation_id: activeId,
        limit: 100,
        cursor: messagePageInfo.nextCursor,
      });
      const rows = Array.isArray(raw) ? raw : (raw as { messages?: unknown[] } | null)?.messages ?? [];
      const next = (raw as { nextCursor?: { createdAt?: string; id?: string } | null } | null)?.nextCursor;
      dispatch({
        type: 'messages/prependPage',
        conversationId: activeId,
        messages: rows.map((row) => mapWireMessage((row ?? {}) as Record<string, unknown>)),
        pageInfo: {
          hasMore: Boolean(next),
          nextCursor: next?.createdAt && next.id ? { createdAt: next.createdAt, id: next.id } : null,
        },
      });
    } finally {
      setLoadingOlderMessages(false);
    }
  }, [activeId, dispatch, gateway, loadingOlderMessages, messagePageInfo]);
  const rootRun = selectActiveRun(state, rootConversationId);
  const surfaceRun = selectActiveRun(state, activeId);
  // Timeline / input / pause track surface; activity inspector prefers root tree.
  const activeRun = surfaceRun ?? rootRun;
  const activeRunId = activeRun?.id;
  const activeRunStatus = activeRun?.status;
  const isStreaming = selectIsRunActive(state, activeId);
  // Permissions / assignments for the root conversation (batch waiters are parent-scoped).
  const interactions = selectPendingInteractions(state, rootConversationId);
  const promptQueue = selectPromptQueue(state, activeId);
  const rootRunId = rootRun?.id ?? null;
  // Keep root and selected-child event sources separate so main Todo never
  // flips when the user inspects a subagent session.
  const rootEvents = useMemo(
    () => selectEventsForRunTree(state, rootRunId),
    [rootRunId, state.eventsByRun, state.childRunsByParent],
  );
  const selectedChildEvents = useMemo(
    () =>
      selectedChildConversationId
        ? selectRunEvents(state, surfaceRun?.id ?? null)
        : EMPTY_CHILD_EVENTS,
    [selectedChildConversationId, surfaceRun?.id, state.eventsByRun],
  );
  // Timeline / file events follow the surface (child when selected).
  const events = selectedChildConversationId ? selectedChildEvents : rootEvents;
  const artifacts = useMemo(
    () =>
      selectedChildConversationId
        ? selectArtifacts(state, surfaceRun?.id ?? null)
        : selectArtifactsForRunTree(state, rootRunId),
    [
      selectedChildConversationId,
      surfaceRun?.id,
      rootRunId,
      state.artifactsByRun,
      state.childRunsByParent,
    ],
  );
  const children = selectChildRuns(state, rootRunId);
  const fileChanges = useMemo(
    () =>
      selectedChildConversationId
        ? selectFileChanges(state, surfaceRun?.id ?? null)
        : selectFileChangesForRunTree(state, rootRunId),
    [
      selectedChildConversationId,
      surfaceRun?.id,
      rootRunId,
      state.fileChangesByRun,
      state.childRunsByParent,
    ],
  );
  const mainTodos = useMemo(() => extractTodosFromEvents(rootEvents), [rootEvents]);
  const selectedChildTodos = useMemo(
    () => extractTodosFromEvents(selectedChildEvents),
    [selectedChildEvents],
  );
  const contextUsage = rootConversationId
    ? state.contextUsageByConversation[rootConversationId] ?? null
    : null;
  const planApproval = interactions.find((i) => i.kind === 'plan_approval');
  const subagentAssignment = interactions.find(
    (i): i is SubagentAssignmentInteraction => i.kind === 'subagent_assignment',
  );

  const conversationChangeSummary = useMemo(
    () => summarizeConversationChanges(events, fileChanges),
    [events, fileChanges],
  );
  const fileEvents = useMemo(
    () =>
      events
        .filter((e) => e.type === 'file_changed')
        .map((e) => {
          const p = (e.payload ?? {}) as Record<string, unknown>;
          return {
            path: String(p.path ?? ''),
            changeType: String(p.change_type ?? p.changeType ?? 'modified'),
            at: e.timestamp,
            runId: e.runId,
          };
        })
        .filter((f) => f.path),
    [events],
  );
  // Goal chrome is opt-in only: conversation.mode must be exactly 'goal'.
  // Ordinary chat/agent runs have NO status bar — progress is timeline
  // streaming / "正在思考" + MessageInput stop only.
  const isGoalMode = activeConversation?.mode === 'goal';
  const goalInstruction = useMemo(() => {
    if (!isGoalMode) return null;
    const firstUser = messages.find((m) => m.role === 'user');
    if (!firstUser) return activeConversation?.title ?? null;
    const text = messagePlainText(firstUser.contentBlocks).trim();
    return text || activeConversation?.title || null;
  }, [isGoalMode, messages, activeConversation?.title]);
  const goalCanResume = Boolean(
    isGoalMode &&
      activeRun &&
      (activeRun.status === 'interrupted' ||
        activeRun.status === 'cancelled' ||
        activeRun.status === 'failed'),
  );

  const timelineMessages = useMemo(
    () =>
      messages.map((m) => ({
        id: m.id,
        role: m.role as 'system' | 'user' | 'assistant',
        contentBlocks: m.contentBlocks,
        status: m.status,
        createdAt: m.createdAt,
        runId: m.runId ?? null,
        inputTokens: m.inputTokens,
        outputTokens: m.outputTokens,
        startedAt: m.runId ? state.runs[m.runId]?.startedAt ?? undefined : undefined,
        finishedAt: m.runId ? state.runs[m.runId]?.finishedAt ?? undefined : undefined,
        reasoningStartedAt: m.runId ? state.liveByRun[m.runId]?.reasoningStartedAt : null,
        reasoningFinishedAt: m.runId ? state.liveByRun[m.runId]?.reasoningFinishedAt : null,
      })),
    [messages, state.runs, state.liveByRun],
  );

  return {
    surfaceConversationId,
    rootConversationId,
    activeId,
    rootConversation,
    activeConversation,
    modelSelection,
    messages,
    messagePageInfo,
    loadingOlderMessages,
    loadOlderMessages,
    rootRun,
    surfaceRun,
    activeRun,
    activeRunId,
    activeRunStatus,
    isStreaming,
    interactions,
    promptQueue,
    rootRunId,
    rootEvents,
    selectedChildEvents,
    events,
    artifacts,
    children,
    fileChanges,
    mainTodos,
    selectedChildTodos,
    contextUsage,
    planApproval,
    subagentAssignment,
    conversationChangeSummary,
    fileEvents,
    isGoalMode,
    goalInstruction,
    goalCanResume,
    timelineMessages,
  };
}
