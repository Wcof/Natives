'use client';

import type { Locale } from '@/i18n';
import { t } from '@/i18n';
import type { EngineCapabilitySnapshot } from '../EngineCapabilitiesPanel';
import {
  traceEntriesForStage,
  type CanvasEdge,
  type CanvasNodeDetail,
  type CanvasNodeRunResult,
  type CanvasRunSnapshot,
  type CanvasStage,
  type CanvasTraceEntry,
  type CanvasWorkspaceTarget,
} from '../nativeExecutionCanvasModel';

export const EVENTS = ['SessionStart', 'SessionEnd', 'UserPromptSubmit', 'PreToolUse', 'PostToolUse', 'PostToolUseFailure', 'PermissionRequest', 'PermissionDenied', 'Notification', 'SubagentStart', 'SubagentStop', 'PreCompact', 'PostCompact', 'Stop', 'StopFailure', 'Error'];
export const ADAPTERS = ['command', 'http', 'mcp_tool', 'prompt', 'agent'] as const;
export const ACTIVE_RUN_STATUSES = new Set(['created', 'queued', 'preparing', 'running', 'waiting_permission', 'waiting_subagent', 'cancelling']);
export const BUILTIN_TOOL_NAMES = ['read_file', 'search_files', 'write_file', 'list_dir', 'grep', 'edit_file', 'run_terminal', 'apply_patch', 'memory_search', 'memory_get', 'task', 'task_output', 'kill_task', 'skill', 'web_search', 'enter_plan_mode', 'exit_plan_mode', 'write_draft_module', 'read_draft_module', 'rollback_draft_revision', 'lint_draft_module'];

// Localized labels for durable hook-invocation trace entries. These keys are
// only ever rendered from a real run_event HookInvocation — never from the Run
// row. A stage with no hook points (provider) or no evidence gets no row at
// all and therefore shows no_evidence.
const HOOK_EVENT_ACTION_KEYS: Record<string, string> = {
  SessionStart: 'settings.engineCanvasRunSessionStart',
  SessionEnd: 'settings.engineCanvasRunTerminalResult',
  UserPromptSubmit: 'settings.engineCanvasRunHookInvocation',
  PreToolUse: 'settings.engineCanvasRunHookInvocation',
  PostToolUse: 'settings.engineCanvasRunHookInvocation',
  PostToolUseFailure: 'settings.engineCanvasRunHookInvocation',
  PermissionRequest: 'settings.engineCanvasRunPermissionGate',
  PermissionDenied: 'settings.engineCanvasRunPermissionGate',
  Notification: 'settings.engineCanvasRunAuditSummary',
  SubagentStart: 'settings.engineCanvasRunSubagentStrategy',
  SubagentStop: 'settings.engineCanvasRunSubagentStrategy',
  PreCompact: 'settings.engineCanvasRunCompactCheck',
  PostCompact: 'settings.engineCanvasRunCompactCheck',
  Stop: 'settings.engineCanvasRunStopDecision',
  StopFailure: 'settings.engineCanvasRunStopDecision',
  Error: 'settings.engineCanvasRunTerminalResult',
};

