// ── Usage Dashboard utility tests ──

import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import type { UsageDashboardResponse, UsageDailyRecord, UsageActivityBucket, UsageSessionRecord } from '@/types/usage';
import {
  filterUsageRecords,
  aggregateUsageMetrics,
  buildDailyTrend,
  buildHourlyHeatmap,
  buildProjectDistribution,
  uniqueSessionCount,
  buildSourceDimensions,
} from './usage-dashboard';

function mockResponse(overrides?: Partial<UsageDashboardResponse>): UsageDashboardResponse {
  return {
    generatedAtMs: Date.now(),
    range: { startMs: 0, endMs: 86400000 },
    daily: [],
    activity: [],
    sessions: [],
    comparison: null,
    dimensions: { sources: [], models: [], projects: [], terminals: [] },
    sources: [],
    rtk: null,
    warnings: [],
    ...overrides,
  };
}

describe('filterUsageRecords', () => {
  it('filters by source, model and project', () => {
    const data = mockResponse({
      daily: [
        { date: '2026-07-01', sourceId: 'claude', modelId: 'sonnet', projectId: 'proj1', terminalId: null, inputTokens: null, outputTokens: null, cacheCreationTokens: null, cacheReadTokens: null, totalTokens: 100, costUsd: null, costQuality: 'unavailable' as const },
        { date: '2026-07-01', sourceId: 'codex', modelId: 'gpt4', projectId: 'proj2', terminalId: null, inputTokens: null, outputTokens: null, cacheCreationTokens: null, cacheReadTokens: null, totalTokens: 200, costUsd: null, costQuality: 'unavailable' as const },
      ],
      sources: [
        { id: 'claude', label: 'Claude', kind: 'external' as const, state: 'ok' as const, breadcrumbs: [], capabilities: { totalTokens: true, tokenBreakdown: false, cache: false, cost: false, hourly: false, project: true, messages: false, sessions: false, duration: false }, durationMethod: null },
        { id: 'codex', label: 'Codex', kind: 'external' as const, state: 'ok' as const, breadcrumbs: [], capabilities: { totalTokens: true, tokenBreakdown: false, cache: false, cost: false, hourly: false, project: true, messages: false, sessions: false, duration: false }, durationMethod: null },
      ],
      activity: [],
      sessions: [],
    });

    const filtered = filterUsageRecords(data, ['claude'], null, null, null);
    assert.equal(filtered.daily.length, 1);
    assert.ok(filtered.daily[0] !== undefined);
    assert.equal(filtered.daily[0]!.sourceId, 'claude');
  });

  it('null cost is not zero', () => {
    const record: UsageDailyRecord = {
      date: '2026-07-01', sourceId: 'test', modelId: null, projectId: null, terminalId: null,
      inputTokens: null, outputTokens: null, cacheCreationTokens: null, cacheReadTokens: null,
      totalTokens: 100, costUsd: null, costQuality: 'unavailable',
    };
    assert.equal(record.costUsd, null);
  });

  it('null token breakdown is not zero', () => {
    const record: UsageDailyRecord = {
      date: '2026-07-01', sourceId: 'test', modelId: null, projectId: null, terminalId: null,
      inputTokens: null, outputTokens: null, cacheCreationTokens: null, cacheReadTokens: null,
      totalTokens: null, costUsd: null, costQuality: 'unavailable',
    };
    assert.equal(record.inputTokens, null);
    assert.equal(record.outputTokens, null);
  });
});

