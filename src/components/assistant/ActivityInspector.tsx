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
import type { InspectorTab } from '@/lib/assistant-workspace';
import {
  canCancelTask,
  canListTaskDepth,
  canListTasks,
  canRewind,
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
import DiffViewer from './DiffViewer';

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
  /** Optional before/after contents keyed by path for Monaco/hunk diffs. */
  fileContentsByPath?: Record<string, { before: string; after: string }>;
  onOpenFile?: (path: string) => void;
  onRollbackFile?: (path: string) => void;
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
}

const TABS: Array<{ id: InspectorTab; zh: string; en: string; icon: typeof Play; devOnly?: boolean }> = [
  { id: 'run', zh: '运行', en: 'Run', icon: Play },
  { id: 'tasks', zh: '任务', en: 'Tasks', icon: ListTree },
  { id: 'changes', zh: '变更', en: 'Changes', icon: FileDiff },
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
  fileContentsByPath = {},
  onOpenFile,
  onRollbackFile,
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
}: ActivityInspectorProps) {
  const zh = locale.startsWith('zh');
  const allowRewind = canRewind(capabilities);
  const allowContextUsage = canShowContextUsage(capabilities);
  const allowTasks = canListTasks(capabilities);
  const useTaskList = canListTaskDepth(capabilities);
  const allowCancelTask = canCancelTask(capabilities);
  const tabs = TABS.filter((tab) => {
    if (tab.devOnly && !developerMode) return false;
    if (tab.id === 'tasks' && !allowTasks) return false;
    return true;
  });
  const [selectedChangePath, setSelectedChangePath] = useState<string | null>(null);
  const [backgroundTasks, setBackgroundTasks] = useState<BackgroundTask[]>([]);
  const [tasksLoading, setTasksLoading] = useState(false);
  const [tasksError, setTasksError] = useState<string | null>(null);
  const [cancellingTaskId, setCancellingTaskId] = useState<string | null>(null);
  const [artifactSubTab, setArtifactSubTab] = useState<'created' | 'modified'>('created');

  const selectedContents = useMemo(() => {
    const path = selectedChangePath ?? fileChanges[0]?.path ?? null;
    if (!path) return null;
    return { path, ...(fileContentsByPath[path] ?? { before: '', after: '' }) };
  }, [selectedChangePath, fileChanges, fileContentsByPath]);

  const effectiveTab =
    activeTab === 'tasks' && !allowTasks
      ? 'run'
      : tabs.some((tab) => tab.id === activeTab)
        ? activeTab
        : (tabs[0]?.id ?? 'run');

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
  const artifactBuckets = useMemo(
    () =>
      aggregateArtifactFiles({
        fileChanges,
        fileEvents,
        artifacts,
        events,
      }),
    [fileChanges, fileEvents, artifacts, events],
  );

  const refreshTasks = useCallback(async () => {
    onRefreshTasks?.();
    if (!useTaskList || !gateway) {
      setBackgroundTasks([]);
      setTasksError(null);
      return;
    }
    setTasksLoading(true);
    setTasksError(null);
    try {
      const params: Record<string, string> = {};
      if (run?.id) params.run_id = run.id;
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
  }, [useTaskList, gateway, run, conversationId, onRefreshTasks]);

  useEffect(() => {
    if (effectiveTab !== 'tasks' || !useTaskList) return;
    void refreshTasks();
  }, [effectiveTab, useTaskList, refreshTasks, run?.id, run?.status]);

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
      // Jump to 《变更》 detail — do not re-implement Diff here.
      setSelectedChangePath(item.path);
      onTabChange('changes');
      onOpenFile?.(item.path);
    },
    [onOpenFile, onTabChange, setSelectedChangePath],
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
            className={`inline-flex items-center gap-1 px-2.5 py-2 text-[11px] font-medium transition-colors ${
              effectiveTab === tab.id
                ? 'border-b-2 border-[var(--primary)] text-[var(--primary)]'
                : 'text-[var(--text-disabled)] hover:text-[var(--text-secondary)]'
            }`}
          >
            <tab.icon size={12} />
            {zh ? tab.zh : tab.en}
          </button>
        ))}
      </div>

      <div className="flex-1 overflow-y-auto p-3 text-xs">
        {!run && (
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
            <Row label={zh ? '模型' : 'Model'} value={`${run.providerId} / ${run.modelId}`} />
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
                      onClick={() => void refreshTasks()}
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
            <section data-testid="subagents-section">
              <SectionTitle title={t(locale, 'assistant.activity.subagents')} />
              {resolvedSubagents.length === 0 ? (
                <Empty
                  zh={zh}
                  zhMsg={t(locale, 'assistant.activity.noSubagents')}
                  enMsg={t(locale, 'assistant.activity.noSubagents')}
                  compact
                />
              ) : (
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
                                agent.providerId,
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
              )}
            </section>

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

        {run && effectiveTab === 'changes' && (
          <div className="space-y-2">
            {fileChanges.length === 0 ? (
              <Empty zh={zh} zhMsg="无文件变更" enMsg="No file changes" />
            ) : (
              <>
                <div className="space-y-0.5">
                  {fileChanges.map((f, i) => (
                    <button
                      key={`${f.path}-${i}`}
                      type="button"
                      onClick={() => setSelectedChangePath(f.path)}
                      className={`flex w-full items-center gap-2 rounded px-1 py-1 text-left font-mono hover:bg-[var(--surface-hover)] ${
                        (selectedChangePath ?? fileChanges[0]?.path) === f.path
                          ? 'bg-[var(--surface-hover)]'
                          : ''
                      }`}
                    >
                      <span className="text-[var(--text-disabled)]">{f.changeType}</span>
                      <span className="truncate">{f.path}</span>
                    </button>
                  ))}
                </div>
                {selectedContents && (
                  <DiffViewer
                    fileName={selectedContents.path}
                    oldContent={selectedContents.before}
                    newContent={selectedContents.after}
                    mode="full"
                    locale={locale}
                    onOpenFile={
                      onOpenFile ? () => onOpenFile(selectedContents.path) : undefined
                    }
                    onRollback={
                      allowRewind && onRollbackFile
                        ? () => onRollbackFile(selectedContents.path)
                        : undefined
                    }
                  />
                )}
              </>
            )}
          </div>
        )}

        {run && effectiveTab === 'artifacts' && (
          <div className="space-y-2" data-testid="artifacts-panel">
            <div className="flex gap-1 border-b border-[var(--border)] pb-1">
              {(
                [
                  {
                    id: 'created' as const,
                    label: t(locale, 'assistant.activity.artifactsCreated'),
                    count: artifactBuckets.created.length,
                  },
                  {
                    id: 'modified' as const,
                    label: t(locale, 'assistant.activity.artifactsEdited'),
                    count: artifactBuckets.modified.length,
                  },
                ] as const
              ).map((sub) => (
                <button
                  key={sub.id}
                  type="button"
                  onClick={() => setArtifactSubTab(sub.id)}
                  className={`rounded px-2 py-1 text-[11px] font-medium ${
                    artifactSubTab === sub.id
                      ? 'bg-[var(--surface-hover)] text-[var(--primary)]'
                      : 'text-[var(--text-disabled)] hover:text-[var(--text-secondary)]'
                  }`}
                  data-testid={`artifact-subtab-${sub.id}`}
                >
                  {sub.label}
                  <span className="ml-1 text-[10px] text-[var(--text-disabled)]">{sub.count}</span>
                </button>
              ))}
            </div>

            {(() => {
              const list =
                artifactSubTab === 'created' ? artifactBuckets.created : artifactBuckets.modified;
              if (list.length === 0) {
                return (
                  <Empty
                    zh={zh}
                    zhMsg={
                      artifactSubTab === 'created'
                        ? t(locale, 'assistant.activity.noCreatedFiles')
                        : t(locale, 'assistant.activity.noEditedFiles')
                    }
                    enMsg={
                      artifactSubTab === 'created'
                        ? t(locale, 'assistant.activity.noCreatedFiles')
                        : t(locale, 'assistant.activity.noEditedFiles')
                    }
                  />
                );
              }
              return (
                <div className="space-y-0.5">
                  {list.map((item) => (
                    <button
                      key={item.path}
                      type="button"
                      onClick={() => openArtifactPath(item)}
                      className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-left font-mono hover:bg-[var(--surface-hover)]"
                      data-testid={`artifact-file-${item.path}`}
                    >
                      <Package size={12} className="shrink-0 text-[var(--text-disabled)]" />
                      <span className="min-w-0 flex-1 truncate">{item.path}</span>
                      <span className="shrink-0 text-[10px] text-[var(--text-disabled)]">
                        {item.changeType === 'created'
                          ? zh
                            ? '新增'
                            : 'new'
                          : zh
                            ? '编辑'
                            : 'edit'}
                      </span>
                    </button>
                  ))}
                </div>
              );
            })()}

            {/* Legacy non-path artifact actions remain available under buckets empty / mixed */}
            {artifacts.length > 0 &&
            artifactBuckets.created.length === 0 &&
            artifactBuckets.modified.length === 0 ? (
              <div className="space-y-1 border-t border-[var(--border)] pt-2">
                {artifacts.map((a) => (
                  <div
                    key={a.id}
                    className="flex items-center gap-2 rounded px-2 py-1.5 hover:bg-[var(--surface-hover)]"
                  >
                    <Package size={12} className="shrink-0 text-[var(--text-disabled)]" />
                    <div className="min-w-0 flex-1">
                      <div className="truncate">{a.label || a.path}</div>
                      {a.staleReason && (
                        <div className="text-[10px] text-[var(--danger)]">{a.staleReason}</div>
                      )}
                    </div>
                    <button type="button" className="text-[var(--primary)]" onClick={() => onOpenArtifact?.(a)}>
                      {zh ? '打开' : 'Open'}
                    </button>
                    <button type="button" onClick={() => onRevealArtifact?.(a)}>
                      {zh ? '显示' : 'Reveal'}
                    </button>
                  </div>
                ))}
              </div>
            ) : null}
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
              events.map((e) => (
                <div key={`${e.runId}-${e.sequence}`} className="flex gap-2 px-1 py-0.5 hover:bg-[var(--surface-hover)]">
                  <span className="w-6 shrink-0 text-[var(--text-disabled)]">#{e.sequence}</span>
                  <span className="text-[var(--text-secondary)]">{e.type}</span>
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
