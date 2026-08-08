'use client';

import { useCallback, useEffect, useMemo, useState, type ReactNode } from 'react';
import {
  Play,
  ListTree,
  FileDiff,
  Package,
  Brain,
  Radio,
  CheckCircle,
  XCircle,
  Clock,
  AlertTriangle,
  Square,
  ArrowLeft,
  RefreshCw,
  KeyRound,
  GitBranch,
  Search,
  Upload,
} from 'lucide-react';
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
import type { ProviderWithModels } from './ModelSelectorDropdown';
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
  todoStatusLabel,
  type ActivityTodo,
  type ArtifactFileItem,
  type FileEventInput,
  type TodoStatus,
} from '@/lib/assistant-activity-view';
import { t } from '@/i18n';
import ArtifactPreviewSurface from '@/components/preview/ArtifactPreviewSurface';

export interface ActivitySubagentView {
  id: string;
  name: string;
  status: string;
  providerId?: string;
  keyLabel?: string;
  childConversationId?: string;
  task?: string;
  todos?: Array<{ id: string; content: string; status: TodoStatus }>;
  /** Failure message — never a raw key. */
  error?: string;
}

interface ActivityInspectorProps {
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

const TABS: Array<{ id: InspectorTab; zh: string; en: string; icon: typeof Play; devOnly?: boolean }> = [
  { id: 'run', zh: '运行', en: 'Run', icon: Play },
  { id: 'tasks', zh: '任务', en: 'Tasks', icon: ListTree },
  { id: 'changes', zh: '审计', en: 'Audit', icon: FileDiff },
  { id: 'artifacts', zh: '产物', en: 'Artifacts', icon: Package },
  { id: 'context', zh: '上下文', en: 'Context', icon: Brain },
  { id: 'events', zh: '事件', en: 'Events', icon: Radio, devOnly: true },
];

function StatusIcon({ status }: { status: string }) {
  if (status === 'completed') return <CheckCircle size={14} className="text-[var(--success)]" />;
  if (status === 'failed' || status === 'closed') return <XCircle size={14} className="text-[var(--danger)]" />;
  if (status === 'waiting_permission' || status === 'waiting_user' || status === 'pending_assignment')
    return <AlertTriangle size={14} className="text-[var(--warning)]" />;
  if (status === 'in_progress' || status === 'running' || status === 'queued')
    return <Clock size={14} className="text-[var(--primary)]" />;
  return <Clock size={14} className="text-[var(--text-disabled)]" />;
}

function mapWireBackgroundTask(raw: Record<string, unknown>): BackgroundTask {
  const id = String(raw.id ?? '');
  const kindRaw = String(raw.kind ?? 'other');
  const kind: BackgroundTask['kind'] =
    kindRaw === 'subagent' ||
    kindRaw === 'terminal' ||
    kindRaw === 'monitor' ||
    kindRaw === 'scheduler'
      ? kindRaw
      : 'other';
  const output =
    typeof raw.output === 'string'
      ? raw.output
      : raw.output == null
        ? null
        : String(raw.output);
  const title =
    typeof raw.title === 'string' && raw.title.trim()
      ? raw.title
      : output
        ? output.slice(0, 80)
        : id.slice(0, 8) || 'task';
  return {
    id,
    kind,
    runId: raw.run_id != null ? String(raw.run_id) : raw.runId != null ? String(raw.runId) : undefined,
    conversationId:
      raw.conversation_id != null
        ? String(raw.conversation_id)
        : raw.conversationId != null
          ? String(raw.conversationId)
          : undefined,
    title,
    status: String(raw.status ?? 'unknown'),
    createdAt: String(raw.created_at ?? raw.createdAt ?? ''),
    error: raw.error != null ? String(raw.error) : undefined,
    output,
  };
}

function snippet(text: string | null | undefined, max = 120): string {
  if (!text) return '';
  const oneLine = text.replace(/\s+/g, ' ').trim();
  if (oneLine.length <= max) return oneLine;
  return `${oneLine.slice(0, max)}…`;
}

function TodoList({
  todos,
  zh,
  emptyZh,
  emptyEn,
}: {
  todos: ActivityTodo[];
  zh: boolean;
  emptyZh: string;
  emptyEn: string;
}) {
  if (todos.length === 0) {
    return <Empty zh={zh} zhMsg={emptyZh} enMsg={emptyEn} compact />;
  }
  return (
    <ul className="space-y-1" data-testid="todo-list">
      {todos.map((todo) => (
        <li
          key={todo.id}
          className="flex items-start gap-2 rounded px-2 py-1 hover:bg-[var(--surface-hover)]"
        >
          <StatusIcon status={todo.status} />
          <div className="min-w-0 flex-1">
            <div className="truncate text-[var(--text-secondary)]">{todo.content}</div>
            <div className="text-[10px] text-[var(--text-disabled)]">
              {todoStatusLabel(todo.status, zh)}
            </div>
          </div>
        </li>
      ))}
    </ul>
  );
}

function SectionTitle({
  title,
  actions,
}: {
  title: string;
  actions?: ReactNode;
}) {
  return (
    <div className="mb-1.5 flex items-center justify-between gap-2 font-medium text-[var(--text-secondary)]">
      <span>{title}</span>
      {actions ? <div className="flex shrink-0 items-center gap-1.5">{actions}</div> : null}
    </div>
  );
}

export default function ActivityInspector({
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
  onOpenArtifact,
  onRevealArtifact,
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
}: ActivityInspectorProps) {
  const zh = locale.startsWith('zh');
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
  const [artifactSubTab, setArtifactSubTab] = useState<'created' | 'modified'>('created');
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

  // Delta events are stream chunks, not repeated tool calls. Keep lifecycle
  // events individual while compacting adjacent delta chunks for inspection.
  const eventGroups = useMemo(() => {
    const groups: Array<{ type: string; first: number; last: number; count: number }> = [];
    for (const event of events) {
      const previous = groups.at(-1);
      const compactable = event.type === 'reasoning_delta' || event.type === 'text_delta';
      if (compactable && previous?.type === event.type && previous.last + 1 === event.sequence) {
        previous.last = event.sequence;
        previous.count += 1;
      } else {
        groups.push({ type: event.type, first: event.sequence, last: event.sequence, count: 1 });
      }
    }
    return groups;
  }, [events]);

  // Keyboard shortcuts (⌘⇧T) can still land on a disabled tab; keep it as the
  // effective tab and render an explanatory empty state instead of a blank panel.
  const effectiveTab = tabs.some((tab) => tab.id === activeTab)
    ? activeTab
    : (tabs[0]?.id ?? 'run');
  const tasksCapabilityMissing = effectiveTab === 'tasks' && !allowTasks;

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

  return (
    <div className="flex h-full flex-col border-l border-[var(--border)] bg-[var(--surface)]">
      <div className="flex flex-wrap border-b border-[var(--border)]">
        {tabs.map((tab) => (
          <button
            key={tab.id}
            type="button"
            onClick={() => onTabChange(tab.id)}
            aria-disabled={tab.disabled}
            title={
              tab.disabled
                ? // i18n-pending: i18n files frozen this round; follow file-local zh/en pattern.
                  zh
                  ? '引擎未广播该能力（task.list / run.listChildren）'
                  : 'Engine did not advertise this capability (task.list / run.listChildren)'
                : undefined
            }
            className={`inline-flex items-center gap-1 px-2.5 py-2 text-[11px] font-medium transition-colors ${
              tab.disabled
                ? 'cursor-not-allowed text-[var(--text-disabled)] opacity-50'
                : effectiveTab === tab.id
                  ? 'border-b-2 border-[var(--primary)] text-[var(--primary)]'
                  : 'text-[var(--text-disabled)] hover:text-[var(--text-secondary)]'
            }`}
            data-testid={`inspector-tab-${tab.id}${tab.disabled ? '-disabled' : ''}`}
          >
            <tab.icon size={12} />
            {zh ? tab.zh : tab.en}
          </button>
        ))}
      </div>

      <div className={`flex-1 ${effectiveTab === 'changes' || effectiveTab === 'artifacts' ? 'flex flex-col min-h-0 overflow-hidden' : 'overflow-y-auto'} p-3 text-xs`}>
        {tasksCapabilityMissing && (
          <div
            className="py-8 text-center text-[var(--text-disabled)]"
            data-testid="tasks-capability-not-ready"
          >
            {/* i18n-pending: i18n files frozen this round; follow file-local zh/en pattern. */}
            <div>{zh ? '任务面板暂不可用' : 'Tasks panel unavailable'}</div>
            <div className="mt-1 text-[10px]">
              {zh
                ? '引擎未广播任务能力（task.list / run.listChildren）。等待引擎连接就绪，或升级引擎后重试。'
                : 'The engine did not advertise task capabilities (task.list / run.listChildren). Wait for the engine to connect, or upgrade the engine.'}
            </div>
          </div>
        )}

        {!run && !tasksCapabilityMissing && (
          <div className="grid h-full place-items-center text-[var(--text-disabled)]">
            {zh ? '选择运行以查看详情' : 'Select a run to inspect'}
          </div>
        )}

        {run && effectiveTab === 'run' && (
          <div className="space-y-2">
            <div className="flex items-center gap-2">
              <StatusIcon status={run.status} />
              <span className="font-mono text-[var(--text-secondary)]">{run.id.slice(0, 10)}</span>
              <span className="rounded bg-[var(--surface-hover)] px-1.5 py-0.5">{run.status}</span>
            </div>
            <Row label={zh ? '模型' : 'Model'} value={providerLabel(run.providerId, run.modelId)} />
            {run.activity && <Row label={zh ? '活动' : 'Activity'} value={run.activity} />}
            {run.errorMessage && (
              <div className="rounded border border-red-400/30 bg-red-50 p-2 text-red-600 dark:bg-red-950/20">
                {run.errorMessage}
              </div>
            )}
            {onRetry && (run.status === 'failed' || run.status === 'interrupted') && (
              <button
                type="button"
                onClick={onRetry}
                className="rounded bg-[var(--primary)] px-3 py-1.5 text-white"
              >
                {zh ? '重试' : 'Retry'}
              </button>
            )}
          </div>
        )}

        {run && effectiveTab === 'tasks' && allowTasks && (
          <div className="space-y-4" data-testid="tasks-panel">
            {/* ── 主任务 ── */}
            <section data-testid="main-task-section">
              <SectionTitle
                title={t(locale, 'assistant.activity.mainTask')}
                actions={
                  <>
                    <button
                      type="button"
                      className="inline-flex items-center gap-0.5 text-[10px] text-[var(--primary)] hover:underline"
                      onClick={handleRefreshTasks}
                      title={t(locale, 'assistant.activity.refresh')}
                    >
                      <RefreshCw size={10} />
                      {t(locale, 'assistant.activity.refresh')}
                    </button>
                    {showingChildSession && onBackToMain ? (
                      <button
                        type="button"
                        className="inline-flex items-center gap-0.5 text-[10px] text-[var(--primary)] hover:underline"
                        onClick={onBackToMain}
                        data-testid="back-to-main"
                      >
                        <ArrowLeft size={10} />
                        {t(locale, 'assistant.activity.backToMain')}
                      </button>
                    ) : null}
                  </>
                }
              />
              <div className="mb-2 flex items-center gap-2 rounded bg-[var(--surface-hover)] px-2 py-1.5">
                <StatusIcon status={resolvedMainStatus} />
                <span className="text-[var(--text-secondary)]">
                  {todoStatusLabel(resolvedMainStatus, zh)}
                </span>
                {run.status ? (
                  <span className="ml-auto font-mono text-[10px] text-[var(--text-disabled)]">
                    {run.id.slice(0, 8)} · {run.status}
                  </span>
                ) : null}
              </div>
              <div className="mb-1 text-[10px] font-medium uppercase tracking-wide text-[var(--text-disabled)]">
                {t(locale, 'assistant.activity.sessionTodos')}
              </div>
              <TodoList
                todos={resolvedMainTodos}
                zh={zh}
                emptyZh={t(locale, 'assistant.activity.noTodos')}
                emptyEn={t(locale, 'assistant.activity.noTodos')}
              />
            </section>

            {/* ── 后台执行 ── */}
            <section data-testid="background-tasks">
              <SectionTitle title={t(locale, 'assistant.activity.background')} />
              {useTaskList ? (
                tasksLoading && backgroundExecTasks.length === 0 ? (
                  <Empty
                    zh={zh}
                    zhMsg={t(locale, 'assistant.activity.loadingTasks')}
                    enMsg={t(locale, 'assistant.activity.loadingTasks')}
                    compact
                  />
                ) : tasksError ? (
                  <div className="rounded border border-red-400/30 bg-red-50 p-2 text-red-600 dark:bg-red-950/20">
                    {tasksError}
                  </div>
                ) : backgroundExecTasks.length === 0 ? (
                  <Empty
                    zh={zh}
                    zhMsg={t(locale, 'assistant.activity.noBackground')}
                    enMsg={t(locale, 'assistant.activity.noBackground')}
                    compact
                  />
                ) : (
                  backgroundExecTasks.map((task) => {
                    const active =
                      task.status === 'running' ||
                      task.status === 'pending' ||
                      task.status === 'in_progress' ||
                      task.status === 'active';
                    return (
                      <div
                        key={task.id}
                        className="flex w-full items-start gap-2 rounded px-2 py-1.5 hover:bg-[var(--surface-hover)]"
                      >
                        <StatusIcon status={String(task.status)} />
                        <div className="min-w-0 flex-1">
                          <div className="truncate font-medium">{task.title}</div>
                          <div className="text-[10px] text-[var(--text-disabled)]">
                            {[task.kind, task.status, task.id.slice(0, 8)].filter(Boolean).join(' · ')}
                          </div>
                          {snippet(task.output || task.error) ? (
                            <div className="mt-0.5 line-clamp-2 text-[10px] text-[var(--text-secondary)]">
                              {snippet(task.output || task.error)}
                            </div>
                          ) : null}
                        </div>
                        {allowCancelTask && active ? (
                          <button
                            type="button"
                            title={zh ? '取消任务' : 'Cancel task'}
                            disabled={cancellingTaskId === task.id}
                            className="shrink-0 rounded p-1 text-[var(--danger)] hover:bg-[var(--surface-hover)] disabled:opacity-40"
                            onClick={() => void handleCancelTask(task.id)}
                          >
                            <Square size={12} fill="currentColor" />
                          </button>
                        ) : null}
                      </div>
                    );
                  })
                )
              ) : (
                <Empty
                  zh={zh}
                  zhMsg={t(locale, 'assistant.activity.backgroundNotReady')}
                  enMsg={t(locale, 'assistant.activity.backgroundNotReady')}
                  compact
                />
              )}
            </section>

            {/* ── 子智能体 ── */}
            {resolvedSubagents.length > 0 && (
              <section data-testid="subagents-section">
                <SectionTitle title={t(locale, 'assistant.activity.subagents')} />
                <div className="space-y-0.5">
                  {resolvedSubagents.map((agent) => {
                    const ui = mapSubagentUiStatus(agent.status);
                    const selected = selectedSubagentId === agent.id;
                    return (
                      <div
                        key={agent.id}
                        className={`flex w-full items-start gap-2 rounded px-2 py-1.5 ${
                          selected ? 'bg-[var(--surface-hover)] ring-1 ring-[var(--primary)]/30' : 'hover:bg-[var(--surface-hover)]'
                        }`}
                      >
                        <button
                          type="button"
                          className="flex min-w-0 flex-1 items-start gap-2 text-left"
                          onClick={() => handleSelectSubagent(agent.id)}
                          data-testid={`subagent-row-${agent.id}`}
                        >
                          <StatusIcon status={ui.key} />
                          <div className="min-w-0 flex-1">
                            <div className="truncate font-medium">{agent.name}</div>
                            <div className="text-[10px] text-[var(--text-disabled)]">
                              {[
                                t(locale, `assistant.activity.subagentStatus.${ui.key}`),
                                providers.find((provider) => provider.id === agent.providerId)?.name ?? agent.providerId,
                                agent.keyLabel,
                              ]
                                .filter(Boolean)
                                .join(' · ')}
                            </div>
                            {ui.key === 'closed' && agent.error ? (
                              <div
                                className="mt-0.5 truncate text-[10px] text-[var(--danger)]"
                                title={agent.error}
                                data-testid={`subagent-error-${agent.id}`}
                              >
                                {agent.error}
                              </div>
                            ) : null}
                          </div>
                        </button>
                        {onSwitchSubagentKey ? (
                          <button
                            type="button"
                            title={t(locale, 'assistant.activity.switchKey')}
                            className="inline-flex shrink-0 items-center gap-0.5 rounded px-1.5 py-0.5 text-[10px] text-[var(--primary)] hover:bg-[var(--surface)]"
                            onClick={() => onSwitchSubagentKey(agent.id)}
                            data-testid={`switch-key-${agent.id}`}
                          >
                            <KeyRound size={10} />
                            {t(locale, 'assistant.activity.switchKey')}
                          </button>
                        ) : null}
                      </div>
                    );
                  })}
                </div>
              </section>
            )}

            {/* ── 子任务（选中子智能体后） ── */}
            {selectedSubagent ? (
              <section data-testid="sub-task-section">
                <SectionTitle
                  title={`${t(locale, 'assistant.activity.subTasks')} · ${selectedSubagent.name}`}
                />
                <TodoList
                  todos={selectedSubTodos}
                  zh={zh}
                  emptyZh={t(locale, 'assistant.activity.noSubTasks')}
                  emptyEn={t(locale, 'assistant.activity.noSubTasks')}
                />
              </section>
            ) : null}
          </div>
        )}

        {effectiveTab === 'changes' && (
          <div className="flex flex-col h-full min-h-0 space-y-2">
            {!projectPath ? (
              <Empty zh={zh} zhMsg="未选择项目" enMsg="No project selected" />
            ) : auditLoading ? (
              <Empty zh={zh} zhMsg="正在读取 Git 状态" enMsg="Loading Git status" compact />
            ) : auditError ? (
              <div className="rounded border border-red-400/30 p-2 text-[var(--danger)]">{auditError}</div>
            ) : (
              <>
                <div className="shrink-0 rounded border border-[var(--border)] p-2">
                  <div className="flex items-center gap-2">
                    <GitBranch size={13} />
                    <span className="min-w-0 flex-1 truncate font-mono">{auditStatus?.branch ?? 'unknown'}</span>
                    <span className="text-emerald-500">+{auditCounts.additions}</span>
                    <span className="text-red-500">−{auditCounts.deletions}</span>
                    <button
                      type="button"
                      onClick={() => void refreshAudit()}
                      className="rounded p-1 hover:bg-[var(--surface-hover)]"
                      title={zh ? '刷新' : 'Refresh'}
                    >
                      <RefreshCw size={12} />
                    </button>
                  </div>
                  <div className="mt-2 flex gap-1">
                    <input
                      value={commitMessage}
                      onChange={(event) => setCommitMessage(event.target.value)}
                      placeholder={zh ? '提交说明' : 'Commit message'}
                      className="min-w-0 flex-1 rounded border border-[var(--border)] bg-transparent px-2 py-1"
                    />
                    <button
                      type="button"
                      disabled={committing || !commitMessage.trim()}
                      onClick={() => void runGitAction('commit')}
                      className="rounded bg-[var(--primary)] px-2 py-1 text-white disabled:opacity-40"
                    >
                      {committing ? (zh ? '提交中' : 'Committing') : (zh ? '提交' : 'Commit')}
                    </button>
                    <button
                      type="button"
                      disabled={pushing}
                      onClick={() => void runGitAction('push')}
                      className="rounded border border-[var(--border)] px-2 py-1 disabled:opacity-40"
                      title={zh ? '推送' : 'Push'}
                    >
                      <Upload size={12} />
                    </button>
                  </div>
                </div>
                <div className="grid flex-1 min-h-0 min-h-[300px] grid-cols-2 gap-2">
                  {/* 文件预览独立滑动区 */}
                  <div
                    className="flex flex-col min-w-0 min-h-0 overflow-y-auto overscroll-contain rounded border border-[var(--border)] bg-[var(--background)] p-2 text-[10px] leading-4 font-mono select-text"
                    onWheel={(e) => e.stopPropagation()}
                  >
                    {auditContent ? (
                      auditContent.split('\n').map((line, idx) => {
                        const isAdd = line.startsWith('+') && !line.startsWith('+++');
                        const isDel = line.startsWith('-') && !line.startsWith('---');
                        const isHunk = line.startsWith('@@');
                        return (
                          <div
                            key={idx}
                            className={
                              isAdd
                                ? 'bg-emerald-500/10 text-emerald-500'
                                : isDel
                                  ? 'bg-red-500/10 text-red-500'
                                  : isHunk
                                    ? 'text-blue-400 font-semibold'
                                    : 'text-[var(--text-secondary)]'
                            }
                          >
                            {line}
                          </div>
                        );
                      })
                    ) : (
                      <span className="text-[var(--text-disabled)]">
                        {zh ? '选择文件以预览' : 'Select a file to preview'}
                      </span>
                    )}
                  </div>
                  {/* 目录结构独立滑动区 */}
                  <div
                    className="flex flex-col min-w-0 min-h-0 rounded border border-[var(--border)] overflow-hidden"
                    onWheel={(e) => e.stopPropagation()}
                  >
                    <label className="flex shrink-0 items-center gap-1 border-b border-[var(--border)] px-2 py-1">
                      <Search size={12} />
                      <input
                        value={auditQuery}
                        onChange={(event) => setAuditQuery(event.target.value)}
                        placeholder={zh ? '筛选文件…' : 'Filter files…'}
                        className="min-w-0 flex-1 bg-transparent outline-none"
                      />
                    </label>
                    <div className="flex-1 min-h-0 overflow-y-auto overscroll-contain">
                      {filteredAuditEntries.map((entry) => {
                        const selected = (selectedAuditPath ?? auditStatus?.entries[0]?.path) === entry.path;
                        const marker =
                          entry.status === 'added' || entry.status === 'untracked'
                            ? '+'
                            : entry.status === 'deleted'
                              ? '−'
                              : entry.status === 'modified'
                                ? '•'
                                : '';
                        return (
                          <button
                            key={`${entry.status}-${entry.path}`}
                            type="button"
                            onClick={() => {
                              setSelectedAuditPath(entry.path);
                              onOpenFile?.(`${projectPath.replace(/\/$/, '')}/${entry.path}`);
                            }}
                            className={`flex w-full items-center gap-2 px-2 py-1 text-left font-mono hover:bg-[var(--surface-hover)] ${
                              selected ? 'bg-[var(--surface-hover)]' : ''
                            }`}
                          >
                            <span
                              className={
                                entry.status === 'deleted'
                                  ? 'text-red-500'
                                  : entry.status === 'added' || entry.status === 'untracked'
                                    ? 'text-emerald-500'
                                    : entry.status === 'modified'
                                      ? 'text-orange-400'
                                      : 'text-transparent'
                              }
                            >
                              {marker}
                            </span>
                            <span className="truncate">{entry.path}</span>
                          </button>
                        );
                      })}
                      {filteredAuditEntries.length === 0 && (
                        <Empty zh={zh} zhMsg="无未提交修改" enMsg="No uncommitted changes" compact />
                      )}
                    </div>
                  </div>
                </div>
              </>
            )}
          </div>
        )}

        {run && effectiveTab === 'artifacts' && (
          <div className="flex h-full flex-col gap-2" data-testid="artifacts-panel">
            {artifactPreviewPath && (
              <div className="flex h-1/2 min-h-0 flex-col rounded-lg border border-[var(--border)] overflow-hidden">
                <div className="flex items-center justify-between border-b border-[var(--border)] px-2 py-1">
                  <span className="truncate font-mono text-[10px] text-[var(--text-secondary)]">{artifactPreviewPath}</span>
                  <button
                    type="button"
                    onClick={() => setArtifactPreviewPath(null)}
                    className="text-[10px] text-[var(--text-disabled)] hover:text-[var(--text)]"
                  >
                    {zh ? '关闭' : 'Close'}
                  </button>
                </div>
                <div className="min-h-0 flex-1 overflow-auto">
                  <ArtifactPreviewSurface
                    source={{ type: 'file', path: artifactPreviewPath }}
                    autoLoad
                  />
                </div>
              </div>
            )}
            {[
              { key: 'used' as const, titleZh: '使用文件', titleEn: 'Used Files', items: artifactBuckets.used },
              { key: 'modified' as const, titleZh: '编辑', titleEn: 'Modified', items: artifactBuckets.modified },
              { key: 'created' as const, titleZh: '新增', titleEn: 'Created', items: artifactBuckets.created },
            ].map((section) => (
              <div key={section.key} className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-lg border border-[var(--border)] p-2">
                <div className="mb-1 flex items-center justify-between font-medium text-[var(--text-secondary)]">
                  <span>{zh ? section.titleZh : section.titleEn}</span>
                  <span className="text-[10px] text-[var(--text-disabled)]">{section.items.length}</span>
                </div>
                <div className="flex-1 overflow-y-auto space-y-0.5">
                  {section.items.length === 0 ? (
                    <div className="py-2 text-center text-[10px] text-[var(--text-disabled)]">
                      {zh ? '暂无文件' : 'No files'}
                    </div>
                  ) : (
                    section.items.map((item) => (
                      <button
                        key={item.path}
                        type="button"
                        onClick={() => {
                          setArtifactPreviewPath(item.path);
                          openArtifactPath(item);
                        }}
                        className="flex w-full items-center gap-2 rounded px-2 py-1 text-left font-mono hover:bg-[var(--surface-hover)]"
                        data-testid={`artifact-file-${item.path}`}
                      >
                        <Package size={12} className="shrink-0 text-[var(--text-disabled)]" />
                        <span className="min-w-0 flex-1 truncate">{item.path}</span>
                      </button>
                    ))
                  )}
                </div>
              </div>
            ))}
          </div>
        )}

        {run && effectiveTab === 'context' && (
          <div className="space-y-2">
            {!allowContextUsage ? (
              <div
                className="py-8 text-center text-[var(--text-disabled)]"
                data-testid="context-capability-not-ready"
              >
                {t(locale, 'assistant.contextUsageNotReady')}
              </div>
            ) : contextUsage ? (
              <>
                <div className="h-2 overflow-hidden rounded-full bg-[var(--surface-hover)]">
                  <div
                    className="h-full bg-[var(--primary)]"
                    style={{
                      width: `${Math.min(100, (contextUsage.usedTokens / Math.max(1, contextUsage.maxTokens)) * 100)}%`,
                    }}
                  />
                </div>
                <Row
                  label={zh ? '用量' : 'Usage'}
                  value={`${contextUsage.usedTokens} / ${contextUsage.maxTokens}`}
                />
              </>
            ) : (
              <Empty zh={zh} zhMsg="暂无上下文数据" enMsg="No context usage" />
            )}
          </div>
        )}

        {run && effectiveTab === 'events' && (
          <div className="space-y-0.5 font-mono">
            {events.length === 0 ? (
              <Empty zh={zh} zhMsg="暂无事件" enMsg="No events" />
            ) : (
              eventGroups.map((event) => (
                <div key={`${event.type}-${event.first}`} className="flex gap-2 px-1 py-0.5 hover:bg-[var(--surface-hover)]">
                  <span className="shrink-0 text-[var(--text-disabled)]">#{event.first}{event.last !== event.first ? `–${event.last}` : ''}</span>
                  <span className="text-[var(--text-secondary)]">{event.type}{event.count > 1 ? ` ×${event.count}` : ''}</span>
                </div>
              ))
            )}
          </div>
        )}
      </div>
    </div>
  );
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex justify-between gap-2">
      <span className="text-[var(--text-disabled)]">{label}</span>
      <span className="truncate text-right text-[var(--text-secondary)]">{value}</span>
    </div>
  );
}

function Empty({
  zh,
  zhMsg,
  enMsg,
  compact = false,
}: {
  zh: boolean;
  zhMsg: string;
  enMsg: string;
  compact?: boolean;
}) {
  return (
    <div
      className={`${compact ? 'py-3' : 'py-8'} text-center text-[var(--text-disabled)]`}
    >
      {zh ? zhMsg : enMsg}
    </div>
  );
}