describe('buildSourceDimensions', () => {
  it('lists every adapted tool even when only one has usage rows', () => {
    const sources = [
      { id: 'claude', label: 'Claude', kind: 'external', state: 'ok', breadcrumbs: [], capabilities: { totalTokens: true, tokenBreakdown: true, cache: true, cost: false, hourly: true, project: true, messages: true, sessions: true, duration: true }, durationMethod: 'event_gap_estimate' },
      { id: 'codex', label: 'Codex', kind: 'external', state: 'ok', breadcrumbs: [], capabilities: { totalTokens: true, tokenBreakdown: true, cache: true, cost: false, hourly: true, project: true, messages: true, sessions: true, duration: true }, durationMethod: 'event_gap_estimate' },
      { id: 'atomcode', label: 'Atomcode', kind: 'external', state: 'ok', breadcrumbs: [], capabilities: { totalTokens: true, tokenBreakdown: true, cache: true, cost: false, hourly: true, project: true, messages: true, sessions: true, duration: true }, durationMethod: 'session_bounds' },
    ] satisfies UsageDashboardResponse['sources'];
    assert.deepEqual(buildSourceDimensions(sources), [
      { id: 'atomcode', label: 'Atomcode' },
      { id: 'claude', label: 'Claude' },
      { id: 'codex', label: 'Codex' },
    ]);
  });
});

describe('aggregateUsageMetrics', () => {
  it('cost coverage uses known token denominator', () => {
    const daily: UsageDailyRecord[] = [
      { date: '2026-07-01', sourceId: 'a', modelId: null, projectId: null, terminalId: null, inputTokens: null, outputTokens: null, cacheCreationTokens: null, cacheReadTokens: null, totalTokens: 100, costUsd: 1.0, costQuality: 'reported' },
      { date: '2026-07-01', sourceId: 'b', modelId: null, projectId: null, terminalId: null, inputTokens: null, outputTokens: null, cacheCreationTokens: null, cacheReadTokens: null, totalTokens: 300, costUsd: null, costQuality: 'unavailable' },
    ];
    const metrics = aggregateUsageMetrics(daily, [], []);
    assert.equal(metrics.totalTokens, 400);
    assert.equal(metrics.estimatedCost, 1.0);
    assert.ok(metrics.costCoverage !== null && Math.abs(metrics.costCoverage - 0.25) < 0.01);
  });

  it('empty records return empty metrics', () => {
    const metrics = aggregateUsageMetrics([], [], []);
    assert.equal(metrics.totalTokens, null);
    assert.equal(metrics.totalSessions, 0);
    assert.equal(metrics.estimatedActiveSeconds, null);
  });
});

describe('uniqueSessionCount', () => {
  it('sessions are deduplicated by source and id', () => {
    const sessions: UsageSessionRecord[] = [
      { sessionId: 's1', sourceId: 'a', modelId: null, projectId: null, terminalId: null, startedAtMs: 0, endedAtMs: 100, userMessages: 1, assistantMessages: 1, activeSeconds: null, durationQuality: 'estimated' },
      { sessionId: 's1', sourceId: 'a', modelId: null, projectId: null, terminalId: null, startedAtMs: 0, endedAtMs: 100, userMessages: 1, assistantMessages: 1, activeSeconds: null, durationQuality: 'estimated' },
      { sessionId: 's2', sourceId: 'a', modelId: null, projectId: null, terminalId: null, startedAtMs: 100, endedAtMs: 200, userMessages: 1, assistantMessages: 1, activeSeconds: null, durationQuality: 'estimated' },
    ];
    assert.equal(uniqueSessionCount(sessions), 2);
  });
});

describe('buildDailyTrend', () => {
  it('aggregates tokens by date', () => {
    const daily: UsageDailyRecord[] = [
      { date: '2026-07-01', sourceId: 'a', modelId: null, projectId: null, terminalId: null, inputTokens: null, outputTokens: null, cacheCreationTokens: null, cacheReadTokens: null, totalTokens: 100, costUsd: 1.0, costQuality: 'reported' },
      { date: '2026-07-01', sourceId: 'b', modelId: null, projectId: null, terminalId: null, inputTokens: null, outputTokens: null, cacheCreationTokens: null, cacheReadTokens: null, totalTokens: 200, costUsd: 2.0, costQuality: 'reported' },
    ];
    const trend = buildDailyTrend(daily, []);
    assert.equal(trend.length, 1);
    assert.ok(trend[0] !== undefined);
    assert.equal(trend[0]!.totalTokens, 300);
  });
});

