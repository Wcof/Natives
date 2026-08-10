'use client';

import { t } from '@/i18n';
import {
  ArtifactsPanel,
  ChangesPanel,
  ContextPanel,
  EventsPanel,
  RunPanel,
  TasksPanel,
} from './activity-inspector/panels';
import {
  useActivityInspector,
  type ActivityInspectorProps,
} from './activity-inspector/useActivityInspector';
// Callers (AssistantWorkbench / WorkbenchPanels) import this type from here —
// re-export from the activity-inspector model (single owner).
export type { ActivitySubagentView } from './activity-inspector/model';

export default function ActivityInspector(props: ActivityInspectorProps) {
  const {
    tabs,
    effectiveTab,
    tasksCapabilityMissing,
    locale,
    onTabChange,
    run,
    providerLabel,
    onRetry,
    resolvedMainTodos,
    resolvedMainStatus,
    backgroundExecTasks,
    tasksLoading,
    tasksError,
    useTaskList,
    resolvedSubagents,
    providers,
    selectedSubagentId,
    selectedSubagent,
    selectedSubTodos,
    showingChildSession,
    onBackToMain,
    onSwitchSubagentKey,
    onSelectSubagent,
    onRefreshTasks,
    onCancelTask,
    allowCancelTask,
    cancellingTaskId,
    projectPath,
    auditLoading,
    auditError,
    auditStatus,
    auditCounts,
    onRefreshAudit,
    commitMessage,
    setCommitMessage,
    committing,
    pushing,
    onGitAction,
    auditQuery,
    setAuditQuery,
    filteredAuditEntries,
    selectedAuditPath,
    setSelectedAuditPath,
    auditContent,
    onOpenFile,
    artifactBuckets,
    artifactPreviewPath,
    onClosePreview,
    onSelectArtifactFile,
    allowContextUsage,
    contextUsage,
    eventGroups,
  } = useActivityInspector(props);

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
                ? t(locale, 'activityInspector.engineCapabilityMissing')
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
            {t(locale, tab.labelKey)}
          </button>
        ))}
      </div>

      <div className={`flex-1 ${effectiveTab === 'changes' || effectiveTab === 'artifacts' ? 'flex flex-col min-h-0 overflow-hidden' : 'overflow-y-auto'} p-3 text-xs`}>
        {tasksCapabilityMissing && (
          <div
            className="py-8 text-center text-[var(--text-disabled)]"
            data-testid="tasks-capability-not-ready"
          >
            <div>{t(locale, 'activityInspector.tasksUnavailable')}</div>
            <div className="mt-1 text-[10px]">
              {t(locale, 'activityInspector.engineTasksMissing')}
            </div>
          </div>
        )}

        {!run && !tasksCapabilityMissing && (
          <div className="grid h-full place-items-center text-[var(--text-disabled)]">
            {t(locale, 'activityInspector.selectRunToInspect')}
          </div>
        )}

        {run && effectiveTab === 'run' && (
          <RunPanel run={run} locale={locale} providerLabel={providerLabel} onRetry={onRetry} />
        )}

        {run && effectiveTab === 'tasks' && !tasksCapabilityMissing && (
          <TasksPanel
            locale={locale}
            run={run}
            resolvedMainTodos={resolvedMainTodos}
            resolvedMainStatus={resolvedMainStatus}
            onRefreshTasks={onRefreshTasks}
            showingChildSession={showingChildSession}
            onBackToMain={onBackToMain}
            backgroundExecTasks={backgroundExecTasks}
            tasksLoading={tasksLoading}
            tasksError={tasksError}
            useTaskList={useTaskList}
            resolvedSubagents={resolvedSubagents}
            providers={providers}
            selectedSubagentId={selectedSubagentId}
            selectedSubagent={selectedSubagent}
            selectedSubTodos={selectedSubTodos}
            onSwitchSubagentKey={onSwitchSubagentKey}
            onSelectSubagent={onSelectSubagent}
            onCancelTask={onCancelTask}
            allowCancelTask={allowCancelTask}
            cancellingTaskId={cancellingTaskId}
          />
        )}

        {effectiveTab === 'changes' && (
          <ChangesPanel
            locale={locale}
            projectPath={projectPath}
            auditLoading={auditLoading}
            auditError={auditError}
            auditStatus={auditStatus}
            auditCounts={auditCounts}
            onRefreshAudit={onRefreshAudit}
            commitMessage={commitMessage}
            onCommitMessageChange={setCommitMessage}
            committing={committing}
            pushing={pushing}
            onGitAction={onGitAction}
            auditQuery={auditQuery}
            onAuditQueryChange={setAuditQuery}
            filteredAuditEntries={filteredAuditEntries}
            selectedAuditPath={selectedAuditPath}
            onSelectAuditPath={setSelectedAuditPath}
            auditContent={auditContent}
            onOpenFile={onOpenFile}
          />
        )}

        {run && effectiveTab === 'artifacts' && (
          <ArtifactsPanel
            locale={locale}
            artifactBuckets={artifactBuckets}
            artifactPreviewPath={artifactPreviewPath}
            onClosePreview={onClosePreview}
            onSelectFile={onSelectArtifactFile}
          />
        )}

        {run && effectiveTab === 'context' && (
          <ContextPanel
            locale={locale}
            allowContextUsage={allowContextUsage}
            contextUsage={contextUsage}
          />
        )}

        {run && effectiveTab === 'events' && (
          <EventsPanel locale={locale} eventGroups={eventGroups} />
        )}
      </div>
    </div>
  );
}
