// Natives Bridge SDK — injected into sandboxed iframes
// This file is served at /natives-sdk.js by the local HTTP server.
(function() {
  'use strict';

  var PORT = '__NATIVES_PORT__';     // replaced at runtime
  var token = null;
  var moduleId = null;

  // Two-phase token handshake. The listener MUST exist before the request so
  // a fast parent response cannot be lost. Opaque sandbox frames can only use
  // `*` as targetOrigin; sender identity is the parent Window reference.
  window.addEventListener('message', function(event) {
    if (event.source !== window.parent) return;
    var data = event.data;
    if (data && data.type === 'token-granted' &&
        typeof data.token === 'string' && typeof data.moduleId === 'string') {
      token = data.token;
      moduleId = data.moduleId;
    }
  });
  window.parent.postMessage({ type: 'token-request' }, '*');

  function postLifecycle(type, extra) {
    if (!token || !moduleId) return false;
    var message = { type: type, moduleId: moduleId, token: token };
    if (extra && Object.prototype.hasOwnProperty.call(extra, 'error')) {
      message.error = extra.error;
    }
    window.parent.postMessage(message, '*');
    return true;
  }

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
        return postLifecycle('lifecycle:ready');
      },
      onUnload: function(cb) {
        window.addEventListener('beforeunload', cb);
      },
      onHeartbeat: function(cb) {
        setInterval(function() {
          postLifecycle('lifecycle:heartbeat');
          if (cb) cb();
        }, 5000);
      },
      error: function(info) {
        return postLifecycle('lifecycle:error', { error: info });
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
