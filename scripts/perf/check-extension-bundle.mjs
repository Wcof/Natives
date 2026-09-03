#!/usr/bin/env node
/** ADR-0023: estimate the Chrome extension's distributable ZIP. */
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { gzipSync } from 'node:zlib';
import { basename, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(fileURLToPath(new URL('../..', import.meta.url)));
export const BUDGET = 250 * 1024;

const DEV_NAMES = new Set(['node_modules', '.git', 'fixtures', 'test', 'tests', '__pycache__']);
// Runtime files plus Chrome's localized message catalogs. Installer helpers,
// manifests and READMEs intentionally stay outside the extension ZIP.
// ui-harness.js and test-dom-mock.js are development/test-only entry points.
const DISTRIBUTABLE = /^(manifest\.json|(?!(?:ui-harness|test-dom-mock)\.js$)(?:[^/]+|plugins\/.+)\.(?:html|css|js)|icons\/[^/]+|_locales\/[^/]+\/messages\.json)$/;

export function distributableFiles(root = ROOT) {
  const dir = join(root, 'extension');
  if (!existsSync(dir)) return [];
  const files = [];
  const walk = (current) => {
    for (const name of readdirSync(current)) {
      if (DEV_NAMES.has(name) || name.endsWith('.map') || name.endsWith('.md')) continue;
      const path = join(current, name);
      const stat = statSync(path);
      if (stat.isDirectory()) walk(path);
      else {
        const rel = relative(dir, path).split('\\').join('/');
        if (DISTRIBUTABLE.test(rel) && !basename(rel).startsWith('.')) files.push(rel);
      }
    }
  };
  walk(dir);
  return files.sort();
}

export function runExtensionBundleCheck(root = ROOT) {
  const files = distributableFiles(root);
  const rows = files.map((file) => {
    const bytes = readFileSync(join(root, 'extension', file));
    return { file, bytes: bytes.byteLength, gzipBytes: gzipSync(bytes).byteLength };
  });
  // ZIP headers/central-directory entries are deliberately included; this is an
  // estimate, not a claim that gzip is a ZIP implementation.
  const estimate = rows.reduce((sum, row) => sum + row.gzipBytes + 76 + row.file.length, 0);
  return { ok: files.length > 0 && estimate <= BUDGET, budget: BUDGET, estimate, rawBytes: rows.reduce((s, r) => s + r.bytes, 0), files, rows };
}

if (process.argv[1] && import.meta.url === new URL(`file://${process.argv[1]}`).href) {
  const rootArg = process.argv[2];
  const summary = runExtensionBundleCheck(rootArg ? resolve(ROOT, rootArg) : ROOT);
  console.log(JSON.stringify({ ...summary, budgetKB: BUDGET / 1024 }, null, 2));
  process.exitCode = summary.ok ? 0 : 1;
}
