#!/usr/bin/env node
/**
 * Hardcoded color checker (R-U1 / R-E4 compliance).
 * Scans src/components and src/app for color literals (#hex, rgb/rgba/hsl,
 * Tailwind fixed-color classes) that bypass the theme token system.
 *
 * Existing violations are grandfathered in scripts/hardcoded-colors-baseline.json
 * (per-file hit counts). The check fails when a file gains new hits or a new
 * file introduces any. Shrinking a file's count prints a reminder to re-baseline.
 *
 * Usage:
 *   node scripts/check-hardcoded-colors.mjs                 # check against baseline
 *   node scripts/check-hardcoded-colors.mjs --update-baseline
 */

import { readFileSync, writeFileSync, readdirSync, statSync } from 'fs';
import { fileURLToPath } from 'url';
import { dirname, resolve, relative, join } from 'path';

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(__dirname, '..');
const BASELINE_PATH = resolve(__dirname, 'hardcoded-colors-baseline.json');

const SCAN_DIRS = ['src/components', 'src/app'];
const SCAN_EXT = /\.(tsx?|css)$/;
// Token definition files where raw color literals are the point.
const EXEMPT = new Set(['src/app/globals.css']);

const HEX_RE = /#(?:[0-9a-fA-F]{8}|[0-9a-fA-F]{6}|[0-9a-fA-F]{4}|[0-9a-fA-F]{3})\b/g;
const FUNC_RE = /\b(?:rgba?|hsla?)\(/g;
const TW_SCALE_RE =
  /\b(?:text|bg|border|ring|fill|stroke|from|via|to|divide|outline|decoration|caret|accent|shadow)-(?:red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose|slate|gray|zinc|neutral|stone)-\d{2,3}\b/g;
const TW_BW_RE = /\b(?:text|bg|border)-(?:white|black)\b/g;

function* walk(dir) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    const st = statSync(p);
    if (st.isDirectory()) yield* walk(p);
    else if (SCAN_EXT.test(name)) yield p;
  }
}

function scanFile(absPath) {
  const hits = [];
  const lines = readFileSync(absPath, 'utf8').split('\n');
  lines.forEach((line, idx) => {
    const trimmed = line.trim();
    // Skip pure comment lines; inline literals in live code are still caught.
    if (trimmed.startsWith('//') || trimmed.startsWith('*') || trimmed.startsWith('/*')) return;
    for (const re of [HEX_RE, FUNC_RE, TW_SCALE_RE, TW_BW_RE]) {
      re.lastIndex = 0;
      let m;
      while ((m = re.exec(line)) !== null) {
        hits.push({ line: idx + 1, match: m[0], text: trimmed.slice(0, 120) });
      }
    }
  });
  return hits;
}

const results = new Map(); // rel path -> hits[]
for (const dir of SCAN_DIRS) {
  for (const abs of walk(resolve(ROOT, dir))) {
    const rel = relative(ROOT, abs);
    if (EXEMPT.has(rel)) continue;
    const hits = scanFile(abs);
    if (hits.length > 0) results.set(rel, hits);
  }
}

if (process.argv.includes('--update-baseline')) {
  const baseline = {};
  for (const [file, hits] of [...results.entries()].sort()) baseline[file] = hits.length;
  writeFileSync(BASELINE_PATH, JSON.stringify(baseline, null, 2) + '\n');
  const total = [...results.values()].reduce((n, h) => n + h.length, 0);
  console.log(`Baseline updated: ${results.size} files, ${total} hits`);
  process.exit(0);
}

let baseline = {};
try {
  baseline = JSON.parse(readFileSync(BASELINE_PATH, 'utf8'));
} catch {
  console.error('No baseline found. Run: node scripts/check-hardcoded-colors.mjs --update-baseline');
  process.exit(1);
}

let exitCode = 0;
let shrunk = 0;
for (const [file, hits] of [...results.entries()].sort()) {
  const allowed = baseline[file] ?? 0;
  if (hits.length > allowed) {
    exitCode = 1;
    console.error(`\n❌ ${file}: ${hits.length} hardcoded color(s), baseline allows ${allowed}`);
    hits.forEach(h => console.error(`   ${file}:${h.line}  [${h.match}]  ${h.text}`));
    console.error('   Use theme tokens (var(--*), .btn classes) instead — see docs/standards/ui-ux/01-design-tokens.md R-U1.');
  } else if (hits.length < allowed) {
    shrunk++;
  }
}
for (const file of Object.keys(baseline)) {
  if (!results.has(file) && baseline[file] > 0) shrunk++;
}

const total = [...results.values()].reduce((n, h) => n + h.length, 0);
if (exitCode === 0) {
  console.log(`✅ Hardcoded colors: no new violations (${total} grandfathered in ${results.size} files)`);
  if (shrunk > 0) {
    console.log(`   ${shrunk} file(s) improved — run with --update-baseline to lock in the progress.`);
  }
}
process.exit(exitCode);
