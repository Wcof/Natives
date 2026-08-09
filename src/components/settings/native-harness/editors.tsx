'use client';

import { useEffect, useRef } from 'react';
import { Loader, Plus, RefreshCw, Trash2 } from 'lucide-react';
import { t, type Locale } from '@/i18n';
import { runStatusLabel } from '../nativeExecutionCanvasModel';
import {
  ADAPTERS,
  EVENTS,
  adapterDefaults,
  newHook,
  newPromptBlock,
  type AdapterType,
  type Blueprint,
  type CatalogHook,
  type HookOverlay,
  type NativeHook,
  type PromptBlock,
  type PromptPreview,
  type Review,
  type Run,
  type RunSnapshot,
  type TraceEntry,
  type VersionSummary,
} from './model';

export function HooksEditor({
  locale,
  document,
  revision,
  stageName,
  stageEvents,
  importedHooks,
  overlays,
  visibleHookIndexes,
  replaceDocument,
  updateHook,
  updateAdapter,
  updateOverlay,
  readOnly,
}: {
  locale: Locale;
  document: Blueprint;
  revision: number;
  stageName: string | null;
  stageEvents: Set<string>;
  importedHooks: CatalogHook[];
  overlays: HookOverlay[];
  visibleHookIndexes: Array<{ hook: NativeHook; index: number }>;
  replaceDocument: (next: Blueprint) => void;
  updateHook: (index: number, patch: Partial<NativeHook>) => void;
  updateAdapter: (index: number, patch: Record<string, unknown>) => void;
  updateOverlay: (hook: CatalogHook, patch: Partial<HookOverlay>) => void;
  readOnly: boolean;
}) {
  const defaultEvent = [...stageEvents][0] ?? 'PreToolUse';
  const imported = stageName ? importedHooks.filter((hook) => stageEvents.has(hook.event)) : importedHooks;
  return (
    <section className="settings-section-card space-y-4">
      <fieldset disabled={readOnly} className="space-y-4">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div>
            <h4>{stageName ? t(locale, 'settings.engineEngineeringStageHooks', { stage: stageName }) : t(locale, 'settings.engineEngineeringTabHooks')}</h4>
            <p className="text-xs text-[var(--text-muted)]">Draft r{revision} · {visibleHookIndexes.length} Hooks</p>
          </div>
          <div className="flex flex-wrap gap-2">{ADAPTERS.map((type) => <button key={type} type="button" className="btn" onClick={() => replaceDocument({ ...document, native_hooks: [...document.native_hooks, newHook(type, defaultEvent)] })}><Plus size={13} />{type}</button>)}</div>
        </div>
        {imported.length > 0 ? <div className="space-y-2">
          <h5 className="text-sm font-semibold">{t(locale, 'settings.engineEngineeringImportedHooks')}</h5>
          {imported.map((hook) => {
            const overlay = overlays.find((item) => item.hook_id === hook.id);
            return <article className="rounded-lg border border-[var(--border)] p-3" key={hook.id}>
              <div className="grid items-center gap-2 lg:grid-cols-5">
                <div><strong>{hook.event}</strong><div className="text-xs text-[var(--text-muted)]">{hook.source.origin}</div></div>
                <input className="input" aria-label="Imported matcher" value={overlay?.matcher ?? hook.matcher ?? ''} onChange={(event) => updateOverlay(hook, { matcher: event.target.value })} />
                <input className="input" aria-label="Imported order" type="number" value={overlay?.order ?? hook.order} onChange={(event) => updateOverlay(hook, { order: Number(event.target.value) })} />
                <input className="input" aria-label="Imported timeout" type="number" min={1000} value={overlay?.timeout_ms ?? hook.timeout_ms} onChange={(event) => updateOverlay(hook, { timeout_ms: Number(event.target.value) })} />
                <label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={overlay?.enabled ?? hook.enabled} onChange={(event) => updateOverlay(hook, { enabled: event.target.checked })} />{t(locale, 'common.enabled')}</label>
              </div>
            </article>;
          })}
        </div> : null}
        {visibleHookIndexes.length === 0 ? <div className="engine-empty">{t(locale, 'settings.engineEngineeringEmpty')}</div> : visibleHookIndexes.map(({ hook, index }) => (
          <article className="rounded-lg border border-[var(--border)] p-4" key={hook.id}>
            <div className="grid gap-3 lg:grid-cols-6">
              <input aria-label="Hook name" className="input lg:col-span-2" value={hook.name} onChange={(event) => updateHook(index, { name: event.target.value })} />
              <select aria-label="Hook event" className="input" value={hook.event} onChange={(event) => updateHook(index, { event: event.target.value })}>{EVENTS.map((event) => <option key={event}>{event}</option>)}</select>
              <select aria-label="Hook adapter" className="input" value={hook.adapter.type} onChange={(event) => updateHook(index, { adapter: adapterDefaults(event.target.value as AdapterType), trust_confirmed: false })}>{ADAPTERS.map((type) => <option key={type}>{type}</option>)}</select>
              <input aria-label="Matcher" className="input" value={hook.matcher ?? ''} placeholder="matcher" onChange={(event) => updateHook(index, { matcher: event.target.value })} />
              <button aria-label="Delete Hook" type="button" className="btn text-[var(--danger)]" onClick={() => replaceDocument({ ...document, native_hooks: document.native_hooks.filter((_, item) => item !== index) })}><Trash2 size={14} /></button>
            </div>
            <HookAdapterFields hook={hook} index={index} locale={locale} updateHook={updateHook} updateAdapter={updateAdapter} />
          </article>
        ))}
      </fieldset>
    </section>
  );
}

