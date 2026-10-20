// Native client for a product-declared built-in module.
//
// Core verifies the declared module host and owns its shared runtime worker.

import { createNativeClient } from './native-client.js';

const CORE_HOST = 'com.natives.file_manager';

export function appHostFor(runtimeHost) {
  if (typeof runtimeHost !== 'string' || !runtimeHost) {
    throw new Error('built-in module runtime host is missing');
  }
  return runtimeHost;
}

export function createAppNativeClient({ runtimeHost, onDisconnect, timeoutMs = 15_000 } = {}) {
  appHostFor(runtimeHost);
  // Core owns the single worker per module across extension and local Web Surfaces.
  return createNativeClient({ host: CORE_HOST, timeoutMs, onDisconnect });
}
