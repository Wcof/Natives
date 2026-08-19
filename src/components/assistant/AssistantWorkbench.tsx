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
  AssistantStoreProvider,
  useAssistantDispatch,
  useAssistantGateway,
  useAssistantStore,
} from '@/lib/assistant-workspace';
import {
  canRewind,
  needsEngineRecovery,
} from '@/lib/assistant-workspace/capability-gate';
import { openConversation } from '@/lib/assistant-workspace/controller';
import { createDefaultGateway, FixtureAssistantAdapter } from '@/lib/assistant-gateway';
import { goldenTextStream } from '@/lib/assistant-fixtures/golden';
import { useAssistantRunSubscription } from '@/hooks/useAssistantRunSubscription';
import { useAssistantSubagentState } from '@/hooks/useAssistantSubagentState';
import { useAssistantRunLifecycle } from '@/hooks/useAssistantRunLifecycle';
import { useAssistantWorkbenchKeyboard } from '@/hooks/useAssistantWorkbenchKeyboard';
import { useAssistantWorkbenchSurface } from '@/hooks/useAssistantWorkbenchSurface';
import { useAssistantWorkbenchActions } from '@/hooks/useAssistantWorkbenchActions';
import { useAssistantWorkbenchBoot } from '@/hooks/useAssistantWorkbenchBoot';
import { useAssistantWorkbenchPublishers } from '@/hooks/useAssistantWorkbenchPublishers';
import { useAssistantWorkbenchHydration } from '@/hooks/useAssistantWorkbenchHydration';
import { useDiagnosticsExport } from '@/hooks/useDiagnosticsExport';
import ConnectionBanner from './ConnectionBanner';
import EngineRecoveryPage from './EngineRecoveryPage';
import CommandPalette from './CommandPalette';
import type { ProviderWithModels } from '@/components/assistant/conversation/ModelSelectorDropdown';
import WorkbenchHeader from './workbench/WorkbenchHeader';
import WorkbenchTimelinePane from './workbench/WorkbenchTimelinePane';
import WorkbenchComposer from './workbench/WorkbenchComposer';
import WorkbenchPanels from './workbench/WorkbenchPanels';
import { WorkbenchOverlays } from './workbench/WorkbenchOverlays';
import type { ActivitySubagentView } from './ActivityInspector';
import {
  useAssistantNavigation,
  useAssistantWorkspaceApi,
} from './AssistantWorkspaceContext';
import { isTempConversationId } from '@/lib/assistant-temp-conversation';

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

  // --- Local state (orchestrator owns authority) ---
  const [providers, setProviders] = useState<ProviderWithModels[]>([]);
  const [providerReadiness, setProviderReadiness] = useState<'no_provider' | 'no_model' | 'ready'>('no_provider');
  const [loadingMessages, setLoadingMessages] = useState(false);
  const [activeProjectPath, setActiveProjectPath] = useState<string | null>(null);
  const [registeredProjects, setRegisteredProjects] = useState<Array<{ id: string; path: string; lastOpenedAt?: string | null; label?: string; exists?: boolean }>>([]);
  const [hiddenProjectPaths, setHiddenProjectPaths] = useState<string[]>([]);
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

  // --- Surface derivation (C1) ---
  const surface = useAssistantWorkbenchSurface({
    providers,
    storeActiveId,
    selectedRootConversationId,
    selectedChildConversationId,
  });
  const {
    rootConversationId,
    activeId,
    rootConversation,
    activeConversation,
    modelSelection,
    messagePageInfo,
    loadingOlderMessages,
    loadOlderMessages,
    rootRun,
    activeRun,
    activeRunId,
    activeRunStatus,
    isStreaming,
    promptQueue,
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
  } = surface;

  // --- Subagent bridge + lifecycle ---
  const ensureRunSubscriptionRef = useRef<(runId: string | null | undefined) => void>(
    () => undefined,
  );
  const ensureRunSubscription = useCallback((runId: string | null | undefined) => {
    ensureRunSubscriptionRef.current(runId);
  }, []);

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

  // UX-12（W8）：会话头部「导出诊断」——只读 Host diagnostics + Run 元数据 +
  // 有限脱敏日志，经 lib/diagnostics-export 保存到本地（不上传）。
  const { exporting: exportingDiagnostics, handleExport: handleExportDiagnostics } =
    useDiagnosticsExport({
      locale,
      run: rootRun ?? activeRun,
      projectPath: activeProjectPath,
      connection: {
        connection: state.connection,
        reconnectAttempts: state.reconnectAttempts,
      },
      protocolVersion: state.capabilities?.protocolVersion ?? null,
    });

  // W8: fork the current conversation at the selected persisted user message
  // (daemon conversation.fork supports through_message_id selected-turn copy).
  const handleForkMessage = useCallback(
    (messageId: string) => {
      const conversationId = stateRef.current.activeConversationId ?? navigation.selectedId;
      if (!conversationId) return;
      void gateway
        .request('conversation.fork', {
          conversation_id: conversationId,
          through_message_id: messageId,
        })
        .then((resp) => {
          const forkId = (resp as { id?: string } | null)?.id;
          // Open the new branch so the fork is immediately visible and active.
          if (forkId) return openConversation(gateway, dispatch, forkId);
          return undefined;
        })
        .catch(() => undefined);
    },
    [gateway, navigation.selectedId, dispatch],
  );

  // --- Activity subagents derivation (C2/C4) ---
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

  // --- Run subscription (multi-run keep-alive) ---
  const runSubscription = useAssistantRunSubscription({
    rootRun,
    rootConversationId,
    children,
    subagentSessions,
  });
  // Point the stable bridge at the real subscription controller from this render.
  ensureRunSubscriptionRef.current = runSubscription.ensureRunSubscription;
  const abortAllSubscriptions = runSubscription.abortAllSubscriptions;

  // --- Boot effect (C2) ---
  useAssistantWorkbenchBoot({
    setProviders,
    setProviderReadiness,
    setActiveProjectPath,
    setRegisteredProjects,
    setHiddenProjectPaths,
    setPinnedConversationIds,
    setLoadingConversations,
    abortAllSubscriptions,
    toast,
  });

  // --- Subagent list/prefetch effects ---
  const rootRunPhase =
    rootRun?.status === 'waiting_subagent' || rootRun?.status === 'running'
      ? 'active'
      : rootRun?.status ?? 'idle';
  useEffect(() => {
    void subagent.refreshSubagentSessions(rootConversationId);
  }, [rootConversationId, subagent.refreshSubagentSessions, rootRunPhase]);

  useEffect(() => {
    if (subagentAssignment || subagent.switchKeySessionId) {
      void subagent.loadAssignmentKeys();
    }
  }, [subagentAssignment, subagent.switchKeySessionId, subagent.loadAssignmentKeys]);

  // --- Navigation + runtime publishers (C3) ---
  useAssistantWorkbenchPublishers({
    rootConversationId,
    activeId,
    activeProjectPath,
    rootConversation,
    activeConversation,
    activeRun,
    events,
    fileChanges,
    artifacts,
    contextUsage,
    registeredProjects,
    hiddenProjectPaths,
    pinnedConversationIds,
    loadingConversations,
    providerReadiness,
    publishNavigation,
    publishRuntime,
  });

  // --- Temp session hydration effects (C4) ---
  useAssistantWorkbenchHydration({
    locale,
    providers,
    loadingConversations,
    navigation,
    stateRef,
    setActiveProjectPath,
    publishNavigation,
  });

  // --- selectConversation callback ---
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

  // --- Keyboard / command palette ownership ---
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

  // --- External sidebar selection sync ---
  useEffect(() => {
    const selected = navigation.selectedId;
    if (!selected || selected === rootConversationId) return;
    if (isTempConversationId(selected)) return;
    void selectConversation(selected);
  }, [navigation.selectedId]);

  // --- Workspace actions registration (C5) ---
  useAssistantWorkbenchActions({
    providers,
    locale,
    activeProjectPath,
    selectConversation,
    handleRetry,
    handlePermission,
    registerActions,
    publishNavigation,
    setActiveProjectPath,
    setRegisteredProjects,
    setHiddenProjectPaths,
    setPinnedConversationIds,
    stateRef,
    toast,
  });

  // --- Layout breakpoint effect ---
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
            projectPath={activeProjectPath}
            onExportDiagnostics={handleExportDiagnostics}
            exportingDiagnostics={exportingDiagnostics}
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
            onForkMessage={handleForkMessage}
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

      <WorkbenchOverlays
        locale={locale}
        rootConversationId={rootConversationId}
        subagentAssignment={subagentAssignment}
        switchKeySessionId={subagent.switchKeySessionId}
        assignmentKeyOptions={subagent.assignmentKeyOptions}
        onSwitchKeySessionId={subagent.setSwitchKeySessionId}
        onAssignmentConfirm={subagent.handleAssignmentConfirm}
        confirmDeleteGoalId={confirmDeleteGoalId}
        setConfirmDeleteGoalId={setConfirmDeleteGoalId}
        stateRef={stateRef}
        toast={toast}
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
