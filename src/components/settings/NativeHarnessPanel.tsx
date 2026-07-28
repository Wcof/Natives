'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { Activity, AlertTriangle, Download, Loader, Plus, RefreshCw, Save, ShieldCheck, Trash2, Workflow, Zap } from 'lucide-react';
import nativesAPI, { type ProjectSummary } from '@/lib/tauri-adapter';
import { classifyError } from '@/lib/error-classifier';
import { t, type Locale } from '@/i18n';

const EVENTS = ['SessionStart', 'UserPromptSubmit', 'PreToolUse', 'PostToolUse', 'PostToolUseFailure', 'PermissionRequest', 'PermissionDenied', 'SubagentStart', 'SubagentStop', 'PreCompact', 'PostCompact', 'Stop', 'StopFailure', 'Error'];
const ADAPTERS = ['command', 'http', 'mcp_tool', 'prompt', 'agent'] as const;
type AdapterType = typeof ADAPTERS[number];
type Profile = { id: string; name: string; kind: string; project_id?: string | null };
type ProjectIdentity = { project_id: string; canonical_path: string; name: string };
type Adapter = { type: AdapterType; [key: string]: unknown };
type NativeHook = {
  id: string; name: string; enabled: boolean; event: string; order: number;
  matcher?: string; conditions: unknown[]; timeout_ms: number; failure_policy: string;
  adapter: Adapter; trust_confirmed: boolean;
};
type Blueprint = {
  schema_version: number; hook_semantics_version: string; prompt_semantics_version: string;
  hooks: HookOverlay[]; hook_overlays?: HookOverlay[]; native_hooks: NativeHook[];
  prompt_blocks: Array<{ id: string; name: string; markdown: string; enabled: boolean; order: number; placement: string }>;
};
type PromptBlock = Blueprint['prompt_blocks'][number];
type HookOverlay = { hook_id: string; enabled?: boolean; order?: number; matcher?: string; timeout_ms?: number; failure_policy?: string };
type CatalogHook = {
  id: string; event: string; stage: string; enabled: boolean; order: number; matcher?: string;
  timeout_ms: number; failure_policy: string; source: { scope: string; origin: string };
};
type Workspace = {
  overview?: { topology_version?: number; blueprint_schema_version?: number; hook_counts?: { enabled?: number; total?: number } };
  topology?: { stages?: Array<{ id: string; order?: number; hook_points?: Array<{ event: string; enabled_hook_count?: number; security_sensitive?: boolean }> }> };
  catalog?: { hooks?: CatalogHook[] };
};
type Review = {
  validation?: { findings?: Array<{ severity?: string; code?: string; message?: string }> };
  diff?: Array<{ field?: string; from?: unknown; to?: unknown }>;
};
type Run = { id: string; conversation_id: string; status: string; provider_id: string; model_id: string; started_at?: string | null; project_id?: string | null };
type AuditEntry = { id: string; action: string; profile_id?: string | null; version_id?: string | null; scope_type?: string | null; scope_id?: string | null; actor: string; created_at: string };
type TraceEntry = { run_id: string; sequence: number; timestamp: string; type?: string; hook_id?: string; status?: string; duration_ms?: number };
type PromptPreview = { blocks?: Array<{ id: string; name: string; order: number; placement: string; source_digest: string; token_estimate: number }>; raw_persisted?: boolean };
type RunSnapshot = { resolved: boolean; canonical_hash?: string; snapshot?: { layers?: unknown[]; enabled_hook_ids?: string[]; prompt_plan?: { token_estimate?: number; source_digests?: string[] } } };
type HarnessSource = { source_id: string; scope?: string; origin?: string; digest: string; mode?: string; status?: string; tracked?: boolean; pinned?: boolean };
type VersionSummary = { id: string; version_number: number; canonical_hash: string; created_at: string };
type DriftCandidate = { source_id: string; observed_digest: string; published_digest?: string; acknowledged?: boolean };
type Tab = 'overview' | 'blueprint' | 'hooks' | 'prompts' | 'runs' | 'audit';
const TABS: Tab[] = ['overview', 'blueprint', 'hooks', 'prompts', 'runs', 'audit'];
const TAB_KEYS = {
  overview: 'settings.engineEngineeringTabOverview',
  blueprint: 'settings.engineEngineeringTabBlueprint',
  hooks: 'settings.engineEngineeringTabHooks',
  prompts: 'settings.engineEngineeringTabPrompts',
  runs: 'settings.engineEngineeringTabRuns',
  audit: 'settings.engineEngineeringTabAudit',
} as const;

const adapterDefaults = (type: AdapterType): Adapter => {
  switch (type) {
    case 'command': return { type, program: '', args: [], working_dir_policy: 'project_root', secret_env_refs: {}, trusted: false, mode: 'exec' };
    case 'http': return { type, url: '', allow_hosts: [], headers: {}, secret_header_refs: {} };
    case 'mcp_tool': return { type, server_id: '', tool_name: '', input_template: '{"payload":"${input}"}' };
    case 'prompt': return { type, template: 'Return JSON: {"decision":"allow|deny","reason":"..."}' };
    case 'agent': return { type, prompt: '', max_steps: 5, readonly_tools: [] };
  }
};

