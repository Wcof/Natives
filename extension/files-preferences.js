export async function storageGet(key, fallback) {
  if (chrome.storage?.local) {
    try {
      const value = await Promise.race([
        chrome.storage.local.get({ [key]: fallback }),
        new Promise((resolve) => setTimeout(() => resolve(undefined), 500)),
      ]);
      if (value) return value[key] ?? fallback;
    } catch {}
  }
  try {
    return JSON.parse(localStorage.getItem(key) || JSON.stringify(fallback));
  } catch {
    return fallback;
  }
}

export async function storageSet(key, value) {
  if (chrome.storage?.local) {
    try {
      const saved = await Promise.race([
        chrome.storage.local.set({ [key]: value }).then(() => true),
        new Promise((resolve) => setTimeout(() => resolve(false), 500)),
      ]);
      if (saved) return;
    } catch {}
  }
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {}
}

export function createFilesLocale({ onChange }) {
  let messages = {};
  let language = 'zh_CN';
  let switchToken = 0;
  const t = (key, fallback) => messages[key]?.message || chrome.i18n?.getMessage(key) || fallback;

  function apply() {
    document.documentElement.lang = language === 'en' ? 'en' : 'zh-CN';
    document.querySelectorAll('[data-i18n]').forEach((element) => { element.textContent = t(element.dataset.i18n, element.textContent); });
    document.querySelectorAll('[data-i18n-title]').forEach((element) => { element.title = t(element.dataset.i18nTitle, element.title); });
    document.querySelectorAll('[data-i18n-placeholder]').forEach((element) => { element.placeholder = t(element.dataset.i18nPlaceholder, element.placeholder); });
    document.querySelectorAll('[data-i18n-aria-label]').forEach((element) => {
      element.setAttribute('aria-label', t(element.dataset.i18nAriaLabel, element.getAttribute('aria-label') || ''));
    });
  }

  async function load(nextLanguage) {
    language = nextLanguage === 'en' ? 'en' : 'zh_CN';
    try {
      const response = await fetch(`_locales/${language}/messages.json`);
      messages = response.ok ? await response.json() : {};
    } catch {
      messages = {};
    }
  }

  const ready = storageGet('natives-language', '').then((stored) => load(stored === 'en' || stored === 'zh_CN' ? stored : 'zh_CN'));

  async function switchLanguage(nextLanguage) {
    const token = ++switchToken;
    const next = nextLanguage === 'en' ? 'en' : 'zh_CN';
    await storageSet('natives-language', next);
    await load(next);
    if (token !== switchToken) return;
    apply();
    onChange?.(language);
  }

  return { t, ready, apply, switchLanguage, get language() { return language; } };
}

export async function loadFilesUiState({
  $, session, searchController, syncSortTabs, updateSortDirection,
  applySidebarCollapsed, syncPreviewLayoutControls,
}) {
  const [
    storedViewMode, storedGridSize, storedSortBy, storedSortDirection,
    storedShowHidden, storedFollowChanges, storedPreviewWidth,
    storedPreviewHeight, storedPreviewBottom, storedSidebarWidth, storedSidebarCollapsed,
    storedRecursiveSearch,
  ] = await Promise.all([
    storageGet('natives-view-mode', 'list'), storageGet('natives-grid-size', 'medium'),
    storageGet('natives-sort-by', 'name'), storageGet('natives-sort-direction', 'asc'),
    storageGet('natives-show-hidden', false), storageGet('natives-follow-changes', false),
    storageGet('natives-preview-width', 360), storageGet('natives-preview-height', 320),
    storageGet('natives-preview-bottom', false), storageGet('natives-sidebar-width', 248),
    storageGet('natives-sidebar-collapsed', false), storageGet('natives-recursive-search', false),
  ]);

  searchController.setRecursive(Boolean(storedRecursiveSearch));
  session.viewMode = storedViewMode === 'grid' ? 'grid' : 'list';
  session.gridSize = ['small', 'medium', 'large'].includes(storedGridSize) ? storedGridSize : 'medium';
  session.sortBy = ['name', 'mtime', 'size'].includes(storedSortBy) ? storedSortBy : 'name';
  session.sortDirection = storedSortDirection === 'desc' ? 'desc' : 'asc';
  session.showHidden = Boolean(storedShowHidden);
  session.followChanges = Boolean(storedFollowChanges);
  session.previewWidth = Math.min(620, Math.max(260, Number(storedPreviewWidth) || 360));
  session.previewHeight = Math.min(600, Math.max(180, Number(storedPreviewHeight) || 320));
  session.sidebarWidth = Math.min(420, Math.max(190, Number(storedSidebarWidth) || 248));
  session.sidebarCollapsed = Boolean(storedSidebarCollapsed);
  session.previewBottom = Boolean(storedPreviewBottom);

  document.documentElement.style.setProperty('--sidebar-width', `${session.sidebarWidth}px`);
  document.documentElement.style.setProperty('--preview-width', `${session.previewWidth}px`);
  document.documentElement.style.setProperty('--preview-height', `${session.previewHeight}px`);
  $('sidebar-resizer')?.setAttribute('aria-valuenow', String(session.sidebarWidth));
  document.querySelector('.layout')?.classList.toggle('preview-bottom', session.previewBottom);
  applySidebarCollapsed();
  syncSortTabs();
  if ($('show-hidden')) $('show-hidden').checked = session.showHidden;
  if ($('follow-changes')) $('follow-changes').checked = session.followChanges;
  if ($('recursive-search')) $('recursive-search').checked = searchController.getRecursive();
  $('list-view')?.setAttribute('aria-pressed', String(session.viewMode === 'list'));
  $('grid-view')?.setAttribute('aria-pressed', String(session.viewMode === 'grid'));
  updateSortDirection();
  syncPreviewLayoutControls();
}
