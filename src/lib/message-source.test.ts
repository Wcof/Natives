import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import {
  isAuthenticatedLifecycleMessage,
  isExpectedMessageSource,
} from './message-source';

describe('isExpectedMessageSource', () => {
  const expected = { contentWindow: true };

  it('accepts a message whose source equals the expected window reference', () => {
    assert.equal(isExpectedMessageSource({ source: expected }, expected), true);
  });

  it('rejects a message from a different window reference', () => {
    assert.equal(isExpectedMessageSource({ source: {} }, expected), false);
  });

  it('rejects a message with null source (synthetic/own-window messages)', () => {
    assert.equal(isExpectedMessageSource({ source: null }, expected), false);
    assert.equal(isExpectedMessageSource({ source: undefined }, expected), false);
  });
});

describe('isAuthenticatedLifecycleMessage', () => {
  const iframeWin = { contentWindow: true };
  const message = {
    source: iframeWin,
    data: {
      type: 'lifecycle:ready',
      moduleId: 'mod-1',
      token: 'current-token',
    },
  };

  it('requires the exact source, module id, current token and lifecycle type', () => {
    assert.equal(
      isAuthenticatedLifecycleMessage(message, iframeWin, 'mod-1', 'current-token'),
      true,
    );
    assert.equal(
      isAuthenticatedLifecycleMessage({ ...message, source: {} }, iframeWin, 'mod-1', 'current-token'),
      false,
    );
    assert.equal(
      isAuthenticatedLifecycleMessage(message, iframeWin, 'mod-2', 'current-token'),
      false,
    );
    assert.equal(
      isAuthenticatedLifecycleMessage(message, iframeWin, 'mod-1', 'old-token'),
      false,
    );
    assert.equal(
      isAuthenticatedLifecycleMessage(
        { source: iframeWin, data: { ...message.data, type: 'token-request' } },
        iframeWin,
        'mod-1',
        'current-token',
      ),
      false,
    );
  });
});
