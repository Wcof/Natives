import assert from 'node:assert/strict';
import test from 'node:test';
import type { UsageDashboardResponse } from '@/types/usage';
import {
  buildOverviewTrend,
  summarizeOverviewUsage,
} from './personal-overview-data';

function usageFixture(): UsageDashboardResponse {
  return {
    generatedAtMs: Date.parse('2026-07-28T08:00:00Z'),
    range: {
      startMs: Date.parse('2026-06-29T00:00:00Z'),
      endMs: Date.parse('2026-07-29T00:00:00Z'),
    },
    daily: [
      {
        date: '2026-07-27',
        sourceId: 'natives',
        modelId: 'gpt-5',
        projectId: '/work/alpha',
        terminalId: null,
        inputTokens: 80,
        outputTokens: 20,
        cacheCreationTokens: null,
        cacheReadTokens: null,
        totalTokens: 100,
        costUsd: null,
        costQuality: 'unavailable',
      },
      {
        date: '2026-07-28',
        sourceId: 'natives',
        modelId: 'gpt-5',
        projectId: '/work/beta',
        terminalId: null,
        inputTokens: 150,
        outputTokens: 50,
        cacheCreationTokens: null,
        cacheReadTokens: null,
        totalTokens: 200,
        costUsd: null,
        costQuality: 'unavailable',
      },
      {
        date: '2026-07-28',
        sourceId: 'external',
        modelId: null,
        projectId: null,
        terminalId: null,
        inputTokens: null,
        outputTokens: null,
        cacheCreationTokens: null,
        cacheReadTokens: null,
        totalTokens: null,
        costUsd: null,
        costQuality: 'unavailable',
      },
    ],
    activity: [],
    sessions: [
      {
        sessionId: 'session-1',
        sourceId: 'natives',
        modelId: 'gpt-5',
        projectId: '/work/alpha',
        terminalId: null,
        startedAtMs: Date.parse('2026-07-27T08:00:00Z'),
        endedAtMs: Date.parse('2026-07-27T08:10:00Z'),
        userMessages: 3,
        assistantMessages: 2,
        activeSeconds: 400,
        durationQuality: 'reported',
      },
      {
        sessionId: 'session-2',
        sourceId: 'natives',
        modelId: 'gpt-5',
        projectId: '/work/beta',
        terminalId: null,
        startedAtMs: Date.parse('2026-07-28T08:00:00Z'),
        endedAtMs: Date.parse('2026-07-28T08:15:00Z'),
        userMessages: 4,
        assistantMessages: 3,
        activeSeconds: null,
        durationQuality: 'unavailable',
      },
    ],
    comparison: null,
    dimensions: { sources: [], models: [], projects: [], terminals: [] },
    sources: [
      {
        id: 'natives',
        label: 'Natives',
        kind: 'natives',
        state: 'ok',
        breadcrumbs: [],
        capabilities: {
          totalTokens: true,
          tokenBreakdown: true,
          cache: false,
          cost: false,
          hourly: false,
          project: true,
          messages: true,
          sessions: true,
          duration: true,
        },
        durationMethod: 'session_bounds',
      },
    ],
    rtk: null,
    warnings: [],
  };
}

test('summarizeOverviewUsage derives only available real metrics', () => {
  const summary = summarizeOverviewUsage(usageFixture(), '2026-07-28');

  assert.deepEqual(summary, {
    todayTokens: 200,
    totalTokens: 300,
    inputTokens: 230,
    outputTokens: 70,
    sessions: 2,
    messages: 12,
    averageMessagesPerSession: 6,
    activeProjects: 2,
  });
});

test('summarizeOverviewUsage preserves unavailable token values', () => {
  const data = usageFixture();
  data.daily = data.daily.filter((record) => record.totalTokens === null);
  data.sessions = [];

  assert.deepEqual(summarizeOverviewUsage(data, '2026-07-28'), {
    todayTokens: null,
    totalTokens: null,
    inputTokens: null,
    outputTokens: null,
    sessions: 0,
    messages: 0,
    averageMessagesPerSession: null,
    activeProjects: 0,
  });
});

test('buildOverviewTrend aggregates records for the same calendar day', () => {
  assert.deepEqual(buildOverviewTrend(usageFixture()), [
    { date: '2026-07-27', totalTokens: 100 },
    { date: '2026-07-28', totalTokens: 200 },
  ]);
});
