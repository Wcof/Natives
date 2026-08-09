#!/usr/bin/env node
/**
 * Natives zero-dependency architecture gate.
 *
 * Checks (each violation maps to a debt-manifest key `file:line` or `file`):
 *   1. over_1000            handwritten source files > 1,000 lines
 *   2. over_700_ledger      files > 700 lines (review ledger, warning only)
 *   3. ui_business          components/ui importing business modules
 *   4. feature_cross        feature importing another feature's internals
 *   5. thick_page           src/app page.tsx with thick logic
 *   6. raw_invoke           bare invoke() outside src/lib/tauri
 *   7. native_dialog        alert()/prompt()/confirm() in src
 *   8. cross_db_daemon      daemon code paths touching Host natives.db
 *   9. cross_db_host        host code paths touching Daemon assistant.db
 *  10. embedded_prod        EmbeddedAuthority in production compile surface
 *  11. global_singleton     new static mut / OnceLock / lazy_static
 *  12. unregistered_interval setInterval in src without ledger entry (warning)
 *  13. hooks_reverse        src/hooks importing @/components
 *  14. budget_functions     heuristic fn bodies > 120 lines (warning)
 *
 * Debt manifest: scripts/architecture-debt-manifest.json. `--baseline`
 * snapshots all current violations so the gate passes while remediation is in
 * flight; afterwards any NEW violation (not in the manifest) fails the gate.
 * Warnings (over_700_ledger / unregistered_interval / budget_functions) never
 * fail; they only produce a review ledger. The manifest must be emptied by the
 * end of the remediation — no permanent exemptions.
 *
 * Usage:
 *   node scripts/architecture-check.mjs [--baseline]
 */

import { readFileSync, readdirSync, statSync, existsSync, writeFileSync } from 'fs';
import { resolve, dirname, relative, join, sep } from 'path';
import { fileURLToPath } from 'url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(__dirname, '..');
const MANIFEST = join(__dirname, 'architecture-debt-manifest.json');
const BASELINE = process.argv.includes('--baseline');

// ---------------------------------------------------------------------------
// Scan helpers
// ---------------------------------------------------------------------------

const SKIP_RE =
  /(^|\/)(node_modules|target|\.next|\.git|coverage|dist|out|\.runtime-evidence|archive|build)(\/|$)/;
const GENERATED_RE = /(^|\/)(generated|gen)(\/|$)/;

function walk(dir, out = []) {
  let entries;
  try {
    entries = readdirSync(dir, { withFileTypes: true });
  } catch {
    return out;
  }
  for (const e of entries) {
    const p = join(dir, e.name);
    if (e.isDirectory()) {
      if (SKIP_RE.test(p) || GENERATED_RE.test(p)) continue;
      walk(p, out);
    } else if (e.isFile()) {
      out.push(p);
    }
  }
  return out;
}

function isHandwrittenTs(p) {
  return /\.(ts|tsx)$/.test(p) && !/\.test\.(ts|tsx)$/.test(p) && !p.includes('src/types/generated');
}

function isTestFile(p) {
  return /\.test\.(ts|tsx|rs)$/.test(p) || /(\/|^)tests?(\/|\.)/.test(p) || /_tests?\.rs$/.test(p);
}

function countLines(p) {
  try {
    return readFileSync(p, 'utf8').split('\n').length;
  } catch {
    return 0;
  }
}

function nonCommentLines(p, re) {
  const hits = [];
  const lines = readFileSync(p, 'utf8').split('\n');
  lines.forEach((line, i) => {
    const t = line.trim();
    if (t.startsWith('//') || t.startsWith('*') || t.startsWith('/*') || t.startsWith('#!')) return;
    if (re.test(t)) hits.push({ line: i + 1, text: t.slice(0, 110) });
  });
  return hits;
}

function importsOf(p, src) {
  const out = [];
  const re = /from\s+['"]([^'"]+)['"]/g;
  let m;
  while ((m = re.exec(src)) !== null) out.push(m[1]);
  return out;
}

function relToRoot(p) {
  return relative(ROOT, p).split(sep).join('/');
}

// ---------------------------------------------------------------------------
// Collectors (return Map<key, reason>)
// ---------------------------------------------------------------------------

function collectOver1000() {
  const map = new Map();
  const roots = [
    join(ROOT, 'src'),
    join(ROOT, 'src-tauri/src'),
    join(ROOT, 'src-agent-daemon/src'),
    join(ROOT, 'crates'),
  ];
  for (const root of roots) {
    for (const p of walk(root)) {
      if (!/\.(ts|tsx|css|rs)$/.test(p)) continue;
      if (GENERATED_RE.test(p)) continue;
      const n = countLines(p);
      if (n > 1000) map.set(relToRoot(p), `${n} lines`);
    }
  }
  return map;
}

