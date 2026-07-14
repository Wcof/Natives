// ── Frontend-only pure filter & aggregation functions ──
// These operate on UsageDashboardResponse data. They are pure functions
// (no side effects, no IPC, no window access), making them testable.

import type {
  UsageDashboardResponse,
  UsageDailyRecord,
  UsageActivityBucket,
  UsageSessionRecord,
  UsageMetrics,
  DailyTrendPoint,
  HourlyHeatmapPoint,
  SourceDistributionItem,
  ModelDistributionItem,
  UsageSourceStatus,
} from '@/types/usage';

// ── Filtering ──

export function filterUsageRecords(
  data: UsageDashboardResponse,
  sourceIds: string[] | null,
  modelIds: string[] | null,
  projectIds: string[] | null,
  terminalIds: string[] | null,
): {
  daily: UsageDailyRecord[];
  activity: UsageActivityBucket[];
  sessions: UsageSessionRecord[];
} {
  const hasSourceFilter = sourceIds && sourceIds.length > 0;
  const hasModelFilter = modelIds && modelIds.length > 0;
  const hasProjectFilter = projectIds && projectIds.length > 0;
  const hasTerminalFilter = terminalIds && terminalIds.length > 0;

  const daily = data.daily.filter((r) => {
    if (hasSourceFilter && !sourceIds!.includes(r.sourceId)) return false;
    if (hasModelFilter && (r.modelId === null || !modelIds!.includes(r.modelId))) return false;
    if (hasProjectFilter && (r.projectId === null || !projectIds!.includes(r.projectId))) return false;
    if (hasTerminalFilter && (r.terminalId === null || !terminalIds!.includes(r.terminalId))) return false;
    return true;
  });

  const activity = data.activity.filter((a) => {
    if (hasSourceFilter && !sourceIds!.includes(a.sourceId)) return false;
    if (hasModelFilter && (a.modelId === null || !modelIds!.includes(a.modelId))) return false;
    if (hasProjectFilter && (a.projectId === null || !projectIds!.includes(a.projectId))) return false;
    if (hasTerminalFilter && (a.terminalId === null || !terminalIds!.includes(a.terminalId))) return false;
    return true;
  });

  const sessions = data.sessions.filter((s) => {
    if (hasSourceFilter && !sourceIds!.includes(s.sourceId)) return false;
    if (hasModelFilter && (s.modelId === null || !modelIds!.includes(s.modelId))) return false;
    if (hasProjectFilter && (s.projectId === null || !projectIds!.includes(s.projectId))) return false;
    if (hasTerminalFilter && (s.terminalId === null || !terminalIds!.includes(s.terminalId))) return false;
    return true;
  });

  return { daily, activity, sessions };
}

// ── Aggregation ──

