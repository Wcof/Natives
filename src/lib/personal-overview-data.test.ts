// Migrated from src/components/settings/personal-overview-data.test.ts so the
// shared personal-overview pure computation keeps its tests next to the source
// (same node:test + tsx style as the rest of src/lib).

import assert from 'node:assert/strict';
import test from 'node:test';
import type { UsageDashboardResponse } from '@/types/usage';
import {
  buildOverviewTrend,
  overviewCoverageStatus,
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

function metadataAt(generatedAtMs: number) {
  return {
    schemaVersion: 6,
    generatedAtMs,
    coverageStartMs: 0,
    coverageEndMs: 0,
    timeZone: 'UTC',
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

test('overviewCoverageStatus reports missing without a snapshot', () => {
  const info = overviewCoverageStatus(null, null, Date.parse('2026-07-28T09:00:00Z'));
  assert.equal(info.status, 'missing');
  assert.equal(info.updatedAtMs, null);
  assert.equal(info.stalenessMs, null);
  assert.deepEqual(info.sources, []);
});

test('overviewCoverageStatus reports fresh when all sources are ok', () => {
  const now = Date.parse('2026-07-28T09:00:00Z');
  const info = overviewCoverageStatus(usageFixture(), metadataAt(now), now);
  assert.equal(info.status, 'fresh');
  assert.equal(info.updatedAtMs, now);
  assert.equal(info.stalenessMs, 0);
});

test('overviewCoverageStatus reports stale when the snapshot is older than the freshness window', () => {
  const now = Date.parse('2026-07-28T09:00:00Z');
  const info = overviewCoverageStatus(
    usageFixture(),
    metadataAt(now - 10 * 60 * 1000),
    now,
  );
  assert.equal(info.status, 'stale');
  assert.equal(info.stalenessMs, 10 * 60 * 1000);
});

test('overviewCoverageStatus reports partial when a source is partial or unavailable', () => {
  const now = Date.parse('2026-07-28T09:00:00Z');
  const data = usageFixture();
  const first = data.sources[0];
  if (!first) throw new Error('fixture sources must not be empty');
  data.sources[0] = { ...first, state: 'partial' };
  const info = overviewCoverageStatus(data, metadataAt(now), now);
  assert.equal(info.status, 'partial');
  assert.equal(info.sources[0]?.incomplete, true);
  assert.match(info.sourceStateSummary, /\(partial\)/);
});
