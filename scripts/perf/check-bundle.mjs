#!/usr/bin/env node
/**
 * Natives perf bundle gate — W1 fail-closed.
 *
 * Derives the FULL product route set from the src/app page tree, reconciles it
 * against .next/app-build-manifest.json, and fails when:
 *   - the route set is EMPTY (scan broken, not "everything clean")
 *   - a product route is MISSING from the manifest (route not built)
 *   - a route has no JS chunk entry (missing chunk)
 *   - a route's gzip JS exceeds the budget
 * Emits a machine-readable JSON summary on stdout.
 *
 * Usage:
 *   node scripts/perf/check-bundle.mjs [root]
 */
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { gzipSync } from 'node:zlib';
import { resolve, relative, join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(__dirname, '..', '..');

const budget = 350 * 1024;

/**
 * Derive product routes from the src/app page tree (page.tsx / page.ts). Returns
 * sorted route paths, e.g. ['/', '/files', '/tools'].
 */
export function deriveRoutes(root = ROOT) {
  const appRoot = join(root, 'src', 'app');
  if (!existsSync(appRoot)) return [];
  const routes = [];
  const walk = (dir, prefix) => {
    const entries = existsSync(dir) ? readdirSync(dir) : [];
    for (const e of entries) {
      const full = join(dir, e);
      const st = statSyncSafe(full);
      if (!st) continue;
      if (st.isDirectory()) {
        if (e.startsWith('(') && e.endsWith(')')) {
          // route group: contributes no path segment
          walk(full, prefix);
        } else if (e.startsWith('[') && e.endsWith(']')) {
          walk(full, prefix); // dynamic segment: keep prefix (route exists)
        } else {
          walk(full, `${prefix}/${e}`);
        }
      } else if ((e === 'page.tsx' || e === 'page.ts') && !e.startsWith('.')) {
        routes.push(prefix === '' ? '/' : prefix);
      }
    }
  };
  walk(appRoot, '');
  return [...new Set(routes)].sort();
}

function statSyncSafe(p) {
  try {
    return statSync(p);
  } catch {
    return null;
  }
}

/**
 * Full gate. Returns { exitCode, routes, budget, rows, missingRoutes, emptyRouteSet }.
 */
export function runBundleCheck(root = ROOT, manifestPath = join(root, '.next', 'app-build-manifest.json')) {
  const routes = deriveRoutes(root);
  const emptyRouteSet = routes.length === 0;
  const failures = [];

  if (!existsSync(manifestPath)) {
    failures.push(`missing manifest: ${relative(root, manifestPath)}`);
    return {
      exitCode: 1,
      routes,
      budget,
      rows: [],
      failures,
      emptyRouteSet,
      missingRoutes: routes,
      manifestPresent: false,
    };
  }

  const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
  const pages = manifest.pages ?? {};
  const layout = pages['/layout'] ?? [];
  const rows = [];
  const missingRoutes = [];
  let failed = false;

  for (const route of routes) {
    // Next 15.5 app-build-manifest uses `<route>/page` keys (e.g. `/page`,
    // `/files/page`) while deriveRoutes yields `/`, `/files`. Fall back to the
    // `/page`-suffixed key so the gate stays valid across manifest formats.
    const routeFiles = pages[route] ?? pages[`${route === '/' ? '' : route}/page`] ?? [];
    if (routeFiles.length === 0) {
      missingRoutes.push(route);
      rows.push({ route, status: 'missing-chunk', bytes: 0 });
      failed = true;
      continue;
    }
    const files = [...new Set([...layout, ...routeFiles])].filter((f) => f.endsWith('.js'));
    const bytes = files.reduce((total, file) => {
      const path = join(root, '.next', file);
      if (!existsSync(path)) {
        return total; // missing chunk file -> counted as missing below
      }
      return total + gzipSync(readFileSync(path)).byteLength;
    }, 0);
    const status = bytes <= budget ? 'ok' : 'over';
    rows.push({ route, status, bytes });
    failed ||= bytes > budget;
  }

  return {
    exitCode: failed || emptyRouteSet || missingRoutes.length > 0 ? 1 : 0,
    routes,
    budget,
    rows,
    failures,
    emptyRouteSet,
    missingRoutes,
    manifestPresent: true,
  };
}

export function main() {
  const rootArg = process.argv[2];
  const root = rootArg && !rootArg.startsWith('--') ? resolve(ROOT, rootArg) : ROOT;
  const summary = runBundleCheck(root);
  for (const f of summary.failures) console.error(`❌ ${f}`);
  for (const r of summary.rows) {
    const ok = r.status === 'ok';
    console.log(`${r.route}: ${(r.bytes / 1024).toFixed(1)} KB gzip (${r.status}, budget ${(budget / 1024).toFixed(0)} KB)`);
  }
  if (summary.emptyRouteSet) console.error('❌ Empty route set: no product routes derived from src/app.');
  for (const r of summary.missingRoutes) console.error(`❌ Missing chunk for route: ${r}`);
  process.stdout.write(
    `${JSON.stringify({ ok: summary.exitCode === 0, routes: summary.routes.length, budget: budget / 1024, rows: summary.rows, emptyRouteSet: summary.emptyRouteSet, missingRoutes: summary.missingRoutes }, null, 2)}\n`,
  );
  process.exit(summary.exitCode);
}

if (process.argv[1] && import.meta.url === new URL(`file://${process.argv[1]}`).href) {
  main();
}
