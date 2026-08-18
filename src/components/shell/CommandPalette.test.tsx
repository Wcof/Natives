import assert from 'node:assert/strict';
import test from 'node:test';
import type { SearchResult } from '@/types/generated/SearchResult';
import {
  createLatestRequestGate,
  resolvePaletteSearchRoot,
  searchResultLabel,
} from './CommandPalette';

test('palette resolves only the Host home root and never accepts slash fallback', () => {
  assert.equal(resolvePaletteSearchRoot([
    { id: 'home', name: 'Home', path: '/Users/tester' },
    { id: 'tmp', name: 'tmp', path: '/tmp' },
  ]), '/Users/tester');
  assert.equal(resolvePaletteSearchRoot([{ id: 'home', name: 'Home', path: '/' }]), null);
  assert.equal(resolvePaletteSearchRoot([]), null);
});

test('palette labels generated SearchResult fields without an undefined name', () => {
  const result: SearchResult = {
    path: '/Users/tester/project/readme.md',
    line: 7,
    text: 'matching text',
    score: 42,
    mtime: null,
  };
  assert.equal(searchResultLabel(result), 'readme.md:7');
  assert.doesNotMatch(searchResultLabel(result), /undefined/);
});

test('latest request gate suppresses a stale completion and invalidates on close', async () => {
  const gate = createLatestRequestGate();
  const published: string[] = [];
  const first = gate.next();
  const second = gate.next();

  await Promise.resolve().then(() => {
    if (gate.isCurrent(second)) published.push('second');
  });
  await Promise.resolve().then(() => {
    if (gate.isCurrent(first)) published.push('first');
  });
  gate.invalidate();

  assert.deepEqual(published, ['second']);
  assert.equal(gate.isCurrent(second), false);
});
