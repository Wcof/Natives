import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

// Extension locales are the authority; this embedded projection serves the opaque iframe.
// Fund-owned key namespaces are defined once here; check-ui and extension packaging reuse them.
export const FUND_MODULE_KEY_PREFIXES = ['fundHome', 'fundLedger', 'fundDetail', 'fundError', 'fundFocus', 'fundSettings', 'fundWatch', 'fundMarket', 'fundTrends', 'fundScan', 'fundChart'];

export function isFundModuleKey(key) {
  return FUND_MODULE_KEY_PREFIXES.some((prefix) => key.startsWith(prefix));
}

export function fundLocalesSource(root) {
  const tables = {};
  for (const [locale, language] of [['zh_CN', 'zh-CN'], ['en', 'en']]) {
    const messages = JSON.parse(readFileSync(resolve(root, 'extension/_locales', locale, 'messages.json'), 'utf8'));
    tables[language] = Object.fromEntries(
      Object.entries(messages)
        .filter(([key]) => isFundModuleKey(key))
        .sort(([a], [b]) => a.localeCompare(b))
        .map(([key, value]) => [key, value.message])
    );
  }
  const zhKeys = Object.keys(tables['zh-CN']);
  const enKeys = Object.keys(tables.en);
  if (zhKeys.length !== enKeys.length || zhKeys.some((key, i) => key !== enKeys[i])) {
    throw new Error('Fund module locale keys diverge between zh_CN and en (R-I2)');
  }
  return '// Generated from extension/_locales by modules:ui:build.\n' + Object.entries(tables).map(([language, messages]) => `Fund.registerMessages(${JSON.stringify(language)}, ${JSON.stringify(messages)});`).join('\n') + '\n';
}
