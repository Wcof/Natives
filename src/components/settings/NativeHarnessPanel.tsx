'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { AlertTriangle, ArrowLeft, Eye, Loader, Pencil, Plus, RefreshCw, Rocket, Save, Search, Star, Trash2, Workflow } from 'lucide-react';
import nativesAPI, { type ProjectSummary, type ProviderSummary } from '@/lib/tauri-adapter';
import { classifyError } from '@/lib/error-classifier';
import { t, type Locale } from '@/i18n';
import {
  NativeExecutionCanvas,
  runStatusLabel,
  stageLabel,
  traceEntriesForStage,
  type CanvasEdge,
  type CanvasMode,
  type CanvasNodeDetail,
  type CanvasRunSnapshot,
  type CanvasStage,
  type CanvasTraceEntry,
  type CanvasWorkspaceTarget,
} from './NativeExecutionCanvas';
import EngineCapabilitiesPanel, { type EngineCapabilitySnapshot } from './EngineCapabilitiesPanel';

const EVENTS = ['SessionStart', 'SessionEnd', 'UserPromptSubmit', 'PreToolUse', 'PostToolUse', 'PostToolUseFailure', 'PermissionRequest', 'PermissionDenied', 'Notification', 'SubagentStart', 'SubagentStop', 'PreCompact', 'PostCompact', 'Stop', 'StopFailure', 'Error'];
const ADAPTERS = ['command', 'http', 'mcp_tool', 'prompt', 'agent'] as const;
const ACTIVE_RUN_STATUSES = new Set(['created', 'queued', 'preparing', 'running', 'waiting_permission', 'waiting_subagent', 'cancelling']);
const BUILTIN_TOOL_NAMES = ['read_file', 'search_files', 'write_file', 'list_dir', 'grep', 'edit_file', 'run_terminal', 'apply_patch', 'memory_search', 'memory_get', 'task', 'task_output', 'kill_task', 'skill', 'web_search', 'enter_plan_mode', 'exit_plan_mode', 'write_draft_module', 'read_draft_module', 'rollback_draft_revision', 'lint_draft_module'];

type AdapterType = typeof ADAPTERS[number];
type DetailMode = 'preview' | 'edit' | 'create';
type WorkspaceTarget = CanvasWorkspaceTarget | 'preview' | 'versions';
type Profile = { id: string; name: string; kind: string; project_id?: string | null };
type ProjectIdentity = { project_id: string; canonical_path: string; name: string };
type Adapter = { type: AdapterType; [key: string]: unknown };
type NativeHook = {
  id: string; name: string; enabled: boolean; event: string; order: number;
  matcher?: string; conditions: unknown[]; timeout_ms: number; failure_policy: string;
  adapter: Adapter; trust_confirmed: boolean;
};
type HookOverlay = { hook_id: string; enabled?: boolean; order?: number; matcher?: string; timeout_ms?: number; failure_policy?: string };
type Blueprint = {
  schema_version: number; hook_semantics_version: string; prompt_semantics_version: string;
  hooks: HookOverlay[]; hook_overlays?: HookOverlay[]; native_hooks: NativeHook[];
  prompt_blocks: Array<{ id: string; name: string; markdown: string; enabled: boolean; order: number; placement: string }>;
  builtin_prompt_replacements?: Array<{ surface_id: string; markdown: string; base_default_digest: string }>;
};
type PromptBlock = Blueprint['prompt_blocks'][number];
type CatalogHook = {
  id: string; event: string; stage: string; enabled: boolean; order: number; matcher?: string;
  timeout_ms: number; failure_policy: string; source: { scope: string; origin: string };
};
type Workspace = {
  overview?: { issues?: unknown[] };
  topology?: { stages?: CanvasStage[]; edges?: CanvasEdge[] };
  catalog?: { hooks?: CatalogHook[] };
  prompt_plan?: { blocks?: Array<{ id: string; name: string }> };
};
type Review = {
  validation?: { findings?: Array<{ severity?: string; code?: string; message?: string }> };
  diff?: Array<{ field?: string; from?: unknown; to?: unknown }>;
};
type Run = {
  id: string;
  conversation_id: string;
  status: string;
  provider_id: string;
  model_id: string;
  permission_profile: string;
  runtime_id?: string | null;
  agent_profile_id?: string | null;
  capability_snapshot?: EngineCapabilitySnapshot | null;
  started_at?: string | null;
  finished_at?: string | null;
  error_code?: string | null;
  project_id?: string | null;
};
type TraceEntry = CanvasTraceEntry;
type PromptPreview = {
  blocks?: Array<{ id: string; name: string; order: number; placement: string; source_digest: string; token_estimate: number }>;
  builtin_surfaces?: Array<{ surface_id: string; default_markdown: string; default_digest: string; effective_digest: string; replaced: boolean }>;
};
type RunSnapshot = CanvasRunSnapshot;
type VersionSummary = { id: string; version_number: number; canonical_hash: string; created_at: string };
type DriftCandidate = { source_id: string; observed_digest: string; acknowledged?: boolean };

const emptyBlueprint = (): Blueprint => ({
  schema_version: 4,
  hook_semantics_version: 'native.hooks.v1',
  prompt_semantics_version: 'native.prompts.v1',
  hooks: [],
  hook_overlays: [],
  native_hooks: [],
  prompt_blocks: [],
  builtin_prompt_replacements: [],
});

const normalizeBlueprint = (raw?: Partial<Blueprint> | null): Blueprint => ({
  ...emptyBlueprint(),
  ...(raw ?? {}),
  hooks: Array.isArray(raw?.hooks) ? raw.hooks : [],
  hook_overlays: Array.isArray(raw?.hook_overlays) ? raw.hook_overlays : Array.isArray(raw?.hooks) ? raw.hooks : [],
  native_hooks: Array.isArray(raw?.native_hooks) ? raw.native_hooks : [],
  prompt_blocks: Array.isArray(raw?.prompt_blocks) ? raw.prompt_blocks : [],
  builtin_prompt_replacements: Array.isArray(raw?.builtin_prompt_replacements) ? raw.builtin_prompt_replacements : [],
});

