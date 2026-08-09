import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import {
  isExpectedMessageSource,
  isHeartbeatFromModuleSource,
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

describe('isHeartbeatFromModuleSource', () => {
  const iframeWin = { contentWindow: true };
  const heartbeat = {
    source: iframeWin,
    data: { type: 'lifecycle:heartbeat', moduleId: 'mod-1' },
  };

  it('accepts a heartbeat from the expected iframe for the expected module', () => {
    assert.equal(
      isHeartbeatFromModuleSource(heartbeat, iframeWin, 'mod-1'),
      true,
    );
  });

  it('rejects a heartbeat whose source is not the expected iframe, even with valid origin-like data', () => {
    // Same data shape, same type/moduleId, but a different sender window.
    const spoofed = { source: {}, data: { type: 'lifecycle:heartbeat', moduleId: 'mod-1' } };
    assert.equal(isHeartbeatFromModuleSource(spoofed, iframeWin, 'mod-1'), false);
  });

  it('rejects a heartbeat with null source', () => {
    assert.equal(
      isHeartbeatFromModuleSource({ source: null, data: heartbeat.data }, iframeWin, 'mod-1'),
      false,
    );
  });

  it('rejects a matching source with a forged moduleId', () => {
    assert.equal(
      isHeartbeatFromModuleSource(heartbeat, iframeWin, 'mod-2'),
      false,
    );
  });

  it('rejects a matching source with a non-heartbeat type', () => {
    assert.equal(
      isHeartbeatFromModuleSource(
        { source: iframeWin, data: { type: 'lifecycle:ready', moduleId: 'mod-1' } },
        iframeWin,
        'mod-1',
      ),
      false,
    );
  });

  it('rejects a matching source with non-object or missing data', () => {
    assert.equal(isHeartbeatFromModuleSource({ source: iframeWin }, iframeWin, 'mod-1'), false);
    assert.equal(
      isHeartbeatFromModuleSource({ source: iframeWin, data: 'nope' }, iframeWin, 'mod-1'),
      false,
    );
  });
});
