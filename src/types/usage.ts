// ── Natives Usage Dashboard IPC Contract ──
// Frozen: do not rename fields without a coordinated backend + frontend update.

export interface UsageDashboardRequest {
  startMs: number;
  endMs: number;
  force: boolean;
  includeComparison: boolean;
  timeZone: string;
}

export interface UsagePeriodData {
  range: {
    startMs: number;
    endMs: number;
  };
  daily: UsageDailyRecord[];
  activity: UsageActivityBucket[];
  sessions: UsageSessionRecord[];
}

export type UsageQuality =
  | 'reported'
  | 'estimated'
  | 'unavailable';

export type UsageSourceState =
  | 'ok'
  | 'partial'
  | 'unavailable';

export interface UsageDimension {
  id: string;
  label: string;
}

export interface UsageDailyRecord {
  date: string;
  sourceId: string;
  modelId: string | null;
  projectId: string | null;
  terminalId: string | null;

  inputTokens: number | null;
  outputTokens: number | null;
  cacheCreationTokens: number | null;
  cacheReadTokens: number | null;
  totalTokens: number | null;

  costUsd: number | null;
  costQuality: UsageQuality;
}

export interface UsageActivityBucket {
  hourStartMs: number;
  sourceId: string;
  modelId: string | null;
  projectId: string | null;
  terminalId: string | null;

  totalTokens: number | null;
  userMessages: number;
  assistantMessages: number;
  activeSeconds: number | null;
}

export interface UsageSessionRecord {
  sessionId: string;
  sourceId: string;
  modelId: string | null;
  projectId: string | null;
  terminalId: string | null;

  startedAtMs: number;
  endedAtMs: number;
  userMessages: number;
  assistantMessages: number;
  activeSeconds: number | null;
  durationQuality: UsageQuality;
}

export interface UsageBreadcrumb {
  kind: 'cli' | 'raw_log' | 'database';
  label: string;
}

export interface UsageSourceStatus {
  id: string;
  label: string;
  kind: 'natives' | 'external';
  state: UsageSourceState;
  breadcrumbs: UsageBreadcrumb[];

  capabilities: {
    totalTokens: boolean;
    tokenBreakdown: boolean;
    cache: boolean;
    cost: boolean;
    hourly: boolean;
    project: boolean;
    messages: boolean;
    sessions: boolean;
    duration: boolean;
  };

  durationMethod:
    | 'event_gap_estimate'
    | 'session_bounds'
    | null;
}

export type UsageWarningCode =
  | 'CLI_NOT_FOUND'
  | 'CLI_TIMEOUT'
  | 'SOURCE_UNAVAILABLE'
  | 'SOURCE_PARSE_PARTIAL'
  | 'TOTAL_MISMATCH'
  | 'COST_UNAVAILABLE'
  | 'NATIVES_HISTORY_PARTIAL';

export interface UsageWarning {
  sourceId: string | null;
  code: UsageWarningCode;
  details: Record<string, string | number>;
}

export interface RtkSummary {
  totalSavedTokens: number;
  totalCommands: number;
}

export interface UsageDashboardResponse {
  generatedAtMs: number;

  range: {
    startMs: number;
    endMs: number;
  };

  daily: UsageDailyRecord[];
  activity: UsageActivityBucket[];
  sessions: UsageSessionRecord[];

  comparison: UsagePeriodData | null;

  dimensions: {
    sources: UsageDimension[];
    models: UsageDimension[];
    projects: UsageDimension[];
    terminals: UsageDimension[];
  };

  sources: UsageSourceStatus[];

  rtk: {
    totalSavedTokens: number;
    totalCommands: number;
  } | null;

  warnings: UsageWarning[];
}

// ── Aggregate metrics (computed on frontend) ──

export interface UsageMetrics {
  estimatedCost: number | null;
  costCoverage: number | null;
  totalTokens: number | null;
  totalInputTokens: number | null;
  totalOutputTokens: number | null;
  totalCacheReadTokens: number | null;
  totalCacheCreationTokens: number | null;
  totalSessions: number;
  totalUserMessages: number;
  totalAssistantMessages: number;
  estimatedActiveSeconds: number | null;
  sessionSpanMs: number | null;
  coveredSources: number;
  totalSources: number;
}

export interface DailyTrendPoint {
  date: string;
  totalTokens: number | null;
  inputTokens?: number | null;
  outputTokens?: number | null;
  cacheReadTokens?: number | null;
  costUsd: number | null;
  activeSeconds: number | null;
}

export interface HourlyHeatmapPoint {
  hour: number;
  dayOfWeek: number;
  totalTokens: number | null;
  activeSeconds: number | null;
}

export interface SourceDistributionItem {
  sourceId: string;
  label: string;
  totalTokens: number | null;
  percentage: number | null;
}

export interface ModelDistributionItem {
  modelId: string | null;
  label: string;
  totalTokens: number | null;
  percentage: number | null;
}