// Built-in module navigation projection.
//
// The signed product manifest defines available modules; natives.db owns
// user display preferences. This module is the UI cache for global sidebar
// surfaces (files.html, space.html): it is written only after a verified Host
// projection and read by
// surfaces that must stay 0-Native-Port (space.html / newtab never start
// a host to render "应用").
//
// Stored under chrome.storage.local as `natives.apps.navigation.v1`:
//   { revision: <host revision>, items: [ { appId, label, labels, icon, route, order } ] }
//
// Projection items never carry secrets, package data, or business data —
// only navigation labels and routes plus revision.
//
// Typed navigation: a module item is
//   { kind: 'app', target: '<appId>' }
// and must never masquerade as a filesystem path.

import { storage as platformStorage } from './platform/storage.js';

export const APP_PROJECTION_STORAGE_KEY = 'natives.apps.navigation.v1';
export const NAVIGATION_KIND_FILESYSTEM = 'filesystem';
export const NAVIGATION_KIND_APP = 'app';

const DEFAULT_PROJECTION = Object.freeze({ revision: 0, items: [] });

// With zero visible modules the "应用" section is absent entirely.
function sanitizeProjection(raw) {
  if (!raw || typeof raw !== 'object') return { ...DEFAULT_PROJECTION, items: [] };
  const revision = Number.isFinite(Number(raw.revision)) ? Number(raw.revision) : 0;
  const items = Array.isArray(raw.items)
    ? raw.items
        .filter((item) => item && typeof item === 'object' && typeof item.appId === 'string' && item.appId.length > 0)
        .map((item) => ({
          appId: item.appId,
          label: typeof item.label === 'string' && item.label ? item.label : item.appId,
          ...(item.labels && typeof item.labels === 'object' && !Array.isArray(item.labels)
            ? { labels: Object.fromEntries(['zh_CN', 'en'].filter((lang) => typeof item.labels[lang] === 'string').map((lang) => [lang, item.labels[lang]])) }
            : {}),
          icon: typeof item.icon === 'string' && item.icon ? item.icon : 'grid',
          route: typeof item.route === 'string' && item.route ? item.route : `app.html?app=${encodeURIComponent(item.appId)}`,
          order: Number.isFinite(Number(item.order)) ? Number(item.order) : 0,
          ...(item.settingsView === 'settings' ? {settingsView:'settings'} : {}),
        }))
        .sort((a, b) => a.order - b.order || a.appId.localeCompare(b.appId))
    : [];
  return { revision, items };
}

export function isStaleProjection(projection, hostRevision) {
  return sanitizeProjection(projection).revision !== Number(hostRevision);
}

// Map the authoritative apps:list result into projection items. Only enabled
// built-in modules that opt into the sidebar are projected.
export function projectionFromApps(apps, revision, modules = []) {
  const names = new Map(modules.map((module) => [module.appId, module.name]));
  const items = Array.isArray(apps)
    ? apps
        .filter((app) => app && typeof app.app_id === 'string' && app.enabled !== false && app.show_in_sidebar !== false)
        .map((app) => ({
          appId: app.app_id,
          label: typeof app.name === 'string' && app.name ? app.name : app.app_id,
          ...(names.get(app.app_id) ? { labels: names.get(app.app_id) } : {}),
          icon: 'grid',
          route: `app.html?app=${encodeURIComponent(app.app_id)}`,
          order: Number.isFinite(Number(app.sidebar_order)) ? Number(app.sidebar_order) : 0,
          ...(Array.isArray(app.views) && app.views.includes('settings') ? {settingsView:'settings'} : {}),
        }))
    : [];
  return { revision: Number(revision) || 0, items };
}

// Shared DOM rendering for the "应用" sidebar section. Both surfaces
// (files.html via FilesSidebar, space.html directly) call this so the
// section markup is identical by construction (Gate A3).
// `nav` must be an existing <nav> element; the section is appended and
// hidden when the projection is empty.
export function renderAppMenuInto(nav, projection, options = {}) {
  if (!nav) return;
  const { t = (key, fallback) => fallback || key, iconBase = '', language } = options;
  const section = appSectionFromProjection(projection, { t, language });
  nav.replaceChildren();
  if (section.length === 0) {
    // No visible module means no section.
    nav.hidden = true;
    return;
  }
  nav.hidden = false;
  const secDiv = document.createElement('div');
  secDiv.className = 'app-menu-section';
  const titleSpan = document.createElement('span');
  titleSpan.setAttribute('data-i18n', 'navApps');
  titleSpan.textContent = section[0].title;
  secDiv.append(titleSpan);
  for (const item of section[0].items) {
    const link = document.createElement('a');
    link.className = 'app-menu-item';
    link.setAttribute('href', item.path);
    link.dataset.kind = NAVIGATION_KIND_APP;
    link.dataset.appId = item.target;
    link.setAttribute('aria-label', item.label);
    link.title = item.label;
    link.innerHTML = `<svg class="icon"><use href="${iconBase}#i-${item.icon || 'grid'}" /></svg><span>${escapeHtml(item.label)}</span>`;
    secDiv.append(link);
  }
  nav.append(secDiv);
}

