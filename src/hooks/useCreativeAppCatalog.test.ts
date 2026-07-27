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
