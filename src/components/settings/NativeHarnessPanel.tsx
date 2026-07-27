'use client';

import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Activity,
  AlertTriangle,
  Braces,
  CheckCircle2,
  ChevronRight,
  CircleDot,
  FileCode2,
  GitBranch,
  History,
  Loader,
  LockKeyhole,
  Play,
  RefreshCw,
  ShieldCheck,
  Sparkles,
  Workflow,
  Zap,
} from 'lucide-react';
import nativesAPI from '@/lib/tauri-adapter';
import { classifyError } from '@/lib/error-classifier';
import { t, type Locale } from '@/i18n';

type HarnessLayer = {
  layer?: string;
  profile_name?: string;
  version_number?: number;
  canonical_hash?: string;
};

type HarnessOverview = {
  topology_version?: number;
  hook_semantics_version?: string;
  blueprint_schema_version?: number;
  layers?: HarnessLayer[];
  hook_counts?: {
    total?: number;
    enabled?: number;
    attached_to_undispatched_points?: number;
  };
  issues?: string[];
};

type HookPoint = {
  event: string;
  security_sensitive?: boolean;
  dispatched?: boolean;
  dispatch_module?: string;
  hook_count?: number;
  enabled_hook_count?: number;
  hook_ids?: string[];
};

type EngineStage = {
  id: string;
  order: number;
  hook_points?: HookPoint[];
  safe_points?: string[];
};

type HarnessTopology = {
  topology_version?: number;
  stages?: EngineStage[];
};

type HarnessHook = {
  id: string;
  event?: string;
  stage?: string;
  enabled?: boolean;
  locked?: boolean;
  trusted?: boolean | null;
  matcher?: string | null;
  timeout_ms?: number;
  failure_policy?: string;
  source?: { scope?: string; origin?: string };
};

type HookCatalog = {
  hooks?: HarnessHook[];
  issues?: string[];
};

type PromptBlock = {
  id?: string;
  label?: string;
  order?: number;
  source_digest?: string;
  token_estimate?: number;
};

type PromptPreview = {
  blocks?: PromptBlock[];
  raw_persisted?: boolean;
};

type RunSummary = {
  id?: string;
  run_id?: string;
  status?: string;
  provider_id?: string;
  model_id?: string;
  created_at?: string;
};

type AuditEntry = {
  id?: string;
  action?: string;
  actor?: string;
  created_at?: string;
  profile_id?: string;
  summary?: Record<string, unknown>;
};

type WorkspaceData = {
  overview: HarnessOverview;
  topology: HarnessTopology;
  catalog: HookCatalog;
  prompts: PromptPreview;
};

type Tab = 'topology' | 'hooks' | 'prompts' | 'runs' | 'audit';

const TAB_ICONS = {
  topology: Workflow,
  hooks: Zap,
  prompts: Sparkles,
  runs: Activity,
  audit: History,
} as const;

const stageLabel = (id: string) =>
  id
    .split('_')
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(' ');

const shortHash = (hash?: string) => (hash ? hash.slice(0, 10) : '—');

function EmptyProjection({ locale }: { locale: Locale }) {
  return (
    <div className="engine-empty">
      <CircleDot size={22} />
      <span>{t(locale, 'settings.engineEngineeringEmpty')}</span>
    </div>
  );
}

