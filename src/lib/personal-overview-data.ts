// ── Personal Overview shared pure computation ──
// Moved from src/components/settings/personal-overview-data.ts so the Settings
// Personal Overview and the Menubar popup reuse the SAME calculation (frozen
// contract: "Shared calculation"). Pure functions only — no IPC, no window
// access. Function names / signatures are kept stable so Settings keeps working.

import type {
  UsageCacheMetadata,
  UsageDashboardResponse,
  UsageSourceState,
} from '@/types/usage';
import { aggregateUsageMetrics, buildDailyTrend } from '@/lib/usage-dashboard';

export interface PersonalOverviewUsageSummary {
  todayTokens: number | null;
  totalTokens: number | null;
  inputTokens: number | null;
  outputTokens: number | null;
  sessions: number;
  messages: number;
  averageMessagesPerSession: number | null;
  activeProjects: number;
}

export interface PersonalOverviewTrendPoint {
  date: string;
  totalTokens: number;
}

export function summarizeOverviewUsage(
  data: UsageDashboardResponse,
  today: string,
): PersonalOverviewUsageSummary {
  const metrics = aggregateUsageMetrics(data.daily, data.sessions, data.sources);
  let todayTokens: number | null = null;

  for (const record of data.daily) {
    if (record.date === today && record.totalTokens !== null) {
      todayTokens = (todayTokens ?? 0) + record.totalTokens;
    }
  }

  const projectIds = new Set<string>();
  for (const record of data.daily) {
    if (record.projectId) projectIds.add(record.projectId);
  }
  for (const session of data.sessions) {
    if (session.projectId) projectIds.add(session.projectId);
  }

  const messages = metrics.totalUserMessages + metrics.totalAssistantMessages;

  return {
    todayTokens,
    totalTokens: metrics.totalTokens,
    inputTokens: metrics.totalInputTokens,
    outputTokens: metrics.totalOutputTokens,
    sessions: metrics.totalSessions,
    messages,
    averageMessagesPerSession:
      metrics.totalSessions > 0 ? messages / metrics.totalSessions : null,
    activeProjects: projectIds.size,
  };
}

export function buildOverviewTrend(
  data: UsageDashboardResponse,
): PersonalOverviewTrendPoint[] {
  return buildDailyTrend(data.daily, data.activity)
    .filter((point): point is typeof point & { totalTokens: number } => point.totalTokens !== null)
    .map(({ date, totalTokens }) => ({ date, totalTokens }));
}

// ── Coverage / staleness semantics (data honesty) ──
// No fake zeros: when there is no snapshot, a source is only partially
// covered, or the cache is stale, these helpers report the truth so the UI can
// label it instead of pretending the numbers are complete.

export type OverviewCoverageStatus = 'fresh' | 'stale' | 'partial' | 'missing';

export interface OverviewSourceCoverage {
  sourceId: string;
  label: string;
  state: UsageSourceState;
  /** True when this source did NOT contribute complete data to the snapshot. */
  incomplete: boolean;
}

export interface OverviewCoverageInfo {
  status: OverviewCoverageStatus;
  /** generatedAtMs from the snapshot metadata; null when no snapshot. */
  updatedAtMs: number | null;
  /** Milliseconds since the snapshot was generated; null when no snapshot. */
  stalenessMs: number | null;
  /** Per-source coverage states, sorted by sourceId. */
  sources: OverviewSourceCoverage[];
  /** Plain-text source-state summary for accessible labeling. */
  sourceStateSummary: string;
}

/** A snapshot newer than this is considered fresh. */
export const OVERVIEW_FRESH_WINDOW_MS = 5 * 60 * 1000;

const PARTIAL_STATES: ReadonlySet<UsageSourceState> = new Set(['partial', 'unavailable']);

export function overviewCoverageStatus(
  data: UsageDashboardResponse | null,
  metadata: UsageCacheMetadata | null,
  nowMs: number = Date.now(),
): OverviewCoverageInfo {
  if (!data) {
    return {
      status: 'missing',
      updatedAtMs: null,
      stalenessMs: null,
      sources: [],
      sourceStateSummary: 'missing',
    };
  }

  const sources: OverviewSourceCoverage[] = [...data.sources]
    .sort((a, b) => a.id.localeCompare(b.id))
    .map((source) => ({
      sourceId: source.id,
      label: source.label,
      state: source.state,
      incomplete: source.state !== 'ok',
    }));

  const hasPartial = sources.some((source) => PARTIAL_STATES.has(source.state));

  const updatedAtMs = metadata?.generatedAtMs ?? data.generatedAtMs ?? null;
  const stalenessMs = updatedAtMs === null ? null : Math.max(0, nowMs - updatedAtMs);
  const stale = stalenessMs !== null && stalenessMs > OVERVIEW_FRESH_WINDOW_MS;

  let status: OverviewCoverageStatus;
  if (hasPartial) {
    status = 'partial';
  } else if (stale) {
    status = 'stale';
  } else {
    status = 'fresh';
  }

  const total = sources.length;
  const complete = sources.filter((source) => !source.incomplete).length;
  const sourceStateSummary =
    total === 0
      ? 'no sources reported'
      : `${complete}/${total} sources complete; ${sources
          .map((source) => `${source.label} (${source.state})`)
          .join(', ')}`;

  return { status, updatedAtMs, stalenessMs, sources, sourceStateSummary };
}