export type AdapterType = typeof ADAPTERS[number];
export type DetailMode = 'preview' | 'edit' | 'create';
export type WorkspaceTarget = CanvasWorkspaceTarget | 'preview' | 'versions';
export type Profile = { id: string; name: string; kind: string; project_id?: string | null };
export type ProjectIdentity = { project_id: string; canonical_path: string; name: string };
export type Adapter = { type: AdapterType; [key: string]: unknown };
export type NativeHook = {
  id: string; name: string; enabled: boolean; event: string; order: number;
  matcher?: string; conditions: unknown[]; timeout_ms: number; failure_policy: string;
  adapter: Adapter; trust_confirmed: boolean;
};
export type HookOverlay = { hook_id: string; enabled?: boolean; order?: number; matcher?: string; timeout_ms?: number; failure_policy?: string };
export type Blueprint = {
  schema_version: number; hook_semantics_version: string; prompt_semantics_version: string;
  hooks: HookOverlay[]; hook_overlays?: HookOverlay[]; native_hooks: NativeHook[];
  prompt_blocks: Array<{ id: string; name: string; markdown: string; enabled: boolean; order: number; placement: string }>;
  builtin_prompt_replacements?: Array<{ surface_id: string; markdown: string; base_default_digest: string }>;
};
export type PromptBlock = Blueprint['prompt_blocks'][number];
export type CatalogHook = {
  id: string; event: string; stage: string; enabled: boolean; order: number; matcher?: string;
  timeout_ms: number; failure_policy: string; source: { scope: string; origin: string };
};
export type Workspace = {
  overview?: { issues?: unknown[] };
  topology?: { stages?: CanvasStage[]; edges?: CanvasEdge[] };
  catalog?: { hooks?: CatalogHook[] };
  prompt_plan?: { blocks?: Array<{ id: string; name: string }> };
};
export type Review = {
  validation?: { findings?: Array<{ severity?: string; code?: string; message?: string }> };
  diff?: Array<{ field?: string; from?: unknown; to?: unknown }>;
};
export type Run = {
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
export type TraceEntry = CanvasTraceEntry;
export type PromptPreview = {
  blocks?: Array<{ id: string; name: string; order: number; placement: string; source_digest: string; token_estimate: number }>;
  builtin_surfaces?: Array<{ surface_id: string; default_markdown: string; default_digest: string; effective_digest: string; replaced: boolean }>;
};
export type RunSnapshot = CanvasRunSnapshot;
export type VersionSummary = { id: string; version_number: number; canonical_hash: string; created_at: string };
export type DriftCandidate = { source_id: string; observed_digest: string; acknowledged?: boolean };

export const emptyBlueprint = (): Blueprint => ({
  schema_version: 4,
  hook_semantics_version: 'native.hooks.v1',
  prompt_semantics_version: 'native.prompts.v1',
  hooks: [],
  hook_overlays: [],
  native_hooks: [],
  prompt_blocks: [],
  builtin_prompt_replacements: [],
});

export const normalizeBlueprint = (raw?: Partial<Blueprint> | null): Blueprint => ({
  ...emptyBlueprint(),
  ...(raw ?? {}),
  hooks: Array.isArray(raw?.hooks) ? raw.hooks : [],
  hook_overlays: Array.isArray(raw?.hook_overlays) ? raw.hook_overlays : Array.isArray(raw?.hooks) ? raw.hooks : [],
  native_hooks: Array.isArray(raw?.native_hooks) ? raw.native_hooks : [],
  prompt_blocks: Array.isArray(raw?.prompt_blocks) ? raw.prompt_blocks : [],
  builtin_prompt_replacements: Array.isArray(raw?.builtin_prompt_replacements) ? raw.builtin_prompt_replacements : [],
});

export const adapterDefaults = (type: AdapterType): Adapter => {
  switch (type) {
    case 'command': return { type, program: '', args: [], working_dir_policy: 'project_root', secret_env_refs: {}, trusted: false, mode: 'exec' };
    case 'http': return { type, url: '', allow_hosts: [], headers: {}, secret_header_refs: {} };
    case 'mcp_tool': return { type, server_id: '', tool_name: '', input_template: '{"payload":"${input}"}' };
    case 'prompt': return { type, template: 'Return JSON: {"decision":"allow|deny","reason":"..."}' };
    case 'agent': return { type, prompt: '', max_steps: 5, readonly_tools: [] };
  }
};

export const newHook = (type: AdapterType, event = 'PreToolUse'): NativeHook => ({
  id: crypto.randomUUID(), name: `New ${type} Hook`, enabled: true, event,
  order: 0, matcher: '*', conditions: [], timeout_ms: 10_000, failure_policy: 'fail',
  adapter: adapterDefaults(type), trust_confirmed: false,
});

export const newPromptBlock = (): PromptBlock => ({
  id: crypto.randomUUID(), name: 'Prompt Block', markdown: '', enabled: true,
  order: 0, placement: 'after_project_instructions',
});

export const builtinToolDescription = (name: string, locale: Locale): string | undefined => {
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

export const promptSurfaceStage = (surfaceId: string): string => {
  const normalized = surfaceId.toLowerCase();
  if (normalized.includes('compact')) return 'compact';
  if (normalized.includes('stop')) return 'stop';
  return 'context';
};

export const promptBlockStage = (_placement: string): string => 'context';

export interface BuildNodeDetailsInput {
  locale: Locale;
  stages: CanvasStage[];
  document: Blueprint | null;
  importedHooks: CatalogHook[];
  overlays: HookOverlay[];
  promptPreview: PromptPreview | null;
  runSnapshot: CanvasRunSnapshot | null;
  selectedRun: Run | null;
  traceEntries: CanvasTraceEntry[];
}

export function buildNodeDetails({
  locale,
  stages,
  document,
  importedHooks,
  overlays,
  promptPreview,
  runSnapshot,
  selectedRun,
  traceEntries,
}: BuildNodeDetailsInput): Record<string, CanvasNodeDetail> {
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
        .filter((hook) => {
          const overlay = overlays.find((item) => item.hook_id === hook.id);
          return events.has(hook.event) && (overlay?.enabled ?? hook.enabled);
        })
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
        ...(promptPreview?.builtin_surfaces ?? []).filter((surface) => promptSurfaceStage(surface.surface_id) === stage.id).map((surface) => {
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
        ...(document?.prompt_blocks ?? []).filter((block) => promptBlockStage(block.placement) === stage.id).map((block) => ({
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
        origin: runSnapshot?.snapshot?.tool_plan?.tools ? t(locale, 'settings.engineCanvasToolOriginSnapshot') : t(locale, 'settings.engineCanvasToolOriginBuiltin'),
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
          reason: t(locale, 'settings.engineCanvasSubagentReasonUnresolved'),
          prompt: t(locale, 'settings.engineCanvasSubagentSnapshotRequired'),
        }] : []),
        ...(selectedRun && snapshot?.agentProfileId ? [{
          id: snapshot.agentProfileId,
          kind: 'expert' as const,
          source: t(locale, 'settings.engineCanvasSubagentSourceCapabilitySnapshot'),
          reason: t(locale, 'settings.engineCanvasSubagentReasonPreset'),
          prompt: t(locale, 'settings.engineCanvasPresetExpertPrompt', { id: snapshot.agentProfileId }),
        }] : []),
        ...(selectedRun && snapshot?.teamId ? [{
          id: snapshot.teamId,
          kind: 'team' as const,
          source: t(locale, 'settings.engineCanvasSubagentSourceCapabilitySnapshot'),
          reason: t(locale, 'settings.engineCanvasSubagentReasonPreset'),
          prompt: t(locale, 'settings.engineCanvasPresetTeamPrompt', { count: snapshot.teamMembers?.length ?? 0 }),
        }] : []),
        ...(selectedRun ? (snapshot?.teamMembers ?? []).map((id) => ({
          id,
          kind: 'member' as const,
          source: t(locale, 'settings.engineCanvasSubagentSourceCapabilitySnapshot'),
          reason: t(locale, 'settings.engineCanvasSubagentReasonTeamMember'),
          prompt: t(locale, 'settings.engineCanvasTeamMemberPrompt'),
        })) : []),
        ...(selectedRun && !snapshot?.agentProfileId && !snapshot?.teamId ? [{
          id: 'task',
          kind: 'dynamic' as const,
          source: t(locale, 'settings.engineCanvasSubagentSourceMasterTask'),
          reason: t(locale, 'settings.engineCanvasSubagentReasonDynamic'),
          prompt: t(locale, 'settings.engineCanvasDynamicSubagentPrompt'),
        }] : []),
      ]
      : [];
    const stageTraces = traceEntriesForStage(stage, traceEntries);
    const completedTraces = stageTraces.filter((entry) => entry.type === 'hook_invocation_completed');
    // NE-P0-06: `executed` is derived ONLY from durable evidence — the Harness
    // Snapshot and the run_event HookInvocation trace. Nothing here is inferred
    // from the Run row (selectedRun), so a stage with no hook points or no
    // evidence gets no rows and therefore shows no_evidence instead of being
    // presented as having executed.
    const runResults: CanvasNodeRunResult[] = [];
    // Durable Harness Snapshot evidence: the frozen prompt plan / tool plan.
    if (stage.id === 'context' && runSnapshot?.snapshot?.prompt_plan) {
      runResults.push({
        id: `${selectedRun?.id ?? 'run'}:prompt-plan`,
        action: t(locale, 'settings.engineCanvasRunPromptAssembly'),
        status: 'completed',
        output: t(locale, 'settings.engineCanvasPromptSnapshotDetail', {
          count: runSnapshot.snapshot.prompt_plan.source_digests?.length ?? 0,
          tokens: runSnapshot.snapshot.prompt_plan.token_estimate ?? 0,
        }),
      });
    }
    if ((stage.id === 'tool_gate' || stage.id === 'tool_execute') && runSnapshot?.snapshot?.tool_plan) {
      runResults.push({
        id: `${selectedRun?.id ?? 'run'}:tool-plan`,
        action: t(locale, 'settings.engineCanvasRunToolPlan'),
        status: 'completed',
        output: t(locale, 'settings.engineCanvasToolSnapshotDetail', {
          count: runSnapshot.snapshot.tool_plan.tools?.length ?? 0,
        }),
      });
    }
    // Durable HookInvocation evidence (run_event hook_invocation_*): one row
    // per trace entry actually dispatched for this stage.
    for (const entry of stageTraces) {
      const actionKey = HOOK_EVENT_ACTION_KEYS[entry.hook_event ?? ''];
      runResults.push({
        id: `${entry.run_id}:${entry.sequence}`,
        action: actionKey ? t(locale, actionKey) : (entry.hook_event ?? t(locale, 'settings.engineCanvasRunHookInvocation')),
        status: entry.type === 'hook_invocation_completed'
          ? (entry.status ?? 'completed')
          : (entry.type ?? 'running'),
        timestamp: entry.timestamp,
        duration_ms: entry.duration_ms,
        input: entry.input_summary ? `${entry.input_summary}${entry.input_truncated ? '…' : ''}` : undefined,
        output: entry.output_summary ? `${entry.output_summary}${entry.output_truncated ? '…' : ''}` : undefined,
        error: entry.error_category ?? undefined,
      });
    }
    // Cross-stage audit summary: only when durable evidence exists anywhere.
    if (stage.id === 'cross_stage' && completedTraces.length > 0) {
      runResults.push({
        id: `${selectedRun?.id ?? 'run'}:cross-stage`,
        action: t(locale, 'settings.engineCanvasRunAuditSummary'),
        status: 'completed',
        output: t(locale, 'settings.engineCanvasRunAuditDetail', {
          count: traceEntries.length,
          failed: traceEntries.filter((entry) => entry.status === 'failed' || Boolean(entry.error_category)).length,
        }),
      });
    }
    details[stage.id] = { hooks, prompts, tools, subagents, runResults };
  }
  return details;
}
