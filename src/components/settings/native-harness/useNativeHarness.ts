'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { assistantV2, project as projectApi } from '@/lib/tauri/assistant';
import { provider as providerApi } from '@/lib/tauri/provider';
import type { ProjectSummary, ProviderSummary } from '@/lib/tauri/types';
import { classifyError } from '@/lib/error-classifier';
import { t, type Locale } from '@/i18n';
import type { CanvasMode, CanvasNodeDetail, CanvasStage, CanvasWorkspaceTarget } from '../nativeExecutionCanvasModel';
import { createHarnessNoticeBatcher } from './notice-batcher';
import {
  ACTIVE_RUN_STATUSES,
  buildNodeDetails,
  normalizeBlueprint,
  type Blueprint,
  type CatalogHook,
  type DetailMode,
  type DriftCandidate,
  type HookOverlay,
  type NativeHook,
  type Profile,
  type ProjectIdentity,
  type PromptPreview,
  type Review,
  type Run,
  type RunSnapshot,
  type TraceEntry,
  type VersionSummary,
  type Workspace,
  type WorkspaceTarget,
} from './model';

export interface NativeHarnessController {
  // navigation / selection
  detailMode: DetailMode | null;
  setDetailMode: (mode: DetailMode | null) => void;
  workspaceTarget: WorkspaceTarget;
  targetStageId: string | null;
  focusPromptId: string | null;
  canvasMode: CanvasMode;
  setCanvasMode: (mode: CanvasMode) => void;
  // profiles / list
  profiles: Profile[];
  loading: boolean;
  busy: boolean;
  error: string | null;
  notice: string | null;
  query: string;
  setQuery: (query: string) => void;
  filteredProfiles: Profile[];
  projects: ProjectSummary[];
  providers: ProviderSummary[];
  projectIdentities: ProjectIdentity[];
  profileProject: (profile?: Profile | null) => ProjectIdentity | undefined;
  // create profile form
  // selected profile detail
  selectedProfile: Profile | null;
  currentProject: ProjectIdentity | undefined;
  workspace: Workspace | null;
  document: Blueprint | null;
  revision: number;
  review: Review | null;
  dirty: boolean;
  detailLoading: boolean;
  sourceCandidate: DriftCandidate[];
  // runs
  liveRuns: Run[];
  selectedRunId: string;
  setSelectedRunId: (id: string) => void;
  selectedRun: Run | null;
  selectedRunStatus: string;
  traceEntries: TraceEntry[];
  runSnapshot: RunSnapshot | null;
  promptPreview: PromptPreview | null;
  versions: VersionSummary[];
  auxLoading: boolean;
  // derived canvas data
  findings: Array<{ severity?: string; code?: string; message?: string }>;
  stages: CanvasStage[];
  hookStage: CanvasStage | null | undefined;
  stageEvents: Set<string>;
  visibleHookIndexes: Array<{ hook: NativeHook; index: number }>;
  importedHooks: CatalogHook[];
  overlays: HookOverlay[];
  nodeDetails: Record<string, CanvasNodeDetail>;
  // run trial form
  runPrompt: string;
  setRunPrompt: (prompt: string) => void;
  runProviderId: string;
  setRunProviderId: (id: string) => void;
  runModelId: string;
  setRunModelId: (id: string) => void;
  runProjectPath: string;
  setRunProjectPath: (path: string) => void;
  runModels: NonNullable<ProviderSummary['models']>;
  runProvider: ProviderSummary | undefined;
  // confirm dialog
  confirmState: { title: string; message: string; onConfirm: () => void } | null;
  dismissConfirm: () => void;
  // actions
  load: () => Promise<void>;
  loadRuns: (runId?: string) => Promise<void>;
  selectRun: (runId: string) => void;
  selectProfile: (id: string, mode: DetailMode, target?: WorkspaceTarget) => void;
  save: () => Promise<void>;
  reviewDraft: () => Promise<void>;
  publish: (id?: string) => Promise<void>;
  setDefaultProfile: (id: string) => Promise<void>;
  runFromCanvas: () => Promise<void>;
  replaceDocument: (next: Blueprint) => void;
  updateHook: (index: number, patch: Partial<NativeHook>) => void;
  updateAdapter: (index: number, patch: Record<string, unknown>) => void;
  updateOverlay: (hook: CatalogHook, patch: Partial<HookOverlay>) => void;
  setNodeHookEnabled: (hookId: string, enabled: boolean) => void;
  authorizeNodeHook: (hookId: string, authorized: boolean) => void;
  removeNodeHook: (hookId: string) => void;
  removeNodePrompt: (promptId: string) => void;
  openCanvasWorkspace: (target: CanvasWorkspaceTarget, stageId: string, itemId?: string) => void;
}

