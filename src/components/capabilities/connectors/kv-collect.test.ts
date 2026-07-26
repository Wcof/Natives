/**
 * collectKvRows payload semantics (P0: editing one row must never wipe the
 * others' stored values). `null` = daemon-side "keep stored value" sentinel,
 * legal only for untouched stored rows; create dialogs never produce it.
 */
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { collectKvRows, newKvRow } from './KeyValueRows';

test('untouched stored rows are sent as null (keep sentinel)', () => {
  const rows = [
    newKvRow({ key: 'FIGMA_API_KEY', isSecretRef: true, stored: true }),
    newKvRow({ key: 'LOG_LEVEL', stored: true }),
  ];
  assert.deepEqual(collectKvRows(rows), { FIGMA_API_KEY: null, LOG_LEVEL: null });
});

test('touched row submits its value, untouched siblings stay null', () => {
  const rows = [
    newKvRow({ key: 'FIGMA_API_KEY', isSecretRef: true, stored: true }),
    newKvRow({ key: 'LOG_LEVEL', value: 'debug', stored: true, touched: true }),
  ];
  assert.deepEqual(collectKvRows(rows), { FIGMA_API_KEY: null, LOG_LEVEL: 'debug' });
});

test('removed rows are absent from the payload (deletion)', () => {
  const rows = [newKvRow({ key: 'KEEP_ME', stored: true })];
  const out = collectKvRows(rows);
  assert.deepEqual(Object.keys(out), ['KEEP_ME']);
});

test('create-mode rows (not stored) never produce null', () => {
  const rows = [
    newKvRow({ key: 'API_URL', value: 'https://x.example' }),
    newKvRow({ key: 'EMPTY_OK' }),
  ];
  assert.deepEqual(collectKvRows(rows), { API_URL: 'https://x.example', EMPTY_OK: '' });
});

test('blank keys are skipped and keys are trimmed', () => {
  const rows = [
    newKvRow({ key: '  ', value: 'ignored' }),
    newKvRow({ key: ' PAD ', value: 'v' }),
  ];
  assert.deepEqual(collectKvRows(rows), { PAD: 'v' });
});
