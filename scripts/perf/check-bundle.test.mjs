/**
 * W1 mutation fixtures for the perf bundle gate (fail-closed).
 * Proves the OLD implementation (3 hard-coded routes, missing chunk counted
 * as 0 bytes) would false-green and the NEW implementation FAILs.
 * Run: node --test scripts/perf/check-bundle.test.mjs
 */
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, mkdirSync, rmSync } from 'node:fs';
import { randomBytes } from 'node:crypto';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const perf = require('./check-bundle.mjs');

function makeFixture(t) {
  return mkdtempSync(join(tmpdir(), `perf-w1-${t}-`));
}
function cleanup(dir) {
  rmSync(dir, { recursive: true, force: true });
}
function writeRoutes(dir, routes) {
  for (const r of routes) {
    const p = r === '/' ? join(dir, 'src', 'app', 'page.tsx') : join(dir, 'src', 'app', `${r.replace(/^\//, '')}`, 'page.tsx');
    mkdirSync(join(p, '..'), { recursive: true });
    writeFileSync(p, 'export default function Page() { return null; }\n');
  }
}
function writeManifest(dir, pages) {
  mkdirSync(join(dir, '.next'), { recursive: true });
  writeFileSync(join(dir, '.next', 'app-build-manifest.json'), JSON.stringify({ pages }, null, 2));
}
function writeChunk(dir, file, sizeBytes) {
  const p = join(dir, '.next', file);
  mkdirSync(join(p, '..'), { recursive: true });
  // random bytes: incompressible, so gzip size stays near the raw size
  writeFileSync(p, randomBytes(sizeBytes));
}

// P1: empty route set (no src/app pages) must fail, not "0 routes = clean"
test('P1: empty route set fails', () => {
  const dir = makeFixture('p1');
  try {
    mkdirSync(join(dir, 'src', 'app'), { recursive: true });
    const summary = perf.runBundleCheck(dir);
    assert.equal(summary.exitCode, 1, 'empty route set must fail');
    assert.equal(summary.emptyRouteSet, true);
  } finally {
    cleanup(dir);
  }
});

// P2: route in src/app missing from manifest must fail (route not built)
test('P2: missing route in manifest fails', () => {
  const dir = makeFixture('p2');
  try {
    writeRoutes(dir, ['/', '/files']);
    writeManifest(dir, { '/layout': [] });
    const summary = perf.runBundleCheck(dir);
    assert.equal(summary.exitCode, 1, 'missing route must fail');
    assert.ok(summary.missingRoutes.includes('/files') || summary.missingRoutes.includes('/'), 'missing route listed');
  } finally {
    cleanup(dir);
  }
});

// P3: route present in manifest but with NO chunk entry must fail
test('P3: missing chunk for a manifest route fails', () => {
  const dir = makeFixture('p3');
  try {
    writeRoutes(dir, ['/files']);
    writeManifest(dir, { '/layout': [], '/files': [] });
    const summary = perf.runBundleCheck(dir);
    assert.equal(summary.exitCode, 1, 'missing chunk must fail');
    assert.ok(summary.missingRoutes.includes('/files'));
  } finally {
    cleanup(dir);
  }
});

// P4: over-budget route fails
test('P4: over-budget route fails', () => {
  const dir = makeFixture('p4');
  try {
    writeRoutes(dir, ['/']);
    writeManifest(dir, { '/layout': [], '/': ['static/chunks/app/page-abc.js'] });
    writeChunk(dir, 'static/chunks/app/page-abc.js', 600 * 1024); // > 350 KB
    const summary = perf.runBundleCheck(dir);
    assert.equal(summary.exitCode, 1, 'over-budget must fail');
    const row = summary.rows.find((r) => r.route === '/');
    assert.equal(row.status, 'over');
  } finally {
    cleanup(dir);
  }
});

// P5: all routes within budget pass (sanity, not a false red)
test('P5: within-budget routes pass', () => {
  const dir = makeFixture('p5');
  try {
    writeRoutes(dir, ['/', '/files']);
    writeManifest(dir, {
      '/layout': [],
      '/': ['static/chunks/app/page-abc.js'],
      '/files': ['static/chunks/app/files/page-def.js'],
    });
    writeChunk(dir, 'static/chunks/app/page-abc.js', 50 * 1024);
    writeChunk(dir, 'static/chunks/app/files/page-def.js', 60 * 1024);
    const summary = perf.runBundleCheck(dir);
    assert.equal(summary.exitCode, 0, 'within-budget must pass');
  } finally {
    cleanup(dir);
  }
});

// P6: route derivation handles route groups and nested dirs
test('P6: deriveRoutes covers route groups and nested pages', () => {
  const dir = makeFixture('p6');
  try {
    writeRoutes(dir, ['/', '/files']);
    const nested = join(dir, 'src', 'app', '(group)', 'settings');
    mkdirSync(nested, { recursive: true });
    writeFileSync(join(nested, 'page.tsx'), 'export default function P() { return null; }\n');
    const routes = perf.deriveRoutes(dir);
    assert.ok(routes.includes('/settings'), 'route group segment normalized');
    assert.ok(routes.includes('/files'));
  } finally {
    cleanup(dir);
  }
});
