'use client';

import { useCallback, useEffect, useMemo, useState } from 'react';
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
import { t } from '@/i18n';
import DiffViewer from './DiffViewer';

interface ActivityInspectorProps {
  run: Run | null;
  events: RunEvent[];
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
  if (status === 'failed') return <XCircle size={14} className="text-[var(--danger)]" />;
  if (status === 'waiting_permission' || status === 'waiting_user')
    return <AlertTriangle size={14} className="text-[var(--warning)]" />;
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

export default function ActivityInspector({
  run,
  events,
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
  }, [useTaskList, gateway, run?.id, conversationId]);

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
          <div className="space-y-3">
            {useTaskList ? (
              <div className="space-y-1" data-testid="background-tasks">
                <div className="mb-2 flex items-center justify-between font-medium text-[var(--text-secondary)]">
                  <span>{zh ? '后台任务' : 'Background tasks'}</span>
                  <button
                    type="button"
                    className="text-[10px] text-[var(--primary)] hover:underline"
                    onClick={() => void refreshTasks()}
                  >
                    {zh ? '刷新' : 'Refresh'}
                  </button>
                </div>
                {tasksLoading && backgroundTasks.length === 0 ? (
                  <Empty zh={zh} zhMsg="加载任务…" enMsg="Loading tasks…" />
                ) : tasksError ? (
                  <div className="rounded border border-red-400/30 bg-red-50 p-2 text-red-600 dark:bg-red-950/20">
                    {tasksError}
                  </div>
                ) : backgroundTasks.length === 0 ? (
                  <Empty zh={zh} zhMsg="无后台任务" enMsg="No background tasks" />
                ) : (
                  backgroundTasks.map((task) => {
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
                )}
              </div>
            ) : null}

            <div className="space-y-1">
              <div className="mb-2 font-medium text-[var(--text-secondary)]">
                {useTaskList
                  ? zh
                    ? '子运行'
                    : 'Child runs'
                  : `${zh ? '父运行' : 'Parent'} · ${run.id.slice(0, 8)}`}
              </div>
              {children.length === 0 ? (
                <Empty
                  zh={zh}
                  zhMsg={useTaskList ? '无子运行' : '无子任务'}
                  enMsg={useTaskList ? 'No child runs' : 'No child tasks'}
                />
              ) : (
                children.map((ch) => (
                  <button
                    key={ch.id}
                    type="button"
                    onClick={() => onSelectChild?.(ch.id)}
                    className="flex w-full items-start gap-2 rounded px-2 py-1.5 text-left hover:bg-[var(--surface-hover)]"
                  >
                    <StatusIcon status={String(ch.status)} />
                    <div className="min-w-0 flex-1">
                      <div className="truncate font-medium">{ch.task || ch.id.slice(0, 8)}</div>
                      <div className="text-[10px] text-[var(--text-disabled)]">
                        {[ch.providerId, ch.modelId, ch.status].filter(Boolean).join(' · ')}
                      </div>
                    </div>
                  </button>
                ))
              )}
            </div>
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
          <div className="space-y-1">
            {artifacts.length === 0 ? (
              <Empty zh={zh} zhMsg="无产物" enMsg="No artifacts" />
            ) : (
              artifacts.map((a) => (
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
              ))
            )}
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

function Empty({ zh, zhMsg, enMsg }: { zh: boolean; zhMsg: string; enMsg: string }) {
  return <div className="py-8 text-center text-[var(--text-disabled)]">{zh ? zhMsg : enMsg}</div>;
}
