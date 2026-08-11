'use client';

import { AlertTriangle, Eye, Loader, Pencil, Rocket, Save } from 'lucide-react';
import { t, type Locale } from '@/i18n';
import { NativeExecutionCanvas, stageLabel } from './NativeExecutionCanvas';
import EngineCapabilitiesPanel from './EngineCapabilitiesPanel';
import { HooksEditor, PromptsEditor, RunsTimeline, VersionsPanel } from './native-harness/editors';
import { useNativeHarness } from './native-harness/useNativeHarness';

export interface NativeHarnessPanelProps { locale: Locale }

/**
 * 问题9（唯一 Harness）：全局只有当前 global binding 指向的 profile。
 * 进入即直接预览该 Harness；保留 Canvas、Prompt/Hook 编辑、版本、运行记录
 * 与测试运行。创建/列表/搜索/设默认/项目绑定/归档 UI 已退役（daemon 侧
 * profile.create/archive 与非 global binding.set 同步 fail-closed）。
 */
export function NativeHarnessPanel({ locale }: NativeHarnessPanelProps) {
  const harness = useNativeHarness(locale);

  if (harness.loading) {
    return (
      <section className="space-y-5" data-testid="native-harness-panel">
        <div className="engine-empty"><Loader size={18} className="animate-spin" />{t(locale, 'common.loading')}</div>
      </section>
    );
  }

  if (!harness.selectedProfile) {
    return (
      <section className="space-y-5" data-testid="native-harness-panel">
        {harness.error ? <div role="alert" className="rounded-lg border border-[var(--danger)] p-4 text-sm text-[var(--danger)]"><AlertTriangle size={16} className="mr-2 inline" />{harness.error}</div> : <div className="engine-empty">{t(locale, 'settings.engineEngineeringEmpty')}</div>}
      </section>
    );
  }

  const readOnly = harness.detailMode === 'preview';
  const runReady = Boolean(harness.runPrompt.trim() && harness.runProviderId && harness.runModelId && harness.runProjectPath);
  const runPanel = (
    <div className="rounded-lg border border-[var(--border-subtle)] bg-[var(--surface)] p-3">
      <div className="mb-3">
        <strong className="block text-sm text-[var(--text)]">{t(locale, 'settings.engineEngineeringRunTrialTitle')}</strong>
        <p className="mt-1 text-xs leading-5 text-[var(--text-secondary)]">{t(locale, 'settings.engineEngineeringRunTrialDesc')}</p>
      </div>
      <div className="grid gap-2 lg:grid-cols-[minmax(10rem,0.8fr)_minmax(10rem,0.8fr)_minmax(10rem,0.8fr)_minmax(16rem,1.2fr)_auto]">
        <select className="input" value={harness.runProjectPath} onChange={(event) => harness.setRunProjectPath(event.target.value)} aria-label={t(locale, 'settings.engineEngineeringRunProject')}>
          <option value="">{t(locale, 'settings.engineEngineeringSelectProject')}</option>
          {harness.projects.filter((project) => project.exists).map((project) => <option key={project.id} value={project.path}>{project.label}</option>)}
        </select>
        <select className="input" value={harness.runProviderId} onChange={(event) => harness.setRunProviderId(event.target.value)} aria-label={t(locale, 'settings.engineEngineeringRunProvider')}>
          <option value="">{t(locale, 'settings.engineEngineeringRunProvider')}</option>
          {harness.providers.map((provider) => <option key={provider.id} value={provider.id}>{provider.displayName}</option>)}
        </select>
        <select className="input" value={harness.runModelId} onChange={(event) => harness.setRunModelId(event.target.value)} aria-label={t(locale, 'settings.engineEngineeringRunModel')}>
          <option value="">{t(locale, 'settings.engineEngineeringRunModel')}</option>
          {harness.runModels.map((model) => <option key={model.id} value={model.id}>{model.displayName ?? model.id}</option>)}
        </select>
        <textarea className="input min-h-10" value={harness.runPrompt} onChange={(event) => harness.setRunPrompt(event.target.value)} placeholder={t(locale, 'settings.engineEngineeringRunPromptPlaceholder')} />
        <button type="button" className="btn btn-primary lg:w-28" disabled={harness.busy || !runReady} onClick={() => void harness.runFromCanvas()}><Rocket size={14} />{t(locale, 'settings.engineEngineeringRun')}</button>
      </div>
      <p className={`mt-2 text-xs ${runReady ? 'text-[var(--success)]' : 'text-[var(--text-secondary)]'}`}>
        {runReady ? t(locale, 'settings.engineEngineeringRunReady') : t(locale, 'settings.engineEngineeringRunMissing')}
      </p>
    </div>
  );
  const workspacePanel = (
    <>
      {harness.workspaceTarget === 'hooks' && harness.document ? <HooksEditor
        locale={locale}
        document={harness.document}
        revision={harness.revision}
        stageName={harness.hookStage ? stageLabel(harness.hookStage.id, locale) : null}
        stageEvents={harness.stageEvents}
        importedHooks={harness.importedHooks}
        overlays={harness.overlays}
        visibleHookIndexes={harness.visibleHookIndexes}
        replaceDocument={harness.replaceDocument}
        updateHook={harness.updateHook}
        updateAdapter={harness.updateAdapter}
        updateOverlay={harness.updateOverlay}
        readOnly={readOnly}
      /> : null}

      {harness.workspaceTarget === 'prompts' && harness.document ? <PromptsEditor
        locale={locale}
        document={harness.document}
        promptPreview={harness.promptPreview}
        replaceDocument={harness.replaceDocument}
        focusPromptId={harness.focusPromptId}
        readOnly={readOnly}
      /> : null}

      {harness.workspaceTarget === 'capabilities' ? (
        <EngineCapabilitiesPanel
          locale={locale}
          selectedRun={harness.selectedRun}
          runSnapshot={harness.runSnapshot}
        />
      ) : null}

      {harness.workspaceTarget === 'runs' ? <RunsTimeline
        locale={locale}
        loading={harness.auxLoading}
        runs={harness.liveRuns}
        selectedRunId={harness.selectedRunId}
        traceEntries={harness.traceEntries}
        runSnapshot={harness.runSnapshot}
        onRefresh={() => void harness.loadRuns(harness.selectedRunId)}
        onSelect={(runId) => void harness.loadRuns(runId)}
      /> : null}

      {harness.workspaceTarget === 'versions' ? <VersionsPanel locale={locale} review={harness.review} versions={harness.versions} findings={harness.findings} /> : null}
    </>
  );
  return (
    <section className="space-y-5" data-testid="native-harness-panel">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex flex-wrap gap-2">
          <button type="button" className="btn" onClick={() => harness.setDetailMode('preview')}><Eye size={14} />{t(locale, 'settings.engineEngineeringPreview')}</button>
          <button type="button" className="btn" onClick={() => harness.setDetailMode('edit')}><Pencil size={14} />{t(locale, 'settings.engineEngineeringEdit')}</button>
        </div>
        {!readOnly ? <div className="flex flex-wrap gap-2">
          <button type="button" className="btn" disabled={harness.busy || !harness.document} onClick={() => void harness.save()}><Save size={14} />{t(locale, 'settings.engineEngineeringSave')}</button>
          <button type="button" className="btn" disabled={harness.busy || !harness.document} onClick={() => void harness.reviewDraft()}>{t(locale, 'settings.engineEngineeringReview')}</button>
          <button type="button" className="btn btn-primary" disabled={harness.busy || !harness.document || harness.findings.some((item) => item.severity === 'error')} onClick={() => void harness.publish()}><Rocket size={14} />{t(locale, 'settings.engineEngineeringPublish')}</button>
        </div> : null}
      </div>

      {harness.error ? <div role="alert" className="rounded-lg border border-[var(--danger)] p-4 text-sm text-[var(--danger)]"><AlertTriangle size={16} className="mr-2 inline" />{harness.error}</div> : null}
      {harness.notice ? <div role="status" className="rounded-lg border border-[var(--success)] p-4 text-sm text-[var(--success)]">{harness.notice}</div> : null}
      {harness.sourceCandidate.length ? <div role="alert" className="settings-section-card border-[var(--warning)] text-sm">{t(locale, 'settings.engineEngineeringDriftDesc', { count: harness.sourceCandidate.length })}</div> : null}

      <section className="settings-section-card space-y-1">
        <p className="settings-section-eyebrow">{readOnly ? t(locale, 'settings.engineEngineeringPreview') : t(locale, 'settings.engineEngineeringEdit')}</p>
        <h3>{harness.selectedProfile.name}</h3>
        <p className="text-sm text-[var(--text-muted)]">{harness.selectedProfile.kind}{harness.currentProject ? ` · ${harness.currentProject.name} · ${harness.currentProject.canonical_path}` : ''}{harness.dirty ? ' · Draft*' : ''}</p>
      </section>

      {harness.detailLoading ? <div className="engine-empty"><Loader size={18} className="animate-spin" />{t(locale, 'common.loading')}</div> : harness.workspace ? (
        <NativeExecutionCanvas
          locale={locale}
          mode={harness.canvasMode}
          stages={harness.stages}
          edges={harness.workspace.topology?.edges ?? []}
          promptBlocks={harness.workspace.prompt_plan?.blocks ?? []}
          issueCount={harness.workspace.overview?.issues?.length ?? 0}
          runs={harness.liveRuns}
          selectedRunId={harness.selectedRunId}
          traceEntries={harness.traceEntries}
          runSnapshot={harness.runSnapshot}
          auditLoading={harness.auxLoading}
          readOnly={readOnly}
          nodeDetails={harness.nodeDetails}
          onModeChange={harness.setCanvasMode}
          onRequestAudit={() => void harness.loadRuns()}
          onSelectRun={harness.selectRun}
          onRefreshAudit={() => void harness.loadRuns(harness.selectedRunId)}
          onOpenWorkspace={harness.openCanvasWorkspace}
          onSetHookEnabled={harness.setNodeHookEnabled}
          onAuthorizeHook={harness.authorizeNodeHook}
          onRemoveHook={harness.removeNodeHook}
          onRemovePrompt={harness.removeNodePrompt}
          runPanel={runPanel}
          workspacePanel={workspacePanel}
        />
      ) : null}
    </section>
  );
}

export default NativeHarnessPanel;
