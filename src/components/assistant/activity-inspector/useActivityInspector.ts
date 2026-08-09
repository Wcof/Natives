'use client';

import { useCallback, useEffect, useMemo, useState } from 'react';
import type {
  Artifact,
  BackgroundTask,
  ChildRunSummary,
  ContextUsage,
  DaemonCapabilities,
  FileChange,
  Run,
  RunEvent,
} from '@/lib/assistant-protocol';
import type { AssistantGateway } from '@/lib/assistant-gateway';
import { fsApi, hasNativeFiles, searchApi } from '@/lib/files-api';
import type { ProviderWithModels } from '@/components/assistant/conversation/ModelSelectorDropdown';
import type { InspectorTab } from '@/lib/assistant-workspace';
import {
  canCancelTask,
  canListTaskDepth,
  canListTasks,
  canShowContextUsage,
} from '@/lib/assistant-workspace/capability-gate';
import {
  aggregateArtifactFiles,
  extractTodosFromEvents,
  mapSubagentUiStatus,
  summarizeTodoStatus,
  type ActivityTodo,
  type ArtifactFileItem,
  type FileEventInput,
  type TodoStatus,
} from '@/lib/assistant-activity-view';
import {
  TABS,
  groupDeltaEvents,
  mapWireBackgroundTask,
  type ActivitySubagentView,
  type DeltaEventGroup,
} from './model';

export interface ActivityInspectorProps {
  run: Run | null;
  /** Root / main-run event tree. Main Todo is always derived from this. */
  events: RunEvent[];
  /** Selected child session events (sub-task Todo source). */
  selectedChildEvents?: RunEvent[];
  artifacts: Artifact[];
  children: ChildRunSummary[];
  fileChanges: FileChange[];
  contextUsage: ContextUsage | null;
  locale: string;
  activeTab: InspectorTab;
  onTabChange: (tab: InspectorTab) => void;
  developerMode?: boolean;
  onOpenArtifact?: (artifact: Artifact) => void;
  onRevealArtifact?: (artifact: Artifact) => void;
  onSelectChild?: (id: string) => void;
  onRetry?: () => void;
  onOpenFile?: (path: string) => void;
  /** daemon.getCapabilities result — gates rewind / context / tasks. */
  capabilities?: DaemonCapabilities | null;
  /** Gateway for task.list / task.cancel when advertised. */
  gateway?: AssistantGateway | null;
  conversationId?: string | null;

  // ── Optional props for 《任务》/《产物》 (Agent E wires these; empty defaults) ──
  mainTodos?: Array<{ id: string; content: string; status: TodoStatus }>;
  mainTaskStatus?: TodoStatus;
  subagents?: ActivitySubagentView[];
  selectedSubagentId?: string | null;
  fileEvents?: FileEventInput[];
  onSelectSubagent?: (id: string) => void;
  onBackToMain?: () => void;
  onSwitchSubagentKey?: (id: string) => void;
  onRefreshTasks?: () => void;
  showingChildSession?: boolean;
  providers?: ProviderWithModels[];
  projectPath?: string | null;
}

export interface ActivityInspectorController {
  // tabs
  tabs: Array<{ id: InspectorTab; labelKey: string; icon: typeof TABS[number]['icon']; disabled: boolean }>;
  effectiveTab: InspectorTab;
  tasksCapabilityMissing: boolean;
  locale: string;
  onTabChange: (tab: InspectorTab) => void;
  // run
  run: Run | null;
  providerLabel: (providerId: string, modelId: string) => string;
  onRetry?: () => void;
  // tasks
  resolvedMainTodos: ActivityTodo[];
  resolvedMainStatus: TodoStatus;
  backgroundExecTasks: BackgroundTask[];
  tasksLoading: boolean;
  tasksError: string | null;
  useTaskList: boolean;
  resolvedSubagents: ActivitySubagentView[];
  providers: ProviderWithModels[];
  selectedSubagentId: string | null;
  selectedSubagent: ActivitySubagentView | null;
  selectedSubTodos: ActivityTodo[];
  showingChildSession: boolean;
  onBackToMain?: () => void;
  onSwitchSubagentKey?: (id: string) => void;
  onSelectSubagent: (id: string) => void;
  onRefreshTasks: () => void;
  onCancelTask: (taskId: string) => Promise<void>;
  allowCancelTask: boolean;
  cancellingTaskId: string | null;
  // changes / audit
  projectPath: string | null;
  auditLoading: boolean;
  auditError: string | null;
  auditStatus: { branch: string; entries: Array<{ path: string; status: string }> } | null;
  auditCounts: { additions: number; deletions: number };
  onRefreshAudit: () => Promise<void>;
  commitMessage: string;
  setCommitMessage: (value: string) => void;
  committing: boolean;
  pushing: boolean;
  onGitAction: (action: 'commit' | 'push') => Promise<void>;
  auditQuery: string;
  setAuditQuery: (value: string) => void;
  filteredAuditEntries: Array<{ path: string; status: string }>;
  selectedAuditPath: string | null;
  setSelectedAuditPath: (path: string) => void;
  auditContent: string | null;
  onOpenFile?: (path: string) => void;
  // artifacts
  artifactBuckets: { used: ArtifactFileItem[]; modified: ArtifactFileItem[]; created: ArtifactFileItem[] };
  artifactPreviewPath: string | null;
  onClosePreview: () => void;
  onSelectArtifactFile: (path: string) => void;
  // context
  allowContextUsage: boolean;
  contextUsage: ContextUsage | null;
  // events
  eventGroups: DeltaEventGroup[];
  events: RunEvent[];
}