function TopologyCanvas({
  locale,
  overview,
  topology,
}: {
  locale: Locale;
  overview: HarnessOverview;
  topology: HarnessTopology;
}) {
  const stages = [...(topology.stages ?? [])].sort((a, b) => a.order - b.order);
  if (!stages.length) return <EmptyProjection locale={locale} />;

  return (
    <div className="engine-topology-wrap">
      <div className="engine-topology-legend">
        <span><i className="engine-dot engine-dot-active" />{t(locale, 'settings.engineEngineeringHookPoint')}</span>
        <span><i className="engine-dot engine-dot-secure" />{t(locale, 'settings.engineEngineeringSecurityGate')}</span>
        <span><ShieldCheck size={13} />{t(locale, 'settings.engineEngineeringSafePoint')}</span>
      </div>
      <div className="engine-flow" aria-label={t(locale, 'settings.engineEngineeringTopology')}>
        {stages.map((stage, index) => {
          const hooks = stage.hook_points ?? [];
          const enabled = hooks.reduce((sum, point) => sum + (point.enabled_hook_count ?? 0), 0);
          const security = hooks.some((point) => point.security_sensitive);
          return (
            <div className="engine-flow-step" key={stage.id}>
              <article className={`engine-stage${security ? ' engine-stage-secure' : ''}`}>
                <div className="engine-stage-order">{String(stage.order).padStart(2, '0')}</div>
                <div className="engine-stage-icon">
                  {security ? <LockKeyhole size={17} /> : <Braces size={17} />}
                </div>
                <h4>{stageLabel(stage.id)}</h4>
                <div className="engine-stage-meta">
                  <span><Zap size={12} />{enabled} Hooks</span>
                  <span><ShieldCheck size={12} />{stage.safe_points?.length ?? 0}</span>
                </div>
                <div className="engine-hook-points">
                  {hooks.map((point) => (
                    <div
                      className={`engine-hook-point${point.security_sensitive ? ' secure' : ''}${!point.dispatched ? ' muted' : ''}`}
                      key={point.event}
                      title={point.dispatch_module ?? point.event}
                    >
                      <i />
                      <span>{point.event}</span>
                      <b>{point.enabled_hook_count ?? 0}</b>
                    </div>
                  ))}
                </div>
                {(stage.safe_points?.length ?? 0) > 0 && (
                  <div className="engine-safe-points">
                    {stage.safe_points?.map((point) => (
                      <span key={point}><ShieldCheck size={11} />{point}</span>
                    ))}
                  </div>
                )}
              </article>
              {index < stages.length - 1 && (
                <div className="engine-flow-arrow" aria-hidden="true">
                  <span />
                  <ChevronRight size={17} />
                </div>
              )}
            </div>
          );
        })}
      </div>
      <div className="engine-blueprint-rail">
        <div>
          <GitBranch size={15} />
          <strong>{t(locale, 'settings.engineEngineeringBlueprintRail')}</strong>
        </div>
        {(overview.layers ?? []).map((layer, index) => (
          <div className="engine-layer" key={`${layer.layer}-${index}`}>
            <span>{layer.layer ?? t(locale, 'settings.engineEngineeringLayer')}</span>
            <strong>{layer.profile_name ?? '—'}</strong>
            <small>v{layer.version_number ?? 0} · {shortHash(layer.canonical_hash)}</small>
          </div>
        ))}
      </div>
    </div>
  );
}

function HooksView({ locale, hooks }: { locale: Locale; hooks: HarnessHook[] }) {
  const grouped = useMemo(() => {
    const result = new Map<string, HarnessHook[]>();
    hooks.forEach((hook) => {
      const key = hook.stage ?? 'unknown';
      result.set(key, [...(result.get(key) ?? []), hook]);
    });
    return [...result.entries()];
  }, [hooks]);

  if (!hooks.length) return <EmptyProjection locale={locale} />;
  return (
    <div className="engine-hook-groups">
      {grouped.map(([stage, entries]) => (
        <section className="engine-hook-group" key={stage}>
          <header>
            <div><Zap size={15} /><strong>{stageLabel(stage)}</strong></div>
            <span>{entries.filter((hook) => hook.enabled).length}/{entries.length}</span>
          </header>
          <div className="engine-hook-grid">
            {entries.map((hook) => (
              <article className={`engine-hook-card${hook.enabled ? ' enabled' : ''}`} key={hook.id}>
                <div className="engine-hook-card-head">
                  <span className={`engine-status-pill ${hook.enabled ? 'success' : 'muted'}`}>
                    {hook.enabled ? <CheckCircle2 size={11} /> : <CircleDot size={11} />}
                    {hook.enabled ? t(locale, 'common.enabled') : t(locale, 'common.disabled')}
                  </span>
                  {hook.locked && <LockKeyhole size={13} />}
                </div>
                <strong>{hook.event ?? hook.id}</strong>
                <code>{hook.source?.origin ?? hook.id}</code>
                <div className="engine-hook-details">
                  <span>{t(locale, 'settings.engineEngineeringMatcher')}<b>{hook.matcher ?? '*'}</b></span>
                  <span>{t(locale, 'settings.engineEngineeringTimeout')}<b>{hook.timeout_ms ?? 0}ms</b></span>
                  <span>{t(locale, 'settings.engineEngineeringPolicy')}<b>{hook.failure_policy ?? '—'}</b></span>
                </div>
              </article>
            ))}
          </div>
        </section>
      ))}
    </div>
  );
}

