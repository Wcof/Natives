// Native client for a product-declared built-in module.
//
// Each module runs through its registered Native Messaging Host. The Host
// name comes from Core's verified apps:list projection, never a page fallback.

import { createNativeClient } from './native-client.js';

export function appHostFor(app) {
  if (typeof app?.runtime_host !== 'string' || !app.runtime_host) {
    throw new Error('built-in module runtime host is missing');
  }
  return app.runtime_host;
}

export function createAppNativeClient({ app, onDisconnect, timeoutMs = 15_000 } = {}) {
  const host = appHostFor(app);
  return createNativeClient({ host, timeoutMs, onDisconnect });
}
