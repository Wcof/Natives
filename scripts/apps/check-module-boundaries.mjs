#!/usr/bin/env node
// 模块边界门禁（ADR-0031 / ADR-0033 / 架构 Wave 9 / 审查 A-01）：
// 静态扫描 modules/**/src 源码，防止跨域文件访问、私有 DB 读写及不合规的系统探测。
import { readFileSync, readdirSync, existsSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const MODULES_DIR = join(ROOT, 'modules');

function walk(dir) {
  let results = [];
  if (!existsSync(dir)) return results;
  for (const ent of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, ent.name);
    if (ent.isDirectory()) {
      if (ent.name === 'target' || ent.name === 'node_modules') continue;
      results = results.concat(walk(full));
    } else if (ent.isFile() && (ent.name.endsWith('.rs') || ent.name.endsWith('.js'))) {
      results.push(full);
    }
  }
  return results;
}

const violations = [];

// 1. 全局模块禁止模式（硬编码主目录、其他模块私有路径、Core DB 等）
const GLOBAL_FORBIDDEN = [
  { pattern: /Library\/Application Support\/Natives\/model-host/, msg: '禁止直接探测/读取 model-host 私有状态目录' },
  { pattern: /\.config\/natives\/model-host/, msg: '禁止直接探测/读取 model-host 配置' },
  { pattern: /core\.db/, msg: '禁止模块访问 Core 数据库' },
  { pattern: /NATIVES_MODEL_HOST_CONFIG_DIR/, msg: '禁止模块私自探测 Model Host 配置环境变量' },
];

// 2. TokenUsage 专属边界约束（严禁直读 usage.db 及私有环境变量）
const TOKENUSAGE_FORBIDDEN = [
  { pattern: /NATIVES_USAGE_DB_PATH/, msg: '禁止探测 NATIVES_USAGE_DB_PATH' },
  { pattern: /USAGE_DB_PATH/, msg: '禁止探测 USAGE_DB_PATH' },
  { pattern: /OpenStore\s*\(/, msg: '禁止直接初始化/读写 usage.db' },
  { pattern: /(?:^|[^\w])usage\.db\b/, msg: '禁止直接访问 Model Host 权威库 usage.db' },
];

const moduleDirs = readdirSync(MODULES_DIR, { withFileTypes: true })
  .filter(d => d.isDirectory())
  .map(d => d.name);

for (const modName of moduleDirs) {
  const srcDir = join(MODULES_DIR, modName, 'src');
  const files = walk(srcDir);

  for (const file of files) {
    const text = readFileSync(file, 'utf8');
    const lines = text.split('\n');

    for (let i = 0; i < lines.length; i++) {
      const line = lines[i];
      // 跳过纯注释行
      const trimmed = line.trim();
      if (trimmed.startsWith('//') || trimmed.startsWith('/*') || trimmed.startsWith('*')) continue;

      for (const rule of GLOBAL_FORBIDDEN) {
        if (rule.pattern.test(line)) {
          violations.push(`${file}:${i + 1} — ${rule.msg}`);
        }
      }

      if (modName === 'tokenusage') {
        for (const rule of TOKENUSAGE_FORBIDDEN) {
          if (rule.pattern.test(line)) {
            violations.push(`${file}:${i + 1} — ${rule.msg}`);
          }
        }
      }
    }
  }
}

if (violations.length > 0) {
  console.error('模块边界门禁未通过（违反 ADR-0031 / ADR-0033 数据所有权隔离）：');
  for (const v of violations) console.error('  ' + v);
  process.exit(1);
}

console.log(`✓ Module boundary gate passed (scanned ${moduleDirs.length} modules, 0 boundary leaks)`);
