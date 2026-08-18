import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { t, type Locale } from '@/i18n';

const panelSource = readFileSync(new URL('./ExecutionEngineSettingsPanel.tsx', import.meta.url), 'utf8');
const settingsPageSource = readFileSync(new URL('../shell/SettingsPage.tsx', import.meta.url), 'utf8');

const expectedLabels: Record<Locale, Record<string, string>> = {
  zh: {
    tabRuntimeSettings: '运行设置',
    tabHarness: 'Harness 编排',
    defaultRuntimeTitle: '默认 Runtime',
    resolvedTitle: '当前解析结果',
    capabilitiesTitle: '能力真相',
    ready: '可用',
    degraded: '降级',
    blocked: '未开放',
    refresh: '重新检测环境',
    saved: '已保存',
  },
  'zh-CN': {
    tabRuntimeSettings: '运行设置',
    tabHarness: 'Harness 编排',
    defaultRuntimeTitle: '默认 Runtime',
    resolvedTitle: '当前解析结果',
    capabilitiesTitle: '能力真相',
    ready: '可用',
    degraded: '降级',
    blocked: '未开放',
    refresh: '重新检测环境',
    saved: '已保存',
  },
  en: {
    tabRuntimeSettings: 'Runtime Settings',
    tabHarness: 'Harness',
    defaultRuntimeTitle: 'Default Runtime',
    resolvedTitle: 'Current Resolution',
    capabilitiesTitle: 'Capability truth',
    ready: 'Ready',
    degraded: 'Degraded',
    blocked: 'Blocked',
    refresh: 'Re-detect environment',
    saved: 'Saved',
  },
};

test('execution engine tabs, headings, statuses, and actions resolve in both languages', () => {
  for (const locale of ['zh', 'en'] as const) {
    for (const [name, expected] of Object.entries(expectedLabels[locale])) {
      const key = `settings.executionEngine.${name}`;
      const translated = t(locale, key);
      assert.equal(translated, expected);
      assert.notEqual(translated, key);
    }
  }
});

test('execution engine UI never requests the obsolete top-level translation domain', () => {
  const obsoleteCall = /t\(locale, ['"]executionEngine\./;
  assert.doesNotMatch(panelSource, obsoleteCall);
  assert.doesNotMatch(settingsPageSource, obsoleteCall);
  assert.match(panelSource, /executionEngine\.getSnapshot/);
});
