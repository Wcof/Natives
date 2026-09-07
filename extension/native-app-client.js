// Native App Client (ADR-0025 D49/D50).
//
// Binds the shared Native Client to ONE app's host. The host name comes
// from the authoritative App's `runtime_spec.host` (never user-supplied);
// the fallback `com.natives.app.<appId>` is only a structural default for
// the demo. The app page owns this port for its lifetime: page closed →
// port closed → stdin EOF → host exits (≤2 s, D49). No background port,
// no polling, no keepalive.

import { createNativeClient } from './native-client.js';

export function appHostFor(app) {
  try {
    const spec = JSON.parse(app?.runtime_spec_json || '{}');
    if (spec && typeof spec.host === 'string' && spec.host) return spec.host;
  } catch {
    // malformed runtime_spec_json → structural fallback below
  }
  return app?.app_id ? `com.natives.app.${app.app_id}` : null;
}

export function createAppNativeClient({ app, onDisconnect, timeoutMs = 15_000 } = {}) {
  const host = appHostFor(app);
  if (!host) throw new Error('app host 未指定');
  // App hosts are already registered by the Core installer with the real
  // caller origin (Phase A5), so no handshake is needed on this port.
  return createNativeClient({ host, timeoutMs, onDisconnect });
}
