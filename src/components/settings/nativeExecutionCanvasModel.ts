import { t, type Locale } from '@/i18n';

export type HookPoint = {
  event: string;
  enabled_hook_count?: number;
  hook_count?: number;
  hook_ids?: string[];
  security_sensitive?: boolean;
  dispatched?: boolean;
  dispatch_module?: string | null;
};

export type CanvasStage = {
  id: string;
  order?: number;
  hook_points?: HookPoint[];
  safe_points?: string[];
};

export type CanvasEdgeKind = 'flow' | 'branch' | 'loop' | 'signal';

export type CanvasEdge = {
  from: string;
  to: string;
  kind?: CanvasEdgeKind;
};

export type PromptBlock = { id: string; name: string };

export type CanvasNodeHook = {
  id: string;
  name: string;
  event: string;
  source: string;
  enabled: boolean;
  authorized: boolean;
  canAuthorize?: boolean;
  canRemove?: boolean;
};

export type CanvasNodePrompt = {
  id: string;
  name: string;
  source?: string;
  placement?: string;
  enabled?: boolean;
  markdown?: string;
  canEdit?: boolean;
  canRemove?: boolean;
};

export type CanvasNodeTool = {
  name: string;
  source: string;
  schema_digest?: string;
  description?: string;
};

export type CanvasNodeSubagent = {
  id: string;
  kind: 'expert' | 'team' | 'member' | 'dynamic' | 'unresolved';
  source?: string;
  prompt?: string;
};

export type CanvasNodeRunResult = {
  id: string;
  action: string;
  status?: string;
  timestamp?: string;
  duration_ms?: number;
  input?: string;
  output?: string;
};

export type CanvasNodeDetail = {
  hooks?: CanvasNodeHook[];
  prompts?: CanvasNodePrompt[];
  tools?: CanvasNodeTool[];
  subagents?: CanvasNodeSubagent[];
  runResults?: CanvasNodeRunResult[];
};

export type CanvasRun = {
  id: string;
  status: string;
  provider_id?: string;
  model_id?: string;
  started_at?: string | null;
};

export type CanvasTraceEntry = {
  run_id: string;
  sequence: number;
  timestamp: string;
  type?: string;
  invocation_id?: string;
  hook_id?: string;
  hook_event?: string;
  source?: string;
  ordinal?: number;
  status?: string;
  effective_decision?: string | null;
  error_category?: string | null;
  duration_ms?: number;
  input_summary?: string;
  input_truncated?: boolean;
  output_summary?: string;
  output_truncated?: boolean;
};

export type PromptLayerEvidence = {
  layer_id: string;
  kind: string;
  source_owner: string;
  digest: string;
  char_estimate: number;
};

export type CanvasRunSnapshot = {
  resolved: boolean;
  canonical_hash?: string;
  snapshot?: {
    enabled_hook_ids?: string[];
    prompt_plan?: {
      token_estimate?: number;
      source_digests?: string[];
      layers?: PromptLayerEvidence[];
      effective_prompt_hash?: string;
    };
    tool_plan?: {
      canonical_hash?: string;
      tools?: Array<{ name: string; source: string; schema_digest: string }>;
    };
  };
};

export type CanvasMode = 'understand' | 'audit';
export type CanvasWorkspaceTarget = 'blueprint' | 'hooks' | 'prompts' | 'runs' | 'capabilities';
export type StageGroupId = 'prepare' | 'decide' | 'execute' | 'finish' | 'observe' | 'other';
export type StageIconId =
  | 'session'
  | 'context'
  | 'provider'
  | 'tool_gate'
  | 'permission'
  | 'tool_execute'
  | 'subagent'
  | 'compact'
  | 'stop'
  | 'terminal'
  | 'cross_stage'
  | 'unknown';

type StageMeta = {
  titleKey: string;
  descriptionKey: string;
  whyKey: string;
  technicalName: string;
  group: StageGroupId;
  icon: StageIconId;
  targets: CanvasWorkspaceTarget[];
};

