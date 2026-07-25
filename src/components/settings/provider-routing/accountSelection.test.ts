import assert from 'node:assert/strict';
import test from 'node:test';
import { selectionAfterDelete, toggleAccountSelection } from './accountSelection';

test('Sub2API account selection toggles only the requested account', () => {
  assert.deepEqual([...toggleAccountSelection(new Set(['a']), 'b')], ['a', 'b']);
  assert.deepEqual([...toggleAccountSelection(new Set(['a']), 'a')], []);
});

test('Sub2API account selection removes deleted accounts', () => {
  assert.deepEqual([...selectionAfterDelete(new Set(['a', 'b']), ['a'])], ['b']);
});
