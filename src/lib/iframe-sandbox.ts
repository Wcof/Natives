import * as crypto from 'crypto';

// ── Token Management ──

const masterSecret = crypto.randomBytes(32).toString('hex');
const tokenMap = new Map<string, { token: string; moduleId: string }>();

export function generateSessionToken(moduleId: string): string {
  const nonce = crypto.randomBytes(8).toString('hex');
  const timestamp = Date.now().toString();
  const data = `${moduleId}:${timestamp}:${nonce}`;
  const token = crypto.createHmac('sha256', masterSecret).update(data).digest('hex');

  // Store with moduleId reference
  tokenMap.set(token, { token, moduleId });

  // Return token + timestamp so the other side can reconstruct
  return `${token}:${timestamp}`;
}

export function validateSessionToken(token: string, moduleId: string): boolean {
  const [hash] = token.split(':');
  if (!hash) return false;
  const entry = tokenMap.get(hash);
  if (!entry) return false;
  return entry.moduleId === moduleId;
}

export function invalidateToken(token: string): void {
  const [hash] = token.split(':');
  if (hash) tokenMap.delete(hash);
}

export function invalidateModuleTokens(moduleId: string): void {
  for (const [key, value] of tokenMap) {
    if (value.moduleId === moduleId) {
      tokenMap.delete(key);
    }
  }
}

export function invalidateAllTokens(): void {
  tokenMap.clear();
}

export interface IframeRecord {
  moduleId: string;
  contentWindow: Window | null;
  token: string;
  sessionStart: number;
  lastHeartbeat: number;
}

// Store for all active iframes with their contentWindow references
// This runs in the browser/Next.js context, not main process
export class IframeSandboxManager {
  private iframes = new Map<string, IframeRecord>();
  private sourceMap = new Map<string, Window | null>(); // moduleId → contentWindow

  register(moduleId: string, contentWindow: Window | null): { token: string } {
    // Invalidate old tokens for same moduleId first
    invalidateModuleTokens(moduleId);

    const token = generateSessionToken(moduleId);
    const record: IframeRecord = {
      moduleId,
      contentWindow,
      token,
      sessionStart: Date.now(),
      lastHeartbeat: Date.now(),
    };

    this.iframes.set(moduleId, record);
    this.sourceMap.set(moduleId, contentWindow);

    return { token };
  }

  unregister(moduleId: string): void {
    const record = this.iframes.get(moduleId);
    if (record) {
      invalidateToken(record.token);
      this.iframes.delete(moduleId);
      this.sourceMap.delete(moduleId);
    }
  }

  verifyMessageSource(moduleId: string, source: Window | null): boolean {
    const expected = this.sourceMap.get(moduleId);
    return expected === source;
  }

  getToken(moduleId: string): string | undefined {
    return this.iframes.get(moduleId)?.token;
  }

  updateHeartbeat(moduleId: string): void {
    const record = this.iframes.get(moduleId);
    if (record) {
      record.lastHeartbeat = Date.now();
    }
  }

  getTimeoutCount(moduleId: string, timeoutMs: number): number {
    const record = this.iframes.get(moduleId);
    if (!record) return 0;
    const elapsed = Date.now() - record.lastHeartbeat;
    return Math.floor(elapsed / timeoutMs);
  }
}

// ── Bridge SDK Builder ──

export function buildBridgeSdkScript(port: number): string {
  return `
(function() {
  'use strict';
  var port = ${port};
  var token = null;
  var moduleId = null;
  var pending = {};
  var msgId = 0;

  // Request token from host
  function requestToken() {
    window.parent.postMessage({ type: 'token-request' }, '*');
  }

  // Listen for token response
  window.addEventListener('message', function(event) {
    var data = event.data;
    if (!data || typeof data !== 'object') return;

    if (data.type === 'token-granted') {
      token = data.token;
      moduleId = data.moduleId;
    }
  });

  // Request token on load
  requestToken();

  // Bridge API
  window.natives = window.natives || {};

  window.natives.db = {
    get: function(key) { return bridgeRequest('db', 'get', { key: key }); },
    set: function(key, value) { return bridgeRequest('db', 'set', { key: key, value: value }); },
    delete: function(key) { return bridgeRequest('db', 'delete', { key: key }); },
    list: function(prefix) { return bridgeRequest('db', 'list', { prefix: prefix }); }
  };

  window.natives.meta = {
    moduleId: '',
    version: '',
    nativesVersion: '0.1.0'
  };

  window.natives.lifecycle = {
    ready: function() {
      window.parent.postMessage({ type: 'lifecycle:ready' }, '*');
    },
    onUnload: function(cb) {
      window._nativesOnUnload = cb;
      window.addEventListener('beforeunload', cb);
    },
    onHeartbeat: function(cb) {
      setInterval(function() {
        cb();
        window.parent.postMessage({ type: 'lifecycle:heartbeat' }, '*');
      }, 5000);
    },
    error: function(info) {
      window.parent.postMessage({ type: 'lifecycle:error', info: info }, '*');
    }
  };

  function bridgeRequest(namespace, method, body) {
    return fetch('http://localhost:' + port + '/api/bridge/' + namespace + '/' + method, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'X-Session-Token': token || '',
        'X-Module-Id': moduleId || ''
      },
      body: JSON.stringify(body)
    }).then(function(res) {
      if (!res.ok) throw new Error('Bridge request failed: ' + res.status);
      return res.json();
    });
  }
})();
`;
}
