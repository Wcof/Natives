// ── Bridge SDK ──
// Injected into plugin iframes via GET /natives-sdk.js
// Provides window.natives.* API

(function () {
  'use strict';

  var port = null;
  var token = null;
  var moduleId = null;

  // Auto-detect port from script src
  var scripts = document.getElementsByTagName('script');
  for (var i = 0; i < scripts.length; i++) {
    var src = scripts[i].src;
    if (src && src.indexOf('natives-sdk.js') !== -1) {
      var url = new URL(src);
      port = url.port;
      break;
    }
  }

  if (!port) {
    console.error('[Natives SDK] Could not detect port from script src');
    return;
  }

  // ── Token Handshake (two-phase) ──

  function requestToken() {
    window.parent.postMessage({ type: 'token-request', moduleId: moduleId || '' }, '*');
  }

  window.addEventListener('message', function (event) {
    var data = event.data;
    if (!data || typeof data !== 'object') return;

    if (data.type === 'token-granted') {
      token = data.token;
      moduleId = data.moduleId;
    }
  });

  // Request token immediately on load
  requestToken();

  // ── Bridge Request Helper ──

  function bridgeRequest(namespace, method, body) {
    var headers = {
      'Content-Type': 'application/json',
    };
    if (token) {
      headers['X-Session-Token'] = token;
      headers['X-Module-Id'] = moduleId || '';
    }

    return fetch('http://localhost:' + port + '/api/bridge/' + namespace + '/' + method, {
      method: 'POST',
      headers: headers,
      body: JSON.stringify(body || {}),
    }).then(function (res) {
      if (!res.ok) {
        return res.json().then(function (err) {
          throw new Error(err.error || 'Bridge request failed: ' + res.status);
        });
      }
      return res.json();
    });
  }

  // ── Build natives API ──

  window.natives = window.natives || {};

  // Data read/write (M9)
  window.natives.db = {
    get: function (key) { return bridgeRequest('db', 'get', { key: key }); },
    set: function (key, value) { return bridgeRequest('db', 'set', { key: key, value: value }); },
    delete: function (key) { return bridgeRequest('db', 'delete', { key: key }); },
    list: function (prefix) { return bridgeRequest('db', 'list', { prefix: prefix }); },
  };

  // Settings (M10)
  window.natives.settings = {
    getTheme: function () { return bridgeRequest('settings', 'getTheme', {}); },
    getLocale: function () { return bridgeRequest('settings', 'getLocale', {}); },
    onThemeChange: function (cb) {
      window.addEventListener('message', function (event) {
        if (event.data && event.data.type === 'theme-changed') {
          cb(event.data.theme);
        }
      });
    },
  };

  // Environment variables (M8, needs permission)
  window.natives.env = {
    get: function (key) { return bridgeRequest('env', 'get', { key: key }); },
  };

  // Notifications (M14)
  window.natives.notification = {
    send: function (title, body, opts) {
      return bridgeRequest('notification', 'send', { title: title, body: body, level: (opts && opts.level) || 'info' });
    },
    badge: function (count) {
      return bridgeRequest('notification', 'badge', { count: count });
    },
  };

  // IPC (M13)
  window.natives.ipc = {
    send: function (targetModuleId, payload) {
      return bridgeRequest('ipc', 'send', { target: targetModuleId, payload: payload });
    },
    on: function (channel, cb) {
      window.addEventListener('message', function (event) {
        if (event.data && event.data.type === 'ipc:message' && event.data.channel === channel) {
          cb(event.data.payload);
        }
      });
    },
    broadcast: function (payload) {
      return bridgeRequest('ipc', 'broadcast', { payload: payload });
    },
  };

  // Lifecycle (M7)
  window.natives.lifecycle = {
    ready: function () {
      window.parent.postMessage({ type: 'lifecycle:ready', moduleId: moduleId }, '*');
    },
    onUnload: function (cb) {
      window.addEventListener('beforeunload', function () {
        cb();
        window.parent.postMessage({ type: 'lifecycle:unload', moduleId: moduleId }, '*');
      });
    },
    onHeartbeat: function (cb) {
      setInterval(function () {
        cb();
        window.parent.postMessage({ type: 'lifecycle:heartbeat', moduleId: moduleId }, '*');
      }, 5000);
    },
    error: function (info) {
      window.parent.postMessage({ type: 'lifecycle:error', moduleId: moduleId, info: info }, '*');
    },
  };

  // Meta (use getter so moduleId is always current after handshake)
  window.natives.meta = {
    get moduleId() { return moduleId; },
    version: '',
    nativesVersion: '0.1.0',
  };
})();
