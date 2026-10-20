#!/usr/bin/env node
// 通用核心层业务泄漏门禁（ADR-0031 / 架构 Wave 9 / 审查 A-06）：
// 静态扫描 Extension 通用页面、Service Worker、Core 以及 App Runtime 通用层，
// 确保通用基础设施不包含 Fund/TokenUsage 等内置模块的具体业务事件与特判分支。
import { readFileSync, existsSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..', '..');

const TARGET_FILES = [
  'extension/app.js',
  'extension/background.js',
  'crates/app-runtime/src/http/client.rs',
  'crates/app-runtime/src/controller/start.rs',
  'crates/app-runtime/src/controller/stopping.rs',
  'crates/app-runtime-core/src/http.rs',
];

const FORBIDDEN_PATTERNS = [
  { pattern: /invest:alert/, msg: '禁止出现特定模块业务事件 invest:alert（应使用通用 natives:notification）' },
  { pattern: /changePercent/, msg: '通用层禁止解析投资行情业务字段 changePercent' },
  { pattern: /tray_readonly/, msg: '通用 HTTP 层禁止针对特定路由硬编码免 Bearer 特权' },
];

const violations = [];

for (const rel of TARGET_FILES) {
  const full = join(ROOT, rel);
  if (!existsSync(full)) continue;

  const text = readFileSync(full, 'utf8');
  const lines = text.split('\n');

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const trimmed = line.trim();
    if (trimmed.startsWith('//') || trimmed.startsWith('/*') || trimmed.startsWith('*')) continue;

    for (const rule of FORBIDDEN_PATTERNS) {
      if (rule.pattern.test(line)) {
        violations.push(`${rel}:${i + 1} — ${rule.msg}`);
      }
    }
  }
}

if (violations.length > 0) {
  console.error('核心层业务泄漏门禁未通过（违反通用基础设施与业务隔离原则）：');
  for (const v of violations) console.error('  ' + v);
  process.exit(1);
}

console.log(`✓ Core business leak gate passed (scanned ${TARGET_FILES.length} files, 0 business leaks)`);
