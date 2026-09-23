// Architecture Death Check（计划 §45.1）：Production scope 出现旧架构标识
// 直接失败。历史 ADR / legacy cleanup fixture 不在本检查范围（scope 明确排除
// docs/archive、tests fixtures 与 legacy 清理语境）。
// 运行：node scripts/apps/check-architecture-death.mjs
import assert from 'node:assert/strict';
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join, resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..');

// 禁止符号（计划 §45.1 / §51.2）
const FORBIDDEN = [
  { pattern: /\bManagedApp\b/, label: 'ManagedApp' },
  { pattern: /\bFundApp\b/, label: 'FundApp' },
  { pattern: /app-host-support|app_host_support/, label: 'app-host-support' },
  { pattern: /APP_PROTOCOL_VERSION_V1/, label: 'APP_PROTOCOL_VERSION_V1' },
  { pattern: /PRODUCT_PROTOCOL_VERSION_V4/, label: 'PRODUCT_PROTOCOL_VERSION_V4' },
  { pattern: /\bregister_runtime_host\b/, label: 'register_runtime_host (legacy per-app)' },
  { pattern: /runtime\/<version>\/app/, label: 'runtime/<version>/app 语义' },
];

// 检查范围：生产代码（排除 legacy 清理文件与测试 fixture）
const SCAN_PATHS = [
  'crates/app-runtime/src',
  'crates/app-runtime-core/src',
  'crates/app-runtime/tests',
  'crates/native-file-host/src',
  'crates/file-manager-core/src',
  'modules/fund/src',
  'extension',
  'scripts/installer-package.mjs',
  'scripts/package-windows-local.mjs',
  'installers/macos/verify.sh',
];

// legacy 清理语境允许出现（只是引用旧名词做删除/识别/反向验证，不是执行能力）
const LEGACY_ALLOWED = /legacy|Legacy|clean_legacy|清理|旧版|旧架构|已删除|no longer accepted|反向验证|payload found|manifest found/;

function* iterFiles(target) {
  const full = join(ROOT, target);
  const stat = statSync(full);
  if (stat.isFile()) {
    yield full;
    return;
  }
  for (const name of readdirSync(full)) {
    if (name === 'tests.rs' && target.includes('native-file-host')) {
      // 聚合测试套件包含 IPC 场景，同样需要检查；继续处理
    }
    const child = join(full, name);
    if (statSync(child).isDirectory()) {
      if (name === 'fixtures' || name === 'node_modules') continue;
      yield* iterFiles(join(target, name));
    } else if (/\.(rs|js|mjs|sh)$/.test(name)) {
      yield child;
    }
  }
}

const violations = [];
for (const target of SCAN_PATHS) {
  for (const file of iterFiles(target)) {
    const lines = readFileSync(file, 'utf8').split('\n');
    lines.forEach((line, index) => {
      for (const { pattern, label } of FORBIDDEN) {
        if (pattern.test(line) && !LEGACY_ALLOWED.test(line)) {
          violations.push(`${label}: ${file}:${index + 1}: ${line.trim().slice(0, 120)}`);
        }
      }
    });
  }
}

assert.deepEqual(violations, [], `旧架构标识出现在生产 scope（禁止架构复活，计划 §45.1）:\n${violations.join('\n')}`);
console.log('architecture death check OK: 生产 scope 无旧架构标识');
