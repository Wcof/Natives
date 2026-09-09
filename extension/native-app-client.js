// Native App Client (ADR-0026 D1).
//
// Apps share the Core Host (com.natives.file_manager) rather than launching
// separate processes. createAppNativeClient connects to the shared host.

import { createNativeClient } from './native-client.js';

export function appHostFor(app) {
  return 'com.natives.file_manager';
}

export function createAppNativeClient({ app, onDisconnect, timeoutMs = 15_000 } = {}) {
  const host = 'com.natives.file_manager';
  return createNativeClient({ host, timeoutMs, onDisconnect });
}
