'use client';

/**
 * Assistant workbench — composition only.
 * Protocol I/O and execution state live in AssistantGateway + Workspace Store.
 * Components never call window.nativesAPI.assistantV2 / streamChat.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { PanelRightClose, PanelRightOpen } from 'lucide-react';
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
  normalizePermissionProfile,
  type AssistantDraft,
  type AssistantPermissionProfile,
} from '@/lib/assistant-composer';
import {
  AssistantStoreProvider,
  useAssistantDispatch,
  useAssistantGateway,
  useAssistantStore,
  selectActiveRun,
  selectArtifacts,
  selectArtifactsForRunTree,
  selectChildRuns,
  selectComposerDraft,
  selectConversationMessages,
  selectEventsForRunTree,
  selectFileChanges,
  selectFileChangesForRunTree,
  selectIsRunActive,
  selectPendingInteractions,
  selectPromptQueue,
  selectRunEvents,
  selectSurfaceConversationId,
  type InspectorTab,
} from '@/lib/assistant-workspace';
// runtime pref loaded via persistence export
import { loadPreferredRuntimeId } from '@/lib/assistant-workspace/persistence';
import {
  canInterject,
  canRewind,
  buildDiagnosticsText,
  needsEngineRecovery,
} from '@/lib/assistant-workspace/capability-gate';
import {
  cancelRun,
  connectWorkspace,
  loadConversations,
  openConversation,
  respondPermission,
  retryRun,
  sendOrQueue,
  subscribeRun,
} from '@/lib/assistant-workspace/controller';
import { hydrateFileDiffContents } from '@/lib/assistant-workspace/file-diff-contents';
import { createDefaultGateway, FixtureAssistantAdapter } from '@/lib/assistant-gateway';
import { goldenTextStream } from '@/lib/assistant-fixtures/golden';
import { isActiveRunStatus, mapWireConversation, mapWireMessage } from '@/lib/assistant-protocol';
import type {
  Conversation,
  RunEvent,
  SubagentAssignmentInteraction,
  SubagentSession,
} from '@/lib/assistant-protocol';
import { messagePlainText } from '@/lib/assistant-message-view';
import { copyToClipboard } from '@/lib/clipboard';
import ConversationTimeline from './ConversationTimeline';
import MessageInput from './MessageInput';
import PermissionRequestCard from './PermissionRequestCard';
import AskUserPromptCard from './AskUserPromptCard';
import { COMPOSER_COLUMN_CLASS } from './InteractionPromptShell';
import GoalStatusBar from './GoalStatusBar';
import PromptQueuePanel from './PromptQueuePanel';
import ActivityInspector from './ActivityInspector';
import SubagentAssignmentModal, {
  type AssignmentKeyOption,
  type SubagentAssignmentConfirmPayload,
} from './SubagentAssignmentModal';
import type { ActivitySubagentView } from './ActivityInspector';
import { extractTodosFromEvents } from '@/lib/assistant-activity-view';
import { summarizeConversationChanges } from '@/lib/assistant-timeline';
import type { ProviderKeySummary } from '@/lib/tauri-adapter';
import ResizableRightPanel from '@/components/ui/ResizableRightPanel';
import ConnectionBanner from './ConnectionBanner';
import EngineRecoveryPage from './EngineRecoveryPage';
import CommandPalette, { type AssistantCommand } from './CommandPalette';
import type { ProviderWithModels } from './ModelSelectorDropdown';
import {
  useAssistantNavigation,
  useAssistantWorkspaceApi,
  type AssistantWorkspaceActions,
} from './AssistantWorkspaceContext';
import {
  ASSISTANT_LOCATE_EVENT,
  type AssistantLocateTarget,
} from '@/lib/assistant-notifications';
import {
  collectTempConversationIds,
  conversationsWithoutTemp,
  createTempConversationShell,
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

function mapSubagentSessions(raw: unknown): SubagentSession[] {
  const sessionsRaw =
    raw && typeof raw === 'object' && Array.isArray((raw as { sessions?: unknown }).sessions)
      ? (raw as { sessions: unknown[] }).sessions
      : Array.isArray(raw)
        ? raw
        : [];
  return sessionsRaw
    .filter((item): item is Record<string, unknown> => Boolean(item) && typeof item === 'object')
    .map((r) => ({
      id: String(r.id ?? ''),
      parentConversationId: String(
        r.parent_conversation_id ?? r.parentConversationId ?? '',
      ),
      childConversationId: String(
        r.child_conversation_id ?? r.childConversationId ?? '',
      ),
      parentRunId:
        r.parent_run_id != null || r.parentRunId != null
          ? String(r.parent_run_id ?? r.parentRunId)
          : null,
      taskCallId:
        r.task_call_id != null || r.taskCallId != null
          ? String(r.task_call_id ?? r.taskCallId)
          : null,
      name: String(r.name ?? r.task ?? r.id ?? ''),
      task: String(r.task ?? ''),
      status: String(r.status ?? 'open'),
      providerId: String(r.provider_id ?? r.providerId ?? ''),
      keyId: String(r.key_id ?? r.keyId ?? ''),
      modelId: String(r.model_id ?? r.modelId ?? ''),
      lastActivityAt:
        r.last_activity_at != null || r.lastActivityAt != null
          ? String(r.last_activity_at ?? r.lastActivityAt)
          : undefined,
      closedAt:
        r.closed_at != null || r.closedAt != null
          ? String(r.closed_at ?? r.closedAt)
          : null,
      error: r.error != null ? String(r.error) : null,
      createdAt:
        r.created_at != null || r.createdAt != null
          ? String(r.created_at ?? r.createdAt)
          : undefined,
      updatedAt:
        r.updated_at != null || r.updatedAt != null
          ? String(r.updated_at ?? r.updatedAt)
          : undefined,
    }))
    .filter((s) => s.id.length > 0);
}

function WorkbenchInner({ locale }: { locale: Locale }) {
  const zh = locale.startsWith('zh');
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
  const subSignalsRef = useRef<Record<string, { aborted: boolean }>>({});
  const resubAttemptsRef = useRef<Record<string, number>>({});

  const [providers, setProviders] = useState<ProviderWithModels[]>([]);
  const [providerReadiness, setProviderReadiness] = useState<ProviderReadiness>('no_provider');
  const [loadingMessages, setLoadingMessages] = useState(false);
  const [activeProjectPath, setActiveProjectPath] = useState<string | null>(null);
  const [registeredProjects, setRegisteredProjects] = useState<Array<{ id: string; path: string; lastOpenedAt?: string | null; label?: string }>>([]);
  const [pinnedConversationIds, setPinnedConversationIds] = useState<Set<string>>(new Set());
  const [rightPanelOpen, setRightPanelOpen] = useState(!state.view.rightCollapsed);
  const [loadingConversations, setLoadingConversations] = useState(true);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [fileContentsByPath, setFileContentsByPath] = useState<
    Record<string, { before: string; after: string }>
  >({});
  /** Project list / navigation always use root; timeline/input use surface. */
  const [selectedRootConversationId, setSelectedRootConversationId] = useState<string | null>(
    null,
  );
  const [selectedChildConversationId, setSelectedChildConversationId] = useState<string | null>(
    null,
  );
  const [subagentSessions, setSubagentSessions] = useState<SubagentSession[]>([]);
  const [switchKeySessionId, setSwitchKeySessionId] = useState<string | null>(null);
  const [assignmentKeyOptions, setAssignmentKeyOptions] = useState<AssignmentKeyOption[]>([]);

  // Keep root selection aligned with store activeConversationId (which is always the root).
  const storeActiveId = state.activeConversationId;
  useEffect(() => {
    if (storeActiveId !== selectedRootConversationId) {
      setSelectedRootConversationId(storeActiveId);
      // Switching another root conversation exits child view.
      setSelectedChildConversationId(null);
    }
  }, [storeActiveId]); // eslint-disable-line react-hooks/exhaustive-deps

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
    // eslint-disable-next-line react-hooks/exhaustive-deps -- maps + live/run tables are the true inputs
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
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [rootRunId, state.eventsByRun, state.childRunsByParent],
  );
  const selectedChildEvents = useMemo(
    () =>
      selectedChildConversationId
        ? selectRunEvents(state, surfaceRun?.id ?? null)
        : EMPTY_CHILD_EVENTS,
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [selectedChildConversationId, surfaceRun?.id, state.eventsByRun],
  );
  // Timeline / file events follow the surface (child when selected).
  const events = selectedChildConversationId ? selectedChildEvents : rootEvents;
  const artifacts = useMemo(
    () =>
      selectedChildConversationId
        ? selectArtifacts(state, surfaceRun?.id ?? null)
        : selectArtifactsForRunTree(state, rootRunId),
    // eslint-disable-next-line react-hooks/exhaustive-deps
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
    // eslint-disable-next-line react-hooks/exhaustive-deps
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
  const permission = interactions.find((i) => i.kind === 'permission');
  const askUser = interactions.find((i) => i.kind === 'ask_user');
  const planApproval = interactions.find((i) => i.kind === 'plan_approval');
  const subagentAssignment = interactions.find(
    (i): i is SubagentAssignmentInteraction => i.kind === 'subagent_assignment',
  );
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

  // Populate DiffViewer contents from file_changed events (+ optional fs read)
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      if (fileChanges.length === 0 && events.length === 0) {
        if (!cancelled) {
          setFileContentsByPath((prev) =>
            Object.keys(prev).length === 0 ? prev : {},
          );
        }
        return;
      }
      const readFile = async (path: string): Promise<string | null> => {
        try {
          const api = window.nativesAPI?.fs;
          if (!api?.readFile) return null;
          const result = await api.readFile(path);
          if (result == null) return null;
          if (typeof result === 'string') return result;
          const rec = result as Record<string, unknown>;
          if (typeof rec.content === 'string') return rec.content;
          return String(result);
        } catch {
          return null;
        }
      };
      const next = await hydrateFileDiffContents({
        fileChanges,
        events,
        readFile,
      });
      if (cancelled) return;
      // Bail when path→content map is unchanged. selectEventsForRunTree used to
      // allocate a fresh [] every render, which re-fired this effect and
      // setFileContentsByPath(newObject) in a tight loop (Maximum update depth).
      setFileContentsByPath((prev) => {
        const prevKeys = Object.keys(prev);
        const nextKeys = Object.keys(next);
        if (prevKeys.length === nextKeys.length) {
          let same = true;
          for (const key of nextKeys) {
            const a = prev[key];
            const b = next[key];
            if (!a || !b || a.before !== b.before || a.after !== b.after) {
              same = false;
              break;
            }
          }
          if (same) return prev;
        }
        return next;
      });
    })();
    return () => {
      cancelled = true;
    };
  }, [fileChanges, events]);

  const startSubscription = useCallback(
    async (runId: string, afterSequence: number) => {
      // Abort only this run's previous soft-resub loop; other runs keep polling.
      const prev = subSignalsRef.current[runId];
      if (prev) prev.aborted = true;
      const signal = { aborted: false };
      subSignalsRef.current[runId] = signal;
      try {
        await subscribeRun(
          gateway,
          dispatch,
          () => stateRef.current,
          runId,
          afterSequence,
          signal,
        );
      } catch {
        // Real transport/IPC errors flip connection in controller; soft-resub below if still active.
      }
      if (signal.aborted) return;
      const run = stateRef.current.runs[runId];
      if (!run || !isActiveRunStatus(run.status)) {
        delete resubAttemptsRef.current[runId];
        delete subSignalsRef.current[runId];
        return;
      }
      // Quiet soft resubscribe with backoff only. Normal long-poll / iterator end
      // without a terminal event must NOT promote the global connection to
      // "reconnecting" (that mis-fired after ~40 quiet polls during healthy runs).
      const nextSeq = stateRef.current.lastSequenceByRun[runId] ?? afterSequence;
      // Progress (received events) resets quiet-resub delay; no progress only stretches delay.
      if (nextSeq > afterSequence) {
        resubAttemptsRef.current[runId] = 0;
      }
      const n = (resubAttemptsRef.current[runId] ?? 0) + 1;
      resubAttemptsRef.current[runId] = n;
      const delay = Math.min(250 * n, 2000);
      if (typeof console !== 'undefined' && typeof console.debug === 'function') {
        console.debug('[assistant] soft-resubscribe', {
          runId,
          lastSequence: nextSeq,
          quietAttempt: n,
          reason: 'subscribe_ended_without_terminal',
        });
      }
      window.setTimeout(() => {
        if (!signal.aborted && subSignalsRef.current[runId] === signal) {
          void startSubscription(runId, nextSeq);
        }
      }, delay);
    },
    [gateway, dispatch],
  );

  const ensureRunSubscription = useCallback(
    (runId: string | null | undefined) => {
      if (!runId) return;
      const run = stateRef.current.runs[runId];
      if (!run || !isActiveRunStatus(run.status)) return;
      // Already tracking this run — leave the soft-resub loop alone.
      if (subSignalsRef.current[runId] && !subSignalsRef.current[runId]!.aborted) return;
      void startSubscription(runId, stateRef.current.lastSequenceByRun[runId] ?? 0);
    },
    [startSubscription],
  );

  const refreshSubagentSessions = useCallback(
    async (parentConversationId: string | null | undefined) => {
      if (!parentConversationId || isTempConversationId(parentConversationId)) {
        setSubagentSessions([]);
        return;
      }
      try {
        const raw = await gateway.request<unknown>('subagent.list', {
          conversation_id: parentConversationId,
          include_closed: true,
        });
        setSubagentSessions(mapSubagentSessions(raw));
      } catch {
        // Method may be unavailable on older daemons — fall back to child runs.
      }
    },
    [gateway],
  );

  const loadAssignmentKeys = useCallback(async () => {
    try {
      const list = await window.nativesAPI?.provider?.list?.();
      if (!Array.isArray(list)) {
        // Fall back to gateway provider list (has_active_key only; no status).
        const opts: AssignmentKeyOption[] = [];
        for (const p of providers) {
          if (!p.keys?.length) continue;
          for (const k of p.keys) {
            opts.push({
              providerId: p.id,
              providerName: p.name,
              keyId: k.id,
              keyLabel: k.label || k.maskedKey || k.id,
              modelId: p.defaultModel || p.models?.[0]?.id || '',
              models: (p.models ?? []).map((m) => ({
                id: m.id,
                displayName: m.displayName,
              })),
              isActive: true,
              status: 'valid',
            });
          }
        }
        setAssignmentKeyOptions(opts);
        return;
      }
      const opts: AssignmentKeyOption[] = [];
      for (const p of list) {
        const provider = p as {
          id: string;
          displayName?: string;
          name?: string;
          defaultModel?: string | null;
          models?: Array<{ id: string; displayName?: string | null }>;
          keys?: Array<
            Partial<ProviderKeySummary> & {
              id: string;
              label?: string;
              maskedKey?: string;
              isActive?: boolean;
              status?: ProviderKeySummary['status'] | string;
            }
          >;
        };
        const models = (provider.models ?? []).map((m) => ({
          id: m.id,
          displayName: m.displayName ?? undefined,
        }));
        for (const k of provider.keys ?? []) {
          // Only active + validated keys for random/custom; keep others out of the pool.
          const active = k.isActive !== false;
          if (!active) continue;
          if (k.status != null && k.status !== 'valid') continue;
          opts.push({
            providerId: provider.id,
            providerName: provider.displayName || provider.name || provider.id,
            keyId: k.id,
            keyLabel: k.label || k.maskedKey || k.id,
            modelId: provider.defaultModel || models[0]?.id || '',
            models,
            isActive: true,
            status: (k.status as AssignmentKeyOption['status']) ?? 'valid',
          });
        }
      }
      setAssignmentKeyOptions(opts);
    } catch {
      setAssignmentKeyOptions([]);
    }
  }, [providers]);

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
      for (const signal of Object.values(subSignalsRef.current)) {
        signal.aborted = true;
      }
      subSignalsRef.current = {};
      void gateway.disconnect();
    };
  }, [gateway, dispatch, toast]);

  // Multi-run subscription: keep root + active child runs subscribed without cancelling others.
  // Depend on status signatures (not array/object identity) so empty selectChildRuns
  // / store map replacement on unrelated ticks cannot thrash soft-resubscribe.
  const childrenSubKey = children
    .map((ch) => `${ch.id}:${ch.status}`)
    .sort()
    .join('|');
  const subagentSubKey = subagentSessions
    .map((s) => {
      const childRunId = state.activeRunByConversation[s.childConversationId] ?? '';
      const status = childRunId ? state.runs[childRunId]?.status ?? '' : '';
      return `${s.childConversationId}:${childRunId}:${status}`;
    })
    .sort()
    .join('|');
  useEffect(() => {
    const wanted = new Set<string>();
    if (rootRun && isActiveRunStatus(rootRun.status)) wanted.add(rootRun.id);
    // children / sessions read from latest render via closure; deps are signature keys.
    for (const ch of children) {
      if (isActiveRunStatus(String(ch.status))) wanted.add(ch.id);
    }
    for (const s of subagentSessions) {
      const childRunId = stateRef.current.activeRunByConversation[s.childConversationId];
      if (childRunId) {
        const run = stateRef.current.runs[childRunId];
        if (run && isActiveRunStatus(run.status)) wanted.add(childRunId);
      }
    }
    // Drop soft-resub loops for runs no longer in the wanted set (switch session /
    // terminal child / parent left). Without this, every historical active run kept
    // polling → multi-subscription thrash and wasted gateway traffic.
    for (const runId of Object.keys(subSignalsRef.current)) {
      if (!wanted.has(runId)) {
        const signal = subSignalsRef.current[runId];
        if (signal) signal.aborted = true;
        delete subSignalsRef.current[runId];
        delete resubAttemptsRef.current[runId];
      }
    }
    for (const runId of wanted) {
      ensureRunSubscription(runId);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps -- children/subagentSessions captured; identity churn ignored via *SubKey
  }, [rootRun?.id, rootRun?.status, childrenSubKey, subagentSubKey, ensureRunSubscription]);

  // Load subagent.list when root conversation is visible. Avoid re-listing on every
  // transient status flicker while the parent is already waiting_subagent / running —
  // use a coarse phase so assignment → first child does not hammer subagent.list.
  const rootRunPhase =
    rootRun?.status === 'waiting_subagent' || rootRun?.status === 'running'
      ? 'active'
      : rootRun?.status ?? 'idle';
  useEffect(() => {
    void refreshSubagentSessions(rootConversationId);
  }, [rootConversationId, refreshSubagentSessions, rootRunPhase]);

  // Heartbeat: touch only the root conversation every 30s while assistant page is visible.
  // Do NOT loop over every subagent — daemon scopes keepalive by parent conversation_id.
  useEffect(() => {
    if (!rootConversationId || isTempConversationId(rootConversationId)) return;
    let cancelled = false;
    const tick = () => {
      if (cancelled) return;
      if (typeof document !== 'undefined' && document.visibilityState === 'hidden') return;
      void gateway
        .request('subagent.touch', {
          conversation_id: rootConversationId,
        })
        .catch(() => undefined);
    };
    tick();
    const handle = window.setInterval(tick, 30_000);
    return () => {
      cancelled = true;
      window.clearInterval(handle);
    };
  }, [rootConversationId, gateway]);

  // Prefetch assignment keys when an assignment interaction appears.
  useEffect(() => {
    if (subagentAssignment || switchKeySessionId) {
      void loadAssignmentKeys();
    }
  }, [subagentAssignment, switchKeySessionId, loadAssignmentKeys]);

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
        pinned: pinnedConversationIds.has(c.id),
      })),
      registeredProjects.map((p) => ({ path: p.path, lastOpenedAt: (p as { lastOpenedAt?: string | null; last_opened_at?: string | null }).lastOpenedAt ?? (p as { last_opened_at?: string | null }).last_opened_at ?? null, label: p.label })),
      zh ? '未关联项目' : 'Unassigned',
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
            })),
            seedPaths,
            zh ? '未关联项目' : 'Unassigned',
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
    zh,
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
          void refreshSubagentSessions(id);
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
        void refreshSubagentSessions(id);
      } catch (err) {
        toast(classifyError(err).userMessage, 'error');
      } finally {
        setLoadingMessages(false);
      }
    },
    [gateway, dispatch, toast, ensureRunSubscription, refreshSubagentSessions],
  );

  // Sync external sidebar selection (persisted sessions only — temps hydrate below).
  useEffect(() => {
    const selected = navigation.selectedId;
    if (!selected || selected === rootConversationId) return;
    if (isTempConversationId(selected)) return;
    void selectConversation(selected);
  }, [navigation.selectedId]); // eslint-disable-line react-hooks/exhaustive-deps

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
  }, [navigation.tempSession?.conversation.id, loadingConversations]); // eslint-disable-line react-hooks/exhaustive-deps

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
  }, [navigation.pendingCreateProjectPath]); // eslint-disable-line react-hooks/exhaustive-deps

  const handleSend = useCallback(
    async (draft: AssistantDraft, forceImmediate = false): Promise<boolean> => {
      // Sends always target the surface conversation (child when selected).
      let conversationId = activeId;
      const pick = resolveModelSelection(providers, {
        providerId: activeConversation?.providerId,
        modelId: activeConversation?.modelId,
      });
      // Stale/deleted provider: block send and require explicit re-select (no ghost remap).
      if (
        activeConversation?.providerId &&
        !providers.some((p) => p.id === activeConversation.providerId)
      ) {
        toast(
          zh
            ? '当前会话的供应商已失效，请重新选择供应商和模型'
            : 'This conversation’s provider is no longer available. Re-select provider and model.',
          'error',
        );
        return false;
      }
      const providerId = pick?.providerId ?? '';
      const modelId = pick?.modelId ?? '';
      if (!providerId || !modelId || providerReadiness !== 'ready') {
        toast(zh ? '请先配置供应商和模型' : 'Configure provider and model first', 'error');
        return false;
      }

      try {
        if (!conversationId || isTempConversationId(conversationId)) {
          const title = draft.content.trim().slice(0, 30) || t(locale, 'assistant.newConversation');
          const createdRaw = await gateway.request<Record<string, unknown> | Conversation>(
            'conversation.create',
            {
              mode: 'agent',
              title,
              provider_id: providerId,
              model_id: modelId,
              // Always use the normalized active project path (from project.register).
              project_id: activeProjectPath,
              permission_profile_id: activeConversation?.permissionProfileId ?? 'ask',
            },
          );
          // Host returns snake_case; map so providerId/modelId actually land in store.
          const created =
            createdRaw && typeof createdRaw === 'object' && 'providerId' in createdRaw
              ? (createdRaw as Conversation)
              : mapWireConversation((createdRaw ?? {}) as Record<string, unknown>);
          // Prefer the selection the user just confirmed if wire fields came back empty.
          const conversation: Conversation = {
            ...created,
            providerId: created.providerId || providerId,
            modelId: created.modelId || modelId,
            projectId: created.projectId ?? activeProjectPath,
            permissionProfileId:
              created.permissionProfileId ??
              activeConversation?.permissionProfileId ??
              'ask',
          };
          const previousTempId =
            activeId && isTempConversationId(activeId) ? activeId : null;
          dispatch({ type: 'conversations/upsert', conversation });
          dispatch({ type: 'conversations/setActive', id: conversation.id });
          // Atomic temp → persisted: drop the local shell so the session appears once.
          if (previousTempId && previousTempId !== conversation.id) {
            const tempDraft = stateRef.current.composerByConversation[previousTempId];
            if (tempDraft) {
              dispatch({
                type: 'composer/set',
                conversationId: conversation.id,
                draft: tempDraft,
              });
              dispatch({ type: 'composer/clear', conversationId: previousTempId });
            }
            dispatch({ type: 'conversations/remove', id: previousTempId });
          }
          // Clear root-level temp shell so sidebar/remount do not resurrect it.
          setSelectedRootConversationId(conversation.id);
          setSelectedChildConversationId(null);
          publishNavigation((prev) => ({
            ...prev,
            tempSession: null,
            selectedId: conversation.id,
            activeProjectPath: activeProjectPath ?? prev.activeProjectPath,
          }));
          conversationId = conversation.id;
        }

        const result = await sendOrQueue(gateway, dispatch, stateRef.current, {
          conversationId,
          content: draft.content,
          providerId,
          modelId,
          projectPath:
            activeProjectPath ??
            stateRef.current.conversations[conversationId]?.projectId ??
            null,
          attachments: draft.attachments.map((a) => ({
            path: a.path,
            name: a.name,
            mimeType: a.mimeType,
            size: a.size,
          })),
          forceImmediate,
          // Preferred runtime from RuntimePanel (persisted); omit → daemon default native.
          runtimeId: loadPreferredRuntimeId(),
        });

        if (!result.queued && result.runId) {
          ensureRunSubscription(result.runId);
        }
        return true;
      } catch (err) {
        // Create/send failure keeps the temp page and user input intact.
        toast(classifyError(err).userMessage, 'error');
        return false;
      }
    },
    [
      activeId,
      activeConversation,
      providers,
      providerReadiness,
      gateway,
      dispatch,
      toast,
      zh,
      locale,
      activeProjectPath,
      ensureRunSubscription,
      publishNavigation,
    ],
  );

  const handleStop = useCallback(async () => {
    if (!activeRunId) return;
    try {
      await cancelRun(gateway, dispatch, activeRunId);
    } catch (err) {
      toast(classifyError(err).userMessage, 'error');
    }
  }, [
    // React Compiler cannot prove selector results immutable; callback dependencies are intentional.
    // eslint-disable-next-line react-hooks/preserve-manual-memoization
    activeRunId, gateway, dispatch, toast,
  ]);

  const handleRetry = useCallback(async () => {
    if (!activeRunId) return;
    try {
      const newId = await retryRun(gateway, dispatch, activeRunId);
      ensureRunSubscription(newId);
    } catch (err) {
      toast(classifyError(err).userMessage, 'error');
    }
  }, [
    // eslint-disable-next-line react-hooks/preserve-manual-memoization
    activeRunId, gateway, dispatch, toast, ensureRunSubscription,
  ]);

  const handlePermission = useCallback(
    async (requestId: string, approved: boolean, scope?: string) => {
      // Throw so PermissionRequestCard can unlock + show in-card error.
      await respondPermission(
        gateway,
        dispatch,
        requestId,
        approved,
        scope ?? 'once',
        activeRunId,
      );
    },
    [
      gateway, dispatch,
      // eslint-disable-next-line react-hooks/preserve-manual-memoization
      activeRunId,
    ],
  );

  const handleSelectSubagent = useCallback(
    async (id: string) => {
      const session = subagentSessions.find((s) => s.id === id);
      const childId = session?.childConversationId;
      if (!childId) {
        // Legacy child-run id without session row — still mark selection for tasks panel.
        setSelectedChildConversationId(null);
        return;
      }
      setSelectedChildConversationId(childId);
      setLoadingMessages(true);
      try {
        // Optional selective touch only when user focuses a specific subagent.
        void gateway
          .request('subagent.touch', { id: session.id })
          .catch(() => undefined);
        // Load hidden child conversation history without flipping sidebar root.
        const snapshot = await gateway.getSnapshot(childId);
        dispatch({ type: 'snapshot/apply', snapshot });
        ensureRunSubscription(stateRef.current.activeRunByConversation[childId]);
      } catch (err) {
        toast(classifyError(err).userMessage, 'error');
      } finally {
        setLoadingMessages(false);
      }
    },
    [subagentSessions, gateway, dispatch, toast, ensureRunSubscription],
  );

  const handleBackToMain = useCallback(() => {
    setSelectedChildConversationId(null);
  }, []);

  const handleAssignmentConfirm = useCallback(
    async (payload: SubagentAssignmentConfirmPayload) => {
      const wireAssignments = payload.assignments.map((a) => ({
        call_id: a.callId,
        provider_id: a.providerId,
        key_id: a.keyId,
        model_id: a.modelId,
      }));
      const wirePool = payload.pool.map((b) => ({
        provider_id: b.providerId,
        key_id: b.keyId,
        model_id: b.modelId,
      }));
      const wireBindings = payload.bindings.map((b) => ({
        provider_id: b.providerId,
        key_id: b.keyId,
        model_id: b.modelId,
      }));

      if (payload.sessionId) {
        const result = (await gateway.request('subagent.switchRoute', {
          conversation_id: rootConversationId,
          session_id: payload.sessionId,
          mode: payload.mode,
          bindings: wireBindings,
          assignments: wireAssignments,
          pool: wirePool,
        })) as { restarted_run_id?: string; restartedRunId?: string } | null;
        setSwitchKeySessionId(null);
        const restarted =
          result?.restarted_run_id ?? result?.restartedRunId ?? null;
        if (restarted) {
          ensureRunSubscription(restarted);
        }
        void refreshSubagentSessions(rootConversationId);
        return;
      }
      if (!subagentAssignment) {
        throw new Error(t(locale, 'assistant.subagentAssignment.errorFallback'));
      }
      await gateway.request('interaction.respond', {
        id: subagentAssignment.id,
        run_id: subagentAssignment.runId,
        response: {
          approved: true,
          mode: payload.mode,
          assignments: wireAssignments,
          pool: wirePool,
          // Legacy flat bindings still accepted by older daemons.
          bindings: wireBindings,
          conversation_id:
            subagentAssignment.conversationId ?? rootConversationId ?? undefined,
        },
      });
      dispatch({ type: 'interaction/remove', id: subagentAssignment.id });
      void refreshSubagentSessions(rootConversationId);
    },
    [
      gateway,
      rootConversationId,
      subagentAssignment,
      dispatch,
      refreshSubagentSessions,
      locale,
      ensureRunSubscription,
    ],
  );

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
      },      removeProject: (path) => {
        void (async () => {
          try {
            const projects = (await window.nativesAPI?.project?.list?.()) ?? [];
            const match = projects.find((p) => p.path === path || p.id === path);
            if (match?.id) {
              await window.nativesAPI?.project?.remove?.(match.id);
            } else if (path) {
              await window.nativesAPI?.project?.remove?.(path);
            }
          } catch (err) {
            toast(classifyError(err).userMessage, 'error');
          }
          const next = (await window.nativesAPI?.project?.list?.()) ?? [];
          setRegisteredProjects(next);
          if (activeProjectPath === path) {
            setActiveProjectPath(null);
          }
        })();
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

  // Keyboard shortcuts + notification deep-link
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const meta = e.metaKey || e.ctrlKey;
      if (meta && e.key === '.') {
        e.preventDefault();
        void handleStop();
      }
      if (meta && e.shiftKey && e.key.toLowerCase() === 'p') {
        e.preventDefault();
        setPaletteOpen(true);
      }
      if (meta && e.shiftKey && e.key.toLowerCase() === 'i') {
        e.preventDefault();
        setRightPanelOpen(true);
        dispatch({ type: 'view/patch', patch: { rightCollapsed: false, inspectorTab: 'run' } });
      }
      if (meta && e.shiftKey && e.key.toLowerCase() === 't') {
        e.preventDefault();
        setRightPanelOpen(true);
        dispatch({ type: 'view/patch', patch: { rightCollapsed: false, inspectorTab: 'tasks' } });
      }
      if (e.key === 'Escape') {
        // close popovers only — never cancel run
        setPaletteOpen(false);
      }
    };
    const onLocate = (ev: Event) => {
      const detail = (ev as CustomEvent<AssistantLocateTarget>).detail;
      if (!detail?.conversationId) return;
      void selectConversation(detail.conversationId);
      if (detail.interactionId) {
        // focus interaction by leaving it in queue — card already binds run
      }
      setRightPanelOpen(true);
    };
    window.addEventListener('keydown', onKey);
    window.addEventListener(ASSISTANT_LOCATE_EVENT, onLocate as EventListener);
    return () => {
      window.removeEventListener('keydown', onKey);
      window.removeEventListener(ASSISTANT_LOCATE_EVENT, onLocate as EventListener);
    };
  }, [handleStop, dispatch, selectConversation]);

  const commands: AssistantCommand[] = useMemo(() => {
    const busy = isStreaming;
    return [
      {
        id: 'new',
        label: zh ? '新会话' : 'New conversation',
        run: () => {
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
      },      {
        id: 'stop',
        label: zh ? '停止当前 Run' : 'Stop current run',
        shortcut: '⌘.',
        disabledReason: busy ? undefined : zh ? '当前无运行中的任务' : 'No active run',
        run: () => void handleStop(),
      },
      {
        id: 'retry',
        label: zh ? '重试' : 'Retry',
        disabledReason: activeRunStatus === 'failed' || activeRunStatus === 'interrupted'
          ? undefined
          : zh
            ? '仅失败/中断可重试'
            : 'Only failed/interrupted runs',
        run: () => void handleRetry(),
      },
      {
        id: 'inspector',
        label: zh ? '打开 Inspector' : 'Open Inspector',
        shortcut: '⌘⇧I',
        run: () => {
          setRightPanelOpen(true);
          dispatch({ type: 'view/patch', patch: { rightCollapsed: false } });
        },
      },
      {
        id: 'tasks',
        label: zh ? '任务面板' : 'Tasks panel',
        shortcut: '⌘⇧T',
        run: () => {
          setRightPanelOpen(true);
          dispatch({ type: 'view/patch', patch: { inspectorTab: 'tasks', rightCollapsed: false } });
        },
      },
      {
        id: 'background',
        label: zh ? '转后台' : 'Background run',
        disabledReason: busy ? undefined : zh ? '无活动 Run' : 'No active run',
        run: () => {
          // Background mode is represented by the live subscription/event stream;
          // it is not a separate engine RPC.
          if (activeRunId) {
            ensureRunSubscription(activeRunId);
          }
        },
      },
      {
        id: 'fork',
        label: zh ? 'Fork 会话' : 'Fork conversation',
        disabledReason: activeId && !isTempConversationId(activeId) ? undefined : zh ? '无会话' : 'No conversation',
        run: () => {
          if (!activeId || isTempConversationId(activeId)) return;
          void gateway.request('conversation.fork', { conversation_id: activeId }).then((forked) => {
            const c = forked as Conversation;
            if (c?.id) {
              dispatch({ type: 'conversations/upsert', conversation: c });
              void selectConversation(c.id);
            }
          });
        },
      },
    ];
  }, [
    zh,
    isStreaming,
    // Selector-derived primitives are immutable for this render.
    // eslint-disable-next-line react-hooks/preserve-manual-memoization
    activeRunId,
    // eslint-disable-next-line react-hooks/preserve-manual-memoization
    activeRunStatus,
    handleStop,
    handleRetry,
    dispatch,
    gateway,
    activeId,
    selectConversation,
    providers,
    locale,
    activeProjectPath,
    publishNavigation,
    ensureRunSubscription,
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

  const permissionProfile = normalizePermissionProfile(
    activeConversation?.permissionProfileId ?? 'ask',
  );

  // Composer-blocking interactions: hide MessageInput so the user cannot type/stop/send.
  const composerBlockedByInteraction = Boolean(
    (permission && permission.kind === 'permission') ||
      (askUser && askUser.kind === 'ask_user'),
  );

  const showRight =
    rightPanelOpen &&
    (state.view.layoutBreakpoint === 'full' || state.view.layoutBreakpoint === 'drawer-right');

  const recoveryMode = needsEngineRecovery(state.connection, state.capabilities);
  const allowRewind = canRewind(state.capabilities);

  const handleRollbackChanges = useCallback(
    async (changes: Array<{ path: string; runId?: string }>): Promise<boolean> => {
      const byRun = new Map<string, string[]>();
      for (const change of changes) {
        const runId = change.runId ?? (rootRun ?? activeRun)?.id;
        if (!runId || !change.path) continue;
        const paths = byRun.get(runId) ?? [];
        if (!paths.includes(change.path)) paths.push(change.path);
        byRun.set(runId, paths);
      }
      if (byRun.size === 0) {
        toast(zh ? '没有可撤销的文件变更' : 'No reversible file changes found', 'error');
        return false;
      }
      try {
        const previews = await Promise.all(
          [...byRun.entries()].map(async ([runId, paths]) => {
            const preview = await gateway.request<Record<string, unknown>>('workspace.restorePreview', {
              run_id: runId,
              paths,
            });
            const checkpointId = String(preview?.checkpoint_id ?? preview?.checkpointId ?? '');
            const conflicts = Array.isArray(preview?.conflicts) ? preview.conflicts : [];
            if (!checkpointId || conflicts.length > 0) {
              throw new Error(zh ? '文件在执行后已被其他修改，无法安全撤销' : 'Files changed after this run; undo was refused safely');
            }
            return { runId, paths, checkpointId };
          }),
        );
        for (const preview of previews) {
          await gateway.request('workspace.restore', {
            run_id: preview.runId,
            checkpoint_id: preview.checkpointId,
            paths: preview.paths,
            conflict_policy: 'fail',
          });
        }
        toast(zh ? '已撤销本次对话的文件修改' : 'Conversation file changes undone', 'success');
        return true;
      } catch (err) {
        toast(classifyError(err).userMessage, 'error');
        return false;
      }
    },
    [rootRun, activeRun, gateway, toast, zh],
  );

  const handleCopyDiagnostics = useCallback(() => {
    const text = buildDiagnosticsText({
      connection: state.connection,
      connectionError: state.connectionError,
      protocolVersion: state.capabilities?.protocolVersion ?? null,
      methodsCount: state.capabilities?.methods?.length ?? 0,
      reconnectAttempts: state.reconnectAttempts,
    });
    void copyToClipboard(text).then((ok) => {
      if (ok) toast(zh ? '诊断已复制' : 'Diagnostics copied', 'success');
      else toast(zh ? '复制失败' : 'Copy failed', 'error');
    });
  }, [
    state.connection,
    state.connectionError,
    state.capabilities,
    state.reconnectAttempts,
    toast,
    zh,
  ]);

  const handleRetryConnection = useCallback(() => {
    void connectWorkspace(gateway, dispatch).catch((err) => {
      toast(classifyError(err).userMessage, 'error');
    });
  }, [gateway, dispatch, toast]);

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
          <div className="flex items-center justify-end gap-1 border-b border-[var(--border)] px-3 py-1">
            <button
              type="button"
              onClick={() => setPaletteOpen(true)}
              className="rounded px-2 py-1 text-[11px] text-[var(--text-disabled)] hover:bg-[var(--surface-hover)] hover:text-[var(--text-secondary)]"
              title={zh ? '命令面板 ⌘⇧P' : 'Command palette ⌘⇧P'}
            >
              {zh ? '命令' : 'Commands'}
            </button>
            <button
              type="button"
              onClick={() => {
                setRightPanelOpen((v) => {
                  const next = !v;
                  dispatch({ type: 'view/patch', patch: { rightCollapsed: !next } });
                  return next;
                });
              }}
              className="rounded p-1.5 text-[var(--text-disabled)] hover:bg-[var(--surface-hover)] hover:text-[var(--text-secondary)]"
              title={zh ? '活动面板' : 'Activity panel'}
              aria-label={zh ? '切换活动面板' : 'Toggle activity panel'}
            >
              {showRight ? <PanelRightClose size={16} /> : <PanelRightOpen size={16} />}
            </button>
          </div>

          <div className="min-h-0 flex-1">
            <ConversationTimeline
              messages={timelineMessages}
              eventsByRun={state.eventsByRun}
              changeEvents={events}
              fileChanges={fileChanges}
              onRollbackChanges={allowRewind ? handleRollbackChanges : undefined}
              loading={loadingMessages}
              locale={locale}
              onRetry={() => void handleRetry()}
              hasMoreOlder={Boolean(messagePageInfo?.hasMore)}
              loadingOlder={loadingOlderMessages}
              onLoadOlder={() => void loadOlderMessages()}
            />
          </div>

          {/* Composer interactions (permission / ask_user) replace MessageInput below. */}

          {planApproval && planApproval.kind === 'plan_approval' && (
            <div className="mx-4 mb-2 max-h-48 overflow-y-auto rounded-lg border border-[var(--border)] bg-[var(--surface)] p-3 text-sm">
              <div className="font-medium">{planApproval.title}</div>
              <pre className="mt-2 whitespace-pre-wrap text-xs text-[var(--text-secondary)]">
                {planApproval.planMarkdown}
              </pre>
              <div className="mt-2 flex gap-2">
                <button
                  type="button"
                  className="rounded bg-[var(--primary)] px-3 py-1 text-white"
                  onClick={() =>
                    void gateway
                      .request('interaction.respond', {
                        id: planApproval.id,
                        approved: true,
                        run_id: planApproval.runId,
                      })
                      .then(() => dispatch({ type: 'interaction/remove', id: planApproval.id }))
                  }
                >
                  {zh ? '批准计划' : 'Approve plan'}
                </button>
                <button
                  type="button"
                  className="rounded border border-[var(--border)] px-3 py-1"
                  onClick={() =>
                    void gateway
                      .request('interaction.respond', {
                        id: planApproval.id,
                        approved: false,
                        run_id: planApproval.runId,
                      })
                      .then(() => dispatch({ type: 'interaction/remove', id: planApproval.id }))
                  }
                >
                  {zh ? '拒绝' : 'Reject'}
                </button>
              </div>
            </div>
          )}

          {isGoalMode ? (
            <GoalStatusBar
              goalTitle={activeConversation?.title ?? (zh ? 'Goal 任务' : 'Goal')}
              instruction={goalInstruction}
              run={activeRun}
              locale={locale}
              tokenLabel={
                contextUsage ? `${contextUsage.usedTokens} tokens` : undefined
              }
              canResume={goalCanResume}
              onPause={() => void handleStop()}
              onResume={() => void handleRetry()}
              onDelete={() => {
                if (!activeId) return;
                const ok = window.confirm(
                  zh ? '确定删除此 Goal 会话？' : 'Delete this goal conversation?',
                );
                if (!ok) return;
                void gateway
                  .request('conversation.delete', { id: activeId })
                  .then(() => {
                    dispatch({ type: 'conversations/remove', id: activeId });
                    if (stateRef.current.activeConversationId === activeId) {
                      dispatch({ type: 'conversations/setActive', id: null });
                    }
                  })
                  .catch((err) => toast(classifyError(err).userMessage, 'error'));
              }}
            />
          ) : null}

          {/* Prompt queue is for in-flight multi-send, not goal chrome. */}
          <PromptQueuePanel
            items={isGoalMode ? [] : promptQueue}
            locale={locale}
            onEdit={(id, content) =>
              void gateway
                .request('promptQueue.update', {
                  id,
                  conversation_id: activeId,
                  content,
                })
                .then(() =>
                  gateway
                    .request('promptQueue.list', { conversation_id: activeId })
                    .then((items) =>
                      dispatch({
                        type: 'promptQueue/set',
                        conversationId: activeId!,
                        items: items as typeof promptQueue,
                      }),
                    ),
                )
            }
            onRemove={(id) =>
              void gateway.request('promptQueue.remove', { id }).then(() =>
                dispatch({
                  type: 'promptQueue/set',
                  conversationId: activeId!,
                  items: promptQueue.filter((i) => i.id !== id),
                }),
              )
            }
            onSendNow={(id) => void gateway.request('promptQueue.sendNow', { id })}
            onReorder={(ids) =>
              void gateway.request('promptQueue.reorder', {
                conversation_id: activeId,
                ids,
              })
            }
          />

          {composerBlockedByInteraction ? (
            <div className={`${COMPOSER_COLUMN_CLASS} pb-5 pt-2`} data-composer-interaction-overlay>
              {permission && permission.kind === 'permission' ? (
                <PermissionRequestCard
                  request={{
                    id: permission.id,
                    toolName: permission.toolName,
                    reason: permission.reason,
                    input: permission.input,
                    status: 'pending',
                    createdAt: permission.createdAt,
                  }}
                  locale={locale}
                  onApprove={(id, scope) => {
                    // Must return the Promise so the card can await + recover on failure.
                    return handlePermission(id, true, scope);
                  }}
                  onReject={(id) => handlePermission(id, false)}
                />
              ) : null}
              {askUser && askUser.kind === 'ask_user' ? (
                <AskUserPromptCard
                  interaction={askUser}
                  locale={locale}
                  onAnswer={async (id, answer) => {
                    await gateway.request('interaction.respond', {
                      id,
                      answer,
                      run_id: askUser.runId,
                    });
                    dispatch({ type: 'interaction/remove', id });
                  }}
                  onCancel={async (id) => {
                    await gateway.request('interaction.respond', {
                      id,
                      cancelled: true,
                      run_id: askUser.runId,
                    });
                    dispatch({ type: 'interaction/remove', id });
                  }}
                />
              ) : null}
            </div>
          ) : (
          <MessageInput
            locale={locale}
            onSend={(draft) => handleSend(draft, false)}
            onForceSend={(draft) => handleSend(draft, true)}
            onInterject={
              canInterject(state.capabilities) && isStreaming && activeId && !isTempConversationId(activeId)
                ? async (content) => {
                    try {
                      await gateway.request('promptQueue.interject', {
                        conversation_id: activeId,
                        content,
                      });
                      return true;
                    } catch (err) {
                      toast(classifyError(err).userMessage, 'error');
                      return false;
                    }
                  }
                : undefined
            }
            onStop={() => void handleStop()}
            isStreaming={isStreaming}
            allowQueueWhileStreaming
            inputDisabledReason={
              providerReadiness === 'no_provider'
                ? 'no_provider'
                : providerReadiness === 'no_model'
                  ? 'no_model'
                  : null
            }
            permissionProfile={permissionProfile}
            onPermissionChange={async (profile: AssistantPermissionProfile) => {
              // Always update local conversation state so the picker reflects the choice.
              if (activeConversation) {
                dispatch({
                  type: 'conversations/upsert',
                  conversation: { ...activeConversation, permissionProfileId: profile },
                });
              } else if (activeId) {
                // Temp conversation shell without full object — create minimal patch via store.
                const existing = stateRef.current.conversations[activeId];
                if (existing) {
                  dispatch({
                    type: 'conversations/upsert',
                    conversation: { ...existing, permissionProfileId: profile },
                  });
                }
              }
              // Persist only when the conversation is real on the host.
              if (!activeId || isTempConversationId(activeId)) return;
              try {
                await gateway.request('conversation.update_permission', {
                  id: activeId,
                  permission_profile_id: profile,
                });
              } catch (err) {
                toast(classifyError(err).userMessage, 'error');
              }
            }}
            providers={providers}
            selectedProviderId={modelSelection?.providerId ?? providers[0]?.id ?? ''}
            selectedModel={modelSelection?.modelId}
            onSelectModel={(providerId, modelId) => {
              const now = new Date().toISOString();
              // Always write selection into store — even without an active conversation —
              // so the picker echoes immediately and temp shells stay editable.
              if (activeConversation) {
                dispatch({
                  type: 'conversations/upsert',
                  conversation: {
                    ...activeConversation,
                    providerId,
                    modelId,
                    updatedAt: now,
                  },
                });
              } else if (activeId) {
                const existing = stateRef.current.conversations[activeId];
                if (existing) {
                  dispatch({
                    type: 'conversations/upsert',
                    conversation: { ...existing, providerId, modelId, updatedAt: now },
                  });
                } else {
                  const shell = createTempConversationShell({
                    id: activeId,
                    projectId: activeProjectPath,
                    title: t(locale, 'assistant.newConversation'),
                    providerId,
                    modelId,
                    now,
                  });
                  dispatch({ type: 'conversations/upsert', conversation: shell });
                }
              } else {
                // No conversation yet: create a temp shell so selection has a home.
                const session = createTempSession({
                  projectId: activeProjectPath,
                  title: t(locale, 'assistant.newConversation'),
                  providerId,
                  modelId,
                  now,
                });
                dispatch({ type: 'conversations/upsert', conversation: session.conversation });
                dispatch({ type: 'conversations/setActive', id: session.conversation.id });
                publishNavigation((prev) => ({
                  ...prev,
                  selectedId: session.conversation.id,
                  tempSession: session,
                }));
              }
              if (activeId && !isTempConversationId(activeId)) {
                void gateway
                  .request('conversation.update_model', {
                    id: activeId,
                    provider_id: providerId,
                    model_id: modelId,
                  })
                  .catch((err) => toast(classifyError(err).userMessage, 'error'));
              }
            }}
            draftText={selectComposerDraft(state, activeId).text}
            draftKey={activeId}
            onDraftChange={(text, conversationId) => {
              // Prefer the id captured at keystroke time so a switch mid-debounce
              // still writes the previous conversation's draft.
              const id = conversationId ?? activeId;
              if (!id) return;
              dispatch({ type: 'composer/set', conversationId: id, draft: { text } });
              // Temp shell remount restore (settings round-trip). Debounced by
              // MessageInput; publishNavigation bails without re-rendering the
              // shell tree when only tempSession.draft.text changes.
              if (isTempConversationId(id)) {
                publishNavigation((prev) => {
                  if (!prev.tempSession || prev.tempSession.conversation.id !== id) {
                    return prev;
                  }
                  if (prev.tempSession.draft.text === text) return prev;
                  return {
                    ...prev,
                    tempSession: {
                      ...prev.tempSession,
                      draft: {
                        ...prev.tempSession.draft,
                        text,
                        updatedAt: new Date().toISOString(),
                      },
                    },
                  };
                });
              }
            }}
            projectPath={activeProjectPath}
            subagents={composerSubagents}
            activeSubagent={activeComposerSubagent}
            onSelectSubagent={(id) => void handleSelectSubagent(id)}
            changeSummary={{
              fileCount: conversationChangeSummary.files.length,
              additions: conversationChangeSummary.additions,
              deletions: conversationChangeSummary.deletions,
            }}
          />
          )}
        </div>

        {showRight && (
          <ResizableRightPanel
            open={showRight}
            width={state.view.rightWidth || 320}
            onResize={(width) => dispatch({ type: 'view/patch', patch: { rightWidth: width } })}
            onClose={() => {
              setRightPanelOpen(false);
              dispatch({ type: 'view/patch', patch: { rightCollapsed: true } });
            }}
            title={zh ? '活动' : 'Activity'}
            ariaLabel={zh ? '助理右侧栏' : 'Assistant inspector'}
            resizeLabel={zh ? '调整宽度' : 'Resize panel'}
            resizeHint={zh ? '拖动调整宽度 · 双击重置' : 'Drag to resize · double-click to reset'}
            closeLabel={zh ? '关闭' : 'Close'}
            scrollBody={false}
          >
            <ActivityInspector
              run={rootRun ?? activeRun}
              events={rootEvents}
              selectedChildEvents={selectedChildEvents}
              mainTodos={mainTodos}
              artifacts={artifacts}
              children={children}
              fileChanges={fileChanges}
              contextUsage={contextUsage}
              locale={locale}
              activeTab={state.view.inspectorTab}
              onTabChange={(tab: InspectorTab) =>
                dispatch({ type: 'view/patch', patch: { inspectorTab: tab } })
              }
              onRetry={() => void handleRetry()}
              onOpenArtifact={(a) =>
                void gateway.request('artifact.open', { id: a.id, path: a.path })
              }
              onRevealArtifact={(a) =>
                void gateway.request('artifact.reveal', { id: a.id, path: a.path })
              }
              fileContentsByPath={fileContentsByPath}
              onOpenFile={(path) => void gateway.request('artifact.open', { path })}
              capabilities={state.capabilities}
              gateway={gateway}
              conversationId={rootConversationId}
              subagents={activitySubagents}
              selectedSubagentId={
                selectedChildConversationId
                  ? activitySubagents.find(
                      (s) => s.childConversationId === selectedChildConversationId,
                    )?.id ?? null
                  : null
              }
              fileEvents={fileEvents}
              onSelectSubagent={(id) => void handleSelectSubagent(id)}
              onBackToMain={handleBackToMain}
              onSwitchSubagentKey={(id) => {
                setSwitchKeySessionId(id);
                void loadAssignmentKeys();
              }}
              onRefreshTasks={() => void refreshSubagentSessions(rootConversationId)}
              showingChildSession={Boolean(selectedChildConversationId)}
              onRollbackFile={
                allowRewind
                  ? (path) => {
                      if (!window.confirm(zh ? `确定撤销 ${path} 的本次修改？` : `Undo this run's changes to ${path}?`)) return;
                      const change = [...fileChanges].reverse().find((item) => item.path === path);
                      void handleRollbackChanges([{ path, runId: change?.runId }]);
                    }
                  : undefined
              }
            />
          </ResizableRightPanel>
        )}
      </div>
      )}

      <SubagentAssignmentModal
        open={Boolean(subagentAssignment) || Boolean(switchKeySessionId)}
        locale={locale}
        interaction={subagentAssignment ?? null}
        keys={assignmentKeyOptions}
        switchSessionId={switchKeySessionId}
        onClose={() => {
          if (switchKeySessionId) {
            setSwitchKeySessionId(null);
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
        onConfirm={handleAssignmentConfirm}
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