const STAGE_META: Record<string, StageMeta> = {
  session: {
    titleKey: 'settings.engineCanvasStageSessionTitle',
    descriptionKey: 'settings.engineCanvasStageSessionDesc',
    whyKey: 'settings.engineCanvasStageSessionWhy',
    technicalName: 'Session',
    group: 'prepare',
    icon: 'session',
    targets: ['blueprint', 'hooks'],
  },
  context: {
    titleKey: 'settings.engineCanvasStageContextTitle',
    descriptionKey: 'settings.engineCanvasStageContextDesc',
    whyKey: 'settings.engineCanvasStageContextWhy',
    technicalName: 'Context & Prompt',
    group: 'prepare',
    icon: 'context',
    targets: ['prompts', 'blueprint'],
  },
  provider: {
    titleKey: 'settings.engineCanvasStageProviderTitle',
    descriptionKey: 'settings.engineCanvasStageProviderDesc',
    whyKey: 'settings.engineCanvasStageProviderWhy',
    technicalName: 'Provider',
    group: 'decide',
    icon: 'provider',
    targets: ['capabilities', 'runs'],
  },
  tool_gate: {
    titleKey: 'settings.engineCanvasStageToolGateTitle',
    descriptionKey: 'settings.engineCanvasStageToolGateDesc',
    whyKey: 'settings.engineCanvasStageToolGateWhy',
    technicalName: 'Tool Gate',
    group: 'decide',
    icon: 'tool_gate',
    targets: ['hooks', 'capabilities'],
  },
  permission: {
    titleKey: 'settings.engineCanvasStagePermissionTitle',
    descriptionKey: 'settings.engineCanvasStagePermissionDesc',
    whyKey: 'settings.engineCanvasStagePermissionWhy',
    technicalName: 'Permission',
    group: 'execute',
    icon: 'permission',
    targets: ['hooks', 'capabilities'],
  },
  tool_execute: {
    titleKey: 'settings.engineCanvasStageToolExecuteTitle',
    descriptionKey: 'settings.engineCanvasStageToolExecuteDesc',
    whyKey: 'settings.engineCanvasStageToolExecuteWhy',
    technicalName: 'Tool Execute',
    group: 'execute',
    icon: 'tool_execute',
    targets: ['hooks', 'capabilities'],
  },
  subagent: {
    titleKey: 'settings.engineCanvasStageSubagentTitle',
    descriptionKey: 'settings.engineCanvasStageSubagentDesc',
    whyKey: 'settings.engineCanvasStageSubagentWhy',
    technicalName: 'Sub-agent',
    group: 'execute',
    icon: 'subagent',
    targets: ['capabilities', 'hooks'],
  },
  compact: {
    titleKey: 'settings.engineCanvasStageCompactTitle',
    descriptionKey: 'settings.engineCanvasStageCompactDesc',
    whyKey: 'settings.engineCanvasStageCompactWhy',
    technicalName: 'Compaction',
    group: 'finish',
    icon: 'compact',
    targets: ['hooks', 'prompts'],
  },
  stop: {
    titleKey: 'settings.engineCanvasStageStopTitle',
    descriptionKey: 'settings.engineCanvasStageStopDesc',
    whyKey: 'settings.engineCanvasStageStopWhy',
    technicalName: 'Stop',
    group: 'finish',
    icon: 'stop',
    targets: ['hooks', 'prompts'],
  },
  terminal: {
    titleKey: 'settings.engineCanvasStageTerminalTitle',
    descriptionKey: 'settings.engineCanvasStageTerminalDesc',
    whyKey: 'settings.engineCanvasStageTerminalWhy',
    technicalName: 'Terminal',
    group: 'finish',
    icon: 'terminal',
    targets: ['hooks', 'runs'],
  },
  cross_stage: {
    titleKey: 'settings.engineCanvasStageCrossTitle',
    descriptionKey: 'settings.engineCanvasStageCrossDesc',
    whyKey: 'settings.engineCanvasStageCrossWhy',
    technicalName: 'Cross-stage',
    group: 'observe',
    icon: 'cross_stage',
    targets: ['hooks', 'runs'],
  },
};

export const CANONICAL_STAGE_IDS = Object.freeze(Object.keys(STAGE_META));

const GROUP_KEYS: Record<StageGroupId, string> = {
  prepare: 'settings.engineCanvasGroupPrepare',
  decide: 'settings.engineCanvasGroupDecide',
  execute: 'settings.engineCanvasGroupExecute',
  finish: 'settings.engineCanvasGroupFinish',
  observe: 'settings.engineCanvasGroupObserve',
  other: 'settings.engineCanvasGroupOther',
};

