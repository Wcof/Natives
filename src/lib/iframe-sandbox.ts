// ── Bridge SDK Builder (P0-3: no Node.js crypto, renderer-safe) ──
// Token management moved to token-manager.ts (main process)
// IframeSandboxManager moved to iframe-sandbox-manager.ts (renderer)

export function buildBridgeSdkScript(port: number, targetOrigin: string): string {
  return `
(function() {
  'use strict';
  var port = ${port};
  var origin = ${JSON.stringify(targetOrigin)};
  var token = null;
  var moduleId = null;
  var pending = {};
  var msgId = 0;

  function requestToken() {
    window.parent.postMessage({ type: 'token-request' }, origin);
  }

  window.addEventListener('message', function(event) {
    if (event.origin !== origin) return;
    var data = event.data;
    if (!data || typeof data !== 'object') return;
    if (data.type === 'token-granted') {
      token = data.token;
      moduleId = data.moduleId;
    }
  });

  requestToken();

  window.natives = window.natives || {};

  window.natives.db = {
    get: function(key) { return bridgeRequest('db', 'get', { key: key }); },
    set: function(key, value) { return bridgeRequest('db', 'set', { key: key, value: value }); },
    delete: function(key) { return bridgeRequest('db', 'delete', { key: key }); },
    list: function(prefix) { return bridgeRequest('db', 'list', { prefix: prefix }); }
  };

  window.natives.settings = {
    getTheme: function() { return bridgeRequest('settings', 'getTheme', {}); },
    getLocale: function() { return bridgeRequest('settings', 'getLocale', {}); },
  };

  window.natives.meta = {
    moduleId: '',
    version: '',
    nativesVersion: '0.1.0'
  };

  window.natives.lifecycle = {
    ready: function() {
      window.parent.postMessage({ type: 'lifecycle:ready' }, origin);
    },
    onUnload: function(cb) {
      window._nativesOnUnload = cb;
      window.addEventListener('beforeunload', cb);
    },
    onHeartbeat: function(cb) {
      setInterval(function() {
        cb();
        window.parent.postMessage({ type: 'lifecycle:heartbeat' }, origin);
      }, 5000);
    },
    error: function(info) {
      window.parent.postMessage({ type: 'lifecycle:error', info: info }, origin);
    }
  };

  window.natives.env = {
    get: function(key) { return bridgeRequest('env', 'get', { key: key }); },
  };

  window.natives.notification = {
    send: function(title, body, level) { return bridgeRequest('notification', 'send', { title: title, body: body, level: level }); },
    badge: function(count) { return bridgeRequest('notification', 'badge', { count: count }); },
  };

  window.natives.ipc = {
    send: function(target, payload) { return bridgeRequest('ipc', 'send', { target: target, payload: payload }); },
    broadcast: function(payload) { return bridgeRequest('ipc', 'broadcast', { payload: payload }); },
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
