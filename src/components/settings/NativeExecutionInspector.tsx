'use client';

import {
  Braces,
  ChevronRight,
  CircleHelp,
  FileClock,
  Layers3,
  ListChecks,
  Settings2,
  ShieldCheck,
  Wrench,
  X,
} from 'lucide-react';
import { useEffect, useMemo, useRef, useState } from 'react';
import { t, type Locale } from '@/i18n';
import {
  enabledHookCount,
  stageEvidenceCode,
  stagePresentation,
  traceEntriesForStage,
  type CanvasMode,
  type CanvasRun,
  type CanvasRunSnapshot,
  type CanvasStage,
  type CanvasTraceEntry,
  type CanvasWorkspaceTarget,
  type PromptBlock,
  type StageEvidenceCode,
} from './nativeExecutionCanvasModel';

type InspectorTab = 'understand' | 'evidence' | 'configure';

const TAB_KEYS: Record<InspectorTab, string> = {
  understand: 'settings.engineCanvasTabUnderstand',
  evidence: 'settings.engineCanvasTabEvidence',
  configure: 'settings.engineCanvasTabConfigure',
};

const EVIDENCE_KEYS: Record<StageEvidenceCode, string> = {
  loaded: 'settings.engineCanvasEvidenceLoaded',
  configured: 'settings.engineCanvasEvidenceConfigured',
  attention: 'settings.engineCanvasEvidenceAttention',
  choose_run: 'settings.engineCanvasEvidenceChooseRun',
  insufficient: 'settings.engineCanvasEvidenceInsufficient',
  no_evidence: 'settings.engineCanvasEvidenceNone',
  running: 'settings.engineCanvasEvidenceRunning',
  hook_evidence: 'settings.engineCanvasEvidenceHook',
  snapshot_recorded: 'settings.engineCanvasEvidenceSnapshot',
  failed: 'settings.engineCanvasEvidenceFailed',
};

const TARGET_META: Record<CanvasWorkspaceTarget, { labelKey: string; descKey: string; icon: typeof Settings2 }> = {
  blueprint: {
    labelKey: 'settings.engineCanvasTargetBlueprint',
    descKey: 'settings.engineCanvasTargetBlueprintDesc',
    icon: Layers3,
  },
  hooks: {
    labelKey: 'settings.engineCanvasTargetHooks',
    descKey: 'settings.engineCanvasTargetHooksDesc',
    icon: Braces,
  },
  prompts: {
    labelKey: 'settings.engineCanvasTargetPrompts',
    descKey: 'settings.engineCanvasTargetPromptsDesc',
    icon: ListChecks,
  },
  capabilities: {
    labelKey: 'settings.engineCanvasTargetCapabilities',
    descKey: 'settings.engineCanvasTargetCapabilitiesDesc',
    icon: Wrench,
  },
  runs: {
    labelKey: 'settings.engineCanvasTargetRuns',
    descKey: 'settings.engineCanvasTargetRunsDesc',
    icon: FileClock,
  },
};

function EvidenceEmpty({ locale, children }: { locale: Locale; children?: React.ReactNode }) {
  return (
    <div className="flex min-h-40 flex-col items-center justify-center gap-2 border-y border-[var(--border-subtle)] py-8 text-center">
      <CircleHelp size={20} className="text-[var(--text-disabled)]" />
      <p className="m-0 max-w-xs text-sm text-[var(--text-secondary)]">
        {children ?? t(locale, 'settings.engineCanvasNoNodeEvidence')}
      </p>
    </div>
  );
}

