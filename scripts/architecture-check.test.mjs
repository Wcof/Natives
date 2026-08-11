/**
 * W1 mutation fixtures for the architecture gate (fail-closed).
 * Proves the OLD implementation would false-green on each case and the NEW
 * implementation FAILs. Run: node --test scripts/architecture-check.test.mjs
 */
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, mkdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

// The gate module must be loadable WITHOUT the repo's node_modules for tests
// that only need fs collectors; the a11y AST collector lazily resolves
// typescript via createRequire inside the gate module (works in both worktree
// and main workspace). Dynamic import (not require()) so this test file itself
// runs under Node 20 (project engines >=20.11 <21): require() of ESM only
// works on Node >=22.12.
let arch;
try {
  arch = await import('./architecture-check.mjs');
} catch {
  throw new Error('architecture-check.mjs must be importable without typescript at module scope');
}

function makeFixture(t) {
  const dir = mkdtempSync(join(tmpdir(), `arch-w1-${t}-`));
  return dir;
}

function cleanup(dir) {
  rmSync(dir, { recursive: true, force: true });
}

// ---------------------------------------------------------------------------
// F1: handwritten test file > 1000 lines must be flagged (production AND tests)
// ---------------------------------------------------------------------------
test('F1: test dir file over 1000 lines is flagged by over_1000', () => {
  const dir = makeFixture('f1');
  try {
    const p = join(dir, 'src-agent-daemon', 'tests', 'huge_test.rs');
    mkdirSync(join(dir, 'src-agent-daemon', 'tests'), { recursive: true });
    writeFileSync(p, Array.from({ length: 1001 }, (_, i) => `// line ${i}`).join('\n'));
    const found = arch.collectOver1000(dir);
    assert.ok(found.size >= 1, `over_1000 must flag ${p}`);
    assert.ok([...found.keys()].some((k) => k.includes('huge_test.rs')), 'flagged file is the 1001-line test');
  } finally {
    cleanup(dir);
  }
});

// ---------------------------------------------------------------------------
// F2: caller-variable path into assistant.db (Host side) must be flagged
// ---------------------------------------------------------------------------
test('F2: host caller-variable path into assistant.db is flagged by cross_db_host', () => {
  const dir = makeFixture('f2');
  try {
    const p = join(dir, 'src-tauri', 'src', 'host_db.rs');
    mkdirSync(join(dir, 'src-tauri', 'src'), { recursive: true });
    writeFileSync(
      p,
      [
        'use rusqlite::Connection;',
        'pub fn legacy(db_path: &str) {',
        '  let db = if db_path.is_empty() { "assistant.db" } else { db_path };',
        '  let conn = Connection::open(db).unwrap();',
        '  let _ = conn;',
        '}',
      ].join('\n'),
    );
    // Without dataflow, the conservative rule: any `Connection::open(` whose
    // argument is an identifier (variable) is a candidate — the manifest/ledger
    // can review false positives, but a path literal must be a hard fail.
    const found = arch.collectCrossDbHost(dir);
    assert.ok(found.size >= 1, 'host Connection::open(variable) must be flagged (fail-closed)');
  } finally {
    cleanup(dir);
  }
});

// ---------------------------------------------------------------------------
// F3: daemon wrapper / env fallback into natives.db must be flagged
// ---------------------------------------------------------------------------
test('F3: daemon env-fallback path into natives.db is flagged by cross_db_daemon', () => {
  const dir = makeFixture('f3');
  try {
    const p = join(dir, 'src-agent-daemon', 'src', 'env_fallback.rs');
    mkdirSync(join(dir, 'src-agent-daemon', 'src'), { recursive: true });
    writeFileSync(
      p,
      [
        'fn open_fallback() -> rusqlite::Result<rusqlite::Connection> {',
        '  let path = std::env::var("NATIVES_HOST_DB").unwrap_or_else(|_| "natives.db".to_string());',
        '  let conn = rusqlite::Connection::open_with_flags(&path, Default::default())?;',
        '  Ok(conn)',
        '}',
      ].join('\n'),
    );
    const found = arch.collectCrossDbDaemon(dir);
    assert.ok(found.size >= 1, 'daemon env fallback to natives.db literal must be flagged');
  } finally {
    cleanup(dir);
  }
});

// ---------------------------------------------------------------------------
// F4: fatal entries inside the manifest still FAIL (no baseline silencing)
// ---------------------------------------------------------------------------
test('F4: over_1000 entry present in manifest still fails the gate', () => {
  // Simulate the main() exit logic for a fail:true check with a known entry.
  const known = { 'src/foo/bar.ts': 'was known' };
  const found = new Map([['src/foo/bar.ts', 'still there']]);
  const fail = arch.isFatalCheck('over_1000');
  assert.equal(fail, true);
  const newCount = [...found.keys()].filter((k) => !Object.prototype.hasOwnProperty.call(known, k)).length;
  // F4 asserts the policy decision, not the old counting: fatal checks must not
  // be silenced by the manifest at all. The gate's main() consults
  // shouldFailCheck(check, found, known) which returns true for fail:true with
  // ANY found entry.
  assert.equal(arch.shouldFailCheck({ id: 'over_1000', fail: true }, found, known), true);
  assert.equal(newCount, 0); // old counting alone would have passed -> false green
});

