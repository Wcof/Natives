// ── Usage Export Utility Tests ──

import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import {
  serializeUsageCsv,
  serializeUsageBadgeSvg,
  serializeUsageMarkdown,
  hasShareableMetrics,
  defaultExportFilename,
} from './usage-export';
import type { UsageDailyRecord, UsageMetrics } from '@/types/usage';

describe('serializeUsageCsv', () => {
  it('produces UTF-8 BOM header', () => {
    const csv = serializeUsageCsv([]);
    assert.ok(csv.startsWith('\uFEFF'), 'should start with BOM');
    assert.ok(csv.includes('date,source,model,project,terminal'), 'should have header');
  });

  it('includes all records', () => {
    const daily: UsageDailyRecord[] = [
      {
        date: '2026-07-01', sourceId: 'claude', modelId: 'sonnet', projectId: 'proj1', terminalId: null,
        inputTokens: 100, outputTokens: 20, cacheCreationTokens: 0, cacheReadTokens: 50, totalTokens: 170,
        costUsd: 0.5, costQuality: 'estimated',
      },
    ];
    const csv = serializeUsageCsv(daily);
    assert.ok(csv.includes('2026-07-01,claude,sonnet,proj1,,100,20,0,50,170,0.5,estimated'));
  });

  it('null values become empty cells', () => {
    const daily: UsageDailyRecord[] = [
      {
        date: '2026-07-01', sourceId: 'test', modelId: null, projectId: null, terminalId: null,
        inputTokens: null, outputTokens: null, cacheCreationTokens: null, cacheReadTokens: null, totalTokens: null,
        costUsd: null, costQuality: 'unavailable',
      },
    ];
    const csv = serializeUsageCsv(daily);
    assert.ok(csv.includes(',test,,,,,,,,,,unavailable'), 'null fields should be empty');
  });

  it('prevents CSV formula injection for = + - @', () => {
    const daily: UsageDailyRecord[] = [
      {
        date: '2026-07-01', sourceId: '=SUM(A1:A10)', modelId: '+GET(/etc/passwd)', projectId: '-cmd', terminalId: '@DANGER',
        inputTokens: 1, outputTokens: 2, cacheCreationTokens: 0, cacheReadTokens: 0, totalTokens: 3,
        costUsd: null, costQuality: 'unavailable',
      },
    ];
    const csv = serializeUsageCsv(daily);
    // Each formula-starting value should be prefixed with a single quote
    assert.ok(csv.includes("'=SUM(A1:A10)"), '= should be escaped');
    assert.ok(csv.includes("'+GET(/etc/passwd)"), '+ should be escaped');
    assert.ok(csv.includes("'-cmd"), '- should be escaped');
    assert.ok(csv.includes("'@DANGER"), '@ should be escaped');
  });
});

describe('serializeUsageBadgeSvg', () => {
  it('returns valid SVG start tag', () => {
    const svg = serializeUsageBadgeSvg({ period: '2026-07', tokens: 1000, cost: 0.5, sessions: 10 });
    assert.ok(svg.startsWith('<svg'), 'should start with <svg');
    assert.ok(svg.includes('Vibe Usage'), 'should include title');
    assert.ok(svg.endsWith('</svg>'), 'should end with </svg>');
  });

  it('handles missing cost gracefully', () => {
    const svg = serializeUsageBadgeSvg({ period: '2026-07', tokens: 500, cost: null, sessions: 5 });
    assert.ok(svg.includes('Tokens: 500'), 'should include tokens');
    assert.ok(!svg.includes('Cost:'), 'should not include cost');
  });

  it('handles no shareable metrics', () => {
    const svg = serializeUsageBadgeSvg({ period: '2026-07', tokens: null, cost: null, sessions: null });
    assert.ok(svg.includes('Vibe Usage'), 'should still show title');
  });
});

describe('serializeUsageMarkdown', () => {
  it('includes heading and period', () => {
    const md = serializeUsageMarkdown('2026-07', null, 0);
    assert.ok(md.includes('Vibe Usage'), 'should include heading');
  });

  it('includes metrics when available', () => {
    const metrics: UsageMetrics = {
      estimatedCost: 1.5,
      costCoverage: 0.5,
      totalTokens: 1000,
      totalInputTokens: 500,
      totalOutputTokens: 500,
      totalCacheReadTokens: null,
      totalCacheCreationTokens: null,
      totalSessions: 10,
      totalUserMessages: 20,
      totalAssistantMessages: 15,
      estimatedActiveSeconds: 3600,
      sessionSpanMs: null,
      coveredSources: 2,
      totalSources: 3,
    };
    const md = serializeUsageMarkdown('2026-07', metrics, 10);
    assert.ok(md.includes('$1.5000'), 'should include cost');
    assert.ok(md.includes('1,000'), 'should include tokens');
    assert.ok(md.includes('1h 0m'), 'should include duration');
  });
});

describe('hasShareableMetrics', () => {
  it('returns false for null metrics', () => {
    assert.equal(hasShareableMetrics(null), false);
  });

  it('returns true when tokens are available', () => {
    const metrics: UsageMetrics = {
      estimatedCost: null, costCoverage: null, totalTokens: 500,
      totalInputTokens: null, totalOutputTokens: null, totalCacheReadTokens: null,
      totalCacheCreationTokens: null, totalSessions: 0, totalUserMessages: 0,
      totalAssistantMessages: 0, estimatedActiveSeconds: null, sessionSpanMs: null,
      coveredSources: 1, totalSources: 1,
    };
    assert.equal(hasShareableMetrics(metrics), true);
  });

  it('returns false when nothing is available', () => {
    const metrics: UsageMetrics = {
      estimatedCost: null, costCoverage: null, totalTokens: null,
      totalInputTokens: null, totalOutputTokens: null, totalCacheReadTokens: null,
      totalCacheCreationTokens: null, totalSessions: 0, totalUserMessages: 0,
      totalAssistantMessages: 0, estimatedActiveSeconds: null, sessionSpanMs: null,
      coveredSources: 0, totalSources: 1,
    };
    assert.equal(hasShareableMetrics(metrics), false);
  });
});

describe('defaultExportFilename', () => {
  it('generates correct format', () => {
    const name = defaultExportFilename('natives-usage', new Date('2026-07-14'));
    assert.equal(name, 'natives-usage-20260714');
  });
});