export function NativeExecutionInspector({
  locale,
  stage,
  promptBlocks,
  mode,
  selectedRun,
  traceEntries,
  runSnapshot,
  onClose,
  onOpenWorkspace,
}: {
  locale: Locale;
  stage: CanvasStage;
  promptBlocks: PromptBlock[];
  mode: CanvasMode;
  selectedRun?: CanvasRun;
  traceEntries: CanvasTraceEntry[];
  runSnapshot?: CanvasRunSnapshot | null;
  onClose: () => void;
  onOpenWorkspace: (target: CanvasWorkspaceTarget, stageId: string) => void;
}) {
  const [tab, setTab] = useState<InspectorTab>('understand');
  const closeRef = useRef<HTMLButtonElement>(null);
  const presentation = stagePresentation(stage.id, locale);
  const stageTraces = useMemo(() => traceEntriesForStage(stage, traceEntries), [stage, traceEntries]);
  const evidence = stageEvidenceCode({
    mode,
    stage,
    promptBlockCount: promptBlocks.length,
    selectedRunId: selectedRun?.id,
    traceEntries,
    runSnapshot,
  });
  const hooks = enabledHookCount(stage);

  useEffect(() => {
    setTab('understand');
  }, [stage.id]);

  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    closeRef.current?.focus();
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKeyDown);
    return () => {
      window.removeEventListener('keydown', onKeyDown);
      previous?.focus();
    };
  }, [onClose]);

  return (
    <>
      <button
        type="button"
        className="fixed inset-0 z-40 cursor-default bg-[var(--background)] opacity-80"
        aria-label={t(locale, 'settings.engineCanvasCloseInspector')}
        onClick={onClose}
      />
      <aside
        role="dialog"
        aria-modal="true"
        aria-labelledby="engine-stage-inspector-title"
        className="fixed inset-x-0 bottom-0 z-50 flex max-h-[84dvh] flex-col rounded-t-lg border border-[var(--border)] bg-[var(--surface)] shadow-2xl lg:inset-y-0 lg:left-auto lg:right-0 lg:max-h-none lg:w-[440px] lg:rounded-none lg:border-y-0 lg:border-r-0"
      >
        <header className="flex items-start justify-between gap-4 border-b border-[var(--border-subtle)] p-5">
          <div className="min-w-0">
            <div className="flex items-center gap-2 text-xs text-[var(--text-secondary)]">
              <span>{presentation.groupLabel}</span>
              <span aria-hidden="true">·</span>
              <span>{stage.order != null ? `#${String(stage.order + 1).padStart(2, '0')}` : '—'}</span>
              <span aria-hidden="true">·</span>
              <span>{presentation.technicalName}</span>
            </div>
            <h3 id="engine-stage-inspector-title" className="mt-1 text-lg font-semibold text-[var(--text)]">
              {presentation.title}
            </h3>
            <span className="mt-2 inline-flex rounded bg-[var(--surface-hover)] px-2 py-1 text-xs text-[var(--text-secondary)]">
              {t(locale, EVIDENCE_KEYS[evidence])}
            </span>
          </div>
          <button
            ref={closeRef}
            type="button"
            className="btn btn-ghost h-8 w-8 shrink-0 p-0"
            aria-label={t(locale, 'settings.engineCanvasCloseInspector')}
            onClick={onClose}
          ><X size={17} /></button>
        </header>

        <div className="flex gap-1 border-b border-[var(--border-subtle)] px-5 py-2" role="tablist" aria-label={t(locale, 'settings.engineCanvasInspectorTabs')}>
          {(Object.keys(TAB_KEYS) as InspectorTab[]).map((item) => (
            <button
              key={item}
              type="button"
              role="tab"
              aria-selected={tab === item}
              className={`btn text-xs ${tab === item ? 'btn-primary' : 'btn-ghost'}`}
              onClick={() => setTab(item)}
            >
              {t(locale, TAB_KEYS[item])}
            </button>
          ))}
        </div>

        <div className="flex-1 overflow-y-auto p-5">
          {tab === 'understand' ? (
            <div className="space-y-6">
              <section>
                <h4 className="text-sm font-semibold text-[var(--text)]">{t(locale, 'settings.engineCanvasWhatItDoes')}</h4>
                <p className="mt-2 text-sm leading-6 text-[var(--text-secondary)]">{presentation.description}</p>
              </section>
              <section>
                <h4 className="text-sm font-semibold text-[var(--text)]">{t(locale, 'settings.engineCanvasWhyItMatters')}</h4>
                <p className="mt-2 text-sm leading-6 text-[var(--text-secondary)]">{presentation.why}</p>
              </section>
              <dl className="divide-y divide-[var(--border-subtle)] border-y border-[var(--border-subtle)] text-sm">
                <div className="flex items-center justify-between gap-4 py-3"><dt className="text-[var(--text-secondary)]">{t(locale, 'settings.engineCanvasHookPoints')}</dt><dd>{stage.hook_points?.length ?? 0}</dd></div>
                <div className="flex items-center justify-between gap-4 py-3"><dt className="text-[var(--text-secondary)]">{t(locale, 'settings.engineCanvasEnabledHookCount')}</dt><dd>{hooks}</dd></div>
                <div className="flex items-center justify-between gap-4 py-3"><dt className="text-[var(--text-secondary)]">{t(locale, 'settings.engineCanvasSafePoints')}</dt><dd>{stage.safe_points?.length ?? 0}</dd></div>
              </dl>
            </div>
          ) : null}

          {tab === 'evidence' ? (
            <div className="space-y-5">
              {mode === 'audit' ? (
                !selectedRun ? (
                  <EvidenceEmpty locale={locale}>{t(locale, 'settings.engineCanvasChooseRunHint')}</EvidenceEmpty>
                ) : runSnapshot?.resolved === false ? (
                  <EvidenceEmpty locale={locale}>{t(locale, 'settings.engineCanvasSnapshotMissing')}</EvidenceEmpty>
                ) : (
                  <>
                    <section className="border-b border-[var(--border-subtle)] pb-4">
                      <h4 className="text-sm font-semibold">{t(locale, 'settings.engineCanvasSelectedRun')}</h4>
                      <p className="mt-1 break-all font-mono text-xs text-[var(--text-secondary)]">{selectedRun.id}</p>
                      <p className="mt-1 text-xs text-[var(--text-secondary)]">{selectedRun.status}{selectedRun.provider_id ? ` · ${selectedRun.provider_id}` : ''}{selectedRun.model_id ? ` · ${selectedRun.model_id}` : ''}</p>
                    </section>

                    {stage.id === 'context' && runSnapshot?.snapshot?.prompt_plan ? (
                      <section className="space-y-2">
                        <h4 className="text-sm font-semibold">{t(locale, 'settings.engineCanvasPromptSnapshot')}</h4>
                        <p className="text-sm text-[var(--text-secondary)]">
                          {t(locale, 'settings.engineCanvasPromptSnapshotDetail', {
                            count: runSnapshot.snapshot.prompt_plan.source_digests?.length ?? 0,
                            tokens: runSnapshot.snapshot.prompt_plan.token_estimate ?? 0,
                          })}
                        </p>
                        <div className="space-y-1">
                          {(runSnapshot.snapshot.prompt_plan.source_digests ?? []).map((digest) => (
                            <code key={digest} className="block truncate text-xs text-[var(--text-secondary)]">{digest}</code>
                          ))}
                        </div>
                      </section>
                    ) : null}

                    {(stage.id === 'tool_gate' || stage.id === 'tool_execute') && runSnapshot?.snapshot?.tool_plan ? (
                      <section className="space-y-2">
                        <h4 className="text-sm font-semibold">{t(locale, 'settings.engineCanvasToolSnapshot')}</h4>
                        <p className="text-sm text-[var(--text-secondary)]">
                          {t(locale, 'settings.engineCanvasToolSnapshotDetail', { count: runSnapshot.snapshot.tool_plan.tools?.length ?? 0 })}
                        </p>
                        {(runSnapshot.snapshot.tool_plan.tools ?? []).slice(0, 12).map((tool) => (
                          <div key={`${tool.source}:${tool.name}`} className="flex items-center justify-between gap-2 border-b border-[var(--border-subtle)] py-2 text-xs">
                            <span className="truncate">{tool.name}</span><span className="truncate text-[var(--text-secondary)]">{tool.source}</span>
                          </div>
                        ))}
                      </section>
                    ) : null}

                    {stageTraces.length > 0 ? (
                      <section className="space-y-2">
                        <h4 className="text-sm font-semibold">{t(locale, 'settings.engineCanvasHookEvidence')}</h4>
                        {stageTraces.map((entry) => (
                          <div key={`${entry.run_id}:${entry.sequence}`} className="border-b border-[var(--border-subtle)] py-3 text-xs">
                            <div className="flex items-center justify-between gap-2">
                              <strong>{entry.hook_event ?? entry.type ?? 'Hook'}</strong>
                              <span className={entry.status === 'failed' ? 'text-[var(--danger)]' : 'text-[var(--text-secondary)]'}>{entry.status ?? entry.type}</span>
                            </div>
                            {entry.hook_id ? <div className="mt-1 truncate font-mono text-[var(--text-secondary)]">{entry.hook_id}</div> : null}
                            {entry.input_summary ? <p className="mt-2 break-words text-[var(--text-secondary)]">{entry.input_summary}{entry.input_truncated ? '…' : ''}</p> : null}
                            {entry.output_summary ? <p className="mt-2 break-words text-[var(--text-secondary)]">{entry.output_summary}{entry.output_truncated ? '…' : ''}</p> : null}
                            <div className="mt-2 flex items-center justify-between gap-2 text-[var(--text-disabled)]"><time>{entry.timestamp}</time>{entry.duration_ms != null ? <span>{entry.duration_ms} ms</span> : null}</div>
                          </div>
                        ))}
                      </section>
                    ) : (
                      stage.id !== 'context' && stage.id !== 'tool_gate' && stage.id !== 'tool_execute'
                        ? <EvidenceEmpty locale={locale} />
                        : null
                    )}
                  </>
                )
              ) : (
                <>
                  {stage.id === 'context' && promptBlocks.length > 0 ? (
                    <section className="space-y-2">
                      <h4 className="text-sm font-semibold">{t(locale, 'settings.engineCanvasCurrentPrompts')}</h4>
                      {promptBlocks.map((block) => <div key={block.id} className="border-b border-[var(--border-subtle)] py-2 text-sm">{block.name}</div>)}
                    </section>
                  ) : null}
                  <section className="space-y-2">
                    <h4 className="text-sm font-semibold">{t(locale, 'settings.engineCanvasCurrentHookPoints')}</h4>
                    {(stage.hook_points ?? []).length > 0 ? (stage.hook_points ?? []).map((point) => (
                      <div key={point.event} className="border-b border-[var(--border-subtle)] py-3 text-xs">
                        <div className="flex items-center justify-between gap-2"><strong>{point.event}</strong>{point.security_sensitive ? <ShieldCheck size={14} className="text-[var(--warning)]" aria-label={t(locale, 'settings.engineCanvasSecuritySensitive')} /> : null}</div>
                        <div className="mt-1 text-[var(--text-secondary)]">{t(locale, 'settings.engineCanvasPointHooks', { enabled: point.enabled_hook_count ?? 0, total: point.hook_count ?? 0 })}</div>
                        <div className={`mt-1 ${point.dispatched === false ? 'text-[var(--warning)]' : 'text-[var(--text-secondary)]'}`}>{point.dispatched === false ? t(locale, 'settings.engineCanvasNotDispatched') : point.dispatch_module ?? t(locale, 'settings.engineCanvasDispatchAvailable')}</div>
                      </div>
                    )) : <EvidenceEmpty locale={locale}>{t(locale, 'settings.engineCanvasNoHookPoints')}</EvidenceEmpty>}
                  </section>
                </>
              )}
            </div>
          ) : null}

          {tab === 'configure' ? (
            <div className="space-y-3">
              <p className="text-sm leading-6 text-[var(--text-secondary)]">{t(locale, 'settings.engineCanvasConfigureHint')}</p>
              {presentation.targets.length > 0 ? presentation.targets.map((target) => {
                const meta = TARGET_META[target];
                const Icon = meta.icon;
                return (
                  <button
                    key={target}
                    type="button"
                    className="flex w-full items-center gap-3 border-b border-[var(--border-subtle)] py-4 text-left hover:text-[var(--text)]"
                    onClick={() => onOpenWorkspace(target, stage.id)}
                  >
                    <Icon size={18} className="shrink-0 text-[var(--text-secondary)]" />
                    <span className="min-w-0 flex-1"><strong className="block text-sm">{t(locale, meta.labelKey)}</strong><span className="mt-1 block text-xs leading-5 text-[var(--text-secondary)]">{t(locale, meta.descKey)}</span></span>
                    <ChevronRight size={16} className="shrink-0 text-[var(--text-disabled)]" />
                  </button>
                );
              }) : <EvidenceEmpty locale={locale}>{t(locale, 'settings.engineCanvasNoConfiguration')}</EvidenceEmpty>}
            </div>
          ) : null}
        </div>
      </aside>
    </>
  );
}