export function aggregateUsageMetrics(
  daily: UsageDailyRecord[],
  sessions: UsageSessionRecord[],
  sources: UsageDashboardResponse['sources'],
): UsageMetrics {
  // Compute token metrics: nullable values must NOT be treated as 0
  let totalTokens: number | null = null;
  let totalInputTokens: number | null = null;
  let totalOutputTokens: number | null = null;
  let totalCacheReadTokens: number | null = null;
  let totalCacheCreationTokens: number | null = null;
  let totalCost: number | null = null;
  let costKnownTokens = 0;
  let totalKnownTokens = 0;

  for (const r of daily) {
    if (r.totalTokens !== null) {
      totalTokens = (totalTokens ?? 0) + r.totalTokens;
    }
    if (r.inputTokens !== null) {
      totalInputTokens = (totalInputTokens ?? 0) + r.inputTokens;
    }
    if (r.outputTokens !== null) {
      totalOutputTokens = (totalOutputTokens ?? 0) + r.outputTokens;
    }
    if (r.cacheReadTokens !== null) {
      totalCacheReadTokens = (totalCacheReadTokens ?? 0) + r.cacheReadTokens;
    }
    if (r.cacheCreationTokens !== null) {
      totalCacheCreationTokens = (totalCacheCreationTokens ?? 0) + r.cacheCreationTokens;
    }

    // Track cost coverage
    if (r.totalTokens !== null) {
      totalKnownTokens += r.totalTokens;
      if (r.costUsd !== null) {
        totalCost = (totalCost ?? 0) + r.costUsd;
        costKnownTokens += r.totalTokens;
      }
    }
  }

  // Session metrics (deduplicated by sourceId + sessionId)
  const seenSessions = new Set<string>();
  let totalSessions = 0;
  let totalUserMessages = 0;
  let totalAssistantMessages = 0;
  let estimatedActiveSeconds: number | null = null;
  let minSessionStart = Infinity;
  let maxSessionEnd = -Infinity;

  for (const s of sessions) {
    const key = `${s.sourceId}:${s.sessionId}`;
    if (seenSessions.has(key)) continue;
    seenSessions.add(key);
    totalSessions++;

    totalUserMessages += s.userMessages;
    totalAssistantMessages += s.assistantMessages;

    if (s.activeSeconds !== null) {
      estimatedActiveSeconds = (estimatedActiveSeconds ?? 0) + s.activeSeconds;
    }

    if (s.startedAtMs < minSessionStart) minSessionStart = s.startedAtMs;
    if (s.endedAtMs > maxSessionEnd) maxSessionEnd = s.endedAtMs;
  }

  const sessionSpanMs =
    maxSessionEnd > minSessionStart && isFinite(minSessionStart) && isFinite(maxSessionEnd)
      ? maxSessionEnd - minSessionStart
      : null;

  // Cost coverage: tokens with known cost / total known tokens
  const costCoverage =
    totalKnownTokens > 0 ? costKnownTokens / totalKnownTokens : null;

  // Covered / total sources
  const coveredSources = sources.filter((s) => s.state === 'ok' || s.state === 'partial').length;
  const totalSources = sources.length;

  return {
    estimatedCost: totalCost,
    costCoverage,
    totalTokens,
    totalInputTokens,
    totalOutputTokens,
    totalCacheReadTokens,
    totalCacheCreationTokens,
    totalSessions,
    totalUserMessages,
    totalAssistantMessages,
    estimatedActiveSeconds,
    sessionSpanMs,
    coveredSources,
    totalSources,
  };
}

// ── Trend ──

export function buildDailyTrend(
  daily: UsageDailyRecord[],
  activity: UsageActivityBucket[],
): DailyTrendPoint[] {
  const dateMap = new Map<string, DailyTrendPoint>();

  for (const r of daily) {
    const existing = dateMap.get(r.date) ?? {
      date: r.date,
      totalTokens: null as number | null,
      inputTokens: null as number | null,
      outputTokens: null as number | null,
      cacheReadTokens: null as number | null,
      costUsd: null as number | null,
      activeSeconds: null as number | null,
    };
    if (r.totalTokens !== null) {
      existing.totalTokens = (existing.totalTokens ?? 0) + r.totalTokens;
    }
    if (r.inputTokens !== null) {
      existing.inputTokens = (existing.inputTokens ?? 0) + r.inputTokens;
    }
    if (r.outputTokens !== null) {
      existing.outputTokens = (existing.outputTokens ?? 0) + r.outputTokens;
    }
    if (r.cacheReadTokens !== null) {
      existing.cacheReadTokens = (existing.cacheReadTokens ?? 0) + r.cacheReadTokens;
    }
    if (r.costUsd !== null) {
      existing.costUsd = (existing.costUsd ?? 0) + r.costUsd;
    }
    dateMap.set(r.date, existing);
  }

  // Add activity seconds per date
  for (const a of activity) {
    const date = new Date(a.hourStartMs).toISOString().slice(0, 10);
    const existing = dateMap.get(date);
    if (existing && a.activeSeconds !== null) {
      existing.activeSeconds = (existing.activeSeconds ?? 0) + a.activeSeconds;
    }
  }

  return Array.from(dateMap.values()).sort((a, b) => a.date.localeCompare(b.date));
}