function PromptsView({ locale, prompts }: { locale: Locale; prompts: PromptBlock[] }) {
  if (!prompts.length) return <EmptyProjection locale={locale} />;
  const totalTokens = prompts.reduce((sum, block) => sum + (block.token_estimate ?? 0), 0);
  return (
    <div className="engine-prompt-plan">
      <div className="engine-prompt-summary">
        <Sparkles size={17} />
        <div><strong>{prompts.length}</strong><span>{t(locale, 'settings.engineEngineeringPromptBlocks')}</span></div>
        <div><strong>≈ {totalTokens}</strong><span>{t(locale, 'settings.engineEngineeringTokens')}</span></div>
      </div>
      <div className="engine-prompt-stack">
        {[...prompts].sort((a, b) => (a.order ?? 0) - (b.order ?? 0)).map((block, index) => (
          <div className="engine-prompt-row" key={block.id ?? index}>
            <div className="engine-prompt-index">{String(index + 1).padStart(2, '0')}</div>
            <FileCode2 size={17} />
            <div>
              <strong>{block.label ?? block.id ?? 'Prompt Block'}</strong>
              <code>{shortHash(block.source_digest)}</code>
            </div>
            <span>≈ {block.token_estimate ?? 0} tokens</span>
          </div>
        ))}
      </div>
    </div>
  );
}

function RunsView({ locale, runs }: { locale: Locale; runs: RunSummary[] }) {
  if (!runs.length) return <EmptyProjection locale={locale} />;
  return (
    <div className="engine-run-list">
      {runs.map((run, index) => {
        const id = run.run_id ?? run.id ?? String(index);
        return (
          <div className="engine-run-row" key={id}>
            <div className="engine-run-state"><Play size={13} /></div>
            <div><strong>{id}</strong><span>{run.provider_id ?? '—'} / {run.model_id ?? '—'}</span></div>
            <span className={`engine-status-pill ${run.status === 'running' ? 'success' : 'muted'}`}>{run.status ?? 'unknown'}</span>
            <time>{run.created_at ?? '—'}</time>
          </div>
        );
      })}
    </div>
  );
}

function AuditView({ locale, entries }: { locale: Locale; entries: AuditEntry[] }) {
  if (!entries.length) return <EmptyProjection locale={locale} />;
  return (
    <div className="engine-audit-list">
      {entries.map((entry, index) => (
        <div className="engine-audit-row" key={entry.id ?? index}>
          <div className="engine-audit-line"><i /></div>
          <History size={15} />
          <div>
            <strong>{entry.action ?? 'event'}</strong>
            <span>{entry.profile_id ?? entry.actor ?? 'system'}</span>
          </div>
          <time>{entry.created_at ?? '—'}</time>
        </div>
      ))}
    </div>
  );
}

