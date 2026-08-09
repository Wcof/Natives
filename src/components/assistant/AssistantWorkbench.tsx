'use client';

/**
 * Assistant workbench — composition only.
 * Protocol I/O and execution state live in AssistantGateway + Workspace Store.
 * Components never call window.nativesAPI.assistantV2 / streamChat.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { Locale } from '@/i18n';
import { t } from '@/i18n';
import { useToast } from '@/components/ui/Toast';
import { classifyError } from '@/lib/error-classifier';
import {
  classifyProviderReadiness,
  mapWireProviders,
  resolveModelSelection,
  selectAssistantModel,
  toProviderInfo,
  type ProviderReadiness,
} from '@/lib/provider-model-selection';
import {
  groupAssistantConversations,
  projectCreationState,
} from '@/lib/assistant-project-groups';
import { readActiveProject, writeActiveProject } from '@/lib/active-project';
import {
  AssistantStoreProvider,
  useAssistantDispatch,
  useAssistantGateway,
  useAssistantStore,
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
} from '@/lib/assistant-workspace';
import {
  canRewind,
  needsEngineRecovery,
} from '@/lib/assistant-workspace/capability-gate';
import {
  connectWorkspace,
  loadConversations,
  openConversation,
} from '@/lib/assistant-workspace/controller';
import { createDefaultGateway, FixtureAssistantAdapter } from '@/lib/assistant-gateway';
import { goldenTextStream } from '@/lib/assistant-fixtures/golden';
import { mapWireMessage } from '@/lib/assistant-protocol';
import type {
  Conversation,
  RunEvent,
  SubagentAssignmentInteraction,
} from '@/lib/assistant-protocol';
import { messagePlainText } from '@/lib/assistant-message-view';
import { useAssistantRunSubscription } from '@/hooks/useAssistantRunSubscription';
import { useAssistantSubagentState } from '@/hooks/useAssistantSubagentState';
import { useAssistantRunLifecycle } from '@/hooks/useAssistantRunLifecycle';
import { useAssistantWorkbenchKeyboard } from '@/hooks/useAssistantWorkbenchKeyboard';
import SubagentAssignmentModal from './SubagentAssignmentModal';
import type { ActivitySubagentView } from './ActivityInspector';
import { extractTodosFromEvents } from '@/lib/assistant-activity-view';
import { summarizeConversationChanges } from '@/lib/assistant-timeline';
import ConfirmDialog from '@/components/ui/ConfirmDialog';
import ConnectionBanner from './ConnectionBanner';
import EngineRecoveryPage from './EngineRecoveryPage';
import CommandPalette from './CommandPalette';
import type { ProviderWithModels } from '@/components/ui/conversation/ModelSelectorDropdown';
import WorkbenchHeader from './workbench/WorkbenchHeader';
import WorkbenchTimelinePane from './workbench/WorkbenchTimelinePane';
import WorkbenchComposer from './workbench/WorkbenchComposer';
import WorkbenchPanels from './workbench/WorkbenchPanels';
import {
  useAssistantNavigation,
  useAssistantWorkspaceApi,
  type AssistantWorkspaceActions,
} from './AssistantWorkspaceContext';
import {
  collectTempConversationIds,
  conversationsWithoutTemp,
  createTempSession,
  isTempConversationId,
  resolveRegisteredProjectPath,
} from '@/lib/assistant-temp-conversation';

/** Shared empty child-event list — avoid `[]` literal thrashing useMemo deps. */
const EMPTY_CHILD_EVENTS: RunEvent[] = [];

interface AssistantWorkbenchProps {
  locale: Locale;
  /** Force fixture adapter (browser / tests). */
  preferFixture?: boolean;
}