// ---------------------------------------------------------------------------
// F5: multiline TSX non-semantic click must be flagged by a11y (AST-based)
// ---------------------------------------------------------------------------
test('F5: multiline TSX div onClick without role/keyboard is flagged', () => {
  const dir = makeFixture('f5');
  try {
    const p = join(dir, 'src', 'components', 'widget.tsx');
    mkdirSync(join(dir, 'src', 'components'), { recursive: true });
    writeFileSync(
      p,
      [
        'export function Widget() {',
        '  return (',
        '    <div',
        '      className="clickable"',
        '      onClick={() => doThing()}',
        '    >',
        '      click me',
        '    </div>',
        '  );',
        '}',
      ].join('\n'),
    );
    const found = arch.collectA11yClickable(dir);
    assert.ok(found.size >= 1, 'multiline non-semantic click must be flagged');
    const key = [...found.keys()][0];
    assert.ok(key.includes('widget.tsx'), 'flagged element is in the fixture');
  } finally {
    cleanup(dir);
  }
});

// ---------------------------------------------------------------------------
// W2a: top-level `export const module = {}` must be flagged (fail-closed)
// ---------------------------------------------------------------------------
test('W2a: top-level export const module is flagged by webpack_module_shadow', () => {
  const dir = makeFixture('w2a');
  try {
    const p = join(dir, 'src', 'lib', 'tauri', 'bad.ts');
    mkdirSync(join(dir, 'src', 'lib', 'tauri'), { recursive: true });
    writeFileSync(
      p,
      [
        'export const module = {',
        '  scan: () => cmd("module_scan"),',
        '};',
      ].join('\n'),
    );
    const found = arch.collectWebpackModuleShadow(dir);
    assert.ok(found.size >= 1, 'top-level export const module must be flagged');
    assert.ok([...found.keys()].some((k) => k.includes('bad.ts')), 'flagged file is the fixture');
    assert.equal(
      arch.shouldFailCheck({ id: 'webpack_module_shadow', fail: true }, found, {}),
      true,
      'fatal gate must fail on top-level module binding',
    );
  } finally {
    cleanup(dir);
  }
});

// ---------------------------------------------------------------------------
// W2b: safe alias `const moduleApi = {}; export { moduleApi as module };` must
// stay green (this is the accepted fix shape)
// ---------------------------------------------------------------------------
test('W2b: export alias `{ moduleApi as module }` stays green', () => {
  const dir = makeFixture('w2b');
  try {
    const p = join(dir, 'src', 'lib', 'tauri', 'ok.ts');
    mkdirSync(join(dir, 'src', 'lib', 'tauri'), { recursive: true });
    writeFileSync(
      p,
      [
        'const moduleApi = {',
        '  scan: () => cmd("module_scan"),',
        '};',
        'export { moduleApi as module };',
      ].join('\n'),
    );
    const found = arch.collectWebpackModuleShadow(dir);
    assert.equal(found.size, 0, 'export alias must not create a local binding');
    assert.equal(
      arch.shouldFailCheck({ id: 'webpack_module_shadow', fail: true }, found, {}),
      false,
      'safe facade must stay green',
    );
    const row = arch.runChecks(dir).rows.find((r) => r.id === 'webpack_module_shadow');
    assert.equal(row && row.status, 'ok', 'end-to-end gate must stay green');
  } finally {
    cleanup(dir);
  }
});

// ---------------------------------------------------------------------------
// W2c: properties, type-only imports, strings, comments and nested locals must
// stay green (no false positives)
// ---------------------------------------------------------------------------
test('W2c: properties, type-only imports, strings, comments and nested locals stay green', () => {
  const dir = makeFixture('w2c');
  try {
    const p = join(dir, 'src', 'safe.tsx');
    mkdirSync(join(dir, 'src'), { recursive: true });
    writeFileSync(
      p,
      [
        "import type { module } from './types';",
        'api.module.list();',
        'const obj = { module: moduleApi };',
        'const s = "module";',
        '// const module = 1;',
        'function f() { const module = 1; return module; }',
      ].join('\n'),
    );
    const found = arch.collectWebpackModuleShadow(dir);
    assert.equal(found.size, 0, 'safe patterns must not be flagged');
  } finally {
    cleanup(dir);
  }
});