export type StagePresentation = {
  title: string;
  description: string;
  why: string;
  technicalName: string;
  group: StageGroupId;
  groupLabel: string;
  icon: StageIconId;
  targets: CanvasWorkspaceTarget[];
};

export function stagePresentation(id: string, locale: Locale): StagePresentation {
  const meta = STAGE_META[id];
  if (!meta) {
    return {
      title: id,
      description: t(locale, 'settings.engineCanvasUnknownStageDesc'),
      why: t(locale, 'settings.engineCanvasUnknownStageWhy'),
      technicalName: id,
      group: 'other',
      groupLabel: t(locale, GROUP_KEYS.other),
      icon: 'unknown',
      targets: [],
    };
  }
  return {
    title: t(locale, meta.titleKey),
    description: t(locale, meta.descriptionKey),
    why: t(locale, meta.whyKey),
    technicalName: meta.technicalName,
    group: meta.group,
    groupLabel: t(locale, GROUP_KEYS[meta.group]),
    icon: meta.icon,
    targets: meta.targets,
  };
}

export function stageLabel(id: string, locale: Locale): string {
  return stagePresentation(id, locale).title;
}

export function sortStages(stages: CanvasStage[]): CanvasStage[] {
  return [...stages].sort((left, right) => {
    const order = (left.order ?? Number.MAX_SAFE_INTEGER) - (right.order ?? Number.MAX_SAFE_INTEGER);
    return order || left.id.localeCompare(right.id);
  });
}

export const NODE_WIDTH = 190;
export const NODE_HEIGHT = 112;

const KNOWN_POSITIONS: Record<string, { x: number; y: number }> = {
  session: { x: 44, y: 82 },
  context: { x: 250, y: 82 },
  provider: { x: 500, y: 82 },
  tool_gate: { x: 706, y: 82 },
  cross_stage: { x: 44, y: 300 },
  subagent: { x: 500, y: 300 },
  permission: { x: 706, y: 300 },
  tool_execute: { x: 912, y: 300 },
  compact: { x: 706, y: 518 },
  stop: { x: 912, y: 518 },
  terminal: { x: 1118, y: 518 },
};

const GROUP_ANCHORS: Record<Exclude<StageGroupId, 'other'>, { x: number; y: number }> = {
  prepare: { x: 44, y: 38 },
  decide: { x: 500, y: 38 },
  execute: { x: 500, y: 256 },
  finish: { x: 706, y: 474 },
  observe: { x: 44, y: 256 },
};

export type CanvasLayout = {
  positions: Map<string, { x: number; y: number }>;
  groups: Array<{ id: StageGroupId; label: string; x: number; y: number }>;
  width: number;
  height: number;
};

export function layoutStages(stages: CanvasStage[], locale: Locale): CanvasLayout {
  const ordered = sortStages(stages);
  const positions = new Map<string, { x: number; y: number }>();
  const unknown = ordered.filter((stage) => !KNOWN_POSITIONS[stage.id]);

  for (const stage of ordered) {
    const known = KNOWN_POSITIONS[stage.id];
    if (known) positions.set(stage.id, known);
  }

  const unknownTop = unknown.length > 0 ? 736 : 0;
  unknown.forEach((stage, index) => {
    positions.set(stage.id, {
      x: 44 + (index % 6) * 206,
      y: unknownTop + Math.floor(index / 6) * 150,
    });
  });

  const activeGroups = new Set(ordered.map((stage) => stagePresentation(stage.id, locale).group));
  const groups: Array<{ id: StageGroupId; label: string; x: number; y: number }> = (Object.entries(GROUP_ANCHORS) as Array<[
    Exclude<StageGroupId, 'other'>,
    { x: number; y: number },
  ]>)
    .filter(([id]) => activeGroups.has(id))
    .map(([id, anchor]) => ({ id, label: t(locale, GROUP_KEYS[id]), ...anchor }));

  if (unknown.length > 0) {
    groups.push({ id: 'other', label: t(locale, GROUP_KEYS.other), x: 44, y: unknownTop - 44 });
  }

  let maxX = 0;
  let maxY = 0;
  positions.forEach(({ x, y }) => {
    maxX = Math.max(maxX, x + NODE_WIDTH);
    maxY = Math.max(maxY, y + NODE_HEIGHT);
  });

  return {
    positions,
    groups,
    width: Math.max(1352, maxX + 44),
    height: Math.max(674, maxY + 44),
  };
}

