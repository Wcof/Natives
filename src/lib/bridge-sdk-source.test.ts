import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import vm from 'node:vm';

// bridge_sdk.js is a Host-injected resource: it is embedded at compile time
// via include_str!() in src-tauri/src/http_server.rs and served at
// /natives-sdk.js. This test executes the real file in a fake window realm
// to prove the token handshake rejects grants whose MessageEvent.source is
// not the actual parent window (SEC-002 / R-S3).

const ORIGIN = 'http://localhost:43210';
const SDK_PATH = path.join(process.cwd(), 'src-tauri', 'src', 'bridge_sdk.js');
const SDK_SOURCE = readFileSync(SDK_PATH, 'utf8')
  .replace('__NATIVES_ORIGIN__', ORIGIN)
  .replace('__NATIVES_PORT__', '43210');

interface FetchCall {
  url: string;
  options: { headers: Record<string, string> };
}

interface FakeNatives {
  db: { get: (key: string) => Promise<unknown> };
  meta: { moduleId: string | null };
}

interface FakeWindow {
  parent: { postMessage: () => void };
  addEventListener: (type: string, cb: (event: unknown) => void) => void;
  postMessage: () => void;
  natives: FakeNatives;
}

interface SdkHarness {
  win: FakeWindow;
  parent: { postMessage: () => void };
  listeners: Array<(event: unknown) => void>;
  fetchCalls: FetchCall[];
  dispatch(event: { source: unknown; origin: string; data: unknown }): void;
}

function loadSdk(): SdkHarness {
  const listeners: Array<(event: unknown) => void> = [];
  const parent: { postMessage: () => void } = { postMessage: () => {} };
  const win: FakeWindow = {
    parent,
    addEventListener: (type: string, cb: (event: unknown) => void) => {
      if (type === 'message') listeners.push(cb);
    },
    postMessage: () => {},
    natives: { db: { get: async () => '' }, meta: { moduleId: null } },
  };
  const fetchCalls: FetchCall[] = [];
  const fetchMock = async (url: string, options: { headers: Record<string, string> }) => {
    fetchCalls.push({ url, options });
    return { json: async () => ({ ok: true }) };
  };
  const context = vm.createContext({
    window: win,
    fetch: fetchMock,
    setInterval: () => 0,
    clearInterval: () => {},
    setTimeout: () => 0,
    clearTimeout: () => {},
    console,
  });
  vm.runInContext(SDK_SOURCE, context);
  return {
    win,
    parent,
    listeners,
    fetchCalls,
    dispatch(event) {
      for (const cb of listeners) cb(event);
    },
  };
}

async function tokenHeader(sdk: SdkHarness, key: string): Promise<string | undefined> {
  await sdk.win.natives.db.get(key);
  const last = sdk.fetchCalls[sdk.fetchCalls.length - 1];
  assert.ok(last, 'expected at least one bridge request');
  return last.options.headers['X-Session-Token'];
}

describe('bridge_sdk token handshake source validation (SEC-002)', () => {
  it('grants the token only when event.source === window.parent', async () => {
    const sdk = loadSdk();
    sdk.dispatch({
      source: sdk.parent,
      origin: ORIGIN,
      data: { type: 'token-granted', token: 'tok-1', moduleId: 'mod-1' },
    });
    assert.equal(sdk.win.natives.meta.moduleId, 'mod-1');
    assert.equal(await tokenHeader(sdk, 'k'), 'tok-1');
  });

  it('rejects a token grant from a different source even with the right origin', async () => {
    const sdk = loadSdk();
    sdk.dispatch({
      source: {}, // not window.parent
      origin: ORIGIN,
      data: { type: 'token-granted', token: 'evil', moduleId: 'mod-1' },
    });
    assert.equal(sdk.win.natives.meta.moduleId, null);
    assert.equal(await tokenHeader(sdk, 'k'), '');
  });

  it('rejects a token grant from the parent window with a mismatched origin', async () => {
    const sdk = loadSdk();
    sdk.dispatch({
      source: sdk.parent,
      origin: 'http://evil.example',
      data: { type: 'token-granted', token: 'evil', moduleId: 'mod-1' },
    });
    assert.equal(sdk.win.natives.meta.moduleId, null);
    assert.equal(await tokenHeader(sdk, 'k'), '');
  });

  it('rejects a non-token message from the parent window', async () => {
    const sdk = loadSdk();
    sdk.dispatch({
      source: sdk.parent,
      origin: ORIGIN,
      data: { type: 'lifecycle:heartbeat', moduleId: 'mod-1' },
    });
    assert.equal(sdk.win.natives.meta.moduleId, null);
  });
});