describe('buildHourlyHeatmap', () => {
  it('uses local hour fields from real-epoch hour starts', () => {
    // Real epoch for a local wall hour: construct via Date so the machine
    // timezone matches what getHours()/getDay() will read back.
    const local = new Date(2026, 6, 1, 22, 0, 0, 0); // 2026-07-01 22:00 local
    const activity: UsageActivityBucket[] = [
      { hourStartMs: local.getTime(), sourceId: 'a', modelId: null, projectId: null, terminalId: null, totalTokens: 100, userMessages: 1, assistantMessages: 1, activeSeconds: null },
    ];
    const heatmap = buildHourlyHeatmap(activity);
    assert.equal(heatmap.length, 1);
    assert.ok(heatmap[0] !== undefined);
    assert.equal(heatmap[0]!.hour, 22);
    assert.equal(heatmap[0]!.dayOfWeek, local.getDay());
  });

  it('zero hourStartMs is excluded', () => {
    const activity: UsageActivityBucket[] = [
      { hourStartMs: 0, sourceId: 'a', modelId: null, projectId: null, terminalId: null, totalTokens: 100, userMessages: 1, assistantMessages: 1, activeSeconds: null },
    ];
    assert.equal(buildHourlyHeatmap(activity).length, 0);
  });
});

describe('buildProjectDistribution', () => {
  it('aggregates tokens by project and merges long tail into others', () => {
    const daily: UsageDailyRecord[] = Array.from({ length: 8 }, (_, i) => ({
      date: '2026-07-01',
      sourceId: 'a',
      modelId: null,
      projectId: `/proj/${i}`,
      terminalId: null,
      inputTokens: null,
      outputTokens: null,
      cacheCreationTokens: null,
      cacheReadTokens: null,
      totalTokens: 100 - i * 5,
      costUsd: null,
      costQuality: 'unavailable' as const,
    }));
    const dist = buildProjectDistribution(daily, 'Others');
    assert.equal(dist.length, 7); // top 6 + others
    assert.equal(dist[6]!.id, '__other__');
    assert.equal(dist[6]!.label, 'Others');
    assert.ok((dist[6]!.totalTokens ?? 0) > 0);
  });
});

describe('rtk is not added to usage', () => {
  it('rtk field is separate from usage metrics', () => {
    const data = mockResponse({ rtk: { totalSavedTokens: 5000, totalCommands: 10 } });
    const metrics = aggregateUsageMetrics(data.daily, data.sessions, data.sources);
    assert.equal(metrics.totalTokens, null);
    // RTK is not included in usage metrics
  });
});

describe('project filter excludes unsupported sources', () => {
  it('sources without project capability are excluded when project filter is active', () => {
    const data = mockResponse({
      daily: [
        { date: '2026-07-01', sourceId: 'noproject', modelId: null, projectId: null, terminalId: null, inputTokens: null, outputTokens: null, cacheCreationTokens: null, cacheReadTokens: null, totalTokens: 100, costUsd: null, costQuality: 'unavailable' },
        { date: '2026-07-01', sourceId: 'withproject', modelId: null, projectId: 'myproj', terminalId: null, inputTokens: null, outputTokens: null, cacheCreationTokens: null, cacheReadTokens: null, totalTokens: 200, costUsd: null, costQuality: 'unavailable' },
      ],
      sources: [
        { id: 'noproject', label: 'No Project', kind: 'external', state: 'ok', breadcrumbs: [], capabilities: { totalTokens: true, tokenBreakdown: false, cache: false, cost: false, hourly: false, project: false, messages: false, sessions: false, duration: false }, durationMethod: null },
        { id: 'withproject', label: 'With Project', kind: 'external', state: 'ok', breadcrumbs: [], capabilities: { totalTokens: true, tokenBreakdown: false, cache: false, cost: false, hourly: false, project: true, messages: false, sessions: false, duration: false }, durationMethod: null },
      ],
      activity: [],
      sessions: [],
    });

    const filtered = filterUsageRecords(data, null, null, ['myproj'], null);
    assert.equal(filtered.daily.length, 1);
    assert.ok(filtered.daily[0] !== undefined);
    assert.equal(filtered.daily[0]!.sourceId, 'withproject');
  });
});