const adapterDefaults = (type: AdapterType): Adapter => {
  switch (type) {
    case 'command': return { type, program: '', args: [], working_dir_policy: 'project_root', secret_env_refs: {}, trusted: false, mode: 'exec' };
    case 'http': return { type, url: '', allow_hosts: [], headers: {}, secret_header_refs: {} };
    case 'mcp_tool': return { type, server_id: '', tool_name: '', input_template: '{"payload":"${input}"}' };
    case 'prompt': return { type, template: 'Return JSON: {"decision":"allow|deny","reason":"..."}' };
    case 'agent': return { type, prompt: '', max_steps: 5, readonly_tools: [] };
  }
};

const newHook = (type: AdapterType, event = 'PreToolUse'): NativeHook => ({
  id: crypto.randomUUID(), name: `New ${type} Hook`, enabled: true, event,
  order: 0, matcher: '*', conditions: [], timeout_ms: 10_000, failure_policy: 'fail',
  adapter: adapterDefaults(type), trust_confirmed: false,
});

const newPromptBlock = (): PromptBlock => ({
  id: crypto.randomUUID(), name: 'Prompt Block', markdown: '', enabled: true,
  order: 0, placement: 'after_project_instructions',
});

const builtinToolDescription = (name: string, locale: Locale): string | undefined => {
  switch (name) {
    case 'read_file': return t(locale, 'settings.engineCanvasToolDescriptions.readFile');
    case 'search_files': return t(locale, 'settings.engineCanvasToolDescriptions.searchFiles');
    case 'write_file': return t(locale, 'settings.engineCanvasToolDescriptions.writeFile');
    case 'list_dir': return t(locale, 'settings.engineCanvasToolDescriptions.listDir');
    case 'grep': return t(locale, 'settings.engineCanvasToolDescriptions.grep');
    case 'edit_file': return t(locale, 'settings.engineCanvasToolDescriptions.editFile');
    case 'run_terminal': return t(locale, 'settings.engineCanvasToolDescriptions.runTerminal');
    case 'apply_patch': return t(locale, 'settings.engineCanvasToolDescriptions.applyPatch');
    case 'memory_search': return t(locale, 'settings.engineCanvasToolDescriptions.memorySearch');
    case 'memory_get': return t(locale, 'settings.engineCanvasToolDescriptions.memoryGet');
    case 'task': return t(locale, 'settings.engineCanvasToolDescriptions.task');
    case 'task_output': return t(locale, 'settings.engineCanvasToolDescriptions.taskOutput');
    case 'kill_task': return t(locale, 'settings.engineCanvasToolDescriptions.killTask');
    case 'skill': return t(locale, 'settings.engineCanvasToolDescriptions.skill');
    case 'web_search': return t(locale, 'settings.engineCanvasToolDescriptions.webSearch');
    case 'enter_plan_mode': return t(locale, 'settings.engineCanvasToolDescriptions.enterPlanMode');
    case 'exit_plan_mode': return t(locale, 'settings.engineCanvasToolDescriptions.exitPlanMode');
    case 'write_draft_module': return t(locale, 'settings.engineCanvasToolDescriptions.writeDraftModule');
    case 'read_draft_module': return t(locale, 'settings.engineCanvasToolDescriptions.readDraftModule');
    case 'rollback_draft_revision': return t(locale, 'settings.engineCanvasToolDescriptions.rollbackDraftRevision');
    case 'lint_draft_module': return t(locale, 'settings.engineCanvasToolDescriptions.lintDraftModule');
    default: return undefined;
  }
};

export interface NativeHarnessPanelProps { locale: Locale }