// ---------------------------------------------------------------------------
// W2d: `const module` / `let module` / `var module` at top level are all flagged
// ---------------------------------------------------------------------------
test('W2d: const/let/var module at top level are all flagged', () => {
  const dir = makeFixture('w2d');
  try {
    for (const [name, decl] of [
      ['c.ts', 'const module = 1;'],
      ['l.ts', 'let module: unknown;'],
      ['v.ts', 'var module = 2;'],
    ]) {
      const p = join(dir, 'src', name);
      mkdirSync(join(dir, 'src'), { recursive: true });
      writeFileSync(p, decl);
      const found = arch.collectWebpackModuleShadow(dir);
      assert.ok(found.size >= 1, `${decl} must be flagged`);
    }
  } finally {
    cleanup(dir);
  }
});

// ---------------------------------------------------------------------------
// W2e: top-level function/class named `module` and runtime import aliasing to
// `module` are flagged
// ---------------------------------------------------------------------------
test('W2e: function/class module and import alias to module are flagged', () => {
  const dir = makeFixture('w2e');
  try {
    const p = join(dir, 'src', 'decls.ts');
    mkdirSync(join(dir, 'src'), { recursive: true });
    writeFileSync(
      p,
      [
        'import { cmd as module } from "./core";',
        'function module() {}',
        'class module {}',
      ].join('\n'),
    );
    const found = arch.collectWebpackModuleShadow(dir);
    assert.ok(found.size >= 1, 'function/class/import-alias module must be flagged');
  } finally {
    cleanup(dir);
  }
});


// ---------------------------------------------------------------------------
// W3: embedded_prod accuracy — the gate must flag production Host references
// to EmbeddedAuthority but NOT cfg-gated code or the Daemon's own definition.
// ---------------------------------------------------------------------------
test('W3a: Host production reference to EmbeddedAuthority stays red', () => {
  const dir = makeFixture('w3a');
  try {
    const p = join(dir, 'src-tauri', 'src', 'authority_facade.rs');
    mkdirSync(join(dir, 'src-tauri', 'src'), { recursive: true });
    writeFileSync(
      p,
      [
        'use natives_agent_daemon::EmbeddedAuthority;',
        'pub fn build() -> Result<Arc<dyn ExecutionAuthority>, String> {',
        '    Ok(Arc::new(EmbeddedAuthority::new()))',
        '}',
      ].join('\n'),
    );
    const found = arch.collectEmbeddedProd(dir);
    assert.ok(found.size >= 1, 'production Host EmbeddedAuthority reference must be flagged');
    assert.ok([...found.keys()].some((k) => k.includes('authority_facade.rs')), 'flagged file is the fixture');
  } finally {
    cleanup(dir);
  }
});

test('W3b: cfg(test/diagnostic)-gated Host reference stays green', () => {
  const dir = makeFixture('w3b');
  try {
    const p = join(dir, 'src-tauri', 'src', 'gated.rs');
    mkdirSync(join(dir, 'src-tauri', 'src'), { recursive: true });
    writeFileSync(
      p,
      [
        '#[cfg(any(test, feature = "diagnostic"))]',
        'use natives_agent_daemon::EmbeddedAuthority;',
        '#[cfg(any(test, feature = "diagnostic"))]',
        'fn build_embedded() -> Result<Arc<dyn ExecutionAuthority>, String> {',
        '    Ok(Arc::new(EmbeddedAuthority::new()))',
        '}',
      ].join('\n'),
    );
    const found = arch.collectEmbeddedProd(dir);
    assert.equal(found.size, 0, 'cfg(test/diagnostic)-gated code must not be flagged');
  } finally {
    cleanup(dir);
  }
});

test('W3c: Daemon-side EmbeddedAuthority definition stays green (Host-only scan)', () => {
  const dir = makeFixture('w3c');
  try {
    // Daemon legitimately DEFINES the embedded capability for tests/diagnostic
    // (architecture #5 / R-T1); the collector must not scan src-agent-daemon.
    const p = join(dir, 'src-agent-daemon', 'src', 'authority.rs');
    mkdirSync(join(dir, 'src-agent-daemon', 'src'), { recursive: true });
    writeFileSync(
      p,
      [
        'pub struct EmbeddedAuthority { /* in-process execution capability */ }',
        'impl ExecutionAuthority for EmbeddedAuthority {}',
      ].join('\n'),
    );
    const found = arch.collectEmbeddedProd(dir);
    assert.equal(found.size, 0, 'Daemon-side EmbeddedAuthority definition must not be flagged');
  } finally {
    cleanup(dir);
  }
});

test('F6: gate with no scanned files must fail (empty scope)', () => {
  const dir = makeFixture('f6');
  try {
    // scope exists but contains nothing scanable
    mkdirSync(join(dir, 'src'), { recursive: true });
    const summary = arch.runChecks(dir);
    assert.ok(Array.isArray(summary.rows));
    assert.ok(summary.scannedFiles === 0 || summary.rows.length > 0);
    // The gate must fail when total found across fatal checks is 0 but scope is
    // empty: empty scope is indistinguishable from "everything clean", so
    // main() must treat scannedFiles === 0 as a fatal.
    assert.equal(arch.shouldFailEmptyScope(summary), true);
  } finally {
    cleanup(dir);
  }
});
