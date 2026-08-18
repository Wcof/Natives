'use client';

import type { ReactNode } from 'react';
import {
  AlertTriangle,
  ArrowLeft,
  CheckCircle,
  Clock,
  GitBranch,
  KeyRound,
  Package,
  RefreshCw,
  Search,
  Square,
  Upload,
  XCircle,
} from 'lucide-react';
import type { BackgroundTask, ContextUsage, Run } from '@/lib/assistant-protocol';
import type { ProviderWithModels } from '@/components/assistant/conversation/ModelSelectorDropdown';
import {
  mapSubagentUiStatus,
  todoStatusLabel,
  type ActivityTodo,
  type ArtifactFileItem,
  type TodoStatus,
} from '@/lib/assistant-activity-view';
import { t } from '@/i18n';
import ArtifactPreviewSurface from '@/components/preview/ArtifactPreviewSurface';
import { snippet, type ActivitySubagentView, type DeltaEventGroup } from './model';

// ── Presentation atoms ───────────────────────────────────────────────────────

export function StatusIcon({ status }: { status: string }) {
  if (status === 'completed') return <CheckCircle size={14} className="text-[var(--success)]" />;
  if (status === 'failed' || status === 'closed') return <XCircle size={14} className="text-[var(--danger)]" />;
  if (status === 'waiting_permission' || status === 'waiting_user' || status === 'pending_assignment')
    return <AlertTriangle size={14} className="text-[var(--warning)]" />;
  if (status === 'in_progress' || status === 'running' || status === 'queued')
    return <Clock size={14} className="text-[var(--primary)]" />;
  return <Clock size={14} className="text-[var(--text-disabled)]" />;
}

export function TodoList({
  todos,
  locale,
  emptyKey,
}: {
  todos: ActivityTodo[];
  locale: string;
  emptyKey: string;
}) {
  if (todos.length === 0) {
    return <Empty locale={locale} messageKey={emptyKey} compact />;
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
              {todoStatusLabel(locale, todo.status)}
            </div>
          </div>
        </li>
      ))}
    </ul>
  );
}

