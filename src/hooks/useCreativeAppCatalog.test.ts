import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';

const source = readFileSync(new URL('./useCreativeAppCatalog.ts', import.meta.url), 'utf8');

test('catalog reconciliation does not restart when a reload replaces the app array', () => {
  assert.match(source, /const dataRef = useRef/);
  assert.doesNotMatch(
    source,
    /\}, \[data,\s*enabled,\s*reload\]\);/,
    'data change → effect restart → reconcile → reload → data change causes the page refresh loop',
  );
});

test('operation journal projection subscribes with cleanup (batch 2 CR-203)', () => {
  // The hook must take an initial snapshot of in-flight operations…
  assert.match(source, /api\.creativeApp\s*\.\s*operations\s*\(\)/);
  // …subscribe to operation-changed events…
  assert.match(source, /onOperationChanged\s*\(/);
  // …and unsubscribe on unmount (R-E13).
  assert.match(source, /return \(\) => \{\s*\n\s*unsub\?\.\(\);\s*\n\s*\};/);
});

test('busy ids derive from Host operation facts, not only session calls', () => {
  // busyIds is the union of the operation-derived set and in-flight commands.
  assert.match(source, /deriveBusyIds/);
  assert.match(source, /const busyIds = useMemo/);
  // Terminal operations trigger a catalog reload so state/lastError refresh.
  assert.match(source, /if \(!isOperationActive\(op\)\)/);
  assert.match(source, /void reload\(\);/);
});