const newHook = (type: AdapterType): NativeHook => ({
  id: crypto.randomUUID(), name: `New ${type} Hook`, enabled: true, event: 'PreToolUse',
  order: 0, matcher: '*', conditions: [], timeout_ms: 10_000, failure_policy: 'fail',
  adapter: adapterDefaults(type), trust_confirmed: false,
});
const newPromptBlock = (): PromptBlock => ({
  id: crypto.randomUUID(), name: 'Prompt Block', markdown: '', enabled: true,
  order: 0, placement: 'after_project_instructions',
});

export interface NativeHarnessPanelProps { locale: Locale }

export function NativeHarnessPanel({ locale }: NativeHarnessPanelProps) {
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [profileId, setProfileId] = useState('');
  const profileIdRef = useRef('');
  const noticeRefreshRef = useRef<number | null>(null);
  const [workspace, setWorkspace] = useState<Workspace | null>(null);
  const [document, setDocument] = useState<Blueprint | null>(null);
  const [revision, setRevision] = useState(0);
  const [review, setReview] = useState<Review | null>(null);
  const [dirty, setDirty] = useState(false);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<Tab>('overview');
  const [projects, setProjects] = useState<ProjectSummary[]>([]);
  const [projectIdentities, setProjectIdentities] = useState<ProjectIdentity[]>([]);
  const [newProfileName, setNewProfileName] = useState('');
  const [newProfileKind, setNewProfileKind] = useState<'global_template' | 'project_overlay'>('project_overlay');
  const [newProfileProjectPath, setNewProfileProjectPath] = useState('');
  const [scopeType, setScopeType] = useState<'global' | 'project' | 'session'>('project');
  const [scopeId, setScopeId] = useState('');
  const [liveRuns, setLiveRuns] = useState<Run[]>([]);
  const [auditEntries, setAuditEntries] = useState<AuditEntry[]>([]);
  const [selectedRunId, setSelectedRunId] = useState('');
  const [traceEntries, setTraceEntries] = useState<TraceEntry[]>([]);
  const [promptPreview, setPromptPreview] = useState<PromptPreview | null>(null);
  const [runSnapshot, setRunSnapshot] = useState<RunSnapshot | null>(null);
  const [sources, setSources] = useState<HarnessSource[]>([]);
  const [versions, setVersions] = useState<VersionSummary[]>([]);
  const [sourceCandidate, setSourceCandidate] = useState<DriftCandidate[]>([]);
  const [auxLoading, setAuxLoading] = useState(false);

  const fail = useCallback((cause: unknown) => {
    setError(classifyError(cause, { locale }).userMessage);
  }, [locale]);

  const load = useCallback(async (requestedProfileId?: string) => {
    setLoading(true); setError(null); setNotice(null); setReview(null);
    try {
      const [listed, identities] = await Promise.all([
        nativesAPI.assistantV2.request<{ profiles?: Profile[] }>('harness.profile.list', {}),
        nativesAPI.assistantV2.request<{ items?: ProjectIdentity[] }>('project.identity.list', {}),
      ]);
      const nextProfiles = listed.profiles ?? [];
      setProjectIdentities(identities.items ?? []);
      const selected = requestedProfileId || profileIdRef.current || nextProfiles[0]?.id || '';
      const selectedProfile = nextProfiles.find((profile) => profile.id === selected);
      const identity = identities.items?.find((item) => item.project_id === selectedProfile?.project_id);
      const scope = identity ? { project_id: identity.project_id, project_path: identity.canonical_path } : {};
      setProfiles(nextProfiles); setProfileId(selected); profileIdRef.current = selected;
      const [nextWorkspace, draft] = await Promise.all([
        nativesAPI.assistantV2.request<Workspace>('harness.workspace.get', selected ? { profile_id: selected, ...scope } : scope),
        selected ? nativesAPI.assistantV2.request<{ revision: number; document: Blueprint; source_candidate?: DriftCandidate[] | null }>('harness.draft.get', { profile_id: selected }) : null,
      ]);
      setWorkspace(nextWorkspace);
      setDocument(draft?.document ?? null);
      setRevision(draft?.revision ?? 0);
      setSourceCandidate((draft?.source_candidate ?? []).filter((candidate) => !candidate.acknowledged));
      setDirty(false);
    } catch (cause) {
      setWorkspace(null); setDocument(null); setSourceCandidate([]); fail(cause);
    } finally { setLoading(false); }
  }, [fail]);

  useEffect(() => { void load(); }, [load]);
  useEffect(() => {
    void nativesAPI.project.list().then(setProjects).catch(fail);
  }, [fail]);

  const loadAuxiliary = useCallback(async (tab: Tab, runId?: string) => {
    if (tab !== 'runs' && tab !== 'audit' && tab !== 'prompts' && tab !== 'hooks' && tab !== 'blueprint') return;
    setAuxLoading(true);
    try {
      const profile = profiles.find((item) => item.id === profileId);
      const identity = projectIdentities.find((item) => item.project_id === profile?.project_id);
      const scope = identity ? { project_id: identity.project_id, project_path: identity.canonical_path } : {};
      if (tab === 'blueprint') {
        if (!profileId) return;
        const result = await nativesAPI.assistantV2.request<{ versions?: VersionSummary[] }>('harness.version.list', { profile_id: profileId, limit: 20 });
        setVersions(result.versions ?? []);
      } else if (tab === 'hooks') {
        const result = await nativesAPI.assistantV2.request<{ sources?: HarnessSource[] }>('harness.source.list', { ...scope, limit: 200 });
        setSources(result.sources ?? []);
      } else if (tab === 'prompts') {
        const result = await nativesAPI.assistantV2.request<PromptPreview>('harness.prompt.preview', identity ? {
          project_id: identity.project_id, project_path: identity.canonical_path,
        } : {});
        setPromptPreview(result);
      } else if (tab === 'runs') {
        const result = await nativesAPI.assistantV2.request<{ runs?: Run[] }>('run.list', {});
        setLiveRuns(result.runs ?? []);
        const selected = runId || selectedRunId || result.runs?.[0]?.id || '';
        setSelectedRunId(selected);
        if (selected) {
          const [trace, snapshot] = await Promise.all([
            nativesAPI.assistantV2.request<{ entries?: TraceEntry[] }>('harness.trace.list', { run_id: selected, limit: 200 }),
            nativesAPI.assistantV2.request<RunSnapshot>('harness.run.getSnapshot', { run_id: selected }),
          ]);
          setTraceEntries(trace.entries ?? []);
          setRunSnapshot(snapshot);
        } else { setTraceEntries([]); setRunSnapshot(null); }
      } else {
        const result = await nativesAPI.assistantV2.request<{ entries?: AuditEntry[] }>('harness.audit.list', { limit: 200 });
        setAuditEntries(result.entries ?? []);
      }
    } catch (cause) { fail(cause); } finally { setAuxLoading(false); }
  }, [fail, profileId, profiles, projectIdentities, selectedRunId]);

  useEffect(() => { void loadAuxiliary(activeTab); }, [activeTab, loadAuxiliary]);
  useEffect(() => {
    const unsubscribe = nativesAPI.assistantV2.subscribeHarness((event) => {
      if (event.kind === 'trace_updated' && activeTab === 'runs') {
        if (noticeRefreshRef.current != null) return;
        noticeRefreshRef.current = window.setTimeout(() => {
          noticeRefreshRef.current = null;
          void loadAuxiliary('runs');
        }, 50);
      } else if (!dirty && event.kind !== 'trace_updated') void load(profileIdRef.current);
      else if (dirty && event.kind !== 'trace_updated') setNotice(t(locale, 'settings.engineEngineeringRemoteChange'));
    }, { onError: fail });
    return () => {
      unsubscribe();
      if (noticeRefreshRef.current != null) window.clearTimeout(noticeRefreshRef.current);
      noticeRefreshRef.current = null;
    };
  }, [activeTab, dirty, fail, load, loadAuxiliary, locale]);

  const replaceDocument = (next: Blueprint) => {
    setDocument(next); setDirty(true); setReview(null); setNotice(null);
  };
  const updateHook = (index: number, patch: Partial<NativeHook>) => {
    if (!document) return;
    const native_hooks = [...document.native_hooks];
    const current = native_hooks[index];
    if (!current) return;
    native_hooks[index] = { ...current, ...patch };
    replaceDocument({ ...document, native_hooks });
  };
  const updateAdapter = (index: number, patch: Record<string, unknown>) => {
    const hook = document?.native_hooks[index];
    if (hook) updateHook(index, { adapter: { ...hook.adapter, ...patch } });
  };

  const persist = async () => {
    if (!document || !profileId) throw new Error('No Harness draft selected');
    const saved = await nativesAPI.assistantV2.request<{ revision: number }>('harness.draft.save', { profile_id: profileId, revision, document });
    setRevision(saved.revision);
    setDirty(false);
    return saved.revision;
  };

  const save = async () => {
    setBusy(true); setError(null); setNotice(null);
    try { await persist(); setNotice(t(locale, 'settings.engineEngineeringSaved')); }
    catch (cause) { fail(cause); } finally { setBusy(false); }
  };

  const reviewDraft = async () => {
    if (!profileId) return;
    setBusy(true); setError(null); setNotice(null);
    try {
      await persist();
      const result = await nativesAPI.assistantV2.request<Review>('harness.draft.review', { profile_id: profileId });
      setReview(result);
    } catch (cause) { fail(cause); } finally { setBusy(false); }
  };

  const publish = async () => {
    if (!profileId) return;
    setBusy(true); setError(null); setNotice(null);
    try {
      await nativesAPI.assistantV2.request('harness.draft.publish', { profile_id: profileId, revision });
      await load(profileId);
      setNotice(t(locale, 'settings.engineEngineeringPublished'));
    } catch (cause) { fail(cause); } finally { setBusy(false); }
  };

  const createProfile = async () => {
    if (!newProfileName.trim()) return;
    setBusy(true); setError(null);
    try {
      let projectId: string | undefined;
      if (newProfileKind === 'project_overlay') {
        const project = projects.find((item) => item.path === newProfileProjectPath);
        if (!project) throw new Error(t(locale, 'settings.engineEngineeringProjectRequired'));
        const identity = await nativesAPI.assistantV2.request<{ project_id: string }>('project.identity.register', { path: project.path, name: project.label });
        projectId = identity.project_id;
      }
      const result = await nativesAPI.assistantV2.request<{ profile: Profile }>('harness.profile.create', {
        name: newProfileName.trim(), kind: newProfileKind, project_id: projectId,
      });
      if (projectId) {
        setScopeType('project');
        setScopeId(newProfileProjectPath);
      }
      setNewProfileName('');
      await load(result.profile.id);
      setNotice(t(locale, 'settings.engineEngineeringProfileCreated'));
    } catch (cause) { fail(cause); } finally { setBusy(false); }
  };

  const bindProfile = async () => {
    if (!profileId) return;
    setBusy(true); setError(null);
    try {
      let resolvedScopeId = scopeType === 'global' ? 'global' : scopeId.trim();
      if (scopeType === 'project') {
        const project = projects.find((item) => item.path === scopeId);
        if (!project) throw new Error(t(locale, 'settings.engineEngineeringProjectRequired'));
        const identity = await nativesAPI.assistantV2.request<{ project_id: string }>('project.identity.register', { path: project.path, name: project.label });
        resolvedScopeId = identity.project_id;
      }
      await nativesAPI.assistantV2.request('harness.binding.set', {
        scope_type: scopeType, scope_id: resolvedScopeId, profile_id: profileId, mode: 'follow_published',
      });
      setNotice(t(locale, 'settings.engineEngineeringBound'));
    } catch (cause) { fail(cause); } finally { setBusy(false); }
  };

  const exportAudit = async () => {
    setBusy(true); setError(null);
    try {
      const result = await nativesAPI.assistantV2.request<{ entries?: AuditEntry[] }>('harness.audit.export', { limit: 500, format: 'json' });
      const path = await nativesAPI.dialog.saveFile();
      if (!path) return;
      await nativesAPI.fs.writeFileAtomic(path, JSON.stringify({ redacted: true, entries: result.entries ?? [] }, null, 2));
      setNotice(t(locale, 'settings.engineEngineeringExported'));
    } catch (cause) { fail(cause); } finally { setBusy(false); }
  };

  const rollback = async (versionId: string) => {
    setBusy(true); setError(null);
    try {
      await nativesAPI.assistantV2.request('harness.version.rollback', {
        version_id: versionId,
        expected_current_version_id: versions[0]?.id,
      });
      await load(profileId);
      await loadAuxiliary('blueprint');
      setNotice(t(locale, 'settings.engineEngineeringRolledBack'));
    } catch (cause) { fail(cause); } finally { setBusy(false); }
  };

  const acknowledgeDrift = async () => {
    if (!profileId || sourceCandidate.length === 0) return;
    setBusy(true); setError(null);
    try {
      let nextRevision = revision;
      for (const candidate of sourceCandidate) {
        const result = await nativesAPI.assistantV2.request<{ revision: number }>('harness.source.acknowledgeDrift', {
          profile_id: profileId,
          source_id: candidate.source_id,
          observed_digest: candidate.observed_digest,
          revision: nextRevision,
        });
        nextRevision = result.revision;
      }
      await load(profileId);
      setNotice(t(locale, 'settings.engineEngineeringDriftAcknowledged'));
    } catch (cause) { fail(cause); } finally { setBusy(false); }
  };

  const stages = [...(workspace?.topology?.stages ?? [])].sort((a, b) => (a.order ?? 0) - (b.order ?? 0));
  const findings = review?.validation?.findings ?? [];
  const discardAndLoad = (nextProfileId: string) => {
    if (!dirty || window.confirm(t(locale, 'settings.engineEngineeringDiscardConfirm'))) void load(nextProfileId);
  };
  const importedHooks = (workspace?.catalog?.hooks ?? []).filter((hook) => !hook.source.origin.startsWith('native:'));
  const overlays = document?.hook_overlays ?? document?.hooks ?? [];
  const updateOverlay = (hook: CatalogHook, patch: Partial<HookOverlay>) => {
    if (!document) return;
    const existing = overlays.find((item) => item.hook_id === hook.id);
    const next = existing ? { ...existing, ...patch } : { hook_id: hook.id, ...patch };
    replaceDocument({ ...document, hook_overlays: [...overlays.filter((item) => item.hook_id !== hook.id), next] });
  };

  return (
    <section className="space-y-5" data-testid="native-harness-panel">
      <header className="settings-section-card flex flex-wrap items-start justify-between gap-4">
        <div>
          <p className="settings-section-eyebrow">{t(locale, 'settings.engineEngineeringEyebrow')}</p>
          <h3>{t(locale, 'settings.engineEngineeringTitle')}</h3>
          <p className="text-sm text-[var(--text-muted)]">{t(locale, 'settings.engineEngineeringDesc')}</p>
        </div>
        <div className="flex flex-wrap gap-2">
          <select className="input" value={profileId} disabled={loading || busy} onChange={(event) => discardAndLoad(event.target.value)}>
            {profiles.map((profile) => <option key={profile.id} value={profile.id}>{profile.name} · {profile.kind}</option>)}
          </select>
          <button type="button" className="btn" onClick={() => discardAndLoad(profileId)} disabled={loading || busy}><RefreshCw size={14} />{t(locale, 'common.refresh')}</button>
          <button type="button" className="btn" onClick={() => void save()} disabled={!document || busy}><Save size={14} />{t(locale, 'settings.engineEngineeringSave')}</button>
          <button type="button" className="btn" onClick={() => void reviewDraft()} disabled={!document || busy}>{t(locale, 'settings.engineEngineeringReview')}</button>
          <button type="button" className="btn btn-primary" onClick={() => void publish()} disabled={dirty || !review || findings.some((item) => item.severity === 'error') || busy}>{t(locale, 'settings.engineEngineeringPublish')}</button>
        </div>
      </header>

      {error ? <div role="alert" className="rounded-lg border border-[var(--danger)] p-4 text-sm text-[var(--danger)]"><AlertTriangle size={16} className="mr-2 inline" />{error}</div> : null}
      {notice ? <div role="status" className="rounded-lg border border-[var(--success)] p-4 text-sm text-[var(--success)]">{notice}</div> : null}
      {loading ? <div className="engine-empty"><Loader size={18} className="animate-spin" />{t(locale, 'common.loading')}</div> : null}

      {!loading ? <div className="settings-section-card space-y-3">
        <div className="grid gap-2 lg:grid-cols-4">
          <input className="input" value={newProfileName} placeholder={t(locale, 'settings.engineEngineeringProfileName')} onChange={(event) => setNewProfileName(event.target.value)} />
          <select className="input" value={newProfileKind} onChange={(event) => setNewProfileKind(event.target.value as typeof newProfileKind)}>
            <option value="project_overlay">{t(locale, 'settings.engineEngineeringProjectOverlay')}</option>
            <option value="global_template">{t(locale, 'settings.engineEngineeringGlobalTemplate')}</option>
          </select>
          {newProfileKind === 'project_overlay' ? <select className="input" value={newProfileProjectPath} onChange={(event) => setNewProfileProjectPath(event.target.value)}>
            <option value="">{t(locale, 'settings.engineEngineeringSelectProject')}</option>
            {projects.filter((project) => project.exists).map((project) => <option key={project.id} value={project.path}>{project.label}</option>)}
          </select> : <div />}
          <button type="button" className="btn" disabled={busy || !newProfileName.trim()} onClick={() => void createProfile()}><Plus size={14} />{t(locale, 'settings.engineEngineeringCreateProfile')}</button>
        </div>
        <div className="grid gap-2 lg:grid-cols-4">
          <select className="input" value={scopeType} onChange={(event) => setScopeType(event.target.value as typeof scopeType)}>
            <option value="global">{t(locale, 'settings.engineEngineeringScopeGlobal')}</option>
            <option value="project">{t(locale, 'settings.engineEngineeringScopeProject')}</option>
            <option value="session">{t(locale, 'settings.engineEngineeringScopeSession')}</option>
          </select>
          {scopeType === 'project' ? <select className="input lg:col-span-2" value={scopeId} onChange={(event) => setScopeId(event.target.value)}>
            <option value="">{t(locale, 'settings.engineEngineeringSelectProject')}</option>
            {projects.filter((project) => project.exists).map((project) => <option key={project.id} value={project.path}>{project.label}</option>)}
          </select> : scopeType === 'session' ? <input className="input lg:col-span-2" value={scopeId} placeholder={t(locale, 'settings.engineEngineeringConversationId')} onChange={(event) => setScopeId(event.target.value)} /> : <div className="lg:col-span-2" />}
          <button type="button" className="btn" disabled={busy || !profileId || (scopeType !== 'global' && !scopeId.trim())} onClick={() => void bindProfile()}>{t(locale, 'settings.engineEngineeringBindProfile')}</button>
        </div>
      </div> : null}

      {!loading ? <nav className="flex gap-2 overflow-x-auto" aria-label={t(locale, 'settings.engineEngineeringTitle')}>
        {TABS.map((tab) => <button type="button" key={tab} className={activeTab === tab ? 'btn btn-primary' : 'btn'} onClick={() => setActiveTab(tab)}>{t(locale, TAB_KEYS[tab])}</button>)}
      </nav> : null}
      {sourceCandidate.length ? <div role="alert" className="settings-section-card flex flex-wrap items-center justify-between gap-3 border-[var(--warning)]">
        <div><strong>{t(locale, 'settings.engineEngineeringDriftTitle')}</strong><p className="text-xs text-[var(--text-muted)]">{t(locale, 'settings.engineEngineeringDriftDesc', { count: sourceCandidate.length })}</p></div>
        <button type="button" className="btn" disabled={busy} onClick={() => void acknowledgeDrift()}>{t(locale, 'settings.engineEngineeringDriftAcknowledge')}</button>
      </div> : null}

      {!loading && workspace && activeTab === 'overview' ? <>
        <div className="grid gap-3 md:grid-cols-3">
          <article className="settings-section-card"><Workflow size={18} /><strong>{workspace.overview?.topology_version ?? '—'}</strong><span>{t(locale, 'settings.engineEngineeringTopologyVersion')}</span></article>
          <article className="settings-section-card"><Zap size={18} /><strong>{workspace.overview?.hook_counts?.enabled ?? 0}/{workspace.overview?.hook_counts?.total ?? 0}</strong><span>{t(locale, 'settings.engineEngineeringActiveHooks')}</span></article>
          <article className="settings-section-card"><Activity size={18} /><strong>v{workspace.overview?.blueprint_schema_version ?? '—'}</strong><span>{t(locale, 'settings.engineEngineeringSchema')}</span></article>
        </div>
      </> : null}
      {!loading && workspace && activeTab === 'blueprint' ? <>
        <div className="settings-section-card space-y-3">
          <h4 className="inline-flex items-center gap-2"><Workflow size={16} />{t(locale, 'settings.engineEngineeringTopology')}</h4>
          <div className="flex gap-2 overflow-x-auto pb-2">{stages.map((stage) => <div className="min-w-48 rounded border border-[var(--border)] p-3" key={stage.id}>
            <strong>{stage.id}</strong>
            <div className="mt-2 space-y-1">{(stage.hook_points ?? []).map((point) => <div className="text-xs" key={point.event}>{point.security_sensitive ? <ShieldCheck size={12} className="mr-1 inline" /> : null}{point.event} · {point.enabled_hook_count ?? 0}</div>)}</div>
          </div>)}</div>
        </div>
      </> : null}

      {!loading && document && activeTab === 'hooks' ? <>
      <div className="settings-section-card space-y-3">
        <h4>{t(locale, 'settings.engineEngineeringSources')}</h4>
        {sources.map((source) => <div className="grid gap-2 rounded border border-[var(--border)] p-3 text-xs lg:grid-cols-[1fr_10rem_8rem]" key={source.source_id}>
          <div><strong>{source.origin ?? source.source_id}</strong><div className="font-mono text-[var(--text-muted)]">{source.digest.slice(0, 16)}…</div></div>
          <span>{source.mode ?? (source.pinned ? 'pinned' : 'tracked')}</span>
          <span className={source.status === 'drifted' ? 'text-[var(--danger)]' : 'text-[var(--success)]'}>{source.status ?? 'current'}</span>
        </div>)}
        {sources.length === 0 ? <div className="engine-empty">{t(locale, 'settings.engineEngineeringEmpty')}</div> : null}
      </div>
      <div className="settings-section-card space-y-4">
        <div><h4>{t(locale, 'settings.engineEngineeringImportedHooks')}</h4><p className="text-xs text-[var(--text-muted)]">{t(locale, 'settings.engineEngineeringImportedReadOnly')}</p></div>
        {importedHooks.length === 0 ? <div className="engine-empty">{t(locale, 'settings.engineEngineeringEmpty')}</div> : importedHooks.map((hook) => {
          const overlay = overlays.find((item) => item.hook_id === hook.id);
          return <article className="rounded-lg border border-[var(--border)] p-3" key={hook.id}>
            <div className="grid items-center gap-2 lg:grid-cols-6">
              <div className="lg:col-span-2"><strong>{hook.event}</strong><div className="text-xs text-[var(--text-muted)]">{hook.source.origin}</div></div>
              <input className="input" aria-label="Imported matcher" value={overlay?.matcher ?? hook.matcher ?? ''} onChange={(event) => updateOverlay(hook, { matcher: event.target.value })} />
              <input className="input" aria-label="Imported order" type="number" value={overlay?.order ?? hook.order} onChange={(event) => updateOverlay(hook, { order: Number(event.target.value) })} />
              <input className="input" aria-label="Imported timeout" type="number" min={1000} value={overlay?.timeout_ms ?? hook.timeout_ms} onChange={(event) => updateOverlay(hook, { timeout_ms: Number(event.target.value) })} />
              <label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={overlay?.enabled ?? hook.enabled} onChange={(event) => updateOverlay(hook, { enabled: event.target.checked })} />{t(locale, 'common.enabled')}</label>
            </div>
          </article>;
        })}
      </div>
      <div className="settings-section-card space-y-4">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div><h4>{t(locale, 'settings.engineEngineeringTabHooks')}</h4><p className="text-xs text-[var(--text-muted)]">Draft r{revision}{dirty ? ' · *' : ''} · {document.native_hooks.length} Hooks</p></div>
          <div className="flex flex-wrap gap-2">{ADAPTERS.map((type) => <button key={type} type="button" className="btn" onClick={() => replaceDocument({ ...document, native_hooks: [...document.native_hooks, newHook(type)] })}><Plus size={13} />{type}</button>)}</div>
        </div>
        {document.native_hooks.length === 0 ? <div className="engine-empty">{t(locale, 'settings.engineEngineeringEmpty')}</div> : document.native_hooks.map((hook, index) => (
          <article className="rounded-lg border border-[var(--border)] p-4" key={hook.id}>
            <div className="grid gap-3 lg:grid-cols-6">
              <input aria-label="Hook name" className="input lg:col-span-2" value={hook.name} onChange={(e) => updateHook(index, { name: e.target.value })} />
              <select aria-label="Hook event" className="input" value={hook.event} onChange={(e) => updateHook(index, { event: e.target.value })}>{EVENTS.map((event) => <option key={event}>{event}</option>)}</select>
              <select aria-label="Hook adapter" className="input" value={hook.adapter.type} onChange={(e) => updateHook(index, { adapter: adapterDefaults(e.target.value as AdapterType), trust_confirmed: false })}>{ADAPTERS.map((type) => <option key={type}>{type}</option>)}</select>
              <input aria-label="Matcher" className="input" value={hook.matcher ?? ''} placeholder="matcher" onChange={(e) => updateHook(index, { matcher: e.target.value })} />
              <button aria-label="Delete Hook" type="button" className="btn text-[var(--danger)]" onClick={() => replaceDocument({ ...document, native_hooks: document.native_hooks.filter((_, item) => item !== index) })}><Trash2 size={14} /></button>
            </div>
            <div className="mt-3 grid gap-3 lg:grid-cols-4">
              {hook.adapter.type === 'command' ? <>
                <input className="input" placeholder="program" value={String(hook.adapter.program ?? '')} onChange={(e) => updateAdapter(index, { program: e.target.value })} />
                <input className="input" placeholder="args (one per line)" value={(hook.adapter.args as string[] ?? []).join('\n')} onChange={(e) => updateAdapter(index, { args: e.target.value.split('\n').filter(Boolean) })} />
                <label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={Boolean(hook.adapter.trusted) && hook.trust_confirmed} onChange={(e) => updateHook(index, { adapter: { ...hook.adapter, trusted: e.target.checked }, trust_confirmed: e.target.checked })} />{t(locale, 'settings.engineEngineeringTrust')}</label>
              </> : null}
              {hook.adapter.type === 'http' ? <>
                <input className="input lg:col-span-2" placeholder="https://…" value={String(hook.adapter.url ?? '')} onChange={(e) => updateAdapter(index, { url: e.target.value })} />
                <input className="input" placeholder="allowed hosts, comma separated" value={(hook.adapter.allow_hosts as string[] ?? []).join(',')} onChange={(e) => updateAdapter(index, { allow_hosts: e.target.value.split(',').map((v) => v.trim()).filter(Boolean) })} />
              </> : null}
              {hook.adapter.type === 'mcp_tool' ? <>
                <input className="input" placeholder="MCP server id" value={String(hook.adapter.server_id ?? '')} onChange={(e) => updateAdapter(index, { server_id: e.target.value })} />
                <input className="input" placeholder="tool name" value={String(hook.adapter.tool_name ?? '')} onChange={(e) => updateAdapter(index, { tool_name: e.target.value })} />
                <textarea className="input lg:col-span-2" placeholder="JSON input template" value={String(hook.adapter.input_template ?? '')} onChange={(e) => updateAdapter(index, { input_template: e.target.value })} />
              </> : null}
              {hook.adapter.type === 'prompt' ? <>
                <textarea className="input lg:col-span-3" placeholder="decision prompt" value={String(hook.adapter.template ?? '')} onChange={(e) => updateAdapter(index, { template: e.target.value })} />
                <input className="input" placeholder="model override (optional)" value={String(hook.adapter.model_override ?? '')} onChange={(e) => updateAdapter(index, { model_override: e.target.value || undefined })} />
              </> : null}
              {hook.adapter.type === 'agent' ? <>
                <textarea className="input lg:col-span-2" placeholder="agent task" value={String(hook.adapter.prompt ?? '')} onChange={(e) => updateAdapter(index, { prompt: e.target.value })} />
                <input className="input" type="number" min={1} max={32} value={Number(hook.adapter.max_steps ?? 5)} onChange={(e) => updateAdapter(index, { max_steps: Number(e.target.value) })} />
                <input className="input" placeholder="readonly tools, comma separated" value={(hook.adapter.readonly_tools as string[] ?? []).join(',')} onChange={(e) => updateAdapter(index, { readonly_tools: e.target.value.split(',').map((v) => v.trim()).filter(Boolean) })} />
              </> : null}
              <input className="input" type="number" min={100} value={hook.timeout_ms} onChange={(e) => updateHook(index, { timeout_ms: Number(e.target.value) })} />
              <select className="input" value={hook.failure_policy} onChange={(e) => updateHook(index, { failure_policy: e.target.value })}><option value="fail">fail</option><option value="skip">skip</option><option value="default">default</option></select>
              <label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={hook.enabled} onChange={(e) => updateHook(index, { enabled: e.target.checked })} />{t(locale, 'common.enabled')}</label>
            </div>
          </article>
        ))}
      </div></> : null}

      {!loading && document && activeTab === 'prompts' ? <div className="settings-section-card space-y-4">
        <div className="flex items-center justify-between gap-3">
          <h4>{t(locale, 'settings.engineEngineeringTabPrompts')}</h4>
          <button type="button" className="btn" onClick={() => replaceDocument({ ...document, prompt_blocks: [...document.prompt_blocks, newPromptBlock()] })}><Plus size={13} />{t(locale, 'settings.engineEngineeringAddPromptBlock')}</button>
        </div>
        {document.prompt_blocks.length === 0 ? <div className="engine-empty">{t(locale, 'settings.engineEngineeringEmpty')}</div> : document.prompt_blocks.map((block, index) => {
          const update = (patch: Partial<PromptBlock>) => {
            const prompt_blocks = [...document.prompt_blocks];
            prompt_blocks[index] = { ...block, ...patch };
            replaceDocument({ ...document, prompt_blocks });
          };
          return <article className="rounded-lg border border-[var(--border)] p-4" key={block.id}>
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
      </div> : null}
      {!loading && activeTab === 'prompts' ? <div className="settings-section-card space-y-3">
        <div className="flex items-center justify-between"><div><h4>{t(locale, 'settings.engineEngineeringPromptPlan')}</h4><p className="text-xs text-[var(--text-muted)]">{t(locale, 'settings.engineEngineeringPromptPlanDesc')}</p></div><button type="button" className="btn" onClick={() => void loadAuxiliary('prompts')}><RefreshCw size={13} /></button></div>
        {(promptPreview?.blocks ?? []).map((block, index) => <div className="grid gap-2 rounded border border-[var(--border)] p-3 text-xs lg:grid-cols-[3rem_1fr_1fr_8rem]" key={block.id}>
          <strong>#{index + 1}</strong><span>{block.name} · {block.placement}</span><code>{block.source_digest.slice(0, 16)}…</code><span>{block.token_estimate} tokens</span>
        </div>)}
        {(promptPreview?.blocks ?? []).length === 0 ? <div className="engine-empty">{t(locale, 'settings.engineEngineeringEmpty')}</div> : null}
      </div> : null}

      {review && activeTab === 'blueprint' ? <div className="settings-section-card space-y-2"><h4>{t(locale, 'settings.engineEngineeringReview')}</h4>
        {findings.length ? findings.map((item, index) => <div key={`${item.code}-${index}`} className={item.severity === 'error' ? 'text-[var(--danger)]' : 'text-[var(--text-muted)]'}>{item.severity} · {item.code} · {item.message}</div>) : <div className="text-[var(--success)]">{t(locale, 'settings.engineEngineeringPublishable')}</div>}
        <div className="pt-2 text-sm text-[var(--text-muted)]">{t(locale, 'settings.engineEngineeringChanges', { count: review.diff?.length ?? 0 })}</div>
        {review.diff?.map((change, index) => <div className="rounded border border-[var(--border)] p-2 text-xs" key={`${change.field}-${index}`}><strong>{change.field ?? 'change'}</strong></div>)}
      </div> : null}
      {!loading && activeTab === 'blueprint' ? <div className="settings-section-card space-y-2">
        <h4>{t(locale, 'settings.engineEngineeringVersions')}</h4>
        {versions.map((version, index) => <div className="grid items-center gap-2 rounded border border-[var(--border)] p-3 text-xs lg:grid-cols-[6rem_1fr_12rem_8rem]" key={version.id}>
          <strong>v{version.version_number}</strong><code>{version.canonical_hash.slice(0, 20)}…</code><time>{version.created_at}</time><button type="button" className="btn" disabled={busy || index === 0} onClick={() => void rollback(version.id)}>{t(locale, 'settings.engineEngineeringRollback')}</button>
        </div>)}
        {versions.length === 0 ? <div className="engine-empty">{t(locale, 'settings.engineEngineeringEmpty')}</div> : null}
      </div> : null}

      {!loading && activeTab === 'runs' ? <div className="grid gap-4 lg:grid-cols-[minmax(18rem,0.7fr)_minmax(24rem,1.3fr)]">
        <div className="settings-section-card space-y-2">
          <div className="flex items-center justify-between"><h4>{t(locale, 'settings.engineEngineeringTabRuns')}</h4><button type="button" className="btn" onClick={() => void loadAuxiliary('runs')}><RefreshCw size={13} /></button></div>
          {auxLoading ? <Loader size={16} className="animate-spin" /> : liveRuns.map((run) => <button type="button" key={run.id} className={`w-full rounded border p-3 text-left ${selectedRunId === run.id ? 'border-[var(--accent)]' : 'border-[var(--border)]'}`} onClick={() => void loadAuxiliary('runs', run.id)}>
            <strong>{run.status}</strong><div className="text-xs text-[var(--text-muted)]">{run.id}</div><div className="text-xs">{run.provider_id} · {run.model_id}</div>
          </button>)}
          {!auxLoading && liveRuns.length === 0 ? <div className="engine-empty">{t(locale, 'settings.engineEngineeringEmpty')}</div> : null}
        </div>
        <div className="settings-section-card space-y-2">
          <h4>{t(locale, 'settings.engineEngineeringHookTrace')}</h4>
          {runSnapshot ? <div className="rounded border border-[var(--border)] p-3 text-xs">
            <strong>{runSnapshot.resolved ? t(locale, 'settings.engineEngineeringSnapshotFrozen') : t(locale, 'settings.engineEngineeringSnapshotMissing')}</strong>
            {runSnapshot.canonical_hash ? <div className="mt-1 font-mono text-[var(--text-muted)]">{runSnapshot.canonical_hash}</div> : null}
            {runSnapshot.snapshot?.prompt_plan ? <div className="mt-1">{runSnapshot.snapshot.prompt_plan.token_estimate ?? 0} tokens · {runSnapshot.snapshot.prompt_plan.source_digests?.length ?? 0} prompt sources</div> : null}
          </div> : null}
          {traceEntries.map((entry, index) => <div className="rounded border border-[var(--border)] p-3 text-xs" key={`${entry.run_id}-${entry.sequence}-${index}`}>
            <strong>#{entry.sequence} · {entry.type ?? entry.status ?? 'hook'}</strong><div>{entry.hook_id ?? '—'}{entry.duration_ms != null ? ` · ${entry.duration_ms}ms` : ''}</div><div className="text-[var(--text-muted)]">{entry.timestamp}</div>
          </div>)}
          {traceEntries.length === 0 ? <div className="engine-empty">{t(locale, 'settings.engineEngineeringEmpty')}</div> : null}
        </div>
      </div> : null}

      {!loading && activeTab === 'audit' ? <div className="settings-section-card space-y-3">
        <div className="flex items-center justify-between"><h4>{t(locale, 'settings.engineEngineeringTabAudit')}</h4><div className="flex gap-2"><button type="button" className="btn" onClick={() => void loadAuxiliary('audit')}><RefreshCw size={13} /></button><button type="button" className="btn" disabled={busy} onClick={() => void exportAudit()}><Download size={13} />{t(locale, 'settings.engineEngineeringExport')}</button></div></div>
        {auxLoading ? <Loader size={16} className="animate-spin" /> : auditEntries.map((entry) => <div className="grid gap-1 rounded border border-[var(--border)] p-3 text-xs lg:grid-cols-4" key={entry.id}>
          <strong>{entry.action}</strong><span>{entry.profile_id ?? '—'}</span><span>{entry.scope_type ? `${entry.scope_type}:${entry.scope_id}` : '—'}</span><time>{entry.created_at}</time>
        </div>)}
        {!auxLoading && auditEntries.length === 0 ? <div className="engine-empty">{t(locale, 'settings.engineEngineeringEmpty')}</div> : null}
      </div> : null}
    </section>
  );
}

export default NativeHarnessPanel;
