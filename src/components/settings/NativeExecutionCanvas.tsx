'use client';

import type { ReactNode } from 'react';
import {
  AlertTriangle,
  CheckCircle2,
  CircleDot,
  History,
  Loader,
  RefreshCw,
  Workflow,
  Zap,
} from 'lucide-react';
import { useMemo, useState } from 'react';
import { t, type Locale } from '@/i18n';
import { NativeExecutionGraph } from './NativeExecutionGraph';
import { NativeExecutionInspector } from './NativeExecutionInspector';
import {
  enabledHookCount,
  sortStages,
  type CanvasEdge,
  type CanvasMode,
  type CanvasNodeDetail,
  type CanvasRun,
  type CanvasRunSnapshot,
  type CanvasStage,
  type CanvasTraceEntry,
  type CanvasWorkspaceTarget,
  type PromptBlock,
} from './nativeExecutionCanvasModel';

export type {
  CanvasEdge,
  CanvasMode,
  CanvasNodeDetail,
  CanvasRun,
  CanvasRunSnapshot,
  CanvasStage,
  CanvasTraceEntry,
  CanvasWorkspaceTarget,
  PromptBlock,
} from './nativeExecutionCanvasModel';
export { stageLabel } from './nativeExecutionCanvasModel';
export { traceEntriesForStage } from './nativeExecutionCanvasModel';

