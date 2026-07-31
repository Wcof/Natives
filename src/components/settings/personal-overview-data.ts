import type { UsageDashboardResponse } from '@/types/usage';
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
