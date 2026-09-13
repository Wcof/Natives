// Native App Client (ADR-0027).
//
// managed_local apps run as their own Native Messaging Host process,
// registered by the Host at commit time. The runtimeHost name comes from
// Core's authoritative apps:list projection (`app.runtimeHost`, derived as
// `com.natives.app.<sha256(app_id)>`) — never from a static mapping here.
// Legacy extension_app builds keep using the shared Core Host.

import { createNativeClient } from './native-client.js';

export function appHostFor(app) {
  if (app?.runtime_host) return app.runtime_host;
  return 'com.natives.file_manager';
}

export function createAppNativeClient({ app, onDisconnect, timeoutMs = 15_000 } = {}) {
  const host = appHostFor(app);
  return createNativeClient({ host, timeoutMs, onDisconnect });
}