function escapeHtml(value) {
  return String(value)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

// Load the projection, render it into `nav`, and keep it in sync with
// chrome.storage.local changes (product refresh or preference changes).
// Returns an unsubscribe function.
// `options.afterRender` runs after every render (initial + each storage
// update) so callers can re-apply view-dependent state (e.g. the active
// app highlight) in the same order as rendering — a separate
// onAppNavigationChange listener registers earlier and would run BEFORE
// the re-render, getting its work wiped.
export async function mountAppMenu(nav, options = {}) {
  const { t = (key, fallback) => fallback || key, iconBase = '', storage = platformStorage, afterRender, language } = options;
  let current = { ...DEFAULT_PROJECTION };
  const renderAndNotify = (projection, language) => {
    current = projection;
    renderAppMenuInto(nav, projection, { t, iconBase, language });
    if (afterRender) {
      try { afterRender(); } catch { /* 视图层回填失败不阻塞导航渲染 */ }
    }
  };
  current = await loadAppNavigation(storage);
  renderAndNotify(current, language);
  const stopNavigation = onAppNavigationChange(renderAndNotify, storage);
  // Shared language changes must relabel the already-rendered module menu;
  // the projection only stores candidate labels, not the active language.
  const onLanguage = (changes, area) => {
    if (area !== 'local') return;
    const next = changes?.['natives-language']?.newValue;
    if (next === 'en' || next === 'zh_CN') renderAndNotify(current, next);
  };
  storage?.onChanged?.addListener?.(onLanguage);
  return () => {
    stopNavigation?.();
    storage?.onChanged?.removeListener?.(onLanguage);
  };
}

async function storageLocalGet(storage, key) {
  try {
    const result = await storage.local.get({ [key]: null });
    return sanitizeProjection(result?.[key]);
  } catch {
    return { ...DEFAULT_PROJECTION, items: [] };
  }
}

export async function loadAppNavigation(storage = platformStorage) {
  if (!storage?.local) return { ...DEFAULT_PROJECTION, items: [] };
  return storageLocalGet(storage, APP_PROJECTION_STORAGE_KEY);
}

export async function saveAppNavigation(projection, storage = platformStorage) {
  const clean = sanitizeProjection(projection);
  if (!storage?.local) return clean;
  try {
    await storage.local.set({ [APP_PROJECTION_STORAGE_KEY]: clean });
  } catch {
    // UI cache only: a failed write degrades to the next host-driven
    // refresh and must never surface as an error to the caller.
  }
  return clean;
}

export async function clearAppNavigation(storage = platformStorage) {
  if (!storage?.local) return;
  try {
    await storage.local.remove(APP_PROJECTION_STORAGE_KEY);
  } catch {
    // best-effort, UI cache only
  }
}

// Build the sidebar "应用" section from a projection.
// Returns [] when there is nothing to show.
export function appSectionFromProjection(projection, options = {}) {
  const { t = (key, fallback) => fallback || key } = options;
  const language = options.language || globalThis.document?.documentElement?.lang || globalThis.navigator?.language || 'zh_CN';
  const clean = sanitizeProjection(projection);
  if (clean.items.length === 0) return [];
  return [
    {
      id: 'apps',
      i18n: 'navApps',
      title: t('navApps', '应用'),
      items: clean.items.map((item) => ({
        id: `app-${item.appId}`,
        kind: NAVIGATION_KIND_APP,
        target: item.appId,
        path: item.route,
        icon: item.icon,
        label: item.labels?.[language.startsWith('en') ? 'en' : 'zh_CN'] || item.label,
        badge: null,
      })),
    },
  ];
}

// Subscribe to projection changes across pages. chrome.storage.onChanged
// fires in every page when the App Center (or the files page while its
// host connection is live) rewrites the cache.
export function onAppNavigationChange(listener, storage = platformStorage) {
  if (!storage?.onChanged?.addListener) return () => {};
  const handler = (changes, area) => {
    if (area !== 'local' || !changes[APP_PROJECTION_STORAGE_KEY]) return;
    listener(sanitizeProjection(changes[APP_PROJECTION_STORAGE_KEY].newValue));
  };
  storage.onChanged.addListener(handler);
  return () => storage.onChanged.removeListener(handler);
}