export default function NativeHarnessPanel({ locale }: { locale: Locale }) {
  const [state, setState] = useState<
    { status: 'loading' | 'success' | 'error'; data?: WorkspaceData; error?: string }
  >({ status: 'loading' });
  const [tab, setTab] = useState<Tab>('topology');
  const [secondary, setSecondary] = useState<{ loading: boolean; error: string | null; runs: RunSummary[]; audit: AuditEntry[] }>({
    loading: false,
    error: null,
    runs: [],
    audit: [],
  });

  const loadWorkspace = useCallback(async () => {
    setState({ status: 'loading' });
    try {
      const [overview, topology, catalog, prompts] = await Promise.all([
        nativesAPI.assistantV2.request<HarnessOverview>('harness.overview'),
        nativesAPI.assistantV2.request<HarnessTopology>('harness.topology'),
        nativesAPI.assistantV2.request<HookCatalog>('harness.hook.catalog'),
        nativesAPI.assistantV2.request<PromptPreview>('harness.prompt.preview'),
      ]);
      setState({ status: 'success', data: { overview, topology, catalog, prompts } });
    } catch (error) {
      setState({
        status: 'error',
        error: classifyError(error, { locale }).userMessage,
      });
    }
  }, [locale]);

  useEffect(() => {
    void loadWorkspace();
  }, [loadWorkspace]);

  useEffect(() => {
    if (tab !== 'runs' && tab !== 'audit') return;
    let alive = true;
    setSecondary((current) => ({ ...current, loading: true, error: null }));
    const method = tab === 'runs' ? 'run.list' : 'harness.audit.list';
    void nativesAPI.assistantV2.request<Record<string, unknown>>(method, { limit: 50 })
      .then((result) => {
        if (!alive) return;
        const values = tab === 'runs'
          ? ((result.runs ?? result.items ?? []) as RunSummary[])
          : ((result.entries ?? []) as AuditEntry[]);
        setSecondary((current) => ({
          ...current,
          loading: false,
          ...(tab === 'runs' ? { runs: values as RunSummary[] } : { audit: values as AuditEntry[] }),
        }));
      })
      .catch((error) => alive && setSecondary((current) => ({
        ...current,
        loading: false,
        error: classifyError(error, { locale }).userMessage,
      })));
    return () => { alive = false; };
  }, [locale, tab]);

  if (state.status === 'loading') {
    return <div className="settings-state"><Loader size={20} className="animate-spin" />{t(locale, 'settings.nativeHarnessLoading')}</div>;
  }
  if (state.status === 'error') {
    return (
      <div className="settings-state settings-state-error">
        <AlertTriangle size={20} />
        <strong>{t(locale, 'settings.nativeHarnessError')}</strong>
        <span>{state.error}</span>
        <button className="btn btn-secondary" type="button" onClick={() => void loadWorkspace()}>
          <RefreshCw size={13} />{t(locale, 'common.retry')}
        </button>
      </div>
    );
  }
  const data = state.data;
  if (!data) return <div className="settings-state">{t(locale, 'settings.nativeHarnessUnavailable')}</div>;

  const stats = [
    { icon: Workflow, label: t(locale, 'settings.engineEngineeringTopologyVersion'), value: `v${data.overview.topology_version ?? 0}` },
    { icon: Zap, label: t(locale, 'settings.engineEngineeringActiveHooks'), value: `${data.overview.hook_counts?.enabled ?? 0}/${data.overview.hook_counts?.total ?? 0}` },
    { icon: FileCode2, label: t(locale, 'settings.engineEngineeringSchema'), value: `v${data.overview.blueprint_schema_version ?? 0}` },
    { icon: GitBranch, label: t(locale, 'settings.engineEngineeringLayers'), value: String(data.overview.layers?.length ?? 0) },
  ];

  return (
    <div className="engine-engineering">
      <header className="engine-hero">
        <div className="engine-hero-icon"><Workflow size={24} /></div>
        <div>
          <span>{t(locale, 'settings.engineEngineeringEyebrow')}</span>
          <h2>{t(locale, 'settings.engineEngineeringTitle')}</h2>
          <p>{t(locale, 'settings.engineEngineeringDesc')}</p>
        </div>
        <button className="btn btn-secondary" type="button" onClick={() => void loadWorkspace()}>
          <RefreshCw size={13} />{t(locale, 'common.refresh')}
        </button>
      </header>

      <div className="engine-stat-grid">
        {stats.map(({ icon: Icon, label, value }) => (
          <div className="engine-stat-card" key={label}>
            <Icon size={17} />
            <div><span>{label}</span><strong>{value}</strong></div>
          </div>
        ))}
      </div>

      {(data.overview.issues?.length ?? 0) > 0 && (
        <div className="engine-issues">
          <AlertTriangle size={15} />
          <span>{data.overview.issues?.join(' · ')}</span>
        </div>
      )}

      <div className="engine-workspace">
        <nav className="engine-tabs" aria-label={t(locale, 'settings.engineEngineeringTitle')}>
          {(['topology', 'hooks', 'prompts', 'runs', 'audit'] as Tab[]).map((item) => {
            const Icon = TAB_ICONS[item];
            return (
              <button
                key={item}
                type="button"
                className={tab === item ? 'active' : ''}
                aria-current={tab === item ? 'page' : undefined}
                onClick={() => setTab(item)}
              >
                <Icon size={15} />
                {t(locale, `settings.engineEngineeringTab${item.charAt(0).toUpperCase()}${item.slice(1)}`)}
              </button>
            );
          })}
        </nav>
        <div className="engine-canvas">
          {tab === 'topology' && <TopologyCanvas locale={locale} overview={data.overview} topology={data.topology} />}
          {tab === 'hooks' && <HooksView locale={locale} hooks={data.catalog.hooks ?? []} />}
          {tab === 'prompts' && <PromptsView locale={locale} prompts={data.prompts.blocks ?? []} />}
          {(tab === 'runs' || tab === 'audit') && secondary.loading && <div className="settings-state"><Loader size={18} className="animate-spin" /></div>}
          {(tab === 'runs' || tab === 'audit') && !secondary.loading && secondary.error && (
            <div className="settings-state settings-state-error"><AlertTriangle size={18} /><span>{secondary.error}</span></div>
          )}
          {tab === 'runs' && !secondary.loading && !secondary.error && <RunsView locale={locale} runs={secondary.runs} />}
          {tab === 'audit' && !secondary.loading && !secondary.error && <AuditView locale={locale} entries={secondary.audit} />}
        </div>
      </div>
    </div>
  );
}