function HookAdapterFields({ hook, index, locale, updateHook, updateAdapter }: {
  hook: NativeHook;
  index: number;
  locale: Locale;
  updateHook: (index: number, patch: Partial<NativeHook>) => void;
  updateAdapter: (index: number, patch: Record<string, unknown>) => void;
}) {
  return (
    <div className="mt-3 grid gap-3 lg:grid-cols-4">
      {hook.adapter.type === 'command' ? <>
        <input className="input" placeholder="program" value={String(hook.adapter.program ?? '')} onChange={(event) => updateAdapter(index, { program: event.target.value })} />
        <input className="input" placeholder="args (one per line)" value={(hook.adapter.args as string[] ?? []).join('\n')} onChange={(event) => updateAdapter(index, { args: event.target.value.split('\n').filter(Boolean) })} />
        <label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={Boolean(hook.adapter.trusted) && hook.trust_confirmed} onChange={(event) => updateHook(index, { adapter: { ...hook.adapter, trusted: event.target.checked }, trust_confirmed: event.target.checked })} />{t(locale, 'settings.engineEngineeringTrust')}</label>
      </> : null}
      {hook.adapter.type === 'http' ? <>
        <input className="input lg:col-span-2" placeholder="https://…" value={String(hook.adapter.url ?? '')} onChange={(event) => updateAdapter(index, { url: event.target.value })} />
        <input className="input" placeholder="allowed hosts, comma separated" value={(hook.adapter.allow_hosts as string[] ?? []).join(',')} onChange={(event) => updateAdapter(index, { allow_hosts: event.target.value.split(',').map((value) => value.trim()).filter(Boolean) })} />
      </> : null}
      {hook.adapter.type === 'mcp_tool' ? <>
        <input className="input" placeholder="MCP server id" value={String(hook.adapter.server_id ?? '')} onChange={(event) => updateAdapter(index, { server_id: event.target.value })} />
        <input className="input" placeholder="tool name" value={String(hook.adapter.tool_name ?? '')} onChange={(event) => updateAdapter(index, { tool_name: event.target.value })} />
        <textarea className="input lg:col-span-2" placeholder="JSON input template" value={String(hook.adapter.input_template ?? '')} onChange={(event) => updateAdapter(index, { input_template: event.target.value })} />
      </> : null}
      {hook.adapter.type === 'prompt' ? <>
        <textarea className="input lg:col-span-3" placeholder="decision prompt" value={String(hook.adapter.template ?? '')} onChange={(event) => updateAdapter(index, { template: event.target.value })} />
        <input className="input" placeholder="model override (optional)" value={String(hook.adapter.model_override ?? '')} onChange={(event) => updateAdapter(index, { model_override: event.target.value || undefined })} />
      </> : null}
      {hook.adapter.type === 'agent' ? <>
        <textarea className="input lg:col-span-2" placeholder="agent task" value={String(hook.adapter.prompt ?? '')} onChange={(event) => updateAdapter(index, { prompt: event.target.value })} />
        <input className="input" type="number" min={1} max={32} value={Number(hook.adapter.max_steps ?? 5)} onChange={(event) => updateAdapter(index, { max_steps: Number(event.target.value) })} />
        <input className="input" placeholder="readonly tools, comma separated" value={(hook.adapter.readonly_tools as string[] ?? []).join(',')} onChange={(event) => updateAdapter(index, { readonly_tools: event.target.value.split(',').map((value) => value.trim()).filter(Boolean) })} />
      </> : null}
      <input className="input" type="number" min={100} value={hook.timeout_ms} onChange={(event) => updateHook(index, { timeout_ms: Number(event.target.value) })} />
      <select className="input" value={hook.failure_policy} onChange={(event) => updateHook(index, { failure_policy: event.target.value })}><option value="fail">fail</option><option value="skip">skip</option><option value="default">default</option></select>
      <label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={hook.enabled} onChange={(event) => updateHook(index, { enabled: event.target.checked })} />{t(locale, 'common.enabled')}</label>
    </div>
  );
}