export function useNativeHarness(locale: Locale): NativeHarnessController {
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [profileId, setProfileId] = useState('');
  const profileIdRef = useRef('');
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
  // T216 (P1-020): unified confirm dialog replaces native window.confirm.
  const [confirmState, setConfirmState] = useState<{
    title: string;
    message: string;
    onConfirm: () => void;
  } | null>(null);

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
        assistantV2.request<Workspace>('harness.workspace.get', { profile_id: selected, ...scope }),
        assistantV2.request<{ revision: number; document: Blueprint; source_candidate?: DriftCandidate[] | null }>('harness.draft.get', { profile_id: selected }),
        assistantV2.request<PromptPreview>('harness.prompt.preview', scope),
        assistantV2.request<{ versions?: VersionSummary[] }>('harness.version.list', { profile_id: selected, limit: 20 }),
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
      const [binding, identities, projectList, providerList] = await Promise.all([
        // 问题9：唯一 Harness = 当前 global binding 指向的 profile；无 binding 时
        // Daemon 幂等使用 harness.global.default（repository.ensure_defaults）。
        assistantV2
          .request<{ binding?: { profile_id?: string; profileId?: string } | null }>(
            'harness.binding.get',
            { scope_type: 'global' },
          )
          .catch(() => null),
        assistantV2.request<{ items?: ProjectIdentity[] }>('project.identity.list', {}),
        projectApi.list(),
        providerApi.list(),
      ]);
      const boundProfileId =
        binding?.binding?.profile_id ?? binding?.binding?.profileId ?? '';
      const globalProfileId = boundProfileId || 'harness.global.default';
      // 只保留唯一 global profile 视图；历史 profile 数据保留可读但不进入列表。
      const listed = await assistantV2
        .request<{ profiles?: Profile[] }>('harness.profile.list', {})
        .catch(() => ({ profiles: [] as Profile[] }));
      const globalProfile =
        listed.profiles?.find((item) => item.id === globalProfileId) ??
        listed.profiles?.find((item) => item.kind === 'global_template') ??
        null;
      const nextProfiles = globalProfile ? [globalProfile] : (listed.profiles ?? []);
      const selected = requestedProfileId || globalProfileId;
      setProfiles(nextProfiles);
      setProjectIdentities(identities.items ?? []);
      setProjects(projectList);
      setProviders(providerList);
      setProfileId(selected);
      profileIdRef.current = selected;
      // 进入即预览唯一 Harness（无列表选择步骤）。
      if (requestedProfileId || !detailMode) setDetailMode('preview');
    } catch (cause) {
      setProfiles([]); fail(cause);
    } finally {
      setLoading(false);
    }
  }, [detailMode, fail]);

  useEffect(() => { void load(); }, [load]);
  useEffect(() => {
    if (detailMode && profileId) void loadDetail(profileId);
  }, [detailMode, loadDetail, profileId]);

  const loadRuns = useCallback(async (runId?: string) => {
    setAuxLoading(true);
    try {
      const result = await assistantV2.request<{ runs?: Run[] }>('run.list', {});
      const runs = result.runs ?? [];
      const selected = runId || selectedRunId || runs[0]?.id || '';
      setLiveRuns(runs);
      setSelectedRunId(selected);
      if (selected) {
        const [trace, snapshot] = await Promise.all([
          assistantV2.request<{ entries?: TraceEntry[] }>('harness.trace.list', { run_id: selected, limit: 200 }),
          assistantV2.request<RunSnapshot>('harness.run.getSnapshot', { run_id: selected }),
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
    const batcher = createHarnessNoticeBatcher({
      detailVisible: Boolean(detailMode),
      dirty,
      refreshWorkspace: () => void load(profileIdRef.current),
      refreshRuns: () => void loadRuns(),
      showRemoteChange: () => setNotice(t(locale, 'settings.engineEngineeringRemoteChange')),
    });
    const unsubscribe = assistantV2.subscribeHarness((event) => {
      batcher.notify(event);
    }, { onError: fail });
    return () => {
      unsubscribe();
      batcher.dispose();
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
    if (dirty) {
      setConfirmState({
        title: t(locale, 'settings.engineEngineeringDiscardTitle'),
        message: t(locale, 'settings.engineEngineeringDiscardConfirm'),
        onConfirm: () => {
          setConfirmState(null);
          selectProfile(id, mode, target);
        },
      });
      return;
    }
    setProfileId(id);
    profileIdRef.current = id;
    setDetailMode(mode);
    setWorkspaceTarget(target);
    setTargetStageId(null);
    setError(null);
    setNotice(null);
    setCanvasMode('understand');
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
    const saved = await assistantV2.request<{ revision: number }>('harness.draft.save', { profile_id: profileId, revision, document });
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
      const result = await assistantV2.request<Review>('harness.draft.review', { profile_id: profileId });
      setReview(result);
      setWorkspaceTarget('versions');
    } catch (cause) { fail(cause); } finally { setBusy(false); }
  };
  const publish = async (id = profileId) => {
    if (!id) return;
    setBusy(true); setError(null); setNotice(null);
    try {
      if (id === profileId && dirty) await persist();
      await assistantV2.request('harness.draft.publish', { profile_id: id, revision: id === profileId ? revision : undefined });
      await load(id);
      if (id === profileId) await loadDetail(id);
      setNotice(t(locale, 'settings.engineEngineeringPublished'));
    } catch (cause) { fail(cause); } finally { setBusy(false); }
  };
  const setDefaultProfile = async (id: string) => {
    setBusy(true); setError(null);
    try {
      await assistantV2.request('harness.binding.set', {
        scope_type: 'global', scope_id: 'global', profile_id: id, mode: 'follow_published',
      });
      setNotice(t(locale, 'settings.engineEngineeringDefaultSet'));
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
      const result = await assistantV2.request<Run>('run.start', {
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
  const nodeDetails = useMemo<Record<string, CanvasNodeDetail>>(() => buildNodeDetails({
    locale,
    stages,
    document,
    importedHooks,
    overlays,
    promptPreview,
    runSnapshot,
    selectedRun,
    traceEntries,
  }), [document, importedHooks, locale, overlays, promptPreview, runSnapshot, selectedRun, stages, traceEntries]);

  useEffect(() => {
    if (!detailMode || canvasMode !== 'audit' || !selectedRunId || !ACTIVE_RUN_STATUSES.has(selectedRunStatus)) return;
    // T218 (P2-004): polling must be >= R-P5's 5s floor and visibility-gated.
    // NOTE: `document` is a Blueprint state variable here, so use window.document.
    const interval = window.setInterval(() => void loadRuns(selectedRunId), 5_000);
    const onVisibility = () => {
      if (window.document.hidden) window.clearInterval(interval);
    };
    window.document.addEventListener('visibilitychange', onVisibility);
    return () => {
      window.clearInterval(interval);
      window.document.removeEventListener('visibilitychange', onVisibility);
    };
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

  const selectRun = (runId: string) => {
    setSelectedRunId(runId);
    if (runId) void loadRuns(runId);
    else { setTraceEntries([]); setRunSnapshot(null); }
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

  return {
    detailMode, setDetailMode, workspaceTarget, targetStageId, focusPromptId, canvasMode, setCanvasMode,
    profiles, loading, busy, error, notice, query, setQuery, filteredProfiles,
    projects, providers, projectIdentities, profileProject,
    selectedProfile, currentProject, workspace, document, revision, review, dirty, detailLoading, sourceCandidate,
    liveRuns, selectedRunId, setSelectedRunId, selectedRun, selectedRunStatus, traceEntries, runSnapshot,
    promptPreview, versions, auxLoading,
    findings, stages, hookStage, stageEvents, visibleHookIndexes, importedHooks, overlays, nodeDetails,
    runPrompt, setRunPrompt, runProviderId, setRunProviderId, runModelId, setRunModelId, runProjectPath, setRunProjectPath,
    runModels, runProvider,
    confirmState, dismissConfirm: () => setConfirmState(null),
    load, loadRuns, selectRun, selectProfile, save, reviewDraft, publish,
    setDefaultProfile, runFromCanvas, replaceDocument, updateHook, updateAdapter,
    updateOverlay, setNodeHookEnabled, authorizeNodeHook, removeNodeHook, removeNodePrompt, openCanvasWorkspace,
  };
}