function WorkbenchInner({ locale }: { locale: Locale }) {
  const { toast } = useToast();
  const state = useAssistantStore();
  const dispatch = useAssistantDispatch();
  const gateway = useAssistantGateway();
  // Navigation for selectedId sync; publishers via stable API so stream-driven
  // runtime publishes do not re-enter Workbench through a second context.
  const { navigation } = useAssistantNavigation();
  const { publishNavigation, publishRuntime, registerActions } = useAssistantWorkspaceApi();

  const stateRef = useRef(state);
  stateRef.current = state;
  /** Per-run soft-resubscribe abort + attempt counts (multi-run table). */

  const [providers, setProviders] = useState<ProviderWithModels[]>([]);
  const [providerReadiness, setProviderReadiness] = useState<ProviderReadiness>('no_provider');
  const [loadingMessages, setLoadingMessages] = useState(false);
  const [activeProjectPath, setActiveProjectPath] = useState<string | null>(null);
  const [registeredProjects, setRegisteredProjects] = useState<Array<{ id: string; path: string; lastOpenedAt?: string | null; label?: string; exists?: boolean }>>([]);
  const [pinnedConversationIds, setPinnedConversationIds] = useState<Set<string>>(new Set());
  const [rightPanelOpen, setRightPanelOpen] = useState(!state.view.rightCollapsed);
  const [loadingConversations, setLoadingConversations] = useState(true);
  /** Project list / navigation always use root; timeline/input use surface. */
  const [selectedRootConversationId, setSelectedRootConversationId] = useState<string | null>(
    null,
  );
  const [selectedChildConversationId, setSelectedChildConversationId] = useState<string | null>(
    null,
  );
  /** T216/T302: unified confirm dialog for goal conversation deletion. */
  const [confirmDeleteGoalId, setConfirmDeleteGoalId] = useState<string | null>(null);

  // Keep root selection aligned with store activeConversationId (which is always the root).
  const storeActiveId = state.activeConversationId;
  useEffect(() => {
    if (storeActiveId !== selectedRootConversationId) {
      setSelectedRootConversationId(storeActiveId);
      // Switching another root conversation exits child view.
      setSelectedChildConversationId(null);
    }
  }, [storeActiveId]);

  const surfaceConversationId = selectSurfaceConversationId(
    selectedRootConversationId ?? storeActiveId,
    selectedChildConversationId,
  );
  const rootConversationId = selectedRootConversationId ?? storeActiveId;
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
  const [loadingOlderMessages, setLoadingOlderMessages] = useState(false);
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

  // Stable bridge so subscription control (owned by useAssistantRunSubscription
  // below) stays available to subagent actions without ordering the two hooks
  // against each other.
  const ensureRunSubscriptionRef = useRef<(runId: string | null | undefined) => void>(
    () => undefined,
  );
  const ensureRunSubscription = useCallback((runId: string | null | undefined) => {
    ensureRunSubscriptionRef.current(runId);
  }, []);

  // Subagent session / key-assignment ownership.
  const subagent = useAssistantSubagentState({
    rootConversationId,
    subagentAssignment,
    providers,
    activeConversation,
    rootConversation,
    locale,
    ensureRunSubscription,
    setLoadingMessages,
    setSelectedChildConversationId,
  });
  const { subagentSessions } = subagent;

  // Run control lifecycle: stopping flag + stop/retry/permission/rollback/
  // diagnostics/reconnect. Kept in one hook so the shell only passes run ids.
  const {
    stoppingRunId,
    handleStop,
    handleRetry,
    handlePermission,
    handleRollbackChanges,
    handleCopyDiagnostics,
    handleRetryConnection,
  } = useAssistantRunLifecycle({
    activeRunId,
    rootRun,
    activeRun,
    ensureRunSubscription,
    locale,
  });

  // Composer ownership (send path + ADR-0016 capability picker) lives inside
  // WorkbenchComposer so the shell stays a pure orchestrator.

  const activitySubagents = useMemo<ActivitySubagentView[]>(() => {
    if (subagentSessions.length > 0) {
      return subagentSessions.map((s) => {
        const isSelected = s.childConversationId === selectedChildConversationId;
        // Never surface raw key ids as labels — use short opaque prefix only.
        const keyLabel = s.keyId ? `key:${s.keyId.slice(0, 6)}` : undefined;
        return {
          id: s.id,
          name: s.name || s.task || s.id.slice(0, 8),
          status: s.status,
          providerId: s.providerId,
          keyLabel,
          childConversationId: s.childConversationId,
          task: s.task,
          // Prefer live child todo_write when this session is selected.
          todos:
            isSelected && selectedChildTodos.length > 0 ? selectedChildTodos : undefined,
          error: s.error ?? undefined,
        };
      });
    }
    return children.map((ch) => ({
      id: ch.id,
      name: ch.task || ch.agentProfileId || ch.id.slice(0, 8),
      status: String(ch.status),
      providerId: ch.providerId,
      keyLabel: ch.keyLabel,
      task: ch.task,
    }));
  }, [subagentSessions, children, selectedChildConversationId, selectedChildTodos]);
  const composerSubagents = useMemo(
    () => activitySubagents
      .filter((agent): agent is ActivitySubagentView & { childConversationId: string } => Boolean(agent.childConversationId))
      .map((agent) => ({ id: agent.id, name: agent.name, status: agent.status })),
    [activitySubagents],
  );
  const activeComposerSubagent = selectedChildConversationId
    ? composerSubagents.find((agent) =>
        activitySubagents.find((item) => item.id === agent.id)?.childConversationId === selectedChildConversationId,
      ) ?? null
    : null;
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

  // Layout breakpoint
  useEffect(() => {
    const update = () => {
      const w = window.innerWidth;
      const layoutBreakpoint = w >= 1200 ? 'full' : w >= 800 ? 'drawer-right' : 'drawer-both';
      // reducer bails out when layoutBreakpoint is unchanged
      dispatch({ type: 'view/patch', patch: { layoutBreakpoint } });
      if (layoutBreakpoint !== 'full') {
        setRightPanelOpen((open) => (open ? false : open));
      }
    };
    update();
    window.addEventListener('resize', update);
    return () => window.removeEventListener('resize', update);
  }, [dispatch]);

  // Run lifecycle (subscription loop, quiet resubscribe, cancel/retry/permission)
  // lives in useAssistantRun so the creator workbench runs the identical logic
  // instead of a second copy that would have to rediscover the same edge cases.
  const runSubscription = useAssistantRunSubscription({
    rootRun,
    rootConversationId,
    children,
    subagentSessions,
  });
  // Point the stable bridge at the real subscription controller from this render.
  ensureRunSubscriptionRef.current = runSubscription.ensureRunSubscription;
  const abortAllSubscriptions = runSubscription.abortAllSubscriptions;

  // Boot: connect + list conversations + providers
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        await connectWorkspace(gateway, dispatch);
        if (cancelled) return;
        await loadConversations(gateway, dispatch);
        if (cancelled) return;

        try {
          const path = await readActiveProject(window.nativesAPI);
          if (!cancelled) setActiveProjectPath(path);
        } catch {
          /* browser */
        }
        try {
          const projects = (await window.nativesAPI?.project?.list?.()) ?? [];
          if (!cancelled) setRegisteredProjects(projects);
        } catch {
          /* */
        }
        try {
          const raw = await window.nativesAPI?.db?.get('assistant:pinnedConversations');
          if (!cancelled && raw) {
            const parsed = JSON.parse(String(raw)) as Record<string, string[]>;
            const ids = new Set<string>();
            for (const list of Object.values(parsed ?? {})) {
              for (const id of list ?? []) ids.add(id);
            }
            setPinnedConversationIds(ids);
          }
        } catch {
          /* */
        }

        try {
          // Production returns `{ providers: [...] }`; fixtures may return a bare array.
          const list = await gateway.request<unknown>('provider.list', {});
          const mapped: ProviderWithModels[] = mapWireProviders(list);
          if (!cancelled) {
            setProviders(mapped);
            setProviderReadiness(classifyProviderReadiness(toProviderInfo(mapped)));
          }
        } catch (err) {
          if (!cancelled) {
            setProviders([]);
            setProviderReadiness('no_provider');
            toast(classifyError(err).userMessage, 'error');
          }
        }
      } catch (err) {
        if (!cancelled) toast(classifyError(err).userMessage, 'error');
      } finally {
        if (!cancelled) setLoadingConversations(false);
      }
    })();
    return () => {
      cancelled = true;
      abortAllSubscriptions();
      void gateway.disconnect();
    };
  }, [gateway, dispatch, toast]);

  // Load subagent.list when root conversation is visible. Avoid re-listing on every
  // transient status flicker while the parent is already waiting_subagent / running —
  // use a coarse phase so assignment → first child does not hammer subagent.list.
  const rootRunPhase =
    rootRun?.status === 'waiting_subagent' || rootRun?.status === 'running'
      ? 'active'
      : rootRun?.status ?? 'idle';
  useEffect(() => {
    void subagent.refreshSubagentSessions(rootConversationId);
  }, [rootConversationId, subagent.refreshSubagentSessions, rootRunPhase]);

  // Prefetch assignment keys when an assignment interaction appears.
  useEffect(() => {
    if (subagentAssignment || subagent.switchKeySessionId) {
      void subagent.loadAssignmentKeys();
    }
  }, [subagentAssignment, subagent.switchKeySessionId, subagent.loadAssignmentKeys]);

  // Publish navigation snapshot for shell sidebar
  useEffect(() => {
    const conversations = conversationsWithoutTemp(
      (state.conversationOrder
        .map((id) => state.conversations[id])
        .filter(Boolean) as Conversation[]),
    );
    const groups = groupAssistantConversations(
      conversations.map((c) => ({
        id: c.id,
        title: c.title,
        mode: c.mode,
        projectId: c.projectId ?? null,
        updatedAt: c.updatedAt,
        parentConversationId: c.parentConversationId ?? null,
        pinned: pinnedConversationIds.has(c.id),
      })),
      registeredProjects.map((p) => ({ path: p.path, lastOpenedAt: (p as { lastOpenedAt?: string | null; last_opened_at?: string | null }).lastOpenedAt ?? (p as { last_opened_at?: string | null }).last_opened_at ?? null, label: p.label, exists: p.exists })),
      t(locale, 'assistant.unassignedProjects'),
    );
    // Merge with projects already seeded by AssistantWorkspaceProvider so a
    // late/empty workbench project.list cannot blank the sidebar on first paint.
    publishNavigation((prev) => {
      let nextGroups = groups;
      if (registeredProjects.length === 0 && prev.groups.length > 0) {
        const seedPaths = prev.groups
          .map((g) => g.path)
          .filter((p): p is string => Boolean(p));
        if (seedPaths.length > 0) {
          nextGroups = groupAssistantConversations(
            conversations.map((c) => ({
              id: c.id,
              title: c.title,
              mode: c.mode,
              projectId: c.projectId ?? null,
              updatedAt: c.updatedAt,
              parentConversationId: c.parentConversationId ?? null,
            })),
            seedPaths,
            t(locale, 'assistant.unassignedProjects'),
          );
        }
      }
      // Prefer root-level temp shell (survives workbench remount) when store has none.
      const storeTempId = isTempConversationId(rootConversationId)
        ? rootConversationId
        : isTempConversationId(activeId)
          ? activeId
          : null;
      const rootTemp = prev.tempSession;
      const selectedId =
        storeTempId ??
        (rootTemp && rootConversationId === null ? rootTemp.conversation.id : rootConversationId) ??
        rootTemp?.conversation.id ??
        rootConversationId;
      return {
        groups: nextGroups,
        selectedId,
        activeProjectPath: activeProjectPath ?? prev.activeProjectPath,
        loading: loadingConversations,
        creationState: projectCreationState({
          engine:
            state.connection === 'connected'
              ? 'ready'
              : state.connection === 'connecting'
                ? 'connecting'
                : 'unavailable',
          providerReadiness,
        }),
        isCreatingConversation: false,
        pendingCreateProjectPath: prev.pendingCreateProjectPath,
        // Drop root temp once the store has a real active session (or a different temp).
        tempSession:
          storeTempId && rootTemp && rootTemp.conversation.id === storeTempId
            ? rootTemp
            : isTempConversationId(rootConversationId)
              ? rootTemp
              : rootConversationId
                ? null
                : rootTemp,
      };
    });
  }, [
    state.conversations,
    state.conversationOrder,
    state.connection,
    rootConversationId,
    activeProjectPath,
    registeredProjects,
    pinnedConversationIds,
    loadingConversations,
    providerReadiness,
    publishNavigation,
  ]);

  // Publish runtime for shell
  useEffect(() => {
    publishRuntime({
      conversationId: rootConversationId,
      conversationTitle: rootConversation?.title ?? activeConversation?.title ?? null,
      conversationMode: rootConversation?.mode ?? activeConversation?.mode ?? 'agent',
      providerId: activeConversation?.providerId ?? rootConversation?.providerId ?? '',
      modelId: activeConversation?.modelId ?? rootConversation?.modelId ?? '',
      runId: activeRun?.id ?? null,
      runStatus: activeRun?.status ?? 'idle',
      runStartedAt: activeRun?.startedAt ?? null,
      runFinishedAt: activeRun?.finishedAt ?? null,
      events: events.map((e) => ({
        runId: e.runId,
        sequence: e.sequence,
        timestamp: e.timestamp,
        type: String(e.type),
        payload: e.payload,
      })),
      fileChanges: fileChanges.map((f) => ({ path: f.path, change: f.changeType, changeType: f.changeType })),
      artifacts: artifacts.map((a) => ({
        id: a.id,
        path: a.path,
        label: a.label,
        size: a.size,
        kind: a.kind,
      })),
      usage: {
        inputTokens: contextUsage?.usedTokens ?? null,
        outputTokens: null,
        reasoningTokens: null,
      },
    });
  }, [
    rootConversationId,
    rootConversation,
    activeConversation,
    activeRun,
    events,
    fileChanges,
    artifacts,
    contextUsage,
    publishRuntime,
  ]);

  const selectConversation = useCallback(
    async (id: string) => {
      // Project list always selects a root conversation.
      setSelectedChildConversationId(null);
      setSelectedRootConversationId(id);
      if (id === stateRef.current.activeConversationId) {
        // Re-open still refreshes snapshot so artifacts recover after completed runs.
        setLoadingMessages(true);
        try {
          await openConversation(gateway, dispatch, id);
          ensureRunSubscription(stateRef.current.activeRunByConversation[id]);
          void subagent.refreshSubagentSessions(id);
        } catch (err) {
          toast(classifyError(err).userMessage, 'error');
        } finally {
          setLoadingMessages(false);
        }
        return;
      }
      setLoadingMessages(true);
      try {
        await openConversation(gateway, dispatch, id);
        ensureRunSubscription(stateRef.current.activeRunByConversation[id]);
        void subagent.refreshSubagentSessions(id);
      } catch (err) {
        toast(classifyError(err).userMessage, 'error');
      } finally {
        setLoadingMessages(false);
      }
    },
    [gateway, dispatch, toast, ensureRunSubscription, subagent.refreshSubagentSessions],
  );

  // Keyboard / command palette ownership (shortcuts + commands + palette flag).
  const { paletteOpen, setPaletteOpen, commands } = useAssistantWorkbenchKeyboard({
    activeId,
    activeRunId,
    activeRunStatus,
    isStreaming,
    providers,
    activeProjectPath,
    locale,
    handleStop,
    handleRetry,
    ensureRunSubscription,
    selectConversation,
    publishNavigation,
    setRightPanelOpen,
  });

  // Sync external sidebar selection (persisted sessions only — temps hydrate below).
  useEffect(() => {
    const selected = navigation.selectedId;
    if (!selected || selected === rootConversationId) return;
    if (isTempConversationId(selected)) return;
    void selectConversation(selected);
  }, [navigation.selectedId]);

  /**
   * Hydrate a root-level temp shell into the workbench store.
   * Shell can create temp-* before this lazy workbench mounts; once mounted we
   * adopt it without calling conversation.create. Consecutive project picks
   * leave only the latest temp shell.
   *
   * Re-runs after loadConversations finishes because `conversations/replace`
   * wipes local-only shells — we re-upsert from the root-level tempSession.
   */
  useEffect(() => {
    const temp = navigation.tempSession;
    if (!temp) return;
    if (loadingConversations) return;
    const { conversation } = temp;
    if (!isTempConversationId(conversation.id)) return;

    if (conversation.projectId) {
      const projectId = conversation.projectId;
      setActiveProjectPath((prev) => (prev === projectId ? prev : projectId));
      void writeActiveProject(window.nativesAPI, projectId).catch(() => undefined);
    }

    // Drop any previous local temp shells (only keep the latest pick).
    for (const oldId of collectTempConversationIds(stateRef.current.conversationOrder)) {
      if (oldId !== conversation.id) {
        dispatch({ type: 'conversations/remove', id: oldId });
        dispatch({ type: 'composer/clear', conversationId: oldId });
      }
    }

    // Prefer a model already chosen on the shell; otherwise fill from live providers
    // without blocking when none are configured (composer shows existing disabled state).
    const pick = selectAssistantModel(toProviderInfo(providers));
    const existing = stateRef.current.conversations[conversation.id];
    const shell: Conversation = {
      ...conversation,
      // Keep in-store edits (provider/model/permission) if hydrate re-runs after list load.
      providerId:
        existing?.providerId || conversation.providerId || pick?.providerId || '',
      modelId: existing?.modelId || conversation.modelId || pick?.modelId || '',
      permissionProfileId:
        existing?.permissionProfileId || conversation.permissionProfileId || 'ask',
    };
    // Skip upsert when the shell is already active and fields match — avoids
    // conversations map identity churn that re-fires the navigation publisher.
    const alreadyActive =
      stateRef.current.activeConversationId === shell.id &&
      existing &&
      existing.providerId === shell.providerId &&
      existing.modelId === shell.modelId &&
      existing.permissionProfileId === shell.permissionProfileId &&
      existing.projectId === shell.projectId &&
      existing.title === shell.title;
    if (!alreadyActive) {
      dispatch({ type: 'conversations/upsert', conversation: shell });
      dispatch({ type: 'conversations/setActive', id: shell.id });
    }
    const draftFromStore = stateRef.current.composerByConversation[shell.id];
    const draft = draftFromStore?.text || (draftFromStore?.attachments?.length ?? 0) > 0
      ? draftFromStore
      : temp.draft;
    if (draft && (draft.text || (draft.attachments?.length ?? 0) > 0)) {
      dispatch({
        type: 'composer/set',
        conversationId: shell.id,
        draft: {
          text: draft.text,
          attachments: draft.attachments,
          updatedAt: draft.updatedAt,
        },
      });
    }
  }, [navigation.tempSession?.conversation.id, loadingConversations]);

  // Legacy deferred path: shell set pendingCreateProjectPath before tempSession existed.
  useEffect(() => {
    const path = navigation.pendingCreateProjectPath;
    if (path === undefined) return;
    // Prefer the modern tempSession path when both are present.
    if (navigation.tempSession) {
      publishNavigation((prev) => ({ ...prev, pendingCreateProjectPath: undefined }));
      return;
    }
    if (path) {
      setActiveProjectPath(path);
      void writeActiveProject(window.nativesAPI, path).catch(() => undefined);
    }
    const session = createTempSession({
      projectId: path,
      title: t(locale, 'assistant.newConversation'),
    });
    const pick = selectAssistantModel(toProviderInfo(providers));
    const conversation: Conversation = {
      ...session.conversation,
      providerId: pick?.providerId ?? '',
      modelId: pick?.modelId ?? '',
    };
    for (const oldId of collectTempConversationIds(stateRef.current.conversationOrder)) {
      if (oldId !== conversation.id) {
        dispatch({ type: 'conversations/remove', id: oldId });
        dispatch({ type: 'composer/clear', conversationId: oldId });
      }
    }
    dispatch({ type: 'conversations/upsert', conversation });
    dispatch({ type: 'conversations/setActive', id: conversation.id });
    publishNavigation((prev) => ({
      ...prev,
      pendingCreateProjectPath: undefined,
      tempSession: { conversation, draft: session.draft },
      selectedId: conversation.id,
      activeProjectPath: path,
    }));
  }, [navigation.pendingCreateProjectPath]);

  // Workspace actions for sidebar
  useEffect(() => {
    const actions: AssistantWorkspaceActions = {
      selectConversation: (id) => {
        // Switching to a persisted session: drop any local temp shell.
        // Do NOT cancel background runs on other conversations.
        if (!isTempConversationId(id)) {
          for (const oldId of collectTempConversationIds(stateRef.current.conversationOrder)) {
            dispatch({ type: 'conversations/remove', id: oldId });
            dispatch({ type: 'composer/clear', conversationId: oldId });
          }
          publishNavigation((prev) => ({
            ...prev,
            tempSession: null,
            selectedId: id,
          }));
        }
        void selectConversation(id);
      },
      selectProject: (path) => {
        setActiveProjectPath(path);
        if (path) void writeActiveProject(window.nativesAPI, path).catch(() => undefined);
      },
      addProjectFolder: () => {
        void window.nativesAPI?.dialog?.pickDirectory?.().then(async (picked) => {
          if (!picked) return;
          try {
            const registered = await window.nativesAPI?.project?.register?.(picked);
            const path = resolveRegisteredProjectPath(registered, picked);
            const projects = (await window.nativesAPI?.project?.list?.()) ?? [];
            setRegisteredProjects(projects);
            setActiveProjectPath(path);
            void writeActiveProject(window.nativesAPI, path).catch(() => undefined);

            // Drop previous local temps; keep only this pick. Never cancel other runs.
            for (const oldId of collectTempConversationIds(stateRef.current.conversationOrder)) {
              dispatch({ type: 'conversations/remove', id: oldId });
              dispatch({ type: 'composer/clear', conversationId: oldId });
            }

            const pick = selectAssistantModel(toProviderInfo(providers));
            const session = createTempSession({
              projectId: path,
              title: t(locale, 'assistant.newConversation'),
              providerId: pick?.providerId,
              modelId: pick?.modelId,
            });
            dispatch({ type: 'conversations/upsert', conversation: session.conversation });
            dispatch({ type: 'conversations/setActive', id: session.conversation.id });
            publishNavigation((prev) => ({
              ...prev,
              activeProjectPath: path,
              selectedId: session.conversation.id,
              tempSession: session,
              pendingCreateProjectPath: undefined,
            }));
          } catch (err) {
            toast(classifyError(err).userMessage, 'error');
          }
        });
      },
      createConversation: () => {
        // Local temp shell only — no conversation.create, no provider hard-gate.
        for (const oldId of collectTempConversationIds(stateRef.current.conversationOrder)) {
          dispatch({ type: 'conversations/remove', id: oldId });
          dispatch({ type: 'composer/clear', conversationId: oldId });
        }
        const pick = selectAssistantModel(toProviderInfo(providers));
        const session = createTempSession({
          projectId: activeProjectPath,
          title: t(locale, 'assistant.newConversation'),
          providerId: pick?.providerId,
          modelId: pick?.modelId,
        });
        dispatch({ type: 'conversations/upsert', conversation: session.conversation });
        dispatch({ type: 'conversations/setActive', id: session.conversation.id });
        publishNavigation((prev) => ({
          ...prev,
          selectedId: session.conversation.id,
          tempSession: session,
          pendingCreateProjectPath: undefined,
        }));
      },
      createConversationInProject: (path) => {
        setActiveProjectPath(path);
        void writeActiveProject(window.nativesAPI, path).catch(() => undefined);
        for (const oldId of collectTempConversationIds(stateRef.current.conversationOrder)) {
          dispatch({ type: 'conversations/remove', id: oldId });
          dispatch({ type: 'composer/clear', conversationId: oldId });
        }
        const pick = selectAssistantModel(toProviderInfo(providers));
        const session = createTempSession({
          projectId: path,
          title: t(locale, 'assistant.newConversation'),
          providerId: pick?.providerId,
          modelId: pick?.modelId,
        });
        dispatch({ type: 'conversations/upsert', conversation: session.conversation });
        dispatch({ type: 'conversations/setActive', id: session.conversation.id });
        publishNavigation((prev) => ({
          ...prev,
          activeProjectPath: path,
          selectedId: session.conversation.id,
          tempSession: session,
          pendingCreateProjectPath: undefined,
        }));
      },      removeProject: async (path) => {
        try {
          const projects = (await window.nativesAPI?.project?.list?.()) ?? [];
          const match = projects.find((p) => p.path === path || p.id === path);
          await window.nativesAPI?.project?.remove?.(match?.id ?? path);
          const next = (await window.nativesAPI?.project?.list?.()) ?? [];
          if (next.some((project) => project.path === path || project.id === path)) {
            throw new Error('Project remains registered after removal');
          }
          setRegisteredProjects(next);
          if (activeProjectPath === path) setActiveProjectPath(null);
          return true;
        } catch (err) {
          toast(classifyError(err).userMessage, 'error');
          return false;
        }
      },
      renameProject: async (path, label) => {
        try {
          const projects = (await window.nativesAPI?.project?.list?.()) ?? [];
          const match = projects.find((p) => p.path === path || p.id === path);
          await window.nativesAPI?.project?.rename?.(match?.id ?? path, label);
          setRegisteredProjects((await window.nativesAPI?.project?.list?.()) ?? []);
          return true;
        } catch (err) {
          toast(classifyError(err).userMessage, 'error');
          return false;
        }
      },
      renameConversation: (id, title) => {
        void gateway.request('conversation.rename', { id, title }).then(() => {
          const c = stateRef.current.conversations[id];
          if (c) dispatch({ type: 'conversations/upsert', conversation: { ...c, title } });
        });
      },
      archiveConversation: (id) => {
        void gateway.request('conversation.archive', { id }).then(() => {
          dispatch({ type: 'conversations/remove', id });
          // Drop pin preference for archived sessions.
          void (async () => {
            try {
              const raw = await window.nativesAPI?.db?.get('assistant:pinnedConversations');
              if (!raw) return;
              const map = JSON.parse(String(raw)) as Record<string, string[]>;
              let changed = false;
              for (const key of Object.keys(map)) {
                const next = (map[key] ?? []).filter((x) => x !== id);
                if (next.length !== (map[key] ?? []).length) {
                  map[key] = next;
                  changed = true;
                }
              }
              if (changed) {
                await window.nativesAPI?.db?.set('assistant:pinnedConversations', JSON.stringify(map));
                setPinnedConversationIds((prev) => {
                  const n = new Set(prev);
                  n.delete(id);
                  return n;
                });
              }
            } catch { /* ignore */ }
          })();
        });
      },
      pinConversation: (id, projectId, pinned) => {
        void (async () => {
          const key = projectId?.trim() || '__unassigned__';
          try {
            const raw = await window.nativesAPI?.db?.get('assistant:pinnedConversations');
            const map = raw ? (JSON.parse(String(raw)) as Record<string, string[]>) : {};
            const list = new Set(map[key] ?? []);
            if (pinned) list.add(id);
            else list.delete(id);
            map[key] = [...list];
            // Clean empty buckets
            if (map[key].length === 0) delete map[key];
            await window.nativesAPI?.db?.set('assistant:pinnedConversations', JSON.stringify(map));
            setPinnedConversationIds((prev) => {
              const n = new Set(prev);
              if (pinned) n.add(id);
              else n.delete(id);
              return n;
            });
          } catch (err) {
            toast(classifyError(err).userMessage, 'error');
          }
        })();
      },
      deleteConversation: async (id) => {
        // Always drop from UI first so the sidebar never looks like a no-op.
        // Host/daemon cleanup is best-effort after the optimistic remove.
        const clearPin = () => {
          setPinnedConversationIds((prev) => {
            if (!prev.has(id)) return prev;
            const n = new Set(prev);
            n.delete(id);
            return n;
          });
          void (async () => {
            try {
              const raw = await window.nativesAPI?.db?.get('assistant:pinnedConversations');
              if (!raw) return;
              const map = JSON.parse(String(raw)) as Record<string, string[]>;
              let changed = false;
              for (const key of Object.keys(map)) {
                const next = (map[key] ?? []).filter((x) => x !== id);
                if (next.length !== (map[key] ?? []).length) {
                  map[key] = next;
                  changed = true;
                }
              }
              if (changed) {
                await window.nativesAPI?.db?.set('assistant:pinnedConversations', JSON.stringify(map));
              }
            } catch { /* ignore */ }
          })();
        };
        const dropFromUi = () => {
          dispatch({ type: 'conversations/remove', id });
          clearPin();
          if (stateRef.current.activeConversationId === id) {
            dispatch({ type: 'conversations/setActive', id: null });
          }
          publishNavigation((prev) => ({
            ...prev,
            groups: prev.groups.map((g) => ({
              ...g,
              conversations: g.conversations.filter((c) => c.id !== id),
            })),
            tempSession:
              prev.tempSession?.conversation.id === id ? null : prev.tempSession,
            selectedId: prev.selectedId === id ? null : prev.selectedId,
          }));
        };

        if (isTempConversationId(id)) {
          dropFromUi();
          return true;
        }

        dropFromUi();
        try {
          await gateway.request('conversation.delete', { id });
          return true;
        } catch (err) {
          const message = err instanceof Error ? err.message : String(err);
          // Already gone on host — UI already updated.
          if (/not found|NOT_FOUND|conversation not found/i.test(message)) {
            return true;
          }
          // Keep UI deleted (idempotent user intent) but surface the host error.
          toast(classifyError(err).userMessage, 'error');
          return true;
        }
      },
      retryRun: () => void handleRetry(),
      respondPermission: (requestId, approved) => void handlePermission(requestId, approved),
    };
    registerActions(actions);
    return () => registerActions(null);
  }, [
    selectConversation,
    providers,
    locale,
    activeProjectPath,
    gateway,
    dispatch,
    handleRetry,
    handlePermission,
    registerActions,
    publishNavigation,
    toast,
  ]);

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

  const showRight =
    rightPanelOpen &&
    (state.view.layoutBreakpoint === 'full' || state.view.layoutBreakpoint === 'drawer-right');

  const recoveryMode = needsEngineRecovery(state.connection, state.capabilities);
  const allowRewind = canRewind(state.capabilities);

  return (
    <div className="relative flex h-full min-h-0 flex-col" data-assistant-workbench data-gateway="1">
      <CommandPalette
        open={paletteOpen}
        onClose={() => setPaletteOpen(false)}
        commands={commands}
        locale={locale}
      />
      <ConnectionBanner
        connection={state.connection}
        error={state.connectionError}
        reconnectAttempts={state.reconnectAttempts}
        locale={locale}
        onReconnect={handleRetryConnection}
        onCopyDiagnostics={handleCopyDiagnostics}
      />

      {recoveryMode ? (
        <EngineRecoveryPage
          locale={locale}
          connection={state.connection}
          error={state.connectionError}
          onRetry={handleRetryConnection}
          onCopyDiagnostics={handleCopyDiagnostics}
        />
      ) : (
      <div className="flex min-h-0 flex-1">
        <div className="flex min-w-0 flex-1 flex-col">
          <WorkbenchHeader
            locale={locale}
            onOpenPalette={() => setPaletteOpen(true)}
            showRight={showRight}
            onToggleRightPanel={() => {
              setRightPanelOpen((v) => {
                const next = !v;
                dispatch({ type: 'view/patch', patch: { rightCollapsed: !next } });
                return next;
              });
            }}
          />

          <WorkbenchTimelinePane
            locale={locale}
            timelineMessages={timelineMessages}
            events={events}
            fileChanges={fileChanges}
            allowRewind={allowRewind}
            onRollbackChanges={handleRollbackChanges}
            loadingMessages={loadingMessages}
            onRetry={() => void handleRetry()}
            hasMoreOlder={Boolean(messagePageInfo?.hasMore)}
            loadingOlder={loadingOlderMessages}
            onLoadOlder={() => void loadOlderMessages()}
            planApproval={planApproval}
            isGoalMode={isGoalMode}
            goalTitle={activeConversation?.title ?? t(locale, 'assistant.goalTask')}
            goalInstruction={goalInstruction}
            activeRun={activeRun}
            contextUsage={contextUsage}
            goalCanResume={goalCanResume}
            onPause={() => void handleStop()}
            onResume={() => void handleRetry()}
            onDeleteGoal={() => setConfirmDeleteGoalId(activeId ?? null)}
          />

          <WorkbenchComposer
            locale={locale}
            activeId={activeId}
            activeConversation={activeConversation}
            activeProjectPath={activeProjectPath}
            providers={providers}
            providerReadiness={providerReadiness}
            registeredProjects={registeredProjects}
            rootConversationId={rootConversationId}
            modelSelection={modelSelection}
            ensureRunSubscription={ensureRunSubscription}
            publishNavigation={publishNavigation}
            setSelectedRootConversationId={setSelectedRootConversationId}
            setSelectedChildConversationId={setSelectedChildConversationId}
            isGoalMode={isGoalMode}
            isStreaming={isStreaming}
            promptQueue={promptQueue}
            stoppingRunId={stoppingRunId}
            activeRunId={activeRunId}
            handlePermission={handlePermission}
            onStop={() => void handleStop()}
            conversationChangeSummary={conversationChangeSummary}
            composerSubagents={composerSubagents}
            activeComposerSubagent={activeComposerSubagent}
            onSelectSubagent={(id) => void subagent.handleSelectSubagent(id)}
          />
        </div>

        <WorkbenchPanels
          locale={locale}
          showRight={showRight}
          rootRun={rootRun}
          activeRun={activeRun}
          rootEvents={rootEvents}
          selectedChildEvents={selectedChildEvents}
          mainTodos={mainTodos}
          artifacts={artifacts}
          children={children}
          fileChanges={fileChanges}
          contextUsage={contextUsage}
          providers={providers}
          activeProjectPath={activeProjectPath}
          fileEvents={fileEvents}
          rootConversationId={rootConversationId}
          activitySubagents={activitySubagents}
          selectedChildConversationId={selectedChildConversationId}
          onRetry={() => void handleRetry()}
          onOpenArtifact={(a) =>
            void gateway.request('artifact.open', { id: a.id, path: a.path })
          }
          onRevealArtifact={(a) =>
            void gateway.request('artifact.reveal', { id: a.id, path: a.path })
          }
          onOpenFile={(path) => void gateway.request('artifact.open', { path })}
          onSelectSubagent={(id) => void subagent.handleSelectSubagent(id)}
          onBackToMain={subagent.handleBackToMain}
          onSwitchSubagentKey={(id) => {
            subagent.setSwitchKeySessionId(id);
            void subagent.loadAssignmentKeys();
          }}
          onRefreshTasks={() => void subagent.refreshSubagentSessions(rootConversationId)}
          onClose={() => {
            setRightPanelOpen(false);
            dispatch({ type: 'view/patch', patch: { rightCollapsed: true } });
          }}
        />
      </div>
      )}

      <SubagentAssignmentModal
        open={Boolean(subagentAssignment) || Boolean(subagent.switchKeySessionId)}
        locale={locale}
        interaction={subagentAssignment ?? null}
        keys={subagent.assignmentKeyOptions}
        switchSessionId={subagent.switchKeySessionId}
        onClose={() => {
          if (subagent.switchKeySessionId) {
            subagent.setSwitchKeySessionId(null);
            return;
          }
          if (subagentAssignment) {
            void gateway
              .request('interaction.respond', {
                id: subagentAssignment.id,
                run_id: subagentAssignment.runId,
                response: {
                  approved: false,
                  cancelled: true,
                  conversation_id:
                    subagentAssignment.conversationId ?? rootConversationId ?? undefined,
                },
              })
              .then(() => dispatch({ type: 'interaction/remove', id: subagentAssignment.id }))
              .catch(() => {
                // Keep card/modal open on failure so user can retry or dismiss again.
              });
          }
        }}
        onConfirm={subagent.handleAssignmentConfirm}
      />
      {/* T216/T302: unified confirm dialog for goal conversation deletion */}
      <ConfirmDialog
        open={confirmDeleteGoalId !== null}
        title={t(locale, 'assistant.goalDelete')}
        message={t(locale, 'assistant.goalDeleteConfirm')}
        confirmLabel={t(locale, 'common.delete')}
        cancelLabel={t(locale, 'common.cancel')}
        danger
        onConfirm={() => {
          const id = confirmDeleteGoalId;
          setConfirmDeleteGoalId(null);
          if (!id) return;
          void gateway
            .request('conversation.delete', { id })
            .then(() => {
              dispatch({ type: 'conversations/remove', id });
              if (stateRef.current.activeConversationId === id) {
                dispatch({ type: 'conversations/setActive', id: null });
              }
            })
            .catch((err) => toast(classifyError(err).userMessage, 'error'));
        }}
        onCancel={() => setConfirmDeleteGoalId(null)}
      />
    </div>
  );
}

export default function AssistantWorkbench({
  locale,
  preferFixture,
}: AssistantWorkbenchProps) {
  const gateway = useMemo(() => {
    if (preferFixture) {
      const adapter = new FixtureAssistantAdapter(goldenTextStream);
      return adapter;
    }
    return createDefaultGateway(false);
  }, [preferFixture]);

  return (
    <AssistantStoreProvider gateway={gateway} preferFixture={preferFixture}>
      <WorkbenchInner locale={locale} />
    </AssistantStoreProvider>
  );
}