export function SectionTitle({
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

export function Row({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex justify-between gap-2">
      <span className="text-[var(--text-disabled)]">{label}</span>
      <span className="truncate text-right text-[var(--text-secondary)]">{value}</span>
    </div>
  );
}

export function Empty({
  locale,
  messageKey,
  compact = false,
}: {
  locale: string;
  messageKey: string;
  compact?: boolean;
}) {
  return (
    <div
      className={`${compact ? 'py-3' : 'py-8'} text-center text-[var(--text-disabled)]`}
    >
      {t(locale, messageKey)}
    </div>
  );
}

// ── Tab panels ───────────────────────────────────────────────────────────────

export function RunPanel({
  run,
  locale,
  providerLabel,
  onRetry,
  runError,
}: {
  run: Run;
  locale: string;
  providerLabel: (providerId: string, modelId: string) => string;
  onRetry?: () => void;
  /** Run-level watch/transport error (product decision 4: never global banner). */
  runError?: string | null;
}) {
  return (
    <div className="space-y-2">
      <div className="flex items-center gap-2">
        <StatusIcon status={run.status} />
        <span className="font-mono text-[var(--text-secondary)]">{run.id.slice(0, 10)}</span>
        <span className="rounded bg-[var(--surface-hover)] px-1.5 py-0.5">{run.status}</span>
      </div>
      <Row label={t(locale, 'activityInspector.model')} value={providerLabel(run.providerId, run.modelId)} />
      {run.activity && <Row label={t(locale, 'activityInspector.activity')} value={run.activity} />}
      {runError && (
        <div
          className="rounded border border-[var(--danger)]/30 bg-[var(--danger-soft)] p-2 text-[var(--danger)]"
          data-testid="run-level-error"
        >
          {t(locale, runError)}
        </div>
      )}
      {run.errorMessage && (
        <div className="rounded border border-[var(--danger)]/30 bg-[var(--danger-soft)] p-2 text-[var(--danger)]">
          {run.errorMessage}
        </div>
      )}
      {onRetry && (run.status === 'failed' || run.status === 'interrupted') && (
        <button
          type="button"
          onClick={onRetry}
          className="rounded bg-[var(--primary)] px-3 py-1.5 text-[var(--accent-ink)]"
        >
          {t(locale, 'common.retry')}
        </button>
      )}
    </div>
  );
}

export interface TasksPanelProps {
  locale: string;
  run: Run;
  resolvedMainTodos: ActivityTodo[];
  resolvedMainStatus: TodoStatus;
  onRefreshTasks: () => void;
  showingChildSession: boolean;
  onBackToMain?: () => void;
  backgroundExecTasks: BackgroundTask[];
  tasksLoading: boolean;
  tasksError: string | null;
  useTaskList: boolean;
  resolvedSubagents: ActivitySubagentView[];
  providers: ProviderWithModels[];
  selectedSubagentId: string | null;
  selectedSubagent: ActivitySubagentView | null;
  selectedSubTodos: ActivityTodo[];
  onSwitchSubagentKey?: (id: string) => void;
  onSelectSubagent: (id: string) => void;
  onCancelTask: (taskId: string) => Promise<void>;
  allowCancelTask: boolean;
  cancellingTaskId: string | null;
}

export function TasksPanel({
  locale,
  run,
  resolvedMainTodos,
  resolvedMainStatus,
  onRefreshTasks,
  showingChildSession,
  onBackToMain,
  backgroundExecTasks,
  tasksLoading,
  tasksError,
  useTaskList,
  resolvedSubagents,
  providers,
  selectedSubagentId,
  selectedSubagent,
  selectedSubTodos,
  onSwitchSubagentKey,
  onSelectSubagent,
  onCancelTask,
  allowCancelTask,
  cancellingTaskId,
}: TasksPanelProps) {
  return (
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
                onClick={onRefreshTasks}
                title={t(locale, 'common.refresh')}
              >
                <RefreshCw size={10} />
                {t(locale, 'common.refresh')}
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
            {todoStatusLabel(locale, resolvedMainStatus)}
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
          locale={locale}
          emptyKey="assistant.activity.noTodos"
        />
      </section>

      {/* ── 后台执行 ── */}
      <section data-testid="background-tasks">
        <SectionTitle title={t(locale, 'assistant.activity.background')} />
        {useTaskList ? (
          tasksLoading && backgroundExecTasks.length === 0 ? (
            <Empty
              locale={locale}
              messageKey="assistant.activity.loadingTasks"
              compact
            />
          ) : tasksError ? (
            <div className="rounded border border-[var(--danger)]/30 bg-[var(--danger-soft)] p-2 text-[var(--danger)]">
              {tasksError}
            </div>
          ) : backgroundExecTasks.length === 0 ? (
            <Empty
              locale={locale}
              messageKey="assistant.activity.noBackground"
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
                      title={t(locale, 'activityInspector.cancelTask')}
                      disabled={cancellingTaskId === task.id}
                      className="shrink-0 rounded p-1 text-[var(--danger)] hover:bg-[var(--surface-hover)] disabled:opacity-40"
                      onClick={() => void onCancelTask(task.id)}
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
            locale={locale}
            messageKey="assistant.activity.backgroundNotReady"
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
                    onClick={() => onSelectSubagent(agent.id)}
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
            locale={locale}
            emptyKey="assistant.activity.noSubTasks"
          />
        </section>
      ) : null}
    </div>
  );
}

export interface ChangesPanelProps {
  locale: string;
  projectPath: string | null;
  auditLoading: boolean;
  auditError: string | null;
  auditStatus: { branch: string; entries: Array<{ path: string; status: string }> } | null;
  auditCounts: { additions: number; deletions: number };
  onRefreshAudit: () => Promise<void>;
  commitMessage: string;
  onCommitMessageChange: (value: string) => void;
  committing: boolean;
  pushing: boolean;
  onGitAction: (action: 'commit' | 'push') => Promise<void>;
  auditQuery: string;
  onAuditQueryChange: (value: string) => void;
  filteredAuditEntries: Array<{ path: string; status: string }>;
  selectedAuditPath: string | null;
  onSelectAuditPath: (path: string) => void;
  auditContent: string | null;
  onOpenFile?: (path: string) => void;
}

export function ChangesPanel({
  locale,
  projectPath,
  auditLoading,
  auditError,
  auditStatus,
  auditCounts,
  onRefreshAudit,
  commitMessage,
  onCommitMessageChange,
  committing,
  pushing,
  onGitAction,
  auditQuery,
  onAuditQueryChange,
  filteredAuditEntries,
  selectedAuditPath,
  onSelectAuditPath,
  auditContent,
  onOpenFile,
}: ChangesPanelProps) {
  return (
    <div className="flex flex-col h-full min-h-0 space-y-2">
      {!projectPath ? (
        <Empty locale={locale} messageKey="activityInspector.noProjectSelected" />
      ) : auditLoading ? (
        <Empty locale={locale} messageKey="activityInspector.loadingGitStatus" compact />
      ) : auditError ? (
        <div className="rounded border border-[var(--danger)]/30 p-2 text-[var(--danger)]">{auditError}</div>
      ) : (
        <>
          <div className="shrink-0 rounded border border-[var(--border)] p-2">
            <div className="flex items-center gap-2">
              <GitBranch size={13} />
              <span className="min-w-0 flex-1 truncate font-mono">{auditStatus?.branch ?? 'unknown'}</span>
              <span className="text-[var(--diff-add)]">+{auditCounts.additions}</span>
              <span className="text-[var(--diff-del)]">−{auditCounts.deletions}</span>
              <button
                type="button"
                onClick={() => void onRefreshAudit()}
                className="rounded p-1 hover:bg-[var(--surface-hover)]"
                title={t(locale, 'common.refresh')}
              >
                <RefreshCw size={12} />
              </button>
            </div>
            <div className="mt-2 flex gap-1">
              <input
                value={commitMessage}
                onChange={(event) => onCommitMessageChange(event.target.value)}
                placeholder={t(locale, 'activityInspector.commitMessage')}
                className="min-w-0 flex-1 rounded border border-[var(--border)] bg-transparent px-2 py-1"
              />
              <button
                type="button"
                disabled={committing || !commitMessage.trim()}
                onClick={() => void onGitAction('commit')}
                className="rounded bg-[var(--primary)] px-2 py-1 text-[var(--accent-ink)] disabled:opacity-40"
              >
                {committing ? t(locale, 'activityInspector.committing') : t(locale, 'activityInspector.commit')}
              </button>
              <button
                type="button"
                disabled={pushing}
                onClick={() => void onGitAction('push')}
                className="rounded border border-[var(--border)] px-2 py-1 disabled:opacity-40"
                title={t(locale, 'activityInspector.push')}
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
                          ? 'bg-[var(--diff-add)]/10 text-[var(--diff-add)]'
                          : isDel
                            ? 'bg-[var(--diff-del)]/10 text-[var(--diff-del)]'
                            : isHunk
                              ? 'text-[var(--diff-mod)] font-semibold'
                              : 'text-[var(--text-secondary)]'
                      }
                    >
                      {line}
                    </div>
                  );
                })
              ) : (
                <span className="text-[var(--text-disabled)]">
                  {t(locale, 'activityInspector.selectFileToPreview')}
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
                  onChange={(event) => onAuditQueryChange(event.target.value)}
                  placeholder={t(locale, 'activityInspector.filterFiles')}
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
                        onSelectAuditPath(entry.path);
                        onOpenFile?.(`${projectPath.replace(/\/$/, '')}/${entry.path}`);
                      }}
                      className={`flex w-full items-center gap-2 px-2 py-1 text-left font-mono hover:bg-[var(--surface-hover)] ${
                        selected ? 'bg-[var(--surface-hover)]' : ''
                      }`}
                    >
                      <span
                        className={
                          entry.status === 'deleted'
                            ? 'text-[var(--diff-del)]'
                            : entry.status === 'added' || entry.status === 'untracked'
                              ? 'text-[var(--diff-add)]'
                              : entry.status === 'modified'
                                ? 'text-[var(--diff-mod)]'
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
                  <Empty locale={locale} messageKey="activityInspector.noUncommittedChanges" compact />
                )}
              </div>
            </div>
          </div>
        </>
      )}
    </div>
  );
}

