// App Navigation Projection (ADR-0025 D37/D38).
//
// The App Store in natives.db is the single source of truth. This module
// is the UI cache for global sidebar surfaces (files.html, space.html):
// it is written only after a successful App Store mutation and read by
// surfaces that must stay 0-Native-Port (space.html / newtab never start
// a host to render "应用").
//
// Stored under chrome.storage.local as `natives.apps.navigation.v1`:
//   { revision: <host revision>, items: [ { appId, label, icon, route, order } ] }
//
// Projection items never carry secrets, package data, or business data —
// only the four navigation fields plus revision (ADR-0025 D35).
//
// Typed navigation (ADR-0025 D40): an app item is
//   { kind: 'app', target: '<appId>' }
// and must never masquerade as a filesystem path.

export const APP_PROJECTION_STORAGE_KEY = 'natives.apps.navigation.v1';
export const NAVIGATION_KIND_FILESYSTEM = 'filesystem';
export const NAVIGATION_KIND_APP = 'app';

const DEFAULT_PROJECTION = Object.freeze({ revision: 0, items: [] });

// ADR-0025 D38: with zero apps the "应用" section is absent entirely.
function sanitizeProjection(raw) {
  if (!raw || typeof raw !== 'object') return { ...DEFAULT_PROJECTION, items: [] };
  const revision = Number.isFinite(Number(raw.revision)) ? Number(raw.revision) : 0;
  const items = Array.isArray(raw.items)
    ? raw.items
        .filter((item) => item && typeof item === 'object' && typeof item.appId === 'string' && item.appId.length > 0)
        .map((item) => ({
          appId: item.appId,
          label: typeof item.label === 'string' && item.label ? item.label : item.appId,
          icon: typeof item.icon === 'string' && item.icon ? item.icon : 'grid',
          route: typeof item.route === 'string' && item.route ? item.route : `app.html?app=${encodeURIComponent(item.appId)}`,
          order: Number.isFinite(Number(item.order)) ? Number(item.order) : 0,
        }))
        .sort((a, b) => a.order - b.order || a.appId.localeCompare(b.appId))
    : [];
  return { revision, items };
}

export function isStaleProjection(projection, hostRevision) {
  return sanitizeProjection(projection).revision !== Number(hostRevision);
}

// Map authoritative App Store rows (apps:list result) into projection
// items. Only enabled apps that opt into the sidebar are projected
// (ADR-0025 D38: 0 App → section absent).
export function projectionFromApps(apps, revision) {
  const items = Array.isArray(apps)
    ? apps
        .filter((app) => app && typeof app.app_id === 'string' && app.enabled !== false && app.show_in_sidebar !== false && !app.needs_migration)
        .map((app) => ({
          appId: app.app_id,
          label: typeof app.name === 'string' && app.name ? app.name : app.app_id,
          icon: appIconFromSurface(app.surface_json),
          route: `app.html?app=${encodeURIComponent(app.app_id)}`,
          order: Number.isFinite(Number(app.sidebar_order)) ? Number(app.sidebar_order) : 0,
        }))
    : [];
  return { revision: Number(revision) || 0, items };
}

function appIconFromSurface(surfaceJson) {
  try {
    const surface = typeof surfaceJson === 'string' ? JSON.parse(surfaceJson) : surfaceJson;
    if (surface && typeof surface.icon === 'string' && surface.icon) return surface.icon;
  } catch {
    // malformed surface_json → default icon
  }
  return 'grid';
}

// Shared DOM rendering for the "应用" sidebar section. Both surfaces
// (files.html via FilesSidebar, space.html directly) call this so the
// section markup is identical by construction (Gate A3).
// `nav` must be an existing <nav> element; the section is appended and
// hidden when the projection is empty (ADR-0025 D38).
export function renderAppMenuInto(nav, projection, options = {}) {
  if (!nav) return;
  const { t = (key, fallback) => fallback || key, iconBase = '' } = options;
  const section = appSectionFromProjection(projection, { t });
  nav.replaceChildren();
  if (section.length === 0) {
    // 0 App → no section at all (ADR-0025 D38)
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
// chrome.storage.local changes (install/uninstall from any page).
// Returns an unsubscribe function.
export async function mountAppMenu(nav, options = {}) {
  const { t = (key, fallback) => fallback || key, iconBase = '', storage = globalThis.chrome?.storage } = options;
  const projection = await loadAppNavigation(storage);
  renderAppMenuInto(nav, projection, { t, iconBase });
  return onAppNavigationChange((next) => renderAppMenuInto(nav, next, { t, iconBase }), storage);
}

async function storageLocalGet(storage, key) {
  try {
    const result = await storage.local.get({ [key]: null });
    return sanitizeProjection(result?.[key]);
  } catch {
    return { ...DEFAULT_PROJECTION, items: [] };
  }
}

export async function loadAppNavigation(storage = globalThis.chrome?.storage) {
  if (!storage?.local) return { ...DEFAULT_PROJECTION, items: [] };
  return storageLocalGet(storage, APP_PROJECTION_STORAGE_KEY);
}

export async function saveAppNavigation(projection, storage = globalThis.chrome?.storage) {
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

export async function clearAppNavigation(storage = globalThis.chrome?.storage) {
  if (!storage?.local) return;
  try {
    await storage.local.remove(APP_PROJECTION_STORAGE_KEY);
  } catch {
    // best-effort, UI cache only
  }
}

// Build the sidebar "应用" section from a projection.
// Returns [] when there is nothing to show (ADR-0025 D38: 0 App → no section).
export function appSectionFromProjection(projection, options = {}) {
  const { t = (key, fallback) => fallback || key } = options;
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
        label: item.label,
        badge: null,
      })),
    },
  ];
}

// Subscribe to projection changes across pages. chrome.storage.onChanged
// fires in every page when the App Center (or the files page while its
// host connection is live) rewrites the cache.
export function onAppNavigationChange(listener, storage = globalThis.chrome?.storage) {
  if (!storage?.onChanged?.addListener) return () => {};
  const handler = (changes, area) => {
    if (area !== 'local' || !changes[APP_PROJECTION_STORAGE_KEY]) return;
    listener(sanitizeProjection(changes[APP_PROJECTION_STORAGE_KEY].newValue));
  };
  storage.onChanged.addListener(handler);
  return () => storage.onChanged.removeListener(handler);
}