export function visibleEdges(stages: CanvasStage[], edges: CanvasEdge[]): CanvasEdge[] {
  const stageIds = new Set(stages.map((stage) => stage.id));
  return edges.filter((edge) => stageIds.has(edge.from) && stageIds.has(edge.to));
}

export function edgeRunResultCount(edge: CanvasEdge, nodeDetails: Record<string, CanvasNodeDetail>): number {
  return Math.max(
    nodeDetails[edge.from]?.runResults?.length ?? 0,
    nodeDetails[edge.to]?.runResults?.length ?? 0,
  );
}

export type StageEvidenceCode =
  | 'loaded'
  | 'configured'
  | 'attention'
  | 'choose_run'
  | 'insufficient'
  | 'no_evidence'
  | 'running'
  | 'hook_evidence'
  | 'snapshot_recorded'
  | 'failed';

export function traceEntriesForStage(
  stage: CanvasStage,
  traceEntries: CanvasTraceEntry[],
): CanvasTraceEntry[] {
  const events = new Set((stage.hook_points ?? []).map((point) => point.event));
  return traceEntries.filter((entry) => entry.hook_event && events.has(entry.hook_event));
}

export function stageEvidenceCode(input: {
  mode: CanvasMode;
  stage: CanvasStage;
  promptBlockCount: number;
  selectedRunId?: string;
  traceEntries?: CanvasTraceEntry[];
  runSnapshot?: CanvasRunSnapshot | null;
}): StageEvidenceCode {
  const { mode, stage, promptBlockCount, selectedRunId, traceEntries = [], runSnapshot } = input;
  if (mode === 'understand') {
    if ((stage.hook_points ?? []).some((point) => point.dispatched === false)) return 'attention';
    const enabledHooks = (stage.hook_points ?? []).reduce(
      (total, point) => total + (point.enabled_hook_count ?? 0),
      0,
    );
    if (enabledHooks > 0 || (stage.id === 'context' && promptBlockCount > 0)) return 'configured';
    return 'loaded';
  }

  if (!selectedRunId) return 'choose_run';
  if (runSnapshot?.resolved === false) return 'insufficient';

  const entries = traceEntriesForStage(stage, traceEntries);
  const completed = entries.filter((entry) => entry.type === 'hook_invocation_completed');
  if (completed.some((entry) => entry.status === 'failed' || Boolean(entry.error_category))) {
    return 'failed';
  }
  const completedIds = new Set(completed.map((entry) => entry.invocation_id).filter(Boolean));
  const hasOpenInvocation = entries.some(
    (entry) => entry.type === 'hook_invocation_started' && !completedIds.has(entry.invocation_id),
  );
  if (hasOpenInvocation) return 'running';
  if (completed.length > 0) return 'hook_evidence';
  if (stage.id === 'context' && runSnapshot?.snapshot?.prompt_plan) return 'snapshot_recorded';
  if ((stage.id === 'tool_gate' || stage.id === 'tool_execute') && runSnapshot?.snapshot?.tool_plan) return 'snapshot_recorded';
  return 'no_evidence';
}

export function enabledHookCount(stage: CanvasStage): number {
  return (stage.hook_points ?? []).reduce(
    (total, point) => total + (point.enabled_hook_count ?? 0),
    0,
  );
}

export function edgeLabelKey(edge: CanvasEdge): string | null {
  if (edge.kind === 'loop') return 'settings.engineCanvasEdgeContinue';
  if (edge.kind === 'signal') return 'settings.engineCanvasEdgeNotify';
  if (edge.from === 'tool_gate' && edge.to === 'permission') {
    return 'settings.engineCanvasEdgeSensitive';
  }
  if (edge.from === 'tool_gate' && edge.to === 'subagent') {
    return 'settings.engineCanvasEdgeDelegate';
  }
  if (edge.from === 'stop' && edge.to === 'terminal') {
    return 'settings.engineCanvasEdgeFinish';
  }
  return null;
}