export interface ArtifactsPanelProps {
  locale: string;
  artifactBuckets: { used: ArtifactFileItem[]; modified: ArtifactFileItem[]; created: ArtifactFileItem[] };
  artifactPreviewPath: string | null;
  onClosePreview: () => void;
  onSelectFile: (path: string) => void;
}

export function ArtifactsPanel({
  locale,
  artifactBuckets,
  artifactPreviewPath,
  onClosePreview,
  onSelectFile,
}: ArtifactsPanelProps) {
  return (
    <div className="flex h-full flex-col gap-2" data-testid="artifacts-panel">
      {artifactPreviewPath && (
        <div className="flex h-1/2 min-h-0 flex-col rounded-lg border border-[var(--border)] overflow-hidden">
          <div className="flex items-center justify-between border-b border-[var(--border)] px-2 py-1">
            <span className="truncate font-mono text-[10px] text-[var(--text-secondary)]">{artifactPreviewPath}</span>
            <button
              type="button"
              onClick={onClosePreview}
              className="text-[10px] text-[var(--text-disabled)] hover:text-[var(--text)]"
            >
              {t(locale, 'common.close')}
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
        { key: 'used' as const, titleKey: 'activityInspector.bucketUsed', items: artifactBuckets.used },
        { key: 'modified' as const, titleKey: 'activityInspector.bucketModified', items: artifactBuckets.modified },
        { key: 'created' as const, titleKey: 'activityInspector.bucketCreated', items: artifactBuckets.created },
      ].map((section) => (
        <div key={section.key} className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-lg border border-[var(--border)] p-2">
          <div className="mb-1 flex items-center justify-between font-medium text-[var(--text-secondary)]">
            <span>{t(locale, section.titleKey)}</span>
            <span className="text-[10px] text-[var(--text-disabled)]">{section.items.length}</span>
          </div>
          <div className="flex-1 overflow-y-auto space-y-0.5">
            {section.items.length === 0 ? (
              <div className="py-2 text-center text-[10px] text-[var(--text-disabled)]">
                {t(locale, 'activityInspector.noFiles')}
              </div>
            ) : (
              section.items.map((item) => (
                <button
                  key={item.path}
                  type="button"
                  onClick={() => onSelectFile(item.path)}
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
  );
}

export function ContextPanel({
  locale,
  allowContextUsage,
  contextUsage,
}: {
  locale: string;
  allowContextUsage: boolean;
  contextUsage: ContextUsage | null;
}) {
  if (!allowContextUsage) {
    return (
      <div
        className="py-8 text-center text-[var(--text-disabled)]"
        data-testid="context-capability-not-ready"
      >
        {t(locale, 'assistant.contextUsageNotReady')}
      </div>
    );
  }
  if (!contextUsage) {
    return <Empty locale={locale} messageKey="activityInspector.noContextUsage" />;
  }
  return (
    <div className="space-y-2">
      <div className="h-2 overflow-hidden rounded-full bg-[var(--surface-hover)]">
        <div
          className="h-full bg-[var(--primary)]"
          style={{
            width: `${Math.min(100, (contextUsage.usedTokens / Math.max(1, contextUsage.maxTokens)) * 100)}%`,
          }}
        />
      </div>
      <Row
        label={t(locale, 'activityInspector.usage')}
        value={`${contextUsage.usedTokens} / ${contextUsage.maxTokens}`}
      />
    </div>
  );
}

export function EventsPanel({
  locale,
  eventGroups,
}: {
  locale: string;
  eventGroups: DeltaEventGroup[];
}) {
  if (eventGroups.length === 0) {
    return <Empty locale={locale} messageKey="activityInspector.noEvents" />;
  }
  return (
    <div className="space-y-0.5 font-mono">
      {eventGroups.map((event) => (
        <div key={`${event.type}-${event.first}`} className="flex gap-2 px-1 py-0.5 hover:bg-[var(--surface-hover)]">
          <span className="shrink-0 text-[var(--text-disabled)]">#{event.first}{event.last !== event.first ? `–${event.last}` : ''}</span>
          <span className="text-[var(--text-secondary)]">{event.type}{event.count > 1 ? ` ×${event.count}` : ''}</span>
        </div>
      ))}
    </div>
  );
}
