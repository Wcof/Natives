'use client';

import { useAssistantDispatch, useAssistantGateway, useAssistantStore } from '@/lib/assistant-workspace';
import type { InspectorTab } from '@/lib/assistant-workspace';
import type {
  Artifact,
  ChildRunSummary,
  ContextUsage,
  FileChange,
  Run,
  RunEvent,
} from '@/lib/assistant-protocol';
import type { ActivityTodo, FileEventInput } from '@/lib/assistant-activity-view';
import type { Locale } from '@/i18n';
import { t } from '@/i18n';
import ResizableRightPanel from '@/components/ui/ResizableRightPanel';
import ActivityInspector, { type ActivitySubagentView } from '../ActivityInspector';
import type { ProviderWithModels } from '@/components/assistant/conversation/ModelSelectorDropdown';

export interface WorkbenchPanelsProps {
  locale: Locale;
  showRight: boolean;
  rootRun: Run | null;
  activeRun: Run | null;
  rootEvents: RunEvent[];
  selectedChildEvents: RunEvent[];
  mainTodos: ActivityTodo[];
  artifacts: Artifact[];
  /** Child runs of the root run tree. */
  children: ChildRunSummary[];
  fileChanges: FileChange[];
  contextUsage: ContextUsage | null;
  providers: ProviderWithModels[];
  activeProjectPath: string | null;
  fileEvents: FileEventInput[];
  rootConversationId: string | null;
  activitySubagents: ActivitySubagentView[];
  selectedChildConversationId: string | null;
  onRetry: () => void;
  onOpenArtifact: (artifact: Artifact) => void;
  onRevealArtifact: (artifact: Artifact) => void;
  onOpenFile: (path: string) => void;
  onSelectSubagent: (id: string) => void;
  onBackToMain: () => void;
  onSwitchSubagentKey: (id: string) => void;
  onRefreshTasks: () => void;
  onClose: () => void;
}

/**
 * Right activity panel: the resizable inspector with run/todo/artifact/tasks
 * tabs and the subagent session list.
 *
 * Reads view width/tab and capabilities from the store; everything else arrives
 * as props from the shell.
 */
export function WorkbenchPanels({
  locale,
  showRight,
  rootRun,
  activeRun,
  rootEvents,
  selectedChildEvents,
  mainTodos,
  artifacts,
  children,
  fileChanges,
  contextUsage,
  providers,
  activeProjectPath,
  fileEvents,
  rootConversationId,
  activitySubagents,
  selectedChildConversationId,
  onRetry,
  onOpenArtifact,
  onRevealArtifact,
  onOpenFile,
  onSelectSubagent,
  onBackToMain,
  onSwitchSubagentKey,
  onRefreshTasks,
  onClose,
}: WorkbenchPanelsProps) {
  const state = useAssistantStore();
  const dispatch = useAssistantDispatch();
  const gateway = useAssistantGateway();

  return (
    <>
      {showRight && (
        <ResizableRightPanel
          open={showRight}
          width={state.view.rightWidth || 320}
          onResize={(width) => dispatch({ type: 'view/patch', patch: { rightWidth: width } })}
          onClose={onClose}
          title={t(locale, 'assistant.activityPanel')}
          ariaLabel={t(locale, 'assistant.inspectorTitle')}
          resizeLabel={t(locale, 'assistant.resizeLabel')}
          resizeHint={t(locale, 'assistant.resizeHint')}
          closeLabel={t(locale, 'assistant.closePanel')}
          scrollBody={false}
        >
          <ActivityInspector
            run={rootRun ?? activeRun}
            runError={state.runErrors[(rootRun ?? activeRun)?.id ?? ''] ?? null}
            events={rootEvents}
            selectedChildEvents={selectedChildEvents}
            mainTodos={mainTodos}
            artifacts={artifacts}
            children={children}
            fileChanges={fileChanges}
            contextUsage={contextUsage}
            locale={locale}
            providers={providers}
            projectPath={activeProjectPath}
            activeTab={state.view.inspectorTab}
            onTabChange={(tab: InspectorTab) =>
              dispatch({ type: 'view/patch', patch: { inspectorTab: tab } })
            }
            onRetry={onRetry}
            onOpenArtifact={onOpenArtifact}
            onRevealArtifact={onRevealArtifact}
            onOpenFile={onOpenFile}
            capabilities={state.capabilities}
            gateway={gateway}
            conversationId={rootConversationId}
            subagents={activitySubagents}
            selectedSubagentId={
              selectedChildConversationId
                ? activitySubagents.find(
                    (s) => s.childConversationId === selectedChildConversationId,
                  )?.id ?? null
                : null
            }
            fileEvents={fileEvents}
            onSelectSubagent={onSelectSubagent}
            onBackToMain={onBackToMain}
            onSwitchSubagentKey={onSwitchSubagentKey}
            onRefreshTasks={onRefreshTasks}
            showingChildSession={Boolean(selectedChildConversationId)}
          />
        </ResizableRightPanel>
      )}
    </>
  );
}

export default WorkbenchPanels;