export function NativeExecutionCanvas({
  locale,
  mode,
  stages,
  edges,
  promptBlocks,
  issueCount = 0,
  runs = [],
  selectedRunId = '',
  traceEntries = [],
  runSnapshot,
  auditLoading = false,
  readOnly = false,
  nodeDetails = {},
  onRequestAudit,
  onModeChange,
  onSelectRun,
  onRefreshAudit,
  onOpenWorkspace,
  onSetHookEnabled,
  onAuthorizeHook,
  onRemoveHook,
  onRemovePrompt,
  runPanel,
  workspacePanel,
}: {
  locale: Locale;
  mode: CanvasMode;
  stages: CanvasStage[];
  edges: CanvasEdge[];
  promptBlocks: PromptBlock[];
  issueCount?: number;
  runs?: CanvasRun[];
  selectedRunId?: string;
  traceEntries?: CanvasTraceEntry[];
  runSnapshot?: CanvasRunSnapshot | null;
  auditLoading?: boolean;
  readOnly?: boolean;
  nodeDetails?: Record<string, CanvasNodeDetail>;
  onRequestAudit?: () => void;
  onModeChange?: (mode: CanvasMode) => void;
  onSelectRun?: (runId: string) => void;
  onRefreshAudit?: () => void;
  onOpenWorkspace: (target: CanvasWorkspaceTarget, stageId: string) => void;
  onSetHookEnabled?: (hookId: string, enabled: boolean) => void;
  onAuthorizeHook?: (hookId: string, authorized: boolean) => void;
  onRemoveHook?: (hookId: string) => void;
  onRemovePrompt?: (promptId: string) => void;
  runPanel?: ReactNode;
  workspacePanel?: ReactNode;
}) {
  const ordered = useMemo(() => sortStages(stages), [stages]);
  const [selectedStageId, setSelectedStageId] = useState<string | null>(null);
  const selectedStage = ordered.find((stage) => stage.id === selectedStageId) ?? null;
  const selectedRun = runs.find((run) => run.id === selectedRunId);
  const enabledHooks = ordered.reduce((total, stage) => total + enabledHookCount(stage), 0);
  const undispatchedPoints = ordered.reduce(
    (total, stage) => total + (stage.hook_points ?? []).filter((point) => point.dispatched === false).length,
    0,
  );
  const failedTraces = traceEntries.filter(
    (entry) => entry.type === 'hook_invocation_completed' && (entry.status === 'failed' || entry.error_category),
  ).length;

  const setCanvasMode = (next: CanvasMode) => {
    onModeChange?.(next);
    if (next === 'audit') onRequestAudit?.();
  };

  const summary = (() => {
    if (mode === 'audit') {
      if (auditLoading) {
        return {
          icon: <Loader size={20} className="animate-spin text-[var(--text-secondary)]" />,
          title: t(locale, 'settings.engineCanvasAuditLoading'),
          detail: t(locale, 'settings.engineCanvasAuditLoadingDesc'),
        };
      }
      if (!selectedRunId) {
        return {
          icon: <History size={20} className="text-[var(--text-secondary)]" />,
          title: t(locale, 'settings.engineCanvasAuditChooseTitle'),
          detail: t(locale, 'settings.engineCanvasAuditChooseDesc'),
        };
      }
      if (runSnapshot?.resolved === false) {
        return {
          icon: <AlertTriangle size={20} className="text-[var(--warning)]" />,
          title: t(locale, 'settings.engineCanvasAuditInsufficientTitle'),
          detail: t(locale, 'settings.engineCanvasAuditInsufficientDesc'),
        };
      }
      return {
        icon: failedTraces > 0
          ? <AlertTriangle size={20} className="text-[var(--danger)]" />
          : <CheckCircle2 size={20} className="text-[var(--success)]" />,
        title: failedTraces > 0
          ? t(locale, 'settings.engineCanvasAuditFailures', { count: failedTraces })
          : t(locale, 'settings.engineCanvasAuditEvidenceLoaded'),
        detail: t(locale, 'settings.engineCanvasAuditEvidenceDetail', { count: traceEntries.length }),
      };
    }

    if (ordered.length === 0) {
      return {
        icon: <AlertTriangle size={20} className="text-[var(--warning)]" />,
        title: t(locale, 'settings.engineCanvasNoStagesTitle'),
        detail: t(locale, 'settings.engineCanvasNoStagesDesc'),
      };
    }

    const attentionCount = issueCount + undispatchedPoints;
    return {
      icon: attentionCount > 0
        ? <AlertTriangle size={20} className="text-[var(--warning)]" />
        : <Workflow size={20} className="text-[var(--text)]" />,
      title: t(locale, 'settings.engineCanvasStagesLoaded', { count: ordered.length }),
      detail: attentionCount > 0
        ? t(locale, 'settings.engineCanvasAttentionDetail', { count: attentionCount })
        : t(locale, 'settings.engineCanvasStructureDetail'),
    };
  })();

  return (
    <section className="overflow-hidden rounded-lg border border-[var(--border-subtle)] bg-[var(--surface)]" aria-label={t(locale, 'settings.engineCanvasTitle')}>
      <header className="space-y-4 border-b border-[var(--border-subtle)] p-5">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div className="min-w-0">
            <div className="flex items-center gap-2">
              <Workflow size={18} className="text-[var(--text-secondary)]" />
              <h4 className="text-base font-semibold text-[var(--text)]">{t(locale, 'settings.engineCanvasTitle')}</h4>
            </div>
            <p className="mt-1 max-w-2xl text-sm leading-5 text-[var(--text-secondary)]">{t(locale, 'settings.engineCanvasDesc')}</p>
          </div>

          <div className="segmented-control" role="group" aria-label={t(locale, 'settings.engineCanvasModeLabel')}>
            <button type="button" className={`seg-item ${mode === 'understand' ? 'active' : ''}`} aria-pressed={mode === 'understand'} onClick={() => setCanvasMode('understand')}>
              {t(locale, 'settings.engineCanvasModeUnderstand')}
            </button>
            <button type="button" className={`seg-item ${mode === 'audit' ? 'active' : ''}`} aria-pressed={mode === 'audit'} onClick={() => setCanvasMode('audit')}>
              {t(locale, 'settings.engineCanvasModeAudit')}
            </button>
          </div>
        </div>

        <div className="flex flex-wrap items-center justify-between gap-3 border-y border-[var(--border-subtle)] py-3">
          <div className="flex min-w-0 items-center gap-3">
            <span className="shrink-0">{summary.icon}</span>
            <div className="min-w-0"><strong className="block text-sm text-[var(--text)]">{summary.title}</strong><span className="block text-xs leading-5 text-[var(--text-secondary)]">{summary.detail}</span></div>
          </div>

          {mode === 'audit' ? (
            <div className="flex min-w-[260px] flex-1 items-center justify-end gap-2 sm:flex-initial">
              <label className="sr-only" htmlFor="engine-canvas-run">{t(locale, 'settings.engineCanvasRunSelect')}</label>
              <select
                id="engine-canvas-run"
                className="input max-w-sm"
                value={selectedRunId}
                disabled={auditLoading}
                onChange={(event) => onSelectRun?.(event.target.value)}
              >
                <option value="">{t(locale, 'settings.engineCanvasRunPlaceholder')}</option>
                {runs.map((run) => <option key={run.id} value={run.id}>{run.status} · {run.id.slice(0, 10)}</option>)}
              </select>
              <button type="button" className="btn h-10 w-10 shrink-0 p-0" aria-label={t(locale, 'common.refresh')} disabled={auditLoading} onClick={onRefreshAudit}>
                <RefreshCw size={14} className={auditLoading ? 'animate-spin' : ''} />
              </button>
            </div>
          ) : (
            <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-[var(--text-secondary)]">
              <span className="inline-flex items-center gap-1.5"><CircleDot size={12} />{t(locale, 'settings.engineCanvasNodeCount', { count: ordered.length })}</span>
              <span className="inline-flex items-center gap-1.5"><Zap size={12} />{t(locale, 'settings.engineCanvasEnabledHooks', { count: enabledHooks })}</span>
            </div>
          )}
        </div>

        {runPanel}
      </header>

      {ordered.length > 0 ? (
        <NativeExecutionGraph
          locale={locale}
          stages={ordered}
          edges={edges}
          promptBlockCount={promptBlocks.length}
          mode={mode}
          selectedStageId={selectedStageId}
          selectedRunId={selectedRunId}
          traceEntries={traceEntries}
          runSnapshot={runSnapshot}
          onSelectStage={setSelectedStageId}
        />
      ) : (
        <div className="flex min-h-72 flex-col items-center justify-center gap-2 p-8 text-center">
          <Workflow size={22} className="text-[var(--text-disabled)]" />
          <strong>{t(locale, 'settings.engineCanvasNoStagesTitle')}</strong>
          <p className="m-0 text-sm text-[var(--text-secondary)]">{t(locale, 'settings.engineCanvasNoStagesDesc')}</p>
        </div>
      )}

      {workspacePanel ? <div className="border-t border-[var(--border-subtle)] p-5">{workspacePanel}</div> : null}

      <footer className="flex flex-wrap items-center gap-x-5 gap-y-2 border-t border-[var(--border-subtle)] px-5 py-3 text-[11px] text-[var(--text-secondary)]">
        <span className="inline-flex items-center gap-1.5"><span className="h-2 w-2 rounded-full bg-[var(--text-secondary)]" />{t(locale, 'settings.engineCanvasLegendNode')}</span>
        <span className="inline-flex items-center gap-1.5"><span className="h-2 w-2 rounded-sm bg-[var(--info)]" />{t(locale, 'settings.engineCanvasLegendConfigured')}</span>
        <span>{t(locale, 'settings.engineCanvasLegendHint')}</span>
      </footer>

      {selectedStage ? (
        <NativeExecutionInspector
          locale={locale}
          stage={selectedStage}
          promptBlocks={promptBlocks}
          mode={mode}
          selectedRun={selectedRun}
          traceEntries={traceEntries}
          runSnapshot={runSnapshot}
          detail={nodeDetails[selectedStage.id]}
          readOnly={readOnly}
          onClose={() => setSelectedStageId(null)}
          onOpenWorkspace={onOpenWorkspace}
          onSetHookEnabled={onSetHookEnabled}
          onAuthorizeHook={onAuthorizeHook}
          onRemoveHook={onRemoveHook}
          onRemovePrompt={onRemovePrompt}
        />
      ) : null}
    </section>
  );
}
