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
  selectChildRuns,
  selectComposerDraft,
  selectConversationMessages,
  selectFileChanges,
  selectIsRunActive,
  selectPendingInteractions,
  selectPromptQueue,
  selectRunEvents,
  type InspectorTab,
} from '@/lib/assistant-workspace';
// runtime pref loaded via persistence export
import { loadPreferredRuntimeId } from '@/lib/assistant-workspace/persistence';
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
import { isActiveRunStatus, mapWireConversation } from '@/lib/assistant-protocol';
import type { Conversation } from '@/lib/assistant-protocol';
import { messagePlainText } from '@/lib/assistant-message-view';
import ConversationTimeline from './ConversationTimeline';
import MessageInput from './MessageInput';
import PermissionRequestCard from './PermissionRequestCard';
import GoalStatusBar from './GoalStatusBar';
import PromptQueuePanel from './PromptQueuePanel';
import ActivityInspector from './ActivityInspector';
import ResizableRightPanel from '@/components/ui/ResizableRightPanel';
import ConnectionBanner from './ConnectionBanner';
import CommandPalette, { type AssistantCommand } from './CommandPalette';
import type { ProviderWithModels } from './ModelSelectorDropdown';
import {
  useAssistantWorkspace,
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

interface AssistantWorkbenchProps {
  locale: Locale;
  /** Force fixture adapter (browser / tests). */
  preferFixture?: boolean;
}

function WorkbenchInner({ locale }: { locale: Locale }) {
  const zh = locale.startsWith('zh');
  const { toast } = useToast();
  const state = useAssistantStore();
  const dispatch = useAssistantDispatch();
  const gateway = useAssistantGateway();
  const { publishNavigation, publishRuntime, registerActions, navigation } = useAssistantWorkspace();

  const stateRef = useRef(state);
  stateRef.current = state;
  const subAbortRef = useRef<{ aborted: boolean }>({ aborted: false });
  /** Quiet soft-resubscribe attempt counts per run (reset on terminal). */
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

  const activeId = state.activeConversationId;
  const activeConversation = activeId ? state.conversations[activeId] : null;
  // Picker selection is resolved against the live provider list so collapsed
  // /stale provider ids still highlight and empty wire fields still show a model.
  const modelSelection = useMemo(
    () =>
      resolveModelSelection(providers, {
        providerId: activeConversation?.providerId,
        modelId: activeConversation?.modelId,
      }),
    [providers, activeConversation?.providerId, activeConversation?.modelId],
  );
  const messages = useMemo(
    () => selectConversationMessages(state, activeId),
    [state, activeId],
  );
  const activeRun = selectActiveRun(state, activeId);
  const activeRunId = activeRun?.id;
  const activeRunStatus = activeRun?.status;
  const isStreaming = selectIsRunActive(state, activeId);
  const interactions = selectPendingInteractions(state, activeId);
  const promptQueue = selectPromptQueue(state, activeId);
  const events = selectRunEvents(state, activeRun?.id ?? null);
  const artifacts = selectArtifacts(state, activeRun?.id ?? null);
  const children = selectChildRuns(state, activeRun?.id ?? null);
  const fileChanges = selectFileChanges(state, activeRun?.id ?? null);
  const contextUsage = activeId ? state.contextUsageByConversation[activeId] ?? null : null;
  const permission = interactions.find((i) => i.kind === 'permission');
  const askUser = interactions.find((i) => i.kind === 'ask_user');
  const planApproval = interactions.find((i) => i.kind === 'plan_approval');
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
      dispatch({ type: 'view/patch', patch: { layoutBreakpoint } });
      if (layoutBreakpoint !== 'full') setRightPanelOpen(false);
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
        if (!cancelled) setFileContentsByPath({});
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
      if (!cancelled) setFileContentsByPath(next);
    })();
    return () => {
      cancelled = true;
    };
  }, [fileChanges, events]);

  const startSubscription = useCallback(
    async (runId: string, afterSequence: number) => {
      subAbortRef.current.aborted = true;
      const signal = { aborted: false };
      subAbortRef.current = signal;
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
        // transport soft-fail handled in controller; resubscribe below if still active
      }
      if (signal.aborted) return;
      const run = stateRef.current.runs[runId];
      if (!run || !isActiveRunStatus(run.status)) {
        delete resubAttemptsRef.current[runId];
        return;
      }
      // Quiet soft resubscribe with backoff. Do not flip ConnectionBanner on
      // every empty poll — that caused permanent "正在重连" during normal answers.
      const nextSeq = stateRef.current.lastSequenceByRun[runId] ?? afterSequence;
      const n = (resubAttemptsRef.current[runId] ?? 0) + 1;
      resubAttemptsRef.current[runId] = n;
      if (n > 40) {
        dispatch({
          type: 'connection/set',
          connection: 'reconnecting',
          error: 'Still waiting for engine terminal event',
        });
        return;
      }
      const delay = Math.min(250 * n, 2000);
      window.setTimeout(() => {
        if (!subAbortRef.current.aborted) {
          void startSubscription(runId, nextSeq);
        }
      }, delay);
    },
    [gateway, dispatch],
  );

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
      subAbortRef.current.aborted = true;
      void gateway.disconnect();
    };
  }, [gateway, dispatch, toast]);

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
      const storeTempId = isTempConversationId(activeId) ? activeId : null;
      const rootTemp = prev.tempSession;
      const selectedId =
        storeTempId ??
        (rootTemp && activeId === null ? rootTemp.conversation.id : activeId) ??
        rootTemp?.conversation.id ??
        activeId;
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
            : isTempConversationId(activeId)
              ? rootTemp
              : activeId
                ? null
                : rootTemp,
      };
    });
  }, [
    state.conversations,
    state.conversationOrder,
    state.connection,
    activeId,
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
      conversationId: activeId,
      conversationTitle: activeConversation?.title ?? null,
      conversationMode: activeConversation?.mode ?? 'agent',
      providerId: activeConversation?.providerId ?? '',
      modelId: activeConversation?.modelId ?? '',
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
    activeId,
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
      if (id === stateRef.current.activeConversationId) return;
      setLoadingMessages(true);
      try {
        await openConversation(gateway, dispatch, id);
        const runId = stateRef.current.activeRunByConversation[id];
        const run = runId ? stateRef.current.runs[runId] : null;
        if (run && isActiveRunStatus(run.status)) {
          void startSubscription(run.id, stateRef.current.lastSequenceByRun[run.id] ?? 0);
        }
      } catch (err) {
        toast(classifyError(err).userMessage, 'error');
      } finally {
        setLoadingMessages(false);
      }
    },
    [gateway, dispatch, toast, startSubscription],
  );

  // Sync external sidebar selection (persisted sessions only — temps hydrate below).
  useEffect(() => {
    const selected = navigation.selectedId;
    if (!selected || selected === activeId) return;
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
      setActiveProjectPath(conversation.projectId);
      void writeActiveProject(window.nativesAPI, conversation.projectId).catch(() => undefined);
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
    dispatch({ type: 'conversations/upsert', conversation: shell });
    dispatch({ type: 'conversations/setActive', id: shell.id });
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
          void startSubscription(result.runId, 0);
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
      startSubscription,
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
      void startSubscription(newId, 0);
    } catch (err) {
      toast(classifyError(err).userMessage, 'error');
    }
  }, [
    // eslint-disable-next-line react-hooks/preserve-manual-memoization
    activeRunId, gateway, dispatch, toast, startSubscription,
  ]);

  const handlePermission = useCallback(
    async (requestId: string, approved: boolean, scope?: string) => {
      try {
        await respondPermission(
          gateway,
          dispatch,
          requestId,
          approved,
          scope ?? 'once',
          activeRunId,
        );
      } catch (err) {
        toast(classifyError(err).userMessage, 'error');
      }
    },
    [
      gateway, dispatch,
      // eslint-disable-next-line react-hooks/preserve-manual-memoization
      activeRunId, toast,
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
          // Host nulls project_id on remove — mirror locally so sessions move to Unassigned.
          const current = stateRef.current;
          for (const id of current.conversationOrder) {
            const c = current.conversations[id];
            if (c?.projectId === path) {
              dispatch({
                type: 'conversations/upsert',
                conversation: { ...c, projectId: null },
              });
            }
          }
          const curId = current.activeConversationId;
          const cur = curId ? current.conversations[curId] : null;
          if (cur?.projectId === path) {
            dispatch({ type: 'conversations/setActive', id: null });
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
            void startSubscription(
              activeRunId,
              stateRef.current.lastSequenceByRun[activeRunId] ?? 0,
            );
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

  const showRight =
    rightPanelOpen &&
    (state.view.layoutBreakpoint === 'full' || state.view.layoutBreakpoint === 'drawer-right');

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
        onReconnect={() => void connectWorkspace(gateway, dispatch)}
      />

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
              loading={loadingMessages}
              locale={locale}
              onRetry={() => void handleRetry()}
            />
          </div>

          {permission && permission.kind === 'permission' && (
            <div className="px-4 pb-2">
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
                  const mapped = scope === 'this_run' ? 'run' : scope === 'project' ? 'project' : 'once';
                  void handlePermission(id, true, mapped);
                }}
                onReject={(id) => void handlePermission(id, false)}
              />
            </div>
          )}

          {askUser && askUser.kind === 'ask_user' && (
            <div className="mx-4 mb-2 rounded-lg border border-[var(--border)] bg-[var(--surface)] p-3 text-sm">
              <div className="font-medium">{askUser.question.prompt}</div>
              <div className="mt-2 flex flex-wrap gap-2">
                {(askUser.question.options ?? []).map((opt) => (
                  <button
                    key={opt.id}
                    type="button"
                    className="rounded-lg border border-[var(--border)] px-3 py-1.5 hover:bg-[var(--surface-hover)]"
                    onClick={() =>
                      void gateway
                        .request('interaction.respond', {
                          id: askUser.id,
                          answer: opt.id,
                          run_id: askUser.runId,
                        })
                        .then(() => dispatch({ type: 'interaction/remove', id: askUser.id }))
                    }
                  >
                    {opt.label}
                  </button>
                ))}
              </div>
            </div>
          )}

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

          <MessageInput
            locale={locale}
            onSend={(draft) => handleSend(draft, false)}
            onForceSend={(draft) => handleSend(draft, true)}
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
            onDraftChange={(text) => {
              if (activeId) {
                dispatch({ type: 'composer/set', conversationId: activeId, draft: { text } });
                // Keep root-level temp draft in sync so remount (settings round-trip)
                // restores the in-progress text — but never write long-term storage.
                if (isTempConversationId(activeId)) {
                  publishNavigation((prev) => {
                    if (!prev.tempSession || prev.tempSession.conversation.id !== activeId) {
                      return prev;
                    }
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
              }
            }}
            projectPath={activeProjectPath}
          />
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
              run={activeRun}
              events={events}
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
              onRollbackFile={(path) => {
                // Rollback must go through engine permission/events — intent only.
                void gateway.request('run.rewind', { path, run_id: activeRun?.id }).catch((err) => {
                  toast(classifyError(err).userMessage, 'error');
                });
              }}
            />
          </ResizableRightPanel>
        )}
      </div>
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