export function useActivityInspector(props: ActivityInspectorProps): ActivityInspectorController {
  const {
    run,
    events,
    selectedChildEvents = [],
    artifacts,
    children,
    fileChanges,
    contextUsage,
    locale,
    activeTab,
    onTabChange,
    developerMode = true,
    onSelectChild,
    onRetry,
    onOpenFile,
    capabilities = null,
    gateway = null,
    conversationId = null,
    mainTodos,
    mainTaskStatus,
    subagents,
    selectedSubagentId = null,
    fileEvents,
    onSelectSubagent,
    onBackToMain,
    onSwitchSubagentKey,
    onRefreshTasks,
    showingChildSession = false,
    providers = [],
    projectPath = null,
  } = props;

  const allowContextUsage = canShowContextUsage(capabilities);
  const allowTasks = canListTasks(capabilities);
  const useTaskList = canListTaskDepth(capabilities);
  const allowCancelTask = canCancelTask(capabilities);
  // Capability-gated tabs stay visible but disabled — hiding them makes the
  // feature look deleted and leaves ⌘⇧T pointing at a blank panel. devOnly
  // tabs remain a deliberate user toggle and are still filtered.
  const tabs = TABS.filter((tab) => !(tab.devOnly && !developerMode)).map((tab) => ({
    ...tab,
    disabled: tab.id === 'tasks' && !allowTasks,
  }));
  const [backgroundTasks, setBackgroundTasks] = useState<BackgroundTask[]>([]);
  const [tasksLoading, setTasksLoading] = useState(false);
  const [tasksError, setTasksError] = useState<string | null>(null);
  const [cancellingTaskId, setCancellingTaskId] = useState<string | null>(null);
  /** T40：产物文件内联预览（Preview V2 artifact surface；普通 chat 不迁移） */
  const [artifactPreviewPath, setArtifactPreviewPath] = useState<string | null>(null);
  const [auditStatus, setAuditStatus] = useState<{
    branch: string;
    entries: Array<{ path: string; status: string }>;
  } | null>(null);
  const [auditLoading, setAuditLoading] = useState(false);
  const [auditError, setAuditError] = useState<string | null>(null);
  const [auditQuery, setAuditQuery] = useState('');
  const [projectFiles, setProjectFiles] = useState<string[]>([]);
  const [selectedAuditPath, setSelectedAuditPath] = useState<string | null>(null);
  const [auditContent, setAuditContent] = useState<string | null>(null);
  const [commitMessage, setCommitMessage] = useState('');
  const [committing, setCommitting] = useState(false);
  const [pushing, setPushing] = useState(false);
  const runId = run?.id ?? null;
  const providerLabel = (providerId: string, modelId: string) => {
    const provider = providers.find((item) => item.id === providerId);
    const model = provider?.models?.find((item) => item.id === modelId);
    return `${provider?.name ?? providerId} / ${model?.displayName ?? modelId}`;
  };

  const eventGroups = useMemo(() => groupDeltaEvents(events), [events]);

  // Keyboard shortcuts (⌘⇧T) can still land on a disabled tab; keep it as the
  // effective tab and render an explanatory empty state instead of a blank panel.
  const effectiveTab = tabs.some((tab) => tab.id === activeTab)
    ? activeTab
    : (tabs[0]?.id ?? 'run');
  const tasksCapabilityMissing = effectiveTab === 'tasks' && !allowTasks;

  // React Compiler flags the stable useState setters as inferred deps only when
  // this logic lives in a hook (the identical code in the pre-split component
  // passed). Setters have stable identity; the manual deps are the real inputs.
  // eslint-disable-next-line react-hooks/preserve-manual-memoization
  const refreshAudit = useCallback(async () => {
    if (!projectPath || !window.nativesAPI?.git?.status) {
      setAuditStatus(null);
      return;
    }
    setAuditLoading(true);
    setAuditError(null);
    try {
      const raw = await window.nativesAPI.git.status(projectPath) as {
        branch?: unknown;
        entries?: unknown;
      };
      setAuditStatus({
        branch: typeof raw?.branch === 'string' ? raw.branch : 'unknown',
        entries: Array.isArray(raw?.entries)
          ? raw.entries.flatMap((entry) => {
              if (!entry || typeof entry !== 'object') return [];
              const value = entry as { path?: unknown; status?: unknown };
              return typeof value.path === 'string'
                ? [{ path: value.path, status: typeof value.status === 'string' ? value.status : 'unknown' }]
                : [];
            })
          : [],
      });
    } catch (error) {
      setAuditError(error instanceof Error ? error.message : String(error));
      setAuditStatus(null);
    } finally {
      setAuditLoading(false);
    }
  }, [projectPath]);

  useEffect(() => {
    if (effectiveTab === 'changes') void refreshAudit();
  }, [effectiveTab, refreshAudit]);

  useEffect(() => {
    if (effectiveTab !== 'changes' || !projectPath) return;
    // files-api 契约：非 Tauri 环境无 search 能力时静默跳过（与原可选链探测语义等价）
    let search: ReturnType<typeof searchApi>;
    try {
      search = searchApi();
    } catch {
      return;
    }
    let cancelled = false;
    const timer = window.setTimeout(() => {
      void search.files(auditQuery, projectPath, { maxResults: 100 }).then((raw) => {
        if (cancelled) return;
        const root = `${projectPath.replace(/\/$/, '')}/`;
        setProjectFiles((Array.isArray(raw) ? raw : []).flatMap((entry) => {
          const path = typeof entry === 'string' ? entry : entry && typeof entry === 'object' && typeof (entry as { path?: unknown }).path === 'string' ? (entry as { path: string }).path : '';
          if (!path) return [];
          return [path.startsWith(root) ? path.slice(root.length) : path];
        }));
      }).catch(() => { if (!cancelled) setProjectFiles([]); });
    }, 150);
    return () => { cancelled = true; window.clearTimeout(timer); };
  }, [effectiveTab, projectPath, auditQuery]);

  useEffect(() => {
    const relative = selectedAuditPath ?? auditStatus?.entries[0]?.path ?? null;
    if (!relative || !projectPath) {
      setAuditContent(null);
      return;
    }
    // files-api 契约：fs 不可用（浏览器 dev）时保持原“清空内容”降级
    if (!hasNativeFiles()) {
      setAuditContent(null);
      return;
    }
    const readFile = fsApi().readFile;
    let cancelled = false;
    const fullPath = `${projectPath.replace(/\/$/, '')}/${relative}`;
    void readFile(fullPath).then((raw) => {
      if (cancelled) return;
      if (typeof raw === 'string') setAuditContent(raw);
      else if (raw && typeof raw === 'object' && typeof (raw as { content?: unknown }).content === 'string') setAuditContent((raw as { content: string }).content);
      else setAuditContent(null);
    }).catch(() => { if (!cancelled) setAuditContent(null); });
    return () => { cancelled = true; };
  }, [selectedAuditPath, auditStatus, projectPath]);

  const filteredAuditEntries = useMemo(() => {
    const changed = new Map((auditStatus?.entries ?? []).map((entry) => [entry.path, entry.status]));
    const names = new Set([...projectFiles, ...changed.keys()]);
    return [...names].sort().map((path) => ({ path, status: changed.get(path) ?? 'clean' }));
  }, [projectFiles, auditStatus]);
  const auditCounts = useMemo(() => ({
    additions: (auditStatus?.entries ?? []).filter((entry) => entry.status === 'added' || entry.status === 'untracked').length,
    deletions: (auditStatus?.entries ?? []).filter((entry) => entry.status === 'deleted').length,
  }), [auditStatus]);
  // eslint-disable-next-line react-hooks/preserve-manual-memoization -- the callback intentionally captures the current commit message.
  const runGitAction = useCallback(async (action: 'commit' | 'push') => {
    if (!projectPath || (action === 'commit' && !commitMessage.trim())) return;
    if (action === 'commit') setCommitting(true);
    else setPushing(true);
    try {
      if (action === 'commit') await window.nativesAPI?.git?.commit?.(projectPath, commitMessage.trim());
      else await window.nativesAPI?.git?.push?.(projectPath);
      if (action === 'commit') setCommitMessage('');
      await refreshAudit();
    } catch (error) {
      setAuditError(error instanceof Error ? error.message : String(error));
    } finally {
      if (action === 'commit') setCommitting(false);
      else setPushing(false);
    }
  }, [projectPath, commitMessage, refreshAudit]);

  // ── Task view model (props preferred, events fallback) ──
  // Main Todo ALWAYS comes from root events (or explicit mainTodos prop).
  // Never use selected-child events here.
  const resolvedMainTodos = useMemo<ActivityTodo[]>(() => {
    // Explicit prop (including empty array) wins; only undefined falls back to root events.
    if (mainTodos !== undefined) return mainTodos;
    return extractTodosFromEvents(events);
  }, [mainTodos, events]);

  const resolvedMainStatus = useMemo<TodoStatus>(() => {
    if (mainTaskStatus) return mainTaskStatus;
    return summarizeTodoStatus(resolvedMainTodos);
  }, [mainTaskStatus, resolvedMainTodos]);

  const resolvedSubagents = useMemo<ActivitySubagentView[]>(() => {
    // Explicit prop (including empty) wins; undefined falls back to child runs.
    if (subagents !== undefined) return subagents;
    return children.map((ch) => ({
      id: ch.id,
      name: ch.task || ch.agentProfileId || ch.id.slice(0, 8),
      status: String(ch.status),
      providerId: ch.providerId,
      keyLabel: ch.keyLabel,
      task: ch.task,
    }));
  }, [subagents, children]);

  const selectedSubagent = useMemo(() => {
    if (!selectedSubagentId) return null;
    return resolvedSubagents.find((s) => s.id === selectedSubagentId) ?? null;
  }, [resolvedSubagents, selectedSubagentId]);

  const selectedSubTodos = useMemo<ActivityTodo[]>(() => {
    if (!selectedSubagent) return [];
    // Prefer explicit todos on the subagent view.
    if (selectedSubagent.todos && selectedSubagent.todos.length > 0) {
      return selectedSubagent.todos;
    }
    // Next: latest todo_write from the selected child's own event stream.
    const fromChildEvents = extractTodosFromEvents(selectedChildEvents);
    if (fromChildEvents.length > 0) return fromChildEvents;
    // Fallback: initial delegated task description.
    if (selectedSubagent.task) {
      const ui = mapSubagentUiStatus(selectedSubagent.status);
      return [
        {
          id: `${selectedSubagent.id}-task`,
          content: selectedSubagent.task,
          status:
            ui.key === 'completed'
              ? 'completed'
              : ui.key === 'in_progress'
                ? 'in_progress'
                : 'pending',
        },
      ];
    }
    return [];
  }, [selectedSubagent, selectedChildEvents]);

  // Background execution: terminal / async tasks only — never mix with subagents
  const backgroundExecTasks = useMemo(
    () => backgroundTasks.filter((task) => task.kind !== 'subagent'),
    [backgroundTasks],
  );

  // ── Artifact view model ──
  const usedFiles = useMemo(() => {
    const list: string[] = [];
    for (const e of events) {
      if (!e.payload || typeof e.payload !== 'object') continue;
      const p = e.payload as Record<string, unknown>;
      for (const key of ['path', 'filePath', 'file_path', 'target', 'filename', 'file']) {
        if (typeof p[key] === 'string' && (p[key] as string).trim()) {
          list.push((p[key] as string).trim());
        }
      }
    }
    return list;
  }, [events]);

  const artifactBuckets = useMemo(
    () =>
      aggregateArtifactFiles({
        fileChanges,
        fileEvents,
        artifacts,
        events,
        usedFiles,
      }),
    [fileChanges, fileEvents, artifacts, events, usedFiles],
  );

  // eslint-disable-next-line react-hooks/preserve-manual-memoization -- stable useState setters are not real inputs
  const refreshTasks = useCallback(async () => {
    if (!useTaskList || !gateway) {
      setBackgroundTasks([]);
      setTasksError(null);
      return;
    }
    setTasksLoading(true);
    setTasksError(null);
    try {
      const params: Record<string, string> = {};
      if (runId) params.run_id = runId;
      if (conversationId) params.conversation_id = conversationId;
      const raw = await gateway.request<unknown>('task.list', params);
      const list = Array.isArray(raw)
        ? raw
        : raw && typeof raw === 'object' && Array.isArray((raw as { tasks?: unknown }).tasks)
          ? ((raw as { tasks: unknown[] }).tasks)
          : [];
      setBackgroundTasks(
        list
          .filter((item): item is Record<string, unknown> => Boolean(item) && typeof item === 'object')
          .map((item) => mapWireBackgroundTask(item)),
      );
    } catch (err) {
      setTasksError(err instanceof Error ? err.message : String(err));
      setBackgroundTasks([]);
    } finally {
      setTasksLoading(false);
    }
  }, [useTaskList, gateway, runId, conversationId]);

  useEffect(() => {
    if (effectiveTab !== 'tasks' || !useTaskList) return;
    void refreshTasks();
  }, [effectiveTab, useTaskList, refreshTasks]);

  const handleRefreshTasks = useCallback(() => {
    onRefreshTasks?.();
    void refreshTasks();
  }, [onRefreshTasks, refreshTasks]);

  const handleCancelTask = useCallback(
    // eslint-disable-next-line react-hooks/preserve-manual-memoization -- stable useState setter is not a real input
    async (taskId: string) => {
      if (!gateway || !allowCancelTask || !taskId) return;
      setCancellingTaskId(taskId);
      try {
        await gateway.request('task.cancel', { task_id: taskId, id: taskId });
        await refreshTasks();
      } catch {
        // Keep list; user can refresh by reopening tab.
      } finally {
        setCancellingTaskId(null);
      }
    },
    [gateway, allowCancelTask, refreshTasks],
  );

  const openArtifactPath = useCallback(
    (item: ArtifactFileItem) => {
      // Jump to 《审计》 and delegate the file handoff to file management.
      onTabChange('changes');
      onOpenFile?.(item.path);
    },
    [onOpenFile, onTabChange],
  );

  const handleSelectSubagent = useCallback(
    (id: string) => {
      onSelectSubagent?.(id);
      // Legacy fallback: also notify child-run selection when no dedicated handler
      if (!onSelectSubagent) onSelectChild?.(id);
    },
    [onSelectSubagent, onSelectChild],
  );

  // eslint-disable-next-line react-hooks/preserve-manual-memoization -- stable useState setter is not a real input
  const selectArtifactFile = useCallback((path: string) => {
    setArtifactPreviewPath(path);
    const item = [...artifactBuckets.used, ...artifactBuckets.modified, ...artifactBuckets.created]
      .find((candidate) => candidate.path === path);
    if (item) openArtifactPath(item);
  }, [artifactBuckets, openArtifactPath]);

  return {
    tabs, effectiveTab, tasksCapabilityMissing, locale, onTabChange,
    run, providerLabel, onRetry,
    resolvedMainTodos, resolvedMainStatus, backgroundExecTasks, tasksLoading, tasksError, useTaskList,
    resolvedSubagents, providers, selectedSubagentId, selectedSubagent, selectedSubTodos,
    showingChildSession, onBackToMain, onSwitchSubagentKey, onSelectSubagent: handleSelectSubagent,
    onRefreshTasks: handleRefreshTasks, onCancelTask: handleCancelTask, allowCancelTask, cancellingTaskId,
    projectPath, auditLoading, auditError, auditStatus, auditCounts, onRefreshAudit: refreshAudit,
    commitMessage, setCommitMessage, committing, pushing, onGitAction: runGitAction,
    auditQuery, setAuditQuery, filteredAuditEntries, selectedAuditPath, setSelectedAuditPath, auditContent, onOpenFile,
    artifactBuckets, artifactPreviewPath, onClosePreview: () => setArtifactPreviewPath(null),
    onSelectArtifactFile: selectArtifactFile,
    allowContextUsage, contextUsage,
    eventGroups, events,
  };
}
