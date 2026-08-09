// ── Natives Usage Dashboard IPC Contract v2 (Snapshot-based) ──
// Supersedes the old UsageDashboardRequest with UsageViewRequest / UsageCacheReadResult.
// Frozen: do not rename fields without a coordinated backend + frontend update.

export type UsageRangePreset =
  | 'today'
  | '24h'
  | '7d'
  | '30d'
  | '90d'
  | 'custom';

export interface UsageViewRequest {
  preset: UsageRangePreset;
  timeZone: string;
  projectPath: string | null;
  customStartMs?: number;
  customEndMs?: number;
}

export interface UsageCacheMetadata {
  schemaVersion: number;
  generatedAtMs: number;
  coverageStartMs: number;
  coverageEndMs: number;
  timeZone: string;
}

export type UsageCacheReadResult =
  | {
      state: 'ready';
      metadata: UsageCacheMetadata;
      response: UsageDashboardResponse;
    }
  | {
      state: 'missing';
      metadata: null;
      response: null;
    };

export interface UsageSyncRequest {
  timeZone: string;
  currentView: UsageViewRequest;
}

export interface UsageSyncResult {
  metadata: UsageCacheMetadata;
  response: UsageDashboardResponse;
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
  | 'detected'
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

// ── Dashboard state model ──

export type DashboardState =
  | { kind: 'reading-cache' }
  | { kind: 'missing-cache' }
  | {
      kind: 'ready';
      data: UsageDashboardResponse;
      metadata: UsageCacheMetadata;
    };

// ── Snapshot change event (frozen contract: `usage:snapshot-changed`) ──

/** Cross-window event name emitted by the Host after a successful usage_sync. */
export const USAGE_SNAPSHOT_CHANGED_EVENT = 'usage:snapshot-changed';

export interface UsageSnapshotChangedPayload {
  channel: string;
  version: number;
  sequence: number;
  data: {
    timeZone: string;
    generatedAtMs: number;
    schemaVersion: number;
  };
}