export function NativeHarnessPanel({ locale }: NativeHarnessPanelProps) {
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [profileId, setProfileId] = useState('');
  const profileIdRef = useRef('');
  const noticeRefreshRef = useRef<number | null>(null);
  const [detailMode, setDetailMode] = useState<DetailMode | null>(null);
  const [workspaceTarget, setWorkspaceTarget] = useState<WorkspaceTarget>('preview');
  const [targetStageId, setTargetStageId] = useState<string | null>(null);
  const [focusPromptId, setFocusPromptId] = useState<string | null>(null);
  const [workspace, setWorkspace] = useState<Workspace | null>(null);
  const [document, setDocument] = useState<Blueprint | null>(null);
  const [revision, setRevision] = useState(0);
  const [review, setReview] = useState<Review | null>(null);
  const [dirty, setDirty] = useState(false);
  const [loading, setLoading] = useState(true);
  const [detailLoading, setDetailLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [query, setQuery] = useState('');
  const [projects, setProjects] = useState<ProjectSummary[]>([]);
  const [providers, setProviders] = useState<ProviderSummary[]>([]);
  const [projectIdentities, setProjectIdentities] = useState<ProjectIdentity[]>([]);
  const [newProfileName, setNewProfileName] = useState('');
  const [newProfileKind, setNewProfileKind] = useState<'global_template' | 'project_overlay'>('project_overlay');
  const [newProfileProjectPath, setNewProfileProjectPath] = useState('');
  const [bindProfileId, setBindProfileId] = useState('');
  const [bindProjectPath, setBindProjectPath] = useState('');
  const [liveRuns, setLiveRuns] = useState<Run[]>([]);
  const [selectedRunId, setSelectedRunId] = useState('');
  const [traceEntries, setTraceEntries] = useState<TraceEntry[]>([]);
  const [promptPreview, setPromptPreview] = useState<PromptPreview | null>(null);
  const [runSnapshot, setRunSnapshot] = useState<RunSnapshot | null>(null);
  const [versions, setVersions] = useState<VersionSummary[]>([]);
  const [sourceCandidate, setSourceCandidate] = useState<DriftCandidate[]>([]);
  const [auxLoading, setAuxLoading] = useState(false);
  const [canvasMode, setCanvasMode] = useState<CanvasMode>('understand');
  const [runPrompt, setRunPrompt] = useState('');
  const [runProviderId, setRunProviderId] = useState('');
  const [runModelId, setRunModelId] = useState('');
  const [runProjectPath, setRunProjectPath] = useState('');

  const fail = useCallback((cause: unknown) => {
    setError(classifyError(cause, { locale }).userMessage);
  }, [locale]);

  const profileProject = useCallback((profile?: Profile | null) => (
    projectIdentities.find((item) => item.project_id === profile?.project_id)
  ), [projectIdentities]);

  const loadDetail = useCallback(async (selected: string) => {
    if (!selected) {
      setWorkspace(null); setDocument(null); setRevision(0); return;
    }
    setDetailLoading(true);
    try {
      const profile = profiles.find((item) => item.id === selected);
      const identity = profileProject(profile);
      const scope = identity ? { project_id: identity.project_id, project_path: identity.canonical_path } : {};
      const [nextWorkspace, draft, prompt, versionList] = await Promise.all([
        nativesAPI.assistantV2.request<Workspace>('harness.workspace.get', { profile_id: selected, ...scope }),
        nativesAPI.assistantV2.request<{ revision: number; document: Blueprint; source_candidate?: DriftCandidate[] | null }>('harness.draft.get', { profile_id: selected }),
        nativesAPI.assistantV2.request<PromptPreview>('harness.prompt.preview', scope),
        nativesAPI.assistantV2.request<{ versions?: VersionSummary[] }>('harness.version.list', { profile_id: selected, limit: 20 }),
      ]);
      setWorkspace(nextWorkspace);
      setDocument(normalizeBlueprint(draft.document));
      setRevision(draft.revision ?? 0);
      setSourceCandidate((draft.source_candidate ?? []).filter((candidate) => !candidate.acknowledged));
      setPromptPreview(prompt);
      setVersions(versionList.versions ?? []);
      setDirty(false);
      setReview(null);
    } catch (cause) {
      setWorkspace(null); setDocument(null); fail(cause);
    } finally {
      setDetailLoading(false);
    }
  }, [fail, profileProject, profiles]);

  const load = useCallback(async (requestedProfileId?: string) => {
    setLoading(true); setError(null); setNotice(null);
    try {
      const [listed, identities, projectList, providerList] = await Promise.all([
        nativesAPI.assistantV2.request<{ profiles?: Profile[] }>('harness.profile.list', {}),
        nativesAPI.assistantV2.request<{ items?: ProjectIdentity[] }>('project.identity.list', {}),
        nativesAPI.project.list(),
        nativesAPI.provider.list(),
      ]);
      const nextProfiles = listed.profiles ?? [];
      const selected = requestedProfileId || profileIdRef.current || nextProfiles[0]?.id || '';
      setProfiles(nextProfiles);
      setProjectIdentities(identities.items ?? []);
      setProjects(projectList);
      setProviders(providerList);
      setProfileId(selected);
      profileIdRef.current = selected;
    } catch (cause) {
      setProfiles([]); fail(cause);
    } finally {
      setLoading(false);
    }
  }, [fail]);

  useEffect(() => { void load(); }, [load]);
  useEffect(() => {
    if (detailMode && profileId) void loadDetail(profileId);
  }, [detailMode, loadDetail, profileId]);

  const loadRuns = useCallback(async (runId?: string) => {
    setAuxLoading(true);
    try {
      const result = await nativesAPI.assistantV2.request<{ runs?: Run[] }>('run.list', {});
      const runs = result.runs ?? [];
      const selected = runId || selectedRunId || runs[0]?.id || '';
      setLiveRuns(runs);
      setSelectedRunId(selected);
      if (selected) {
        const [trace, snapshot] = await Promise.all([
          nativesAPI.assistantV2.request<{ entries?: TraceEntry[] }>('harness.trace.list', { run_id: selected, limit: 200 }),
          nativesAPI.assistantV2.request<RunSnapshot>('harness.run.getSnapshot', { run_id: selected }),
        ]);
        setTraceEntries(trace.entries ?? []);
        setRunSnapshot(snapshot);
      } else {
        setTraceEntries([]); setRunSnapshot(null);
      }
    } catch (cause) {
      fail(cause);
    } finally {
      setAuxLoading(false);
    }
  }, [fail, selectedRunId]);

  useEffect(() => {
    if (detailMode) void loadRuns();
  }, [detailMode, loadRuns]);
  useEffect(() => {
    const unsubscribe = nativesAPI.assistantV2.subscribeHarness((event) => {
      if (event.kind === 'trace_updated' && detailMode) {
        if (noticeRefreshRef.current != null) return;
        noticeRefreshRef.current = window.setTimeout(() => {
          noticeRefreshRef.current = null;
          void loadRuns();
        }, 50);
      } else if (!dirty) void load(profileIdRef.current);
      else setNotice(t(locale, 'settings.engineEngineeringRemoteChange'));
    }, { onError: fail });
    return () => {
      unsubscribe();
      if (noticeRefreshRef.current != null) window.clearTimeout(noticeRefreshRef.current);
      noticeRefreshRef.current = null;
    };
  }, [detailMode, dirty, fail, load, loadRuns, locale]);

  const selectedProfile = profiles.find((profile) => profile.id === profileId) ?? null;
  const currentProject = profileProject(selectedProfile);
  const runProvider = providers.find((provider) => provider.id === runProviderId);
  const runModels = runProvider?.models ?? [];
  const filteredProfiles = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return profiles;
    return profiles.filter((profile) => {
      const project = profileProject(profile);
      return [profile.name, profile.kind, profile.id, project?.name, project?.canonical_path]
        .filter(Boolean)
        .some((value) => String(value).toLowerCase().includes(needle));
    });
  }, [profileProject, profiles, query]);

  const selectProfile = (id: string, mode: DetailMode, target: WorkspaceTarget = 'preview') => {
    if (dirty && !window.confirm(t(locale, 'settings.engineEngineeringDiscardConfirm'))) return;
    setProfileId(id);
    profileIdRef.current = id;
    setDetailMode(mode);
    setWorkspaceTarget(target);
    setTargetStageId(null);
    setError(null);
    setNotice(null);
    setCanvasMode('understand');
  };

  const openCreateProfile = () => {
    if (dirty && !window.confirm(t(locale, 'settings.engineEngineeringDiscardConfirm'))) return;
    setDetailMode('create');
    setProfileId('');
    profileIdRef.current = '';
    setError(null);
    setNotice(null);
  };

  const replaceDocument = (next: Blueprint) => {
    setDocument(normalizeBlueprint(next)); setDirty(true); setReview(null); setNotice(null);
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
      setWorkspaceTarget('versions');
    } catch (cause) { fail(cause); } finally { setBusy(false); }
  };
  const publish = async (id = profileId) => {
    if (!id) return;
    setBusy(true); setError(null); setNotice(null);
    try {
      if (id === profileId && dirty) await persist();
      await nativesAPI.assistantV2.request('harness.draft.publish', { profile_id: id, revision: id === profileId ? revision : undefined });
      await load(id);
      if (id === profileId) await loadDetail(id);
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
      setNewProfileName('');
      setNewProfileProjectPath('');
      await load(result.profile.id);
      selectProfile(result.profile.id, 'edit', 'hooks');
      setNotice(t(locale, 'settings.engineEngineeringProfileCreated'));
    } catch (cause) { fail(cause); } finally { setBusy(false); }
  };
  const setDefaultProfile = async (id: string) => {
    setBusy(true); setError(null);
    try {
      await nativesAPI.assistantV2.request('harness.binding.set', {
        scope_type: 'global', scope_id: 'global', profile_id: id, mode: 'follow_published',
      });
      setNotice(t(locale, 'settings.engineEngineeringDefaultSet'));
    } catch (cause) { fail(cause); } finally { setBusy(false); }
  };
  const bindProject = async () => {
    if (!bindProfileId || !bindProjectPath) return;
    setBusy(true); setError(null);
    try {
      const project = projects.find((item) => item.path === bindProjectPath);
      if (!project) throw new Error(t(locale, 'settings.engineEngineeringProjectRequired'));
      const identity = await nativesAPI.assistantV2.request<{ project_id: string }>('project.identity.register', { path: project.path, name: project.label });
      await nativesAPI.assistantV2.request('harness.binding.set', {
        scope_type: 'project', scope_id: identity.project_id, profile_id: bindProfileId, mode: 'follow_published',
      });
      setBindProfileId('');
      setBindProjectPath('');
      setNotice(t(locale, 'settings.engineEngineeringBound'));
    } catch (cause) { fail(cause); } finally { setBusy(false); }
  };
  const archiveProfile = async (id: string) => {
    if (!window.confirm(t(locale, 'settings.engineEngineeringDeleteConfirm'))) return;
    setBusy(true); setError(null);
    try {
      await nativesAPI.assistantV2.request('harness.profile.archive', { profile_id: id });
      if (id === profileId) {
        setDetailMode(null);
        setProfileId('');
        profileIdRef.current = '';
      }
      await load();
      setNotice(t(locale, 'settings.engineEngineeringDeleted'));
    } catch (cause) { fail(cause); } finally { setBusy(false); }
  };
  const runFromCanvas = async () => {
    const content = runPrompt.trim();
    if (!content) return;
    if (!runProviderId || !runModelId || !runProjectPath) {
      setError(t(locale, 'settings.engineEngineeringRunConfigRequired'));
      return;
    }
    setBusy(true); setError(null); setNotice(null);
    try {
      const result = await nativesAPI.assistantV2.request<Run>('run.start', {
        conversation_id: crypto.randomUUID(),
        provider_id: runProviderId,
        model_id: runModelId,
        content,
        permission_profile: 'ask',
        runtime_id: 'native',
        project_path: runProjectPath,
        idempotency_key: crypto.randomUUID(),
      });
      setRunPrompt('');
      setCanvasMode('audit');
      setWorkspaceTarget('runs');
      setSelectedRunId(result.id);
      await loadRuns(result.id);
    } catch (cause) { fail(cause); } finally { setBusy(false); }
  };

  const findings = review?.validation?.findings ?? [];
  const stages = workspace?.topology?.stages ?? [];
  const hookStage = targetStageId ? stages.find((stage) => stage.id === targetStageId) : null;
  const stageEvents = new Set((hookStage?.hook_points ?? []).map((point) => point.event));
  const visibleHookIndexes = document?.native_hooks
    .map((hook, index) => ({ hook, index }))
    .filter(({ hook }) => !hookStage || stageEvents.has(hook.event)) ?? [];
  const importedHooks = (workspace?.catalog?.hooks ?? []).filter((hook) => !hook.source.origin.startsWith('native:'));
  const overlays = document?.hook_overlays ?? document?.hooks ?? [];
  const selectedRun = liveRuns.find((run) => run.id === selectedRunId) ?? null;
  const selectedRunStatus = selectedRun?.status ?? '';
  const nodeDetails = useMemo<Record<string, CanvasNodeDetail>>(() => {
    const details: Record<string, CanvasNodeDetail> = {};
    for (const stage of stages) {
      const events = new Set((stage.hook_points ?? []).map((point) => point.event));
      const hooks = [
        ...(document?.native_hooks ?? [])
          .filter((hook) => events.has(hook.event))
          .map((hook) => ({
            id: hook.id,
            name: hook.name,
            event: hook.event,
            source: `Harness Draft · ${hook.adapter.type}`,
            enabled: hook.enabled,
            authorized: hook.adapter.type === 'command' ? hook.trust_confirmed : true,
            authorization: hook.adapter.type === 'command' ? t(locale, 'settings.engineCanvasHookAuthorizationHarness') : t(locale, 'settings.engineCanvasHookAuthorizationImplicit'),
            canAuthorize: hook.adapter.type === 'command',
            canRemove: true,
          })),
        ...importedHooks
          .filter((hook) => events.has(hook.event))
          .map((hook) => {
            const overlay = overlays.find((item) => item.hook_id === hook.id);
            const enabled = overlay?.enabled ?? hook.enabled;
            return {
              id: hook.id,
              name: hook.id,
              event: hook.event,
              source: `${hook.source.scope} · ${hook.source.origin}`,
              enabled,
              authorized: true,
              authorization: t(locale, 'settings.engineCanvasHookAuthorizationImported'),
              canRemove: true,
              removeLabel: t(locale, 'settings.engineCanvasRemoveHookBinding'),
            };
          }),
      ];
      const prompts = ['context', 'compact', 'stop'].includes(stage.id)
        ? [
          ...(promptPreview?.builtin_surfaces ?? []).map((surface) => {
            const replacement = document?.builtin_prompt_replacements?.find((item) => item.surface_id === surface.surface_id);
            return {
              id: surface.surface_id,
              name: surface.surface_id,
              source: t(locale, 'settings.engineEngineeringBuiltinPrompt'),
              placement: surface.surface_id,
              enabled: true,
              markdown: replacement?.markdown ?? surface.default_markdown,
              canEdit: true,
              canRemove: Boolean(replacement),
              removeLabel: t(locale, 'settings.engineCanvasRestoreDefaultPrompt'),
            };
          }),
          ...(document?.prompt_blocks ?? []).map((block) => ({
            id: block.id,
            name: block.name,
            source: t(locale, 'settings.engineCanvasHarnessPromptBlock'),
            placement: block.placement,
            enabled: block.enabled,
            markdown: block.markdown,
            canEdit: true,
            canRemove: true,
          })),
        ]
        : [];
      const tools = ['provider', 'tool_gate', 'tool_execute'].includes(stage.id)
        ? (runSnapshot?.snapshot?.tool_plan?.tools ?? BUILTIN_TOOL_NAMES.map((name) => ({
          name,
          source: 'native:builtin',
          schema_digest: undefined,
        }))).map((tool) => ({
          name: tool.name,
          source: tool.source,
          schema_digest: tool.schema_digest,
          description: builtinToolDescription(tool.name, locale),
        }))
        : [];
      const snapshot = selectedRun?.capability_snapshot;
      const subagents = stage.id === 'subagent'
        ? [
          ...(!selectedRun ? [{
            id: 'run_snapshot',
            kind: 'unresolved' as const,
            source: t(locale, 'settings.engineCanvasSubagentSourceRunSnapshot'),
            prompt: t(locale, 'settings.engineCanvasSubagentSnapshotRequired'),
          }] : []),
          ...(selectedRun && snapshot?.agentProfileId ? [{
            id: snapshot.agentProfileId,
            kind: 'expert' as const,
            source: t(locale, 'settings.engineCanvasSubagentSourceCapabilitySnapshot'),
            prompt: t(locale, 'settings.engineCanvasPresetExpertPrompt', { id: snapshot.agentProfileId }),
          }] : []),
          ...(selectedRun && snapshot?.teamId ? [{
            id: snapshot.teamId,
            kind: 'team' as const,
            source: t(locale, 'settings.engineCanvasSubagentSourceCapabilitySnapshot'),
            prompt: t(locale, 'settings.engineCanvasPresetTeamPrompt', { count: snapshot.teamMembers?.length ?? 0 }),
          }] : []),
          ...(selectedRun ? (snapshot?.teamMembers ?? []).map((id) => ({
            id,
            kind: 'member' as const,
            source: t(locale, 'settings.engineCanvasSubagentSourceCapabilitySnapshot'),
            prompt: t(locale, 'settings.engineCanvasTeamMemberPrompt'),
          })) : []),
          ...(selectedRun && !snapshot?.agentProfileId && !snapshot?.teamId ? [{
            id: 'task',
            kind: 'dynamic' as const,
            source: t(locale, 'settings.engineCanvasSubagentSourceMasterTask'),
            prompt: t(locale, 'settings.engineCanvasDynamicSubagentPrompt'),
          }] : []),
        ]
        : [];
      const stageTraces = traceEntriesForStage(stage, traceEntries);
      const runResults = [
        ...(stage.id === 'session' && selectedRun ? [{
          id: `${selectedRun.id}:session`,
          action: t(locale, 'settings.engineCanvasRunSessionStart'),
          status: selectedRun.status,
          timestamp: selectedRun.started_at ?? undefined,
          output: t(locale, 'settings.engineCanvasRunSessionDetail', {
            conversation: selectedRun.conversation_id,
            permission: selectedRun.permission_profile,
            runtime: selectedRun.runtime_id ?? 'native',
          }),
        }] : []),
        ...(stage.id === 'provider' && selectedRun ? [{
          id: `${selectedRun.id}:provider`,
          action: t(locale, 'settings.engineCanvasRunProviderSelection'),
          status: selectedRun.status,
          timestamp: selectedRun.started_at ?? undefined,
          output: t(locale, 'settings.engineCanvasRunProviderDetail', {
            provider: selectedRun.provider_id,
            model: selectedRun.model_id,
          }),
        }] : []),
        ...(stage.id === 'permission' && selectedRun ? [{
          id: `${selectedRun.id}:permission`,
          action: t(locale, 'settings.engineCanvasRunPermissionGate'),
          status: selectedRun.status,
          timestamp: selectedRun.started_at ?? undefined,
          output: t(locale, 'settings.engineCanvasRunPermissionDetail', {
            permission: selectedRun.permission_profile,
          }),
        }] : []),
        ...(stage.id === 'context' && runSnapshot?.snapshot?.prompt_plan ? [{
          id: `${selectedRun?.id ?? 'run'}:prompt-plan`,
          action: t(locale, 'settings.engineCanvasRunPromptAssembly'),
          status: selectedRun?.status,
          output: t(locale, 'settings.engineCanvasPromptSnapshotDetail', {
            count: runSnapshot.snapshot.prompt_plan.source_digests?.length ?? 0,
            tokens: runSnapshot.snapshot.prompt_plan.token_estimate ?? 0,
          }),
        }] : []),
        ...((stage.id === 'tool_gate' || stage.id === 'tool_execute') && runSnapshot?.snapshot?.tool_plan ? [{
          id: `${selectedRun?.id ?? 'run'}:tool-plan`,
          action: t(locale, 'settings.engineCanvasRunToolPlan'),
          status: selectedRun?.status,
          output: t(locale, 'settings.engineCanvasToolSnapshotDetail', {
            count: runSnapshot.snapshot.tool_plan.tools?.length ?? 0,
          }),
        }] : []),
        ...(stage.id === 'subagent' && selectedRun ? [{
          id: `${selectedRun.id}:subagent`,
          action: t(locale, 'settings.engineCanvasRunSubagentStrategy'),
          status: selectedRun.status,
          timestamp: selectedRun.started_at ?? undefined,
          output: t(locale, 'settings.engineCanvasRunSubagentDetail', {
            mode: snapshot?.agentProfileId
              ? t(locale, 'settings.engineCanvasSubagentKind.expert')
              : snapshot?.teamId
                ? t(locale, 'settings.engineCanvasSubagentKind.team')
                : t(locale, 'settings.engineCanvasSubagentKind.dynamic'),
            target: snapshot?.agentProfileId ?? snapshot?.teamId ?? 'task',
            count: snapshot?.teamMembers?.length ?? 0,
          }),
        }] : []),
        ...(stage.id === 'compact' && selectedRun ? [{
          id: `${selectedRun.id}:compact`,
          action: t(locale, 'settings.engineCanvasRunCompactCheck'),
          status: selectedRun.status,
          timestamp: selectedRun.finished_at ?? selectedRun.started_at ?? undefined,
          output: t(locale, selectedRun.error_code ? 'settings.engineCanvasRunCompactErrorDetail' : 'settings.engineCanvasRunCompactDetail', {
            status: selectedRun.status,
            code: selectedRun.error_code ?? '',
          }),
        }] : []),
        ...(stage.id === 'stop' && selectedRun ? [{
          id: `${selectedRun.id}:stop`,
          action: t(locale, 'settings.engineCanvasRunStopDecision'),
          status: selectedRun.status,
          timestamp: selectedRun.finished_at ?? selectedRun.started_at ?? undefined,
          output: t(locale, selectedRun.error_code ? 'settings.engineCanvasRunStopErrorDetail' : 'settings.engineCanvasRunStopDetail', {
            status: selectedRun.status,
            code: selectedRun.error_code ?? '',
          }),
        }] : []),
        ...(stage.id === 'cross_stage' && selectedRun ? [{
          id: `${selectedRun.id}:cross-stage`,
          action: t(locale, 'settings.engineCanvasRunAuditSummary'),
          status: selectedRun.status,
          timestamp: selectedRun.finished_at ?? selectedRun.started_at ?? undefined,
          output: t(locale, 'settings.engineCanvasRunAuditDetail', {
            count: traceEntries.length,
            failed: traceEntries.filter((entry) => entry.status === 'failed' || Boolean(entry.error_category)).length,
          }),
        }] : []),
        ...(stage.id === 'terminal' && selectedRun ? [{
          id: `${selectedRun.id}:terminal`,
          action: t(locale, 'settings.engineCanvasRunTerminalResult'),
          status: selectedRun.status,
          timestamp: selectedRun.finished_at ?? selectedRun.started_at ?? undefined,
          output: t(locale, selectedRun.error_code ? 'settings.engineCanvasRunTerminalErrorDetail' : 'settings.engineCanvasRunTerminalDetail', {
            provider: selectedRun.provider_id,
            model: selectedRun.model_id,
            code: selectedRun.error_code ?? '',
          }),
        }] : []),
        ...stageTraces.map((entry) => ({
          id: `${entry.run_id}:${entry.sequence}`,
          action: entry.hook_event ?? entry.type ?? t(locale, 'settings.engineCanvasRunHookInvocation'),
          status: entry.status ?? entry.type,
          timestamp: entry.timestamp,
          duration_ms: entry.duration_ms,
          input: entry.input_summary ? `${entry.input_summary}${entry.input_truncated ? '…' : ''}` : undefined,
          output: entry.output_summary ? `${entry.output_summary}${entry.output_truncated ? '…' : ''}` : undefined,
        })),
      ];
      details[stage.id] = { hooks, prompts, tools, subagents, runResults };
    }
    return details;
  }, [document, importedHooks, locale, overlays, promptPreview, runSnapshot, selectedRun, stages, traceEntries]);

  useEffect(() => {
    if (!detailMode || canvasMode !== 'audit' || !selectedRunId || !ACTIVE_RUN_STATUSES.has(selectedRunStatus)) return;
    const interval = window.setInterval(() => void loadRuns(selectedRunId), 2_000);
    return () => window.clearInterval(interval);
  }, [canvasMode, detailMode, loadRuns, selectedRunId, selectedRunStatus]);

  const updateOverlay = (hook: CatalogHook, patch: Partial<HookOverlay>) => {
    if (!document) return;
    const existing = overlays.find((item) => item.hook_id === hook.id);
    const next = existing ? { ...existing, ...patch } : { hook_id: hook.id, ...patch };
    replaceDocument({ ...document, hook_overlays: [...overlays.filter((item) => item.hook_id !== hook.id), next] });
  };
  const setNodeHookEnabled = (hookId: string, enabled: boolean) => {
    const nativeIndex = document?.native_hooks.findIndex((hook) => hook.id === hookId) ?? -1;
    if (nativeIndex >= 0) {
      updateHook(nativeIndex, { enabled });
      return;
    }
    const imported = importedHooks.find((hook) => hook.id === hookId);
    if (imported) updateOverlay(imported, { enabled });
  };
  const authorizeNodeHook = (hookId: string, authorized: boolean) => {
    const nativeIndex = document?.native_hooks.findIndex((hook) => hook.id === hookId) ?? -1;
    const hook = nativeIndex >= 0 ? document?.native_hooks[nativeIndex] : null;
    if (hook) updateHook(nativeIndex, { adapter: { ...hook.adapter, trusted: authorized }, trust_confirmed: authorized });
  };
  const removeNodeHook = (hookId: string) => {
    if (!document) return;
    if (document.native_hooks.some((hook) => hook.id === hookId)) {
      replaceDocument({ ...document, native_hooks: document.native_hooks.filter((hook) => hook.id !== hookId) });
      return;
    }
    const imported = importedHooks.find((hook) => hook.id === hookId);
    if (imported) updateOverlay(imported, { enabled: false });
  };
  const removeNodePrompt = (promptId: string) => {
    if (!document) return;
    if (document.builtin_prompt_replacements?.some((item) => item.surface_id === promptId)) {
      replaceDocument({
        ...document,
        builtin_prompt_replacements: document.builtin_prompt_replacements.filter((item) => item.surface_id !== promptId),
      });
      return;
    }
    replaceDocument({ ...document, prompt_blocks: document.prompt_blocks.filter((block) => block.id !== promptId) });
  };
  const openCanvasWorkspace = (target: CanvasWorkspaceTarget, stageId: string, itemId?: string) => {
    setWorkspaceTarget(target);
    setTargetStageId(stageId);
    setFocusPromptId(target === 'prompts' ? itemId ?? null : null);
    if (target === 'runs') {
      setCanvasMode('audit');
      void loadRuns(selectedRunId);
    }
  };

  useEffect(() => {
    if (!runProjectPath && currentProject?.canonical_path) setRunProjectPath(currentProject.canonical_path);
  }, [currentProject?.canonical_path, runProjectPath]);

  useEffect(() => {
    if (runProviderId) return;
    const next = providers.find((provider) => provider.keys.some((key) => key.isActive)) ?? providers[0];
    if (!next) return;
    setRunProviderId(next.id);
    setRunModelId(next.defaultModel ?? next.models?.[0]?.id ?? '');
  }, [providers, runProviderId]);

  useEffect(() => {
    if (!runProvider || runModels.some((model) => model.id === runModelId)) return;
    setRunModelId(runProvider.defaultModel ?? runModels[0]?.id ?? '');
  }, [runModelId, runModels, runProvider]);

  if (detailMode === 'create') {
    return (
      <section className="space-y-5" data-testid="native-harness-panel">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <button type="button" className="btn" onClick={() => setDetailMode(null)}><ArrowLeft size={14} />{t(locale, 'common.back')}</button>
        </div>
        {error ? <div role="alert" className="rounded-lg border border-[var(--danger)] p-4 text-sm text-[var(--danger)]"><AlertTriangle size={16} className="mr-2 inline" />{error}</div> : null}
        <section className="settings-section-card space-y-3">
          <h3>{t(locale, 'settings.engineEngineeringCreateProfile')}</h3>
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
            <button type="button" className="btn btn-primary" disabled={busy || !newProfileName.trim()} onClick={() => void createProfile()}><Plus size={14} />{t(locale, 'settings.engineEngineeringCreateProfile')}</button>
          </div>
        </section>
      </section>
    );
  }

  if ((detailMode === 'preview' || detailMode === 'edit') && selectedProfile) {
    const readOnly = detailMode === 'preview';
    const runPanel = (
      <div className="rounded-lg border border-[var(--border-subtle)] bg-[var(--surface)] p-3">
        <div className="mb-3">
          <strong className="block text-sm text-[var(--text)]">{t(locale, 'settings.engineEngineeringRunTrialTitle')}</strong>
          <p className="mt-1 text-xs leading-5 text-[var(--text-secondary)]">{t(locale, 'settings.engineEngineeringRunTrialDesc')}</p>
        </div>
        <div className="grid gap-2 lg:grid-cols-[minmax(10rem,0.8fr)_minmax(10rem,0.8fr)_minmax(10rem,0.8fr)_minmax(16rem,1.2fr)_auto]">
          <select className="input" value={runProjectPath} onChange={(event) => setRunProjectPath(event.target.value)} aria-label={t(locale, 'settings.engineEngineeringRunProject')}>
            <option value="">{t(locale, 'settings.engineEngineeringSelectProject')}</option>
            {projects.filter((project) => project.exists).map((project) => <option key={project.id} value={project.path}>{project.label}</option>)}
          </select>
          <select className="input" value={runProviderId} onChange={(event) => setRunProviderId(event.target.value)} aria-label={t(locale, 'settings.engineEngineeringRunProvider')}>
            <option value="">{t(locale, 'settings.engineEngineeringRunProvider')}</option>
            {providers.map((provider) => <option key={provider.id} value={provider.id}>{provider.displayName}</option>)}
          </select>
          <select className="input" value={runModelId} onChange={(event) => setRunModelId(event.target.value)} aria-label={t(locale, 'settings.engineEngineeringRunModel')}>
            <option value="">{t(locale, 'settings.engineEngineeringRunModel')}</option>
            {runModels.map((model) => <option key={model.id} value={model.id}>{model.displayName ?? model.id}</option>)}
          </select>
          <textarea className="input min-h-10" value={runPrompt} onChange={(event) => setRunPrompt(event.target.value)} placeholder={t(locale, 'settings.engineEngineeringRunPromptPlaceholder')} />
          <button type="button" className="btn btn-primary lg:w-28" disabled={busy || !runPrompt.trim() || !runProviderId || !runModelId || !runProjectPath} onClick={() => void runFromCanvas()}><Rocket size={14} />{t(locale, 'settings.engineEngineeringRun')}</button>
        </div>
      </div>
    );
    const workspacePanel = (
      <>
        {workspaceTarget === 'hooks' && document ? <HooksEditor
          locale={locale}
          document={document}
          revision={revision}
          stageName={hookStage ? stageLabel(hookStage.id, locale) : null}
          stageEvents={stageEvents}
          importedHooks={importedHooks}
          overlays={overlays}
          visibleHookIndexes={visibleHookIndexes}
          replaceDocument={replaceDocument}
          updateHook={updateHook}
          updateAdapter={updateAdapter}
          updateOverlay={updateOverlay}
          readOnly={readOnly}
        /> : null}

        {workspaceTarget === 'prompts' && document ? <PromptsEditor
          locale={locale}
          document={document}
          promptPreview={promptPreview}
          replaceDocument={replaceDocument}
          focusPromptId={focusPromptId}
          readOnly={readOnly}
        /> : null}

        {workspaceTarget === 'capabilities' ? (
          <EngineCapabilitiesPanel
            locale={locale}
            selectedRun={selectedRun}
            runSnapshot={runSnapshot}
          />
        ) : null}

        {workspaceTarget === 'runs' ? <RunsTimeline
          locale={locale}
          loading={auxLoading}
          runs={liveRuns}
          selectedRunId={selectedRunId}
          traceEntries={traceEntries}
          runSnapshot={runSnapshot}
          onRefresh={() => void loadRuns(selectedRunId)}
          onSelect={(runId) => void loadRuns(runId)}
        /> : null}

        {workspaceTarget === 'versions' ? <VersionsPanel locale={locale} review={review} versions={versions} findings={findings} /> : null}
      </>
    );
    return (
      <section className="space-y-5" data-testid="native-harness-panel">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <button type="button" className="btn" onClick={() => setDetailMode(null)}><ArrowLeft size={14} />{t(locale, 'common.back')}</button>
          {!readOnly ? <div className="flex flex-wrap gap-2">
            <button type="button" className="btn" disabled={busy || !document} onClick={() => void save()}><Save size={14} />{t(locale, 'settings.engineEngineeringSave')}</button>
            <button type="button" className="btn" disabled={busy || !document} onClick={() => void reviewDraft()}>{t(locale, 'settings.engineEngineeringReview')}</button>
            <button type="button" className="btn btn-primary" disabled={busy || !document || findings.some((item) => item.severity === 'error')} onClick={() => void publish()}><Rocket size={14} />{t(locale, 'settings.engineEngineeringPublish')}</button>
          </div> : null}
        </div>

        {error ? <div role="alert" className="rounded-lg border border-[var(--danger)] p-4 text-sm text-[var(--danger)]"><AlertTriangle size={16} className="mr-2 inline" />{error}</div> : null}
        {notice ? <div role="status" className="rounded-lg border border-[var(--success)] p-4 text-sm text-[var(--success)]">{notice}</div> : null}
        {sourceCandidate.length ? <div role="alert" className="settings-section-card border-[var(--warning)] text-sm">{t(locale, 'settings.engineEngineeringDriftDesc', { count: sourceCandidate.length })}</div> : null}

        <section className="settings-section-card space-y-1">
          <p className="settings-section-eyebrow">{detailMode === 'preview' ? t(locale, 'settings.engineEngineeringPreview') : t(locale, 'settings.engineEngineeringEdit')}</p>
          <h3>{selectedProfile.name}</h3>
          <p className="text-sm text-[var(--text-muted)]">{selectedProfile.kind}{currentProject ? ` · ${currentProject.name} · ${currentProject.canonical_path}` : ''}{dirty ? ' · Draft*' : ''}</p>
        </section>

        {detailLoading ? <div className="engine-empty"><Loader size={18} className="animate-spin" />{t(locale, 'common.loading')}</div> : workspace ? (
          <NativeExecutionCanvas
            locale={locale}
            mode={canvasMode}
            stages={stages}
            edges={workspace.topology?.edges ?? []}
            promptBlocks={workspace.prompt_plan?.blocks ?? []}
            issueCount={workspace.overview?.issues?.length ?? 0}
            runs={liveRuns}
            selectedRunId={selectedRunId}
            traceEntries={traceEntries}
            runSnapshot={runSnapshot}
            auditLoading={auxLoading}
            readOnly={readOnly}
            nodeDetails={nodeDetails}
            onModeChange={setCanvasMode}
            onRequestAudit={() => void loadRuns()}
            onSelectRun={(runId) => {
              setSelectedRunId(runId);
              if (runId) void loadRuns(runId);
              else { setTraceEntries([]); setRunSnapshot(null); }
            }}
            onRefreshAudit={() => void loadRuns(selectedRunId)}
            onOpenWorkspace={openCanvasWorkspace}
            onSetHookEnabled={setNodeHookEnabled}
            onAuthorizeHook={authorizeNodeHook}
            onRemoveHook={removeNodeHook}
            onRemovePrompt={removeNodePrompt}
            runPanel={runPanel}
            workspacePanel={workspacePanel}
          />
        ) : null}
      </section>
    );
  }

  return (
    <section className="space-y-5" data-testid="native-harness-panel">
      {error ? <div role="alert" className="rounded-lg border border-[var(--danger)] p-4 text-sm text-[var(--danger)]"><AlertTriangle size={16} className="mr-2 inline" />{error}</div> : null}
      {notice ? <div role="status" className="rounded-lg border border-[var(--success)] p-4 text-sm text-[var(--success)]">{notice}</div> : null}

      <section className="settings-section-card space-y-3">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div>
            <h3>{t(locale, 'settings.engineEngineeringListTitle')}</h3>
            <p className="text-sm text-[var(--text-muted)]">{t(locale, 'settings.engineEngineeringListDesc', { count: profiles.length })}</p>
          </div>
          <div className="flex flex-wrap gap-2">
            <button type="button" className="btn" disabled={loading || busy} onClick={() => void load()}><RefreshCw size={14} />{t(locale, 'common.refresh')}</button>
            <button type="button" className="btn btn-primary" disabled={loading || busy} onClick={openCreateProfile}><Plus size={14} />{t(locale, 'settings.engineEngineeringCreateProfile')}</button>
          </div>
        </div>
        <div className="relative">
          <Search size={15} className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-[var(--text-disabled)]" />
          <input className="input w-full pl-9" value={query} onChange={(event) => setQuery(event.target.value)} placeholder={t(locale, 'settings.engineEngineeringSearchPlaceholder')} />
        </div>
      </section>

      {bindProfileId ? <section className="settings-section-card space-y-3">
        <h4>{t(locale, 'settings.engineEngineeringBindProject')}</h4>
        <div className="grid gap-2 lg:grid-cols-[1fr_auto_auto]">
          <select className="input" value={bindProjectPath} onChange={(event) => setBindProjectPath(event.target.value)}>
            <option value="">{t(locale, 'settings.engineEngineeringSelectProject')}</option>
            {projects.filter((project) => project.exists).map((project) => <option key={project.id} value={project.path}>{project.label}</option>)}
          </select>
          <button type="button" className="btn btn-primary" disabled={busy || !bindProjectPath} onClick={() => void bindProject()}>{t(locale, 'settings.engineEngineeringBindProfile')}</button>
          <button type="button" className="btn" onClick={() => setBindProfileId('')}>{t(locale, 'common.cancel')}</button>
        </div>
      </section> : null}

      {loading ? <div className="engine-empty"><Loader size={18} className="animate-spin" />{t(locale, 'common.loading')}</div> : filteredProfiles.length === 0 ? (
        <div className="engine-empty">{t(locale, 'settings.engineEngineeringEmpty')}</div>
      ) : (
        <div className="grid gap-3 xl:grid-cols-2">
          {filteredProfiles.map((profile) => {
            const project = profileProject(profile);
            return (
              <article className="settings-section-card space-y-4" key={profile.id}>
                <div className="flex items-start justify-between gap-3">
                  <div className="min-w-0">
                    <h4 className="truncate">{profile.name}</h4>
                    <p className="mt-1 truncate text-xs text-[var(--text-muted)]">{profile.kind}{project ? ` · ${project.name} · ${project.canonical_path}` : ''}</p>
                  </div>
                  <Workflow size={18} className="shrink-0 text-[var(--text-secondary)]" />
                </div>
                <div className="flex flex-wrap gap-2">
                  <button type="button" className="btn" onClick={() => void setDefaultProfile(profile.id)} disabled={busy}><Star size={13} />{t(locale, 'settings.engineEngineeringSetDefault')}</button>
                  <button type="button" className="btn" onClick={() => selectProfile(profile.id, 'preview')}><Eye size={13} />{t(locale, 'settings.engineEngineeringPreview')}</button>
                  <button type="button" className="btn" onClick={() => selectProfile(profile.id, 'edit', 'hooks')}><Pencil size={13} />{t(locale, 'settings.engineEngineeringEdit')}</button>
                  <button type="button" className="btn" onClick={() => void publish(profile.id)} disabled={busy}><Rocket size={13} />{t(locale, 'settings.engineEngineeringPublish')}</button>
                  <button type="button" className="btn" onClick={() => { setBindProfileId(profile.id); setBindProjectPath(''); }}>{t(locale, 'settings.engineEngineeringBindProject')}</button>
                  <button type="button" className="btn text-[var(--danger)]" onClick={() => void archiveProfile(profile.id)} disabled={busy}><Trash2 size={13} />{t(locale, 'common.delete')}</button>
                </div>
              </article>
            );
          })}
        </div>
      )}
    </section>
  );
}

function HooksEditor({
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

function PromptsEditor({ locale, document, promptPreview, replaceDocument, focusPromptId, readOnly }: {
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

function RunsTimeline({ locale, loading, runs, selectedRunId, traceEntries, runSnapshot, onRefresh, onSelect }: {
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

function VersionsPanel({ locale, review, versions, findings }: {
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

export default NativeHarnessPanel;
