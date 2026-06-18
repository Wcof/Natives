// Natives Bridge SDK — injected into sandboxed iframes
// This file is served at /natives-sdk.js by the local HTTP server.
(function() {
  'use strict';

  var ORIGIN = '__NATIVES_ORIGIN__'; // replaced at runtime
  var PORT = '__NATIVES_PORT__';     // replaced at runtime
  var token = null;
  var moduleId = null;

  // Two-phase token handshake
  // Phase 1: request token from parent
  window.parent.postMessage({ type: 'token-request' }, ORIGIN);

  // Phase 2: receive token grant from parent
  window.addEventListener('message', function(event) {
    if (event.origin !== ORIGIN) return;
    var data = event.data;
    if (data && data.type === 'token-granted') {
      token = data.token;
      moduleId = data.moduleId;
    }
  });

  // Bridge request helper
  function bridgeRequest(namespace, method, body) {
    return fetch('http://localhost:' + PORT + '/api/bridge/' + namespace + '/' + method, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'X-Session-Token': token || '',
        'X-Module-Id': moduleId || ''
      },
      body: JSON.stringify(body || {})
    }).then(function(r) { return r.json(); });
  }

  // Public API
  window.natives = {
    // Module metadata
    meta: {
      moduleId: moduleId,
      version: '0.1.0',
      nativesVersion: '0.1.0'
    },

    // Database access
    db: {
      get: function(key) { return bridgeRequest('db', 'get', { key: key }); },
      set: function(key, value) { return bridgeRequest('db', 'set', { key: key, value: value }); },
      delete: function(key) { return bridgeRequest('db', 'delete', { key: key }); },
      list: function(prefix) { return bridgeRequest('db', 'list', { prefix: prefix }); }
    },

    // Settings (read-only)
    settings: {
      getTheme: function() { return bridgeRequest('settings', 'getTheme'); },
      getLocale: function() { return bridgeRequest('settings', 'getLocale'); }
    },

    // Lifecycle management
    lifecycle: {
      ready: function() {
        window.parent.postMessage({ type: 'lifecycle:ready', moduleId: moduleId }, ORIGIN);
      },
      onUnload: function(cb) {
        window.addEventListener('beforeunload', cb);
      },
      onHeartbeat: function(cb) {
        setInterval(function() {
          window.parent.postMessage({ type: 'lifecycle:heartbeat', moduleId: moduleId }, ORIGIN);
          if (cb) cb();
        }, 5000);
      },
      error: function(info) {
        window.parent.postMessage({ type: 'lifecycle:error', moduleId: moduleId, error: info }, ORIGIN);
      }
    },

    // Environment variables
    env: {
      get: function(key) { return bridgeRequest('env', 'get', { key: key }); }
    },

    // Notifications
    notification: {
      send: function(title, body, level) {
        return bridgeRequest('notification', 'send', { title: title, body: body, level: level || 'info' });
      },
      badge: function(count) {
        return bridgeRequest('notification', 'badge', { count: count });
      }
    },

    // IPC between modules
    ipc: {
      send: function(target, payload) {
        return bridgeRequest('ipc', 'send', { target: target, payload: payload });
      },
      broadcast: function(payload) {
        return bridgeRequest('ipc', 'broadcast', { payload: payload });
      }
    }
  };

  // Update meta.moduleId after token is received
  Object.defineProperty(window.natives.meta, 'moduleId', {
    get: function() { return moduleId; }
  });
})();