export function PromptsEditor({ locale, document, promptPreview, replaceDocument, focusPromptId, readOnly }: {
  locale: Locale;
  document: Blueprint;
  promptPreview: PromptPreview | null;
  replaceDocument: (next: Blueprint) => void;
  focusPromptId: string | null;
  readOnly: boolean;
}) {
  const promptRefs = useRef<Record<string, HTMLElement | null>>({});
  const builtinCount = promptPreview?.builtin_surfaces?.length ?? 0;
  const promptCount = document.prompt_blocks.length;

  useEffect(() => {
    const node = focusPromptId ? promptRefs.current[focusPromptId] : null;
    if (!node) return;
    node.scrollIntoView({ block: 'center', behavior: 'smooth' });
    node.querySelector<HTMLTextAreaElement | HTMLInputElement | HTMLSelectElement>('textarea,input,select')?.focus();
  }, [builtinCount, focusPromptId, promptCount]);

  return (
    <section className="settings-section-card space-y-4">
      <fieldset disabled={readOnly} className="space-y-4">
        <div className="flex items-center justify-between gap-3">
          <h4>{t(locale, 'settings.engineEngineeringTabPrompts')}</h4>
          <button type="button" className="btn" onClick={() => replaceDocument({ ...document, prompt_blocks: [...document.prompt_blocks, newPromptBlock()] })}><Plus size={13} />{t(locale, 'settings.engineEngineeringAddPromptBlock')}</button>
        </div>
        {(promptPreview?.builtin_surfaces ?? []).map((surface) => {
          const replacements = document.builtin_prompt_replacements ?? [];
          const replacement = replacements.find((item) => item.surface_id === surface.surface_id);
          const updateReplacement = (markdown: string) => replaceDocument({
            ...document,
            schema_version: 4,
            builtin_prompt_replacements: [
              ...replacements.filter((item) => item.surface_id !== surface.surface_id),
              { surface_id: surface.surface_id, markdown, base_default_digest: surface.default_digest },
            ],
          });
          const focused = focusPromptId === surface.surface_id;
          return <article
            ref={(node) => { promptRefs.current[surface.surface_id] = node; }}
            className={`rounded-lg border p-4 ${focused ? 'border-[var(--primary)] bg-[var(--primary-soft)]' : 'border-[var(--border)]'}`}
            key={surface.surface_id}
          >
            <div className="flex flex-wrap items-start justify-between gap-3">
              <div><strong>{t(locale, 'settings.engineEngineeringBuiltinPrompt')}</strong><div className="mt-1 font-mono text-xs text-[var(--text-muted)]">{surface.surface_id}</div></div>
              {replacement
                ? <button type="button" className="btn" onClick={() => replaceDocument({ ...document, builtin_prompt_replacements: replacements.filter((item) => item.surface_id !== surface.surface_id) })}>{t(locale, 'settings.engineEngineeringRestorePrompt')}</button>
                : <button type="button" className="btn" onClick={() => updateReplacement(surface.default_markdown)}>{t(locale, 'settings.engineEngineeringCreatePromptReplacement')}</button>}
            </div>
            <textarea className="input mt-3 min-h-48 w-full font-mono text-xs" readOnly={!replacement} value={replacement?.markdown ?? surface.default_markdown} onChange={(event) => updateReplacement(event.target.value)} />
          </article>;
        })}
        {document.prompt_blocks.length === 0 ? <div className="engine-empty">{t(locale, 'settings.engineEngineeringEmpty')}</div> : document.prompt_blocks.map((block, index) => {
          const update = (patch: Partial<PromptBlock>) => {
            const prompt_blocks = [...document.prompt_blocks];
            prompt_blocks[index] = { ...block, ...patch };
            replaceDocument({ ...document, prompt_blocks });
          };
          const focused = focusPromptId === block.id;
          return <article
            ref={(node) => { promptRefs.current[block.id] = node; }}
            className={`rounded-lg border p-4 ${focused ? 'border-[var(--primary)] bg-[var(--primary-soft)]' : 'border-[var(--border)]'}`}
            key={block.id}
          >
            <div className="grid gap-3 lg:grid-cols-5">
              <input className="input lg:col-span-2" aria-label={t(locale, 'settings.engineEngineeringPromptName')} value={block.name} onChange={(event) => update({ name: event.target.value })} />
              <select className="input" aria-label={t(locale, 'settings.engineEngineeringPromptPlacement')} value={block.placement} onChange={(event) => update({ placement: event.target.value })}>
                {['before_profile', 'after_profile', 'after_project_instructions', 'final'].map((placement) => <option key={placement}>{placement}</option>)}
              </select>
              <input className="input" type="number" aria-label={t(locale, 'settings.engineEngineeringPromptOrder')} value={block.order} onChange={(event) => update({ order: Number(event.target.value) })} />
              <button type="button" className="btn text-[var(--danger)]" aria-label={t(locale, 'settings.engineEngineeringDeletePromptBlock')} onClick={() => replaceDocument({ ...document, prompt_blocks: document.prompt_blocks.filter((_, item) => item !== index) })}><Trash2 size={14} /></button>
            </div>
            <textarea className="input mt-3 min-h-32 w-full" aria-label={t(locale, 'settings.engineEngineeringPromptContent')} value={block.markdown} onChange={(event) => update({ markdown: event.target.value })} />
            <label className="mt-2 flex items-center gap-2 text-sm"><input type="checkbox" checked={block.enabled} onChange={(event) => update({ enabled: event.target.checked })} />{t(locale, 'common.enabled')}</label>
          </article>;
        })}
        {(promptPreview?.blocks ?? []).length > 0 ? <div className="space-y-2">
          <h4>{t(locale, 'settings.engineEngineeringPromptPlan')}</h4>
          {(promptPreview?.blocks ?? []).map((block, index) => <div className="grid gap-2 rounded border border-[var(--border)] p-3 text-xs lg:grid-cols-[3rem_1fr_1fr_8rem]" key={block.id}>
            <strong>#{index + 1}</strong><span>{block.name} · {block.placement}</span><code>{block.source_digest.slice(0, 16)}…</code><span>{block.token_estimate} tokens</span>
          </div>)}
        </div> : null}
      </fieldset>
    </section>
  );
}

