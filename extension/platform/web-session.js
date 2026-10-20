// Shared Web session adapter. The extension never loads pairing/session code;
// Web loads session.js dynamically. session.js and web-entry.js are web-only
// assets embedded in Core and excluded from the Chrome extension package.
import { isExtension } from './env.js';

let session = null;

async function sessionModule() {
  if (typeof __NATIVES_EXTENSION_BUILD__ !== 'undefined' ? __NATIVES_EXTENSION_BUILD__ : isExtension) return null;
  if (!session) session = await import('./session.js');
  return session;
}

export async function ensureAuthenticated() {
  if (typeof __NATIVES_EXTENSION_BUILD__ !== 'undefined' ? __NATIVES_EXTENSION_BUILD__ : isExtension) return true;
  const module = await import('../web-entry.js');
  return module.ensureAuthenticated();
}

export async function initWebSession() {
  const module = await sessionModule();
  return module ? module.initWebSession() : { authenticated: true };
}

export function getCsrfToken() {
  if (typeof __NATIVES_EXTENSION_BUILD__ !== 'undefined' ? __NATIVES_EXTENSION_BUILD__ : isExtension) return null;
  return session?.getCsrfToken() ?? null;
}

export function invalidateWebSession() {
  if (typeof __NATIVES_EXTENSION_BUILD__ !== 'undefined' ? __NATIVES_EXTENSION_BUILD__ : isExtension) return;
  session?.invalidateWebSession();
}
