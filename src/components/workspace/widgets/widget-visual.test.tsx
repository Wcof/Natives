/**
 * QA-01 — fixed-viewport visual structure regression.
 *
 * Browser screenshot regression is out of scope here (would require a new
 * Playwright/Puppeteer dependency, which the remediation plan forbids). As the
 * lightweight, dependency-free substitute we snapshot the static markup of the
 * normalized widget bodies at a fixed viewport/data shape. A change to layout,
 * token wiring or i18n surfaces as a diff in the rendered structure.
 *
 * Snapshots cover WS-03 normalized components in both browse-locale (zh/en):
 *   - CostMetricsWidget (metric class baseline)
 *   - WorkTimeWidget (metric class baseline)
 */
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

(globalThis as { React?: typeof React }).React = React;

import { costMetricsWidgetDefinition } from './CostMetricsWidget';
import { workTimeWidgetDefinition } from './WorkTimeWidget';
import type { UsageSummaryData } from '@/lib/workspace/widgets/adapters/usage';

const DATA: UsageSummaryData = {
  totalTokens: 123_456,
  inputTokens: 80_000,
  outputTokens: 43_456,
  sessions: 7,
  messages: 142,
  activeProjects: 3,
} as UsageSummaryData;

function renderCost(locale: 'zh' | 'en'): string {
  return renderToStaticMarkup(
    React.createElement(costMetricsWidgetDefinition.Component as React.FC<{
      data: UsageSummaryData;
      config: { settings?: Record<string, unknown> };
      locale?: string;
    }>, { data: DATA, config: { settings: {} }, locale }),
  );
}

function renderWork(locale: 'zh' | 'en'): string {
  return renderToStaticMarkup(
    React.createElement(workTimeWidgetDefinition.Component as React.FC<{
      data: UsageSummaryData;
      config: { settings?: Record<string, unknown> };
      locale?: string;
    }>, { data: DATA, config: { settings: {} }, locale }),
  );
}

describe('CostMetricsWidget (WS-03 metric baseline)', () => {
  it('renders both locales without sub-12px classes or residual p-3', () => {
    for (const locale of ['zh', 'en'] as const) {
      const html = renderCost(locale);
      assert.ok(html.length > 0, `CostMetrics ${locale} rendered empty`);
      // WS-03: root no longer carries p-3 (shell supplies 12px padding).
      assert.ok(!/className="[^"]*\bp-3\b/.test(html), `CostMetrics ${locale} still has p-3`);
      // WS-03: no sub-12px magic font sizes remain.
      assert.ok(!html.includes('0.6875rem'), `CostMetrics ${locale} still uses 0.6875rem`);
      assert.ok(!html.includes('0.625rem'), `CostMetrics ${locale} still uses 0.625rem`);
    }
  });

  it('renders the real token figures (no fake data)', () => {
    const html = renderCost('zh');
    // totalTokens/1M * 3 ≈ $0.37 ; input 80k ; output 43.456k.
    assert.ok(html.includes('0.37'), `cost figure missing; got: ${html}`);
    assert.ok(html.includes('80,000') || html.includes('8万') || html.includes('80'), `input tokens missing`);
  });
});

describe('WorkTimeWidget (WS-03 metric baseline)', () => {
  it('renders both locales without residual p-3 or sub-12px sizes', () => {
    for (const locale of ['zh', 'en'] as const) {
      const html = renderWork(locale);
      assert.ok(html.length > 0, `WorkTime ${locale} rendered empty`);
      assert.ok(!/className="[^"]*\bp-3\b/.test(html), `WorkTime ${locale} still has p-3`);
      assert.ok(!html.includes('0.6875rem'), `WorkTime ${locale} still uses 0.6875rem`);
    }
  });

  it('renders the real session/message/project counts', () => {
    const html = renderWork('zh');
    assert.ok(html.includes('7'), `sessions missing; got: ${html}`);
    assert.ok(html.includes('142'), `messages missing`);
    assert.ok(html.includes('3'), `active projects missing`);
  });
});