export function RunsTimeline({ locale, loading, runs, selectedRunId, traceEntries, runSnapshot, onRefresh, onSelect }: {
  locale: Locale;
  loading: boolean;
  runs: Run[];
  selectedRunId: string;
  traceEntries: TraceEntry[];
  runSnapshot: RunSnapshot | null;
  onRefresh: () => void;
  onSelect: (runId: string) => void;
}) {
  return (
    <section className="grid gap-4 lg:grid-cols-[minmax(18rem,0.7fr)_minmax(24rem,1.3fr)]">
      <div className="settings-section-card space-y-2">
        <div className="flex items-center justify-between"><h4>{t(locale, 'settings.engineEngineeringTabRuns')}</h4><button type="button" className="btn" onClick={onRefresh}><RefreshCw size={13} /></button></div>
        {loading ? <Loader size={16} className="animate-spin" /> : runs.map((run) => <button type="button" key={run.id} className={`w-full rounded border p-3 text-left ${selectedRunId === run.id ? 'border-[var(--accent)]' : 'border-[var(--border)]'}`} onClick={() => onSelect(run.id)}>
          <strong>{runStatusLabel(locale, run.status)}</strong><div className="text-xs text-[var(--text-muted)]">{run.id}</div><div className="text-xs">{run.provider_id} · {run.model_id}</div>
        </button>)}
        {!loading && runs.length === 0 ? <div className="engine-empty">{t(locale, 'settings.engineEngineeringEmpty')}</div> : null}
      </div>
      <div className="settings-section-card space-y-2">
        <h4>{t(locale, 'settings.engineEngineeringHookTrace')}</h4>
        {runSnapshot ? <div className="rounded border border-[var(--border)] p-3 text-xs">
          <strong>{runSnapshot.resolved ? t(locale, 'settings.engineEngineeringSnapshotFrozen') : t(locale, 'settings.engineEngineeringSnapshotMissing')}</strong>
          {runSnapshot.canonical_hash ? <div className="mt-1 font-mono text-[var(--text-muted)]">{runSnapshot.canonical_hash}</div> : null}
        </div> : null}
        {traceEntries.map((entry, index) => <div className="rounded border border-[var(--border)] p-3 text-xs" key={`${entry.run_id}-${entry.sequence}-${index}`}>
          <strong>#{entry.sequence} · {entry.type ?? entry.status ?? 'hook'}</strong><div>{entry.hook_id ?? '—'}{entry.duration_ms != null ? ` · ${entry.duration_ms}ms` : ''}</div><div className="text-[var(--text-muted)]">{entry.timestamp}</div>
        </div>)}
        {traceEntries.length === 0 ? <div className="engine-empty">{t(locale, 'settings.engineEngineeringEmpty')}</div> : null}
      </div>
    </section>
  );
}

export function VersionsPanel({ locale, review, versions, findings }: {
  locale: Locale;
  review: Review | null;
  versions: VersionSummary[];
  findings: Array<{ severity?: string; code?: string; message?: string }>;
}) {
  return (
    <section className="settings-section-card space-y-3">
      {review ? <div className="space-y-2">
        <h4>{t(locale, 'settings.engineEngineeringReview')}</h4>
        {findings.length ? findings.map((item, index) => <div key={`${item.code}-${index}`} className={item.severity === 'error' ? 'text-[var(--danger)]' : 'text-[var(--text-muted)]'}>{item.severity} · {item.code} · {item.message}</div>) : <div className="text-[var(--success)]">{t(locale, 'settings.engineEngineeringPublishable')}</div>}
        <div className="pt-2 text-sm text-[var(--text-muted)]">{t(locale, 'settings.engineEngineeringChanges', { count: review.diff?.length ?? 0 })}</div>
      </div> : null}
      <h4>{t(locale, 'settings.engineEngineeringVersions')}</h4>
      {versions.map((version) => <div className="grid items-center gap-2 rounded border border-[var(--border)] p-3 text-xs lg:grid-cols-[6rem_1fr_12rem]" key={version.id}>
        <strong>v{version.version_number}</strong><code>{version.canonical_hash.slice(0, 20)}…</code><time>{version.created_at}</time>
      </div>)}
      {versions.length === 0 ? <div className="engine-empty">{t(locale, 'settings.engineEngineeringEmpty')}</div> : null}
    </section>
  );
}
