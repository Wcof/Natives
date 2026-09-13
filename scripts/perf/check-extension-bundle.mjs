#!/usr/bin/env node
import { distributableFiles, extensionFileBytes } from '../extension-package.mjs';
/** ADR-0023: estimate the Chrome extension's distributable ZIP. */
import { gzipSync } from 'node:zlib';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(fileURLToPath(new URL('../..', import.meta.url)));
export const BUDGET = 360 * 1024;
// 2026-09-13 用户决定：Hard Gate 自 300 KiB 放宽 20%（ADR-0024 §8 资源预算修订），为内置模块
// 图标资产与产品功能字节预留空间。漂移锚点同步重置为本次批准的字节状态；20 KiB 漂移归因纪律不变。
const DRIFT_BASELINE = 318632;
const DRIFT_ALLOWANCE = 20480;

export { distributableFiles };

export function runExtensionBundleCheck(root = ROOT) {
  const files = distributableFiles(root);
  const rows = files.map((file) => {
    const bytes = extensionFileBytes(file, root);
    return { file, bytes: bytes.byteLength, gzipBytes: gzipSync(bytes).byteLength };
  });
  // ZIP headers/central-directory entries are deliberately included; this is an
  // estimate, not a claim that gzip is a ZIP implementation.
  const estimate = rows.reduce((sum, row) => sum + row.gzipBytes + 76 + row.file.length, 0);
  return { ok: files.length > 0 && estimate <= BUDGET && estimate - DRIFT_BASELINE <= DRIFT_ALLOWANCE, budget: BUDGET,
    current: estimate, headroom: BUDGET - estimate, deltaFromBaseline: estimate - DRIFT_BASELINE,
    warning: estimate >= 348 * 1024 ? 'NEAR_BUDGET' : null, estimate, rawBytes: rows.reduce((s, r) => s + r.bytes, 0), files, rows };
}

if (process.argv[1] && import.meta.url === new URL(`file://${process.argv[1]}`).href) {
  const rootArg = process.argv[2];
  const summary = runExtensionBundleCheck(rootArg ? resolve(ROOT, rootArg) : ROOT);
  console.log(JSON.stringify({ ...summary, budgetKB: BUDGET / 1024 }, null, 2));
  process.exitCode = summary.ok ? 0 : 1;
}
