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
  selectAssistantModel,
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
import { isActiveRunStatus } from '@/lib/assistant-protocol';
import type { Conversation } from '@/lib/assistant-protocol';
import ConversationTimeline from './ConversationTimeline';
import MessageInput from './MessageInput';
import PermissionRequestCard from './PermissionRequestCard';
import RunStatusBar from './RunStatusBar';
import PromptQueuePanel from './PromptQueuePanel';
import ActivityInspector from './ActivityInspector';
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

  const [providers, setProviders] = useState<ProviderWithModels[]>([]);
  const [providerReadiness, setProviderReadiness] = useState<ProviderReadiness>('no_provider');
  const [loadingMessages, setLoadingMessages] = useState(false);
  const [activeProjectPath, setActiveProjectPath] = useState<string | null>(null);
  const [registeredProjects, setRegisteredProjects] = useState<Array<{ id: string; path: string }>>([]);
  const [rightPanelOpen, setRightPanelOpen] = useState(!state.view.rightCollapsed);
  const [loadingConversations, setLoadingConversations] = useState(true);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [fileContentsByPath, setFileContentsByPath] = useState<
    Record<string, { before: string; after: string }>
  >({});

  const activeId = state.activeConversationId;
  const activeConversation = activeId ? state.conversations[activeId] : null;
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
        // connection soft-fail handled in controller
      }
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
          const list = await gateway.request<Array<Record<string, unknown>>>('provider.list', {});
          const mapped: ProviderWithModels[] = (Array.isArray(list) ? list : []).map((p) => {
            const name = String(p.display_name ?? p.displayName ?? p.name ?? p.id);
            return {
              id: String(p.id),
              name,
              presetName: String(p.provider_type ?? p.presetName ?? name),
              baseUrl: String(p.api_base_url ?? p.baseUrl ?? ''),
              keys: (p.has_active_key ?? p.hasActiveKey ?? true)
                ? [{ id: 'active', label: 'default', maskedKey: '••••' }]
                : [],
              models: Array.isArray(p.models)
                ? (p.models as Array<Record<string, unknown>>).map((m) => ({
                    id: String(m.id),
                    displayName: String(m.display_name ?? m.displayName ?? m.id),
                  }))
                : [],
            };
          });
          if (!cancelled) {
            setProviders(mapped);
            setProviderReadiness(
              classifyProviderReadiness(
                mapped.map((p) => ({
                  id: p.id,
                  provider_type: p.presetName,
                  display_name: p.name,
                  has_active_key: p.keys.length > 0,
                  models: (p.models ?? []).map((m) => ({ id: m.id, display_name: m.displayName })),
                })),
              ),
            );
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
    const conversations = state.conversationOrder
      .map((id) => state.conversations[id])
      .filter(Boolean) as Conversation[];
    const groups = groupAssistantConversations(
      conversations.map((c) => ({
        id: c.id,
        title: c.title,
        mode: c.mode,
        projectId: c.projectId ?? null,
        updatedAt: c.updatedAt,
      })),
      registeredProjects.map((p) => p.path),
      zh ? '未关联项目' : 'Unassigned',
    );
    publishNavigation({
      groups,
      selectedId: activeId,
      activeProjectPath,
      loading: loadingConversations,
      creationState: projectCreationState({
        engine: state.connection === 'connected' ? 'ready' : state.connection === 'connecting' ? 'connecting' : 'unavailable',
        providerReadiness,
      }),
      isCreatingConversation: false,
    });
  }, [
    state.conversations,
    state.conversationOrder,
    state.connection,
    activeId,
    activeProjectPath,
    registeredProjects,
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

  // Sync external sidebar selection
  useEffect(() => {
    if (navigation.selectedId && navigation.selectedId !== activeId) {
      void selectConversation(navigation.selectedId);
    }
  }, [navigation.selectedId]); // eslint-disable-line react-hooks/exhaustive-deps

  const handleSend = useCallback(
    async (draft: AssistantDraft, forceImmediate = false): Promise<boolean> => {
      let conversationId = activeId;
      let providerId = activeConversation?.providerId ?? '';
      let modelId = activeConversation?.modelId ?? '';

      if (!providerId || !modelId) {
        const pick = selectAssistantModel(
          providers.map((p) => ({
            id: p.id,
            provider_type: p.presetName,
            display_name: p.name,
            has_active_key: p.keys.length > 0,
            models: (p.models ?? []).map((m) => ({ id: m.id, display_name: m.displayName })),
          })),
        );
        if (pick) {
          providerId = pick.providerId;
          modelId = pick.modelId;
        }
      }
      if (!providerId || !modelId || providerReadiness !== 'ready') {
        toast(zh ? '请先配置供应商和模型' : 'Configure provider and model first', 'error');
        return false;
      }

      try {
        if (!conversationId || conversationId.startsWith('temp-')) {
          const title = draft.content.trim().slice(0, 30) || t(locale, 'assistant.newConversation');
          const created = await gateway.request<Conversation>('conversation.create', {
            mode: 'agent',
            title,
            provider_id: providerId,
            model_id: modelId,
            project_id: activeProjectPath,
            permission_profile_id: activeConversation?.permissionProfileId ?? 'ask',
          });
          dispatch({ type: 'conversations/upsert', conversation: created });
          dispatch({ type: 'conversations/setActive', id: created.id });
          conversationId = created.id;
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
        });

        if (!result.queued && result.runId) {
          void startSubscription(result.runId, 0);
        }
        return true;
      } catch (err) {
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
      selectConversation: (id) => void selectConversation(id),
      selectProject: (path) => {
        setActiveProjectPath(path);
        if (path) void writeActiveProject(window.nativesAPI, path).catch(() => undefined);
      },
      addProjectFolder: () => {
        void window.nativesAPI?.dialog?.pickDirectory?.().then(async (path) => {
          if (!path) return;
          await window.nativesAPI?.project?.register?.(path);
          const projects = (await window.nativesAPI?.project?.list?.()) ?? [];
          setRegisteredProjects(projects);
          setActiveProjectPath(path);
        });
      },
      createConversation: () => {
        const id = `temp-${Date.now()}`;
        const now = new Date().toISOString();
        const pick = selectAssistantModel(
          providers.map((p) => ({
            id: p.id,
            provider_type: p.presetName,
            display_name: p.name,
            has_active_key: p.keys.length > 0,
            models: (p.models ?? []).map((m) => ({ id: m.id, display_name: m.displayName })),
          })),
        );
        dispatch({
          type: 'conversations/upsert',
          conversation: {
            id,
            mode: 'agent',
            title: t(locale, 'assistant.newConversation'),
            providerId: pick?.providerId ?? 'openai',
            modelId: pick?.modelId ?? 'gpt-4o',
            projectId: activeProjectPath,
            permissionProfileId: 'ask',
            createdAt: now,
            updatedAt: now,
          },
        });
        dispatch({ type: 'conversations/setActive', id });
      },
      createConversationInProject: (path) => {
        setActiveProjectPath(path);
        actions.createConversation();
      },
      removeProject: (path) => {
        void (async () => {
          try {
            const projects = (await window.nativesAPI?.project?.list?.()) ?? [];
            const match = projects.find((p) => p.path === path);
            if (match?.id) await window.nativesAPI?.project?.remove?.(match.id);
          } catch {
            /* best-effort */
          }
          const next = (await window.nativesAPI?.project?.list?.()) ?? [];
          setRegisteredProjects(next);
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
        });
      },
      deleteConversation: async (id) => {
        await gateway.request('conversation.delete', { id });
        dispatch({ type: 'conversations/remove', id });
        return true;
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
          const id = `temp-${Date.now()}`;
          const now = new Date().toISOString();
          const pick = selectAssistantModel(
            providers.map((p) => ({
              id: p.id,
              provider_type: p.presetName,
              display_name: p.name,
              has_active_key: p.keys.length > 0,
              models: (p.models ?? []).map((m) => ({ id: m.id, display_name: m.displayName })),
            })),
          );
          dispatch({
            type: 'conversations/upsert',
            conversation: {
              id,
              mode: 'agent',
              title: t(locale, 'assistant.newConversation'),
              providerId: pick?.providerId ?? 'openai',
              modelId: pick?.modelId ?? 'gpt-4o',
              projectId: activeProjectPath,
              permissionProfileId: 'ask',
              createdAt: now,
              updatedAt: now,
            },
          });
          dispatch({ type: 'conversations/setActive', id });
        },
      },
      {
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
        disabledReason: activeId && !activeId.startsWith('temp-') ? undefined : zh ? '无会话' : 'No conversation',
        run: () => {
          if (!activeId) return;
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
  ]);

  const timelineMessages = useMemo(
    () =>
      messages.map((m) => ({
        id: m.id,
        role: m.role as 'system' | 'user' | 'assistant',
        contentBlocks: m.contentBlocks,
        status: m.status,
        createdAt: m.createdAt,
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
    <div className="flex h-full min-h-0 flex-col" data-assistant-workbench data-gateway="1">
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

          <RunStatusBar
            run={activeRun}
            locale={locale}
            queueCount={promptQueue.length}
            tokenLabel={
              contextUsage ? `${contextUsage.usedTokens} tokens` : undefined
            }
            connectionHint={
              state.connection === 'recovering'
                ? zh
                  ? '恢复中…'
                  : 'Recovering…'
                : state.connection === 'reconnecting'
                  ? zh
                    ? '重连中…'
                    : 'Reconnecting…'
                  : null
            }
            onStop={() => void handleStop()}
            onBackground={
              activeRun
                ? () => {
                    // Background mode is represented by the live subscription/event stream.
                    void startSubscription(
                      activeRun.id,
                      stateRef.current.lastSequenceByRun[activeRun.id] ?? 0,
                    );
                  }
                : undefined
            }
          />

          <PromptQueuePanel
            items={promptQueue}
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
              if (!activeId || activeId.startsWith('temp-')) return;
              await gateway.request('conversation.update_permission', {
                id: activeId,
                permission_profile_id: profile,
              });
              if (activeConversation) {
                dispatch({
                  type: 'conversations/upsert',
                  conversation: { ...activeConversation, permissionProfileId: profile },
                });
              }
            }}
            providers={providers}
            selectedProviderId={activeConversation?.providerId ?? providers[0]?.id ?? ''}
            selectedModel={activeConversation?.modelId}
            onSelectModel={(providerId, modelId) => {
              if (!activeConversation) return;
              dispatch({
                type: 'conversations/upsert',
                conversation: { ...activeConversation, providerId, modelId },
              });
              if (!activeId?.startsWith('temp-') && activeId) {
                void gateway.request('conversation.update_model', {
                  id: activeId,
                  provider_id: providerId,
                  model_id: modelId,
                });
              }
            }}
            draftText={selectComposerDraft(state, activeId).text}
            onDraftChange={(text) => {
              if (activeId) {
                dispatch({ type: 'composer/set', conversationId: activeId, draft: { text } });
              }
            }}
            projectPath={activeProjectPath}
          />
        </div>

        {showRight && (
          <div
            className="shrink-0"
            style={{ width: state.view.rightWidth || 320 }}
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
          </div>
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
