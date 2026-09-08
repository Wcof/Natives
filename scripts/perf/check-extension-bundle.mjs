#!/usr/bin/env node
import { distributableFiles, extensionFileBytes } from '../extension-package.mjs';
/** ADR-0023: estimate the Chrome extension's distributable ZIP. */
import { gzipSync } from 'node:zlib';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(fileURLToPath(new URL('../..', import.meta.url)));
export const BUDGET = 300 * 1024;

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
  return { ok: files.length > 0 && estimate <= BUDGET && estimate - 283245 <= 20480, budget: BUDGET,
    current: estimate, headroom: BUDGET - estimate, deltaFromBaseline: estimate - 283245,
    warning: estimate >= 290 * 1024 ? 'NEAR_BUDGET' : null, estimate, rawBytes: rows.reduce((s, r) => s + r.bytes, 0), files, rows };
}

if (process.argv[1] && import.meta.url === new URL(`file://${process.argv[1]}`).href) {
  const rootArg = process.argv[2];
  const summary = runExtensionBundleCheck(rootArg ? resolve(ROOT, rootArg) : ROOT);
  console.log(JSON.stringify({ ...summary, budgetKB: BUDGET / 1024 }, null, 2));
  process.exitCode = summary.ok ? 0 : 1;
}
