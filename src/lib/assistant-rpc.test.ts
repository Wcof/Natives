import assert from 'node:assert/strict';
import test from 'node:test';

import { unwrapAssistantRpc } from './assistant-rpc';

test('unwrapAssistantRpc returns the service payload', () => {
  assert.deepEqual(
    unwrapAssistantRpc({ success: true, data: { conversations: [{ id: 'c1' }] } }),
    { conversations: [{ id: 'c1' }] },
  );
});

test('unwrapAssistantRpc throws the service error', () => {
  assert.throws(
    () => unwrapAssistantRpc({ success: false, error: { code: 'DB_ERROR', message: 'database unavailable' } }),
    /database unavailable/,
  );
});
