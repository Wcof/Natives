import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

describe('Workshop bridge SDK handshake', () => {
  it('listens before requesting and authenticates the opaque parent by source', () => {
    const order: string[] = [];
    const sent: Array<{ data: unknown; targetOrigin: string }> = [];
    let listener: ((event: { source: unknown; origin: string; data: unknown }) => void) | undefined;
    let heartbeat: (() => void) | undefined;
    const parent = {
      postMessage(data: unknown, targetOrigin: string) {
        order.push('request');
        sent.push({ data, targetOrigin });
      },
    };
    const windowObject: Record<string, unknown> = {
      parent,
      addEventListener(type: string, handler: typeof listener) {
        if (type === 'message') {
          order.push('listen');
          listener = handler;
        }
      },
    };
    const script = readFileSync(
      new URL('../../src-tauri/src/bridge_sdk.js', import.meta.url),
      'utf8',
    )
      .replaceAll('__NATIVES_ORIGIN__', 'http://localhost:4321')
      .replaceAll('__NATIVES_PORT__', '4321');

    runInNewContext(script, {
      window: windowObject,
      fetch: () => Promise.resolve({ json: () => Promise.resolve({}) }),
      setInterval: (callback: () => void) => {
        heartbeat = callback;
        return 1;
      },
    });

    assert.deepEqual(order.slice(0, 2), ['listen', 'request']);
    assert.equal(sent[0]?.targetOrigin, '*');
    assert.ok(listener);
    const natives = windowObject.natives as {
      lifecycle: {
        ready(): boolean;
        onHeartbeat(callback?: () => void): void;
        error(info: unknown): boolean;
      };
      meta: { moduleId: string | null };
    };
    assert.equal(natives.lifecycle.ready(), false);
    assert.equal(sent.length, 1, 'lifecycle signals must wait for the handshake');
    listener({
      source: {},
      origin: 'http://localhost:4321',
      data: { type: 'token-granted', token: 'spoofed', moduleId: 'evil' },
    });
    assert.equal(natives.meta.moduleId, null, 'a non-parent source cannot grant a token');
    listener({
      source: parent,
      origin: 'null',
      data: { type: 'token-granted', token: 'session-token', moduleId: 'mod-1' },
    });

    natives.lifecycle.ready();
    assert.deepEqual(JSON.parse(JSON.stringify(sent[1])), {
      data: {
        type: 'lifecycle:ready',
        moduleId: 'mod-1',
        token: 'session-token',
      },
      targetOrigin: '*',
    });

    natives.lifecycle.onHeartbeat();
    assert.ok(heartbeat);
    heartbeat();
    natives.lifecycle.error({ message: 'boom' });
    for (const entry of sent.slice(2)) {
      const data = entry.data as { moduleId?: unknown; token?: unknown };
      assert.equal(entry.targetOrigin, '*');
      assert.equal(data.moduleId, 'mod-1');
      assert.equal(data.token, 'session-token');
    }
  });
});
