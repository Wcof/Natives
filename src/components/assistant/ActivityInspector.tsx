'use client';

import { t } from '@/i18n';
import {
  useActivityInspector,
  type ActivityInspectorProps,
} from './activity-inspector/useActivityInspector';
import {
  ArtifactsPanel,
  ChangesPanel,
  ContextPanel,
  EventsPanel,
  RunPanel,
  TasksPanel,
} from './activity-inspector/panels';

export type { ActivitySubagentView } from './activity-inspector/model';

export default function ActivityInspector(props: ActivityInspectorProps) {
  const inspector = useActivityInspector(props);

  return (
    <div className="flex h-full flex-col border-l border-[var(--border)] bg-[var(--surface)]">
      <div className="flex flex-wrap border-b border-[var(--border)]">
        {inspector.tabs.map((tab) => (
          <button
            key={tab.id}
            type="button"
            onClick={() => inspector.onTabChange(tab.id)}
            aria-disabled={tab.disabled}
            title={
              tab.disabled
                ? t(inspector.locale, 'activityInspector.engineCapabilityMissing')
                : undefined
            }
            className={`inline-flex items-center gap-1 px-2.5 py-2 text-[11px] font-medium transition-colors ${
              tab.disabled
                ? 'cursor-not-allowed text-[var(--text-disabled)] opacity-50'
                : inspector.effectiveTab === tab.id
                  ? 'border-b-2 border-[var(--primary)] text-[var(--primary)]'
                  : 'text-[var(--text-disabled)] hover:text-[var(--text-secondary)]'
            }`}
            data-testid={`inspector-tab-${tab.id}${tab.disabled ? '-disabled' : ''}`}
          >
            <tab.icon size={12} />
            {t(inspector.locale, tab.labelKey)}
          </button>
        ))}
      </div>

      <div className={`flex-1 ${inspector.effectiveTab === 'changes' || inspector.effectiveTab === 'artifacts' ? 'flex flex-col min-h-0 overflow-hidden' : 'overflow-y-auto'} p-3 text-xs`}>
        {inspector.tasksCapabilityMissing && (
          <div
            className="py-8 text-center text-[var(--text-disabled)]"
            data-testid="tasks-capability-not-ready"
          >
            <div>{t(inspector.locale, 'activityInspector.tasksUnavailable')}</div>
            <div className="mt-1 text-[10px]">
              {t(inspector.locale, 'activityInspector.engineTasksMissing')}
            </div>
          </div>
        )}

        {!inspector.run && !inspector.tasksCapabilityMissing && (
          <div className="grid h-full place-items-center text-[var(--text-disabled)]">
            {t(inspector.locale, 'activityInspector.selectRunToInspect')}
          </div>
        )}

        {inspector.run && inspector.effectiveTab === 'run' && (
          <RunPanel
            run={inspector.run}
            locale={inspector.locale}
            providerLabel={inspector.providerLabel}
            onRetry={inspector.onRetry}
          />
        )}

        {inspector.run && inspector.effectiveTab === 'tasks' && !inspector.tasksCapabilityMissing && (
          <TasksPanel
            locale={inspector.locale}
            run={inspector.run}
            resolvedMainTodos={inspector.resolvedMainTodos}
            resolvedMainStatus={inspector.resolvedMainStatus}
            onRefreshTasks={inspector.onRefreshTasks}
            showingChildSession={inspector.showingChildSession}
            onBackToMain={inspector.onBackToMain}
            backgroundExecTasks={inspector.backgroundExecTasks}
            tasksLoading={inspector.tasksLoading}
            tasksError={inspector.tasksError}
            useTaskList={inspector.useTaskList}
            resolvedSubagents={inspector.resolvedSubagents}
            providers={inspector.providers}
            selectedSubagentId={inspector.selectedSubagentId}
            selectedSubagent={inspector.selectedSubagent}
            selectedSubTodos={inspector.selectedSubTodos}
            onSwitchSubagentKey={inspector.onSwitchSubagentKey}
            onSelectSubagent={inspector.onSelectSubagent}
            onCancelTask={inspector.onCancelTask}
            allowCancelTask={inspector.allowCancelTask}
            cancellingTaskId={inspector.cancellingTaskId}
          />
        )}

        {inspector.effectiveTab === 'changes' && (
          <ChangesPanel
            locale={inspector.locale}
            projectPath={inspector.projectPath}
            auditLoading={inspector.auditLoading}
            auditError={inspector.auditError}
            auditStatus={inspector.auditStatus}
            auditCounts={inspector.auditCounts}
            onRefreshAudit={inspector.onRefreshAudit}
            commitMessage={inspector.commitMessage}
            onCommitMessageChange={inspector.setCommitMessage}
            committing={inspector.committing}
            pushing={inspector.pushing}
            onGitAction={inspector.onGitAction}
            auditQuery={inspector.auditQuery}
            onAuditQueryChange={inspector.setAuditQuery}
            filteredAuditEntries={inspector.filteredAuditEntries}
            selectedAuditPath={inspector.selectedAuditPath}
            onSelectAuditPath={inspector.setSelectedAuditPath}
            auditContent={inspector.auditContent}
            onOpenFile={inspector.onOpenFile}
          />
        )}

        {inspector.run && inspector.effectiveTab === 'artifacts' && (
          <ArtifactsPanel
            locale={inspector.locale}
            artifactBuckets={inspector.artifactBuckets}
            artifactPreviewPath={inspector.artifactPreviewPath}
            onClosePreview={inspector.onClosePreview}
            onSelectFile={inspector.onSelectArtifactFile}
          />
        )}

        {inspector.run && inspector.effectiveTab === 'context' && (
          <ContextPanel
            locale={inspector.locale}
            allowContextUsage={inspector.allowContextUsage}
            contextUsage={inspector.contextUsage}
          />
        )}

        {inspector.run && inspector.effectiveTab === 'events' && (
          <EventsPanel
            locale={inspector.locale}
            eventGroups={inspector.eventGroups}
          />
        )}
      </div>
    </div>
  );
}