function collectOver700() {
  const map = new Map();
  for (const root of [join(ROOT, 'src'), join(ROOT, 'src-tauri/src'), join(ROOT, 'src-agent-daemon/src'), join(ROOT, 'crates')]) {
    for (const p of walk(root)) {
      if (!/\.(ts|tsx|css|rs)$/.test(p)) continue;
      const n = countLines(p);
      if (n > 700 && n <= 1000) map.set(relToRoot(p), `${n} lines (review ledger)`);
    }
  }
  return map;
}

function collectUiBusiness() {
  const map = new Map();
  const uiRoot = join(ROOT, 'src/components/ui');
  if (!existsSync(uiRoot)) return map;
  // UI atoms may only import: external packages, other ui files, design-tokens,
  // i18n (framework), pure type files, and generic UI infrastructure libs
  // (focus trap / markdown safety / toast context — no business logic).
  const LIB_ALLOW = ['design-tokens', 'i18n', 'types/', 'useFocusTrap', 'markdown-safety', 'toast-context'];
  for (const p of walk(uiRoot)) {
    if (!isHandwrittenTs(p)) continue;
    const src = readFileSync(p, 'utf8');
    for (const spec of importsOf(p, src)) {
      if (/^['"]/.test(spec) && !spec.startsWith('@/') && !spec.startsWith('.')) continue; // external
      if (spec.startsWith('@/components/ui')) continue; // sibling ui
      if (spec.startsWith('@/lib/')) {
        const rest = spec.slice('@/lib/'.length);
        if (LIB_ALLOW.some((a) => rest.startsWith(a))) continue;
        map.set(`${relToRoot(p)}:${spec}`, `ui imports business lib ${spec}`);
      } else if (spec.startsWith('@/components/')) {
        map.set(`${relToRoot(p)}:${spec}`, `ui imports business component ${spec}`);
      } else if (spec.startsWith('.')) {
        const target = resolve(p, '..', spec);
        if (target.startsWith(uiRoot)) continue;
        map.set(`${relToRoot(p)}:${spec}`, `ui imports relative business module ${spec}`);
      }
    }
  }
  return map;
}

function collectFeatureCross() {
  const map = new Map();
  const compRoot = join(ROOT, 'src/components');
  if (!existsSync(compRoot)) return map;
  for (const p of walk(compRoot)) {
    if (!isHandwrittenTs(p)) continue;
    const rel = relToRoot(p);
    const m = /^src\/components\/([^/]+)\//.exec(rel);
    if (!m) continue;
    const myDomain = m[1];
    if (myDomain === 'ui' || myDomain === 'shell') continue;
    const src = readFileSync(p, 'utf8');
    for (const spec of importsOf(p, src)) {
      if (!spec.startsWith('@/components/')) continue;
      const target = spec.slice('@/components/'.length).split('/')[0];
      if (target === myDomain || target === 'ui' || target === 'shell') continue;
      map.set(`${rel}:${spec}`, `feature ${myDomain} imports ${target} internals`);
    }
  }
  return map;
}

function collectThickPages() {
  const map = new Map();
  const appRoot = join(ROOT, 'src/app');
  if (!existsSync(appRoot)) return map;
  for (const p of walk(appRoot)) {
    if (!p.endsWith('page.tsx') && !p.endsWith('page.ts')) continue;
    const n = countLines(p);
    if (n > 80) map.set(relToRoot(p), `${n} lines (page should stay thin)`);
  }
  return map;
}

function collectRawInvoke() {
  const map = new Map();
  for (const p of walk(join(ROOT, 'src'))) {
    if (!isHandwrittenTs(p)) continue;
    const rel = relToRoot(p);
    if (rel.startsWith('src/lib/tauri/') || rel.startsWith('src/lib/tauri-adapter')) continue;
    for (const h of nonCommentLines(p, /(^|[^\w.])\binvoke\s*\(/)) {
      map.set(`${rel}:${h.line}`, h.text);
    }
  }
  return map;
}

function collectNativeDialog() {
  const map = new Map();
  for (const p of walk(join(ROOT, 'src'))) {
    if (!isHandwrittenTs(p)) continue;
    for (const h of nonCommentLines(p, /(^|[^\w.])(alert|prompt|confirm)\s*\(/)) {
      map.set(`${relToRoot(p)}:${h.line}`, h.text);
    }
  }
  return map;
}

function collectCrossDbDaemon() {
  const map = new Map();
  const daemonRoot = join(ROOT, 'src-agent-daemon/src');
  if (!existsSync(daemonRoot)) return map;
  for (const p of walk(daemonRoot)) {
    if (!p.endsWith('.rs')) continue;
    if (isTestFile(p)) continue;
    // Only flag an actual DB-open into the Host-authoritative natives.db:
    //   Connection::open / open_with_flags whose target is default_natives_db_path
    //   or the `natives.db` literal. Broker lease clients (`NativesDbBroker::*`,
    //   `open_default`) never open the file, fail-closed `write_setting` never
    //   writes it, and `default_assistant_db_path` is the daemon's own store.
    const re = /(Connection::open|open_with_flags)\([^)]*(default_natives_db_path|join\("natives\.db"\)|"natives\.db")/;
    for (const h of nonCommentLines(p, re)) {
      map.set(`${relToRoot(p)}:${h.line}`, h.text);
    }
  }
  return map;
}

function collectCrossDbHost() {
  const map = new Map();
  const hostRoot = join(ROOT, 'src-tauri/src');
  if (!existsSync(hostRoot)) return map;
  for (const p of walk(hostRoot)) {
    if (!p.endsWith('.rs')) continue;
    if (isTestFile(p)) continue;
    // Host must never open the Daemon-authoritative assistant.db. `get_assistant_db_conn`
    // is the pooled open; a direct `Connection::open` of the assistant.db literal is
    // equally a violation. Setting the NATIVES_ASSISTANT_DB_PATH env var only hands the
    // path to the daemon sidecar (which owns the file) and is not a Host open.
    const re = /get_assistant_db_conn|init_assistant_db|(Connection::open)\([^)]*assistant\.db/;
    for (const h of nonCommentLines(p, re)) {
      map.set(`${relToRoot(p)}:${h.line}`, h.text);
    }
  }
  return map;
}

function collectEmbeddedProd() {
  const map = new Map();
  for (const root of [join(ROOT, 'src-tauri/src'), join(ROOT, 'src-agent-daemon/src')]) {
    for (const p of walk(root)) {
      if (!p.endsWith('.rs')) continue;
      if (isTestFile(p)) continue;
      for (const h of nonCommentLines(p, /EmbeddedAuthority/)) {
        map.set(`${relToRoot(p)}:${h.line}`, h.text);
      }
    }
  }
  return map;
}

function collectGlobalSingleton() {
  const map = new Map();
  for (const root of [join(ROOT, 'src-tauri/src'), join(ROOT, 'src-agent-daemon/src'), join(ROOT, 'crates')]) {
    for (const p of walk(root)) {
      if (!p.endsWith('.rs')) continue;
      if (isTestFile(p)) continue;
      for (const h of nonCommentLines(p, /static\s+mut|OnceLock<|lazy_static!|static\s+Lazy</)) {
        map.set(`${relToRoot(p)}:${h.line}`, h.text);
      }
    }
  }
  return map;
}

function collectUnregisteredInterval() {
  const map = new Map();
  for (const p of walk(join(ROOT, 'src'))) {
    if (!isHandwrittenTs(p)) continue;
    const rel = relToRoot(p);
    for (const h of nonCommentLines(p, /setInterval\s*\(/)) {
      map.set(`${rel}:${h.line}`, h.text);
    }
  }
  return map;
}

function collectHooksReverse() {
  const map = new Map();
  const hooksRoot = join(ROOT, 'src/hooks');
  if (!existsSync(hooksRoot)) return map;
  for (const p of walk(hooksRoot)) {
    if (!isHandwrittenTs(p)) continue;
    const src = readFileSync(p, 'utf8');
    for (const spec of importsOf(p, src)) {
      if (spec.startsWith('@/components/')) {
        map.set(`${relToRoot(p)}:${spec}`, `hook imports component ${spec}`);
      }
    }
  }
  return map;
}

function collectBudgetFunctions() {
  const map = new Map();
  for (const root of [join(ROOT, 'src-tauri/src'), join(ROOT, 'src-agent-daemon/src'), join(ROOT, 'crates')]) {
    for (const p of walk(root)) {
      if (!p.endsWith('.rs')) continue;
      if (isTestFile(p)) continue;
      const lines = readFileSync(p, 'utf8').split('\n');
      let fnStart = -1;
      let brace = 0;
      let started = false;
      for (let i = 0; i < lines.length; i++) {
        const t = lines[i].trim();
        if (/^(pub\s+)?(pub\([^)]*\)\s+)?(async\s+)?(unsafe\s+)?fn\s+\w+/.test(t)) {
          if (fnStart >= 0 && started && i - fnStart > 120) {
            map.set(`${relToRoot(p)}:${fnStart + 1}`, `fn body ~${i - fnStart} lines`);
          }
          fnStart = i;
          started = false;
          brace = 0;
          const open = (t.match(/{/g) || []).length;
          const close = (t.match(/}/g) || []).length;
          brace = open - close;
          if (brace > 0) started = true;
          continue;
        }
        if (fnStart >= 0 && started) {
          brace += (t.match(/{/g) || []).length - (t.match(/}/g) || []).length;
          if (brace <= 0) {
            if (i - fnStart > 120) map.set(`${relToRoot(p)}:${fnStart + 1}`, `fn body ~${i - fnStart} lines`);
            fnStart = -1;
            started = false;
          }
        }
      }
    }
  }
  return map;
}

// ---------------------------------------------------------------------------
// Checks registry: id -> { collect, fail: bool }
// ---------------------------------------------------------------------------

const CHECKS = [
  { id: 'over_1000', collect: collectOver1000, fail: true },
  { id: 'over_700_ledger', collect: collectOver700, fail: false },
  { id: 'ui_business', collect: collectUiBusiness, fail: true },
  { id: 'feature_cross', collect: collectFeatureCross, fail: true },
  { id: 'thick_page', collect: collectThickPages, fail: true },
  { id: 'raw_invoke', collect: collectRawInvoke, fail: true },
  { id: 'native_dialog', collect: collectNativeDialog, fail: true },
  { id: 'cross_db_daemon', collect: collectCrossDbDaemon, fail: true },
  { id: 'cross_db_host', collect: collectCrossDbHost, fail: true },
  { id: 'embedded_prod', collect: collectEmbeddedProd, fail: true },
  { id: 'global_singleton', collect: collectGlobalSingleton, fail: true },
  { id: 'unregistered_interval', collect: collectUnregisteredInterval, fail: false },
  { id: 'hooks_reverse', collect: collectHooksReverse, fail: true },
  { id: 'budget_functions', collect: collectBudgetFunctions, fail: false },
];

function loadManifest() {
  if (!existsSync(MANIFEST)) return {};
  try {
    return JSON.parse(readFileSync(MANIFEST, 'utf8'));
  } catch {
    return {};
  }
}

function main() {
  const manifest = loadManifest();
  let newCount = 0;
  let knownCount = 0;
  const rows = [];

  for (const check of CHECKS) {
    const found = check.collect();
    const known = manifest[check.id] || {};
    const knownKeys = new Set(Object.keys(known));
    const newEntries = [];
    const knownEntries = [];
    for (const [key, reason] of found) {
      if (knownKeys.has(key)) knownEntries.push({ key, reason });
      else newEntries.push({ key, reason });
    }
    knownCount += knownEntries.length;
    newCount += newEntries.length;
    rows.push({ id: check.id, fail: check.fail, found: found.size, known: knownEntries.length, new: newEntries.length });

    if (BASELINE) continue;
    for (const e of newEntries) {
      const severity = check.fail ? 'ERROR' : 'WARN';
      console.log(`[${severity}] ${check.id}: ${e.key} — ${e.reason}`);
    }
  }

  if (BASELINE) {
    const next = {};
    for (const check of CHECKS) {
      const found = check.collect();
      next[check.id] = Object.fromEntries(found);
    }
    writeFileSync(MANIFEST, `${JSON.stringify(next, null, 2)}\n`);
    console.log('architecture:check baseline written to scripts/architecture-debt-manifest.json');
    console.log('  violations snapshotted:');
    for (const r of rows) console.log(`  - ${r.id}: ${r.found}`);
    process.exit(0);
  }

  const failing = rows.filter((r) => r.fail && r.new > 0);
  console.log('---');
  console.log(
    `architecture:check ${failing.length ? 'FAILED' : 'OK'} — known(debt): ${knownCount}, new: ${newCount}`,
  );
  for (const r of rows) {
    const tag = r.fail ? (r.new > 0 ? 'FAIL' : 'ok') : 'ledger';
    console.log(`  [${tag}] ${r.id}: total=${r.found} known=${r.known} new=${r.new}`);
  }
  if (failing.length) {
    console.error('New architecture violations detected. Fix them or, during an approved migration step, run --baseline.');
    process.exit(1);
  }
  process.exit(0);
}

main();