// ── Heatmap ──

export function buildHourlyHeatmap(
  activity: UsageActivityBucket[],
): HourlyHeatmapPoint[] {
  const map = new Map<string, HourlyHeatmapPoint>();

  for (const a of activity) {
    if (a.hourStartMs === 0) continue;

    const date = new Date(a.hourStartMs);
    const hour = date.getUTCHours();
    const dayOfWeek = date.getUTCDay(); // 0=Sun, 6=Sat
    const key = `${dayOfWeek}-${hour}`;

    const existing = map.get(key) ?? {
      hour,
      dayOfWeek,
      totalTokens: null as number | null,
      activeSeconds: null as number | null,
    };

    if (a.totalTokens !== null) {
      existing.totalTokens = (existing.totalTokens ?? 0) + a.totalTokens;
    }
    if (a.activeSeconds !== null) {
      existing.activeSeconds = (existing.activeSeconds ?? 0) + a.activeSeconds;
    }

    map.set(key, existing);
  }

  return Array.from(map.values());
}

// ── Source distribution ──

export function buildSourceDistribution(
  daily: UsageDailyRecord[],
  sources: UsageDashboardResponse['sources'],
): SourceDistributionItem[] {
  const sourceMap = new Map<string, number>();
  let grandTotal = 0;

  for (const r of daily) {
    if (r.totalTokens === null) continue;
    const current = sourceMap.get(r.sourceId) ?? 0;
    sourceMap.set(r.sourceId, current + r.totalTokens);
    grandTotal += r.totalTokens;
  }

  return Array.from(sourceMap.entries())
    .map(([sourceId, totalTokens]) => {
      const sourceInfo = sources.find((s) => s.id === sourceId);
      return {
        sourceId,
        label: sourceInfo?.label ?? sourceId,
        totalTokens,
        percentage: grandTotal > 0 ? totalTokens / grandTotal : null,
      };
    })
    .sort((a, b) => (b.totalTokens ?? 0) - (a.totalTokens ?? 0));
}

// ── Model distribution ──

export function buildModelDistribution(
  daily: UsageDailyRecord[],
): ModelDistributionItem[] {
  const modelMap = new Map<string, number>();
  let grandTotal = 0;

  for (const r of daily) {
    if (r.totalTokens === null) continue;
    const key = r.modelId ?? '__unrecorded__';
    const current = modelMap.get(key) ?? 0;
    modelMap.set(key, current + r.totalTokens);
    grandTotal += r.totalTokens;
  }

  return Array.from(modelMap.entries())
    .map(([key, totalTokens]) => ({
      modelId: key === '__unrecorded__' ? null : key,
      label: key === '__unrecorded__' ? '__unrecorded__' : key,
      totalTokens,
      percentage: grandTotal > 0 ? totalTokens / grandTotal : null,
    }))
    .sort((a, b) => (b.totalTokens ?? 0) - (a.totalTokens ?? 0));
}

// ── Coverage ──

export function calculateMetricCoverage(
  daily: UsageDailyRecord[],
): {
  tokenCoverage: number;
  costCoverage: number;
  cacheCoverage: number;
} {
  let totalRecords = 0;
  let tokensPresent = 0;
  let costPresent = 0;
  let cachePresent = 0;

  for (const r of daily) {
    totalRecords++;
    if (r.totalTokens !== null && r.totalTokens > 0) tokensPresent++;
    if (r.costUsd !== null && r.costUsd > 0) costPresent++;
    if (r.cacheReadTokens !== null || r.cacheCreationTokens !== null) cachePresent++;
  }

  return {
    tokenCoverage: totalRecords > 0 ? tokensPresent / totalRecords : 0,
    costCoverage: totalRecords > 0 ? costPresent / totalRecords : 0,
    cacheCoverage: totalRecords > 0 ? cachePresent / totalRecords : 0,
  };
}

// ── Unique session count ──

export function uniqueSessionCount(sessions: UsageSessionRecord[]): number {
  const seen = new Set<string>();
  for (const s of sessions) {
    seen.add(`${s.sourceId}:${s.sessionId}`);
  }
  return seen.size;
}

// ── Date range builder ──

export interface DateRangeResult {
  startMs: number;
  endMs: number;
}

export function buildDateRange(
  preset: 'today' | '24h' | '7d' | '30d' | '90d' | 'custom',
  customStart: string,
  customEnd: string,
  now: Date = new Date(),
): DateRangeResult | null {
  if (preset === 'custom') {
    if (!customStart || !customEnd) return null;
    const start = new Date(customStart + 'T00:00:00');
    
    // Check if customEnd is today or later
    const todayStr = now.toISOString().slice(0, 10);
    let end: Date;
    if (customEnd >= todayStr) {
      end = now;
    } else {
      // next day 00:00
      const nextDay = new Date(customEnd + 'T00:00:00');
      nextDay.setDate(nextDay.getDate() + 1);
      end = nextDay;
    }

    if (start.getTime() > end.getTime()) return null;
    return { startMs: start.getTime(), endMs: end.getTime() };
  }

  const dayStart = new Date(now.getFullYear(), now.getMonth(), now.getDate(), 0, 0, 0, 0);

  switch (preset) {
    case 'today':
      return { startMs: dayStart.getTime(), endMs: now.getTime() };
    case '24h':
      return { startMs: now.getTime() - 24 * 3600 * 1000, endMs: now.getTime() };
    case '7d':
      return { startMs: dayStart.getTime() - 6 * 86400000, endMs: now.getTime() };
    case '30d':
      return { startMs: dayStart.getTime() - 29 * 86400000, endMs: now.getTime() };
    case '90d':
      return { startMs: dayStart.getTime() - 89 * 86400000, endMs: now.getTime() };
    default:
      return null;
  }
}

// ── Metric delta computation ──

export interface MetricDelta {
  current: number | null;
  previous: number | null;
  changePercent: number | null;
}

export function computeMetricDeltas(
  current: UsageMetrics | null,
  previous: UsageMetrics | null,
): {
  costDelta: MetricDelta;
  tokensDelta: MetricDelta;
  sessionsDelta: MetricDelta;
} {
  const computeDelta = (cur: number | null, prev: number | null): MetricDelta => {
    if (cur === null || prev === null) {
      return { current: cur, previous: prev, changePercent: null };
    }
    if (prev === 0) {
      return { current: cur, previous: prev, changePercent: cur > 0 ? Infinity : 0 };
    }
    return { current: cur, previous: prev, changePercent: ((cur - prev) / prev) * 100 };
  };

  return {
    costDelta: computeDelta(current?.estimatedCost ?? null, previous?.estimatedCost ?? null),
    tokensDelta: computeDelta(current?.totalTokens ?? null, previous?.totalTokens ?? null),
    sessionsDelta: computeDelta(current?.totalSessions ?? null, previous?.totalSessions ?? null),
  };
}

// ── Generic distribution builder ──

export type DistributionDimension = 'sourceId' | 'modelId' | 'projectId' | 'terminalId';

export interface DistributionItem {
  id: string;
  label: string;
  totalTokens: number | null;
  percentage: number | null;
}

export function buildDistribution(
  daily: UsageDailyRecord[],
  dimension: DistributionDimension,
): DistributionItem[] {
  const map = new Map<string, number>();
  let totalTokens = 0;

  for (const r of daily) {
    const key = r[dimension];
    if (key === null) continue;
    if (r.totalTokens !== null) {
      map.set(key, (map.get(key) ?? 0) + r.totalTokens);
      totalTokens += r.totalTokens;
    }
  }

  return Array.from(map.entries())
    .map(([id, tokens]) => ({
      id,
      label: id,
      totalTokens: tokens,
      percentage: totalTokens > 0 ? tokens / totalTokens : null,
    }))
    .sort((a, b) => (b.totalTokens ?? 0) - (a.totalTokens ?? 0));
}
