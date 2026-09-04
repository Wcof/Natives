import { createFilesSession } from './files-session.js';
import { createFilesSidebar } from './files-sidebar.js';
import { createFilesHomeView } from './files-home-view.js';
import { entryIcon, iconElement, htmlEscape, setIconTheme, isTextFile, TEXT_KINDS } from './files-icons.js';
import { createPreviewControllers } from './files-preview-controllers.js';
import { createFileOperations } from './files-operations.js';
import { createDiskUsage } from './files-disk-usage.js';
import { createFilesSearch } from './files-search.js';
import { bindFilesShortcuts } from './files-shortcuts.js';
import { createFilesEntriesRenderer } from './files-entries-renderer.js';
import { createFilesContextMenu } from './files-context-menu.js';
import { bindFilesEditorInteractions } from './files-editor-bindings.js';
import { bindFilesEntryEffects } from './files-entry-effects.js';
import { bindFilesPreviewLayout } from './files-preview-layout.js';
import { bindFilesToolbar } from './files-toolbar.js';
import { bindFilesWorkspaceInteractions } from './files-workspace-bindings.js';
import { createFilesLocale, loadFilesUiState, storageGet, storageSet } from './files-preferences.js';
import { createFilesHostConnection } from './files-host-connection.js';
import { createFilesFeedback } from './files-feedback.js';
import { createFilesWatchController } from './files-watch-controller.js';
import { fileUri, normalizePathInput, parentAndName, pathParts, renderFilesBreadcrumb } from './files-paths.js';

const PAGE_SIZE = 100;
const nativeHost = 'com.natives.file_manager';
const $ = (id) => document.getElementById(id);
let selectedTheme = 'archive';

const session = createFilesSession();
const historyScroll = new Map();
let pendingScrollTop;
let pendingSelectionPath;
let directoryToken = 0;
let pathToken = 0;
let rootPaths = [];

let filesLocale;
const t = (key, fallback) => (filesLocale?.t ? filesLocale.t(key, fallback) : fallback);

const rootLabels = {
  home: '主目录', desktop: '桌面', documents: '文稿',
  downloads: '下载', pictures: '图片', projects: '项目',
  movies: '影片', music: '音乐', tmp: '临时目录',
};

const feedback = createFilesFeedback({
  $, session, t, formatSize,
  selectedItems: () => selectedItems(),
  getClipboard: () => ops.getClipboard(),
  isHostConnected: () => nativeClient.connected,
});
const { setStatus, renderStatusBar, updateProgress, setOperationCancelable, toast } = feedback;

const searchController = createFilesSearch({
  $, call: (...args) => call(...args), t, session, setStatus, sortEntries, renderEntries,
  renderSelection, updatePager, getRootPaths: () => rootPaths,
  PAGE_SIZE, loadDirectory, iconElement,
});

filesLocale = createFilesLocale({
  onChange: (language) => {
    for (const id of Object.keys(rootLabels)) rootLabels[id] = t(id, rootLabels[id]);
    filesSidebar?.setPreferences({ language });
    renderBreadcrumb(session?.currentPath || '/');
    renderEntries();
    renderSelection();
    searchController?.updateSearchScopeButton?.();
    syncPreviewLayoutControls();
    renderStatusBar();
  },
});
const localeReady = filesLocale.ready;
const applyI18n = () => filesLocale.apply();
const switchLanguage = (language) => filesLocale.switchLanguage(language);

function applyTheme(theme, persist = true) {
  selectedTheme = ['volt', 'archive'].includes(theme) ? theme : 'archive';
  document.documentElement.dataset.theme = selectedTheme;
  setIconTheme(selectedTheme);
  filesSidebar?.setPreferences({ theme: selectedTheme });
  if (persist) storageSet('natives-theme', selectedTheme).catch(() => {});
  if (session?.entries?.length) renderEntries();
}

storageGet('natives-theme', 'archive').then((theme) => applyTheme(theme, false));

async function openModelSettings(returnFocus) {
  const module = await import('./model-settings.js');
  await module.openModelSettings({ t, language: filesLocale.language, returnFocus });
}

chrome.storage?.onChanged?.addListener((changes, area) => {
  if (area !== 'local') return;
  const next = changes['natives-language']?.newValue;
  if ((next === 'en' || next === 'zh_CN') && next !== filesLocale.language) switchLanguage(next);
});

const watchController = createFilesWatchController({
  $, session, t, parentAndName: (path) => parentAndName(path),
  editorState: () => preview.getEditorState(), imageEditorState: () => preview.getImageEditorState(),
  hasDirtyEditor: () => preview.hasDirtyEditor(), resetPreview: () => preview.resetPreviewNow(),
  getBatch: () => ops.getBatch(), updateProgress, setStatus, renderEntries: () => renderEntries(),
  getSearchQuery: () => searchController.getSearchQuery(), search: (query) => searchController.search(query),
  loadDirectory: (path) => loadDirectory(path), renderPreviewSelection: () => preview.renderPreviewSelection(),
  refreshEditorAfterExternalChange: (path) => preview.refreshEditorAfterExternalChange(path),
  renderSelection: () => renderSelection(), storageSet,
});
const changedPaths = watchController.changedPaths;

const hostConnection = createFilesHostConnection({
  host: nativeHost,
  writeMethods: new Set([
    'write_file', 'create_folder', 'trash', 'rename', 'copy_paths',
    'copy_batch', 'move_batch', 'trash_batch', 'duplicate_batch',
    'extract_archive', 'create_zip', 'import_begin', 'import_chunk', 'import_end',
  ]),
  t,
  onMessage: watchController.handleNativeMessage,
  onDisconnect: (error, intentional) => {
    watchController.handleNativeDisconnect(error, intentional);
    renderStatusBar();
  },
  onVisible: () => session.currentPath ? loadDirectory(session.currentPath) : init(),
  onMoveBatchResult: (params, result) => ops.migrateBatchPaths(params.paths || [], result.moved || [], result.errors || [], result.skipped || []),
  onTrashBatchErrors: (errors) => ops.rememberFailedOperation({ method: 'trash_batch', paths: errors.map((error) => error.path).filter(Boolean) }),
  hasDirtyEditor: () => preview.hasDirtyEditor(),
  onConnectionChange: () => {
    watchController.resetWatchedPath();
    renderStatusBar();
  },
});
const nativeClient = hostConnection.client;
const call = hostConnection.call;
const disconnectNative = hostConnection.disconnect;

async function loadUiState() {
  await loadFilesUiState({
    $, session, searchController, syncSortTabs, updateSortDirection,
    applySidebarCollapsed, syncPreviewLayoutControls,
  });
}

function syncSortTabs() {
  document.querySelectorAll('.sort-tab').forEach((tab) => {
    const active = tab.dataset.sort === session.sortBy;
    tab.setAttribute('aria-pressed', String(active));
    tab.classList.toggle('active', active);
  });
}

let previewLayout;
function syncPreviewLayoutControls() {
  previewLayout?.syncControls();
}

function renderBreadcrumb(path) {
  renderFilesBreadcrumb($('breadcrumb'), path, navigate);
}

function navigate(path, push = true) {
  if (typeof path === 'string' && path) storageSet('natives-last-path', path).catch(() => {});
  if (!path) return;
  if (path === session.currentPath) {
    loadDirectory(path);
    return;
  }
  const followChanges = session.followChanges;
  if (session.currentPath && followChanges && !pendingSelectionPath) watchController.stopFollowOnManual();
  if (preview.getEditorState()?.dirty) {
    preview.guardDirty(() => navigateNow(path, push));
    return;
  }
  navigateNow(path, push);
}

function navigateNow(path, push) {
  pathToken++;
  watchController.clearPendingFollow();
  if (pendingSelectionPath && path !== parentAndName(pendingSelectionPath).parent) {
    pendingSelectionPath = undefined;
  }
  if (session.currentPath) {
    historyScroll.set(session.currentPath, document.querySelector('.content')?.scrollTop || 0);
    while (historyScroll.size > 100) historyScroll.delete(historyScroll.keys().next().value);
  }
  pendingScrollTop = historyScroll.get(path) ?? 0;
  searchController.cancelActiveSearches();
  searchController.setGlobalMode(false);
  session.navigate(path, push);
  searchController.setSearchQuery('');
  if ($('search')) $('search').value = '';
  if ($('quick-filter')) $('quick-filter').value = '';
  closeQuickFilter();
  updateNavigationButtons();
  renderSelection();
  renderBreadcrumb(path);
  filesSidebar?.setActivePath(path);
  loadDirectory(path);
}

function updateNavigationButtons() {
  if ($('back')) $('back').disabled = session.historyIndex <= 0;
  if ($('forward')) $('forward').disabled = session.historyIndex < 0 || session.historyIndex >= session.history.length - 1;
  if ($('up')) $('up').disabled = !session.currentPath || session.currentPath === '/';
}

function updatePager(count = session.entries.length) {
  const pager = document.querySelector('.pagination');
  const hasMultiplePages = session.pageOffset > 0 || session.pageHasMore;
  if (pager) pager.hidden = !hasMultiplePages;
  if ($('previous-page')) $('previous-page').disabled = session.pageOffset === 0;
  if ($('next-page')) $('next-page').disabled = !session.pageHasMore;
  if ($('page-label')) $('page-label').textContent = count ? `${session.pageOffset + 1}–${session.pageOffset + count}` : '';
}

function updateSortDirection() {
  const descending = session.sortDirection === 'desc';
  const button = $('sort-direction');
  if (!button) return;
  button.dataset.direction = session.sortDirection;
  const key = descending ? 'sortDescending' : 'sortAscending';
  button.title = t(key, descending ? 'Descending' : 'Ascending');
  button.setAttribute('aria-label', button.title);
}

function sortEntries(list) {
  const sq = searchController.getSearchQuery();
  return [...list].sort((a, b) => {
    if (sq && (Number(a.searchScore) || Number(b.searchScore))) {
      const score = (Number(b.searchScore) || 0) - (Number(a.searchScore) || 0);
      if (score) return score;
    }
    if (a.isDir !== b.isDir) return a.isDir ? -1 : 1;
    const result = session.sortBy === 'mtime' ? Number(a.mtime) - Number(b.mtime) : session.sortBy === 'size' ? Number(a.size) - Number(b.size) : a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: 'base' });
    return session.sortDirection === 'desc' ? -result : result;
  });
}

async function loadDirectory(path) {
  const token = ++directoryToken;
  const viewport = document.querySelector('.content');
  const scrollTop = pendingScrollTop ?? (viewport?.scrollTop || 0);
  pendingScrollTop = undefined;
  setStatus(t('loading', '加载中…'));
  if ($('empty')) $('empty').hidden = true;
  $('entries')?.setAttribute('aria-busy', 'true');

  try {
    const result = await call('list_dir', {
      path,
      offset: session.pageOffset,
      limit: PAGE_SIZE,
      sortBy: session.sortBy,
      sortDir: session.sortDirection,
      showHidden: session.showHidden,
    });
    if (token !== directoryToken || session.currentPath !== path) return;
    session.entries = sortEntries(result.entries || []);
    const visiblePaths = new Set(session.entries.map((item) => item.path));
    session.selectedPaths = new Set([...session.selectedPaths].filter((p) => visiblePaths.has(p)));
    const previewPending = Boolean(pendingSelectionPath && visiblePaths.has(pendingSelectionPath));
    if (previewPending) {
      session.selectedPaths = new Set([pendingSelectionPath]);
      pendingSelectionPath = undefined;
    }
    const previewPath = preview.getEditorState()?.path || preview.getImageEditorState()?.path;
    if (previewPath && !visiblePaths.has(previewPath) && !preview.hasDirtyEditor()) preview.resetPreviewNow();
    session.lastSelectedIndex = session.selectedPaths.size
      ? session.entries.findIndex((item) => session.selectedPaths.has(item.path))
      : -1;
    session.pageHasMore = Boolean(result.hasMore);
    renderEntries();
    if (viewport) viewport.scrollTop = scrollTop;
    updatePager();
    renderStatusBar();
    if (previewPending) renderSelection();
    setStatus('');
    if (watchController.watchedPath !== path) {
      if (watchController.watchedPath) call('watch_stop', { path: watchController.watchedPath }).catch(() => {});
      await call('watch_start', { path });
      if (token !== directoryToken || session.currentPath !== path) {
        if (session.currentPath !== path) call('watch_stop', { path }).catch(() => {});
        return;
      }
      watchController.setWatchedPath(path);
    }
    await watchController.applyPendingFollow(path);
  } catch (error) {
    if (token !== directoryToken || session.currentPath !== path) return;
    session.entries = [];
    session.selectedPaths.clear();
    session.lastSelectedIndex = -1;
    renderEntries();
    renderStatusBar();
    setStatus(error.message, 'error');
    if ($('retry')) $('retry').hidden = false;
  } finally {
    if (token === directoryToken) $('entries')?.removeAttribute('aria-busy');
  }
}

let filesHomeView;
function renderHomeWelcome() {
  session.currentPath = '';
  session.entries = [];
  session.selectedPaths.clear();
  session.lastSelectedIndex = -1;
  renderBreadcrumb('');
  const countEl = $('item-count');
  if (countEl) countEl.textContent = '';
  const box = $('entries');
  if (box) {
    box.replaceChildren();
    box.classList.remove('grid');
  }
  if ($('empty')) $('empty').hidden = true;
  renderStatusBar();
  if (!filesHomeView && box) {
    filesHomeView = createFilesHomeView({
      container: box,
      onNavigate: (path) => navigate(path),
      onSearch: () => openSearchDialog(),
      t,
    });
  }
  filesHomeView?.render();
  setStatus(t('ready', '就绪'));
}

const entriesRenderer = createFilesEntriesRenderer({
  $, t, session, entryIcon, formatSize, parentAndName, fileUri, htmlEscape,
  selectEntry, openItemFromDoubleClick: (item) => ops.openItemFromDoubleClick(item), showContextMenu: (x, y, item) => contextMenu.showContextMenu(x, y, item),
  guardDirty: (action) => preview.guardDirty(action),
  editorState: () => preview.getEditorState(),
  renderSelection: (opts) => renderSelection(opts),
  changedPaths, searchController,
  navigate: (p) => navigate(p),
  importFileList: (files, p) => ops.importFileList(files, p),
  moveDroppedPaths: (raw, p) => ops.moveDroppedPaths(raw, p),
  copyDroppedUris: (uris, p) => ops.copyDroppedUris(uris, p),
  setStatus: (msg, type) => setStatus(msg, type),
  setPendingSelectionPath: (val) => { pendingSelectionPath = val; },
});

function renderEntries() {
  entriesRenderer.render(renderHomeWelcome);
}

function applyQuickFilter() {
  const query = $('quick-filter')?.value.toLowerCase().trim() || '';
  document.querySelectorAll('.entry').forEach((row) => {
    const name = row.querySelector('.entry-name')?.textContent.toLowerCase() || '';
    row.hidden = query ? !name.includes(query) : false;
  });
}

function openQuickFilter() {
  const wrap = $('quick-filter-wrap');
  if (!wrap) return;
  wrap.classList.add('open');
  $('quick-filter-toggle')?.setAttribute('aria-expanded', 'true');
  const input = $('quick-filter');
  input?.focus();
  input?.select();
}

function closeQuickFilter() {
  $('quick-filter-wrap')?.classList.remove('open');
  $('quick-filter-toggle')?.setAttribute('aria-expanded', 'false');
}

function renderSelection(options) {
  document.querySelectorAll('.entry').forEach((row) => {
    const selected = session.selectedPaths.has(row.dataset.path);
    row.classList.toggle('selected', selected);
    row.setAttribute('aria-selected', String(selected));
  });
  if (session.lastSelectedIndex < 0 || !session.selectedPaths.has(session.entries[session.lastSelectedIndex]?.path)) {
    session.lastSelectedIndex = session.selectedPaths.size
      ? session.entries.findIndex((item) => session.selectedPaths.has(item.path))
      : -1;
  }
  updateActions();
  renderStatusBar();
  preview.renderPreviewSelection(options);
}

async function selectEntry(index, event = {}) {
  watchController.clearPendingFollow();
  if (session.followChanges) watchController.stopFollowOnManual();
  if (preview.getEditorState()?.dirty) {
    const previous = new Set(session.selectedPaths);
    await preview.guardDirty(() => {});
    if (preview.getEditorState()?.dirty) {
      session.selectedPaths = previous;
      return;
    }
  }
  if (!session.entries[index]) return;
  session.select(index, event);
  renderSelection();
}

function updateActions() {
  for (const id of ['open', 'editor', 'reveal', 'rename', 'copy', 'move', 'trash']) {
    const button = $(id);
    if (button) button.disabled = session.selectedPaths.size === 0;
  }
  const archive = $('create-archive');
  if (archive) archive.disabled = session.selectedPaths.size === 0 || !session.currentPath;
}

function selectedItems() {
  return session.entries.filter((item) => session.selectedPaths.has(item.path));
}

function formatSize(size = 0) {
  if (size < 1024) return `${size} B`;
  if (size < 1024 ** 2) return `${(size / 1024).toFixed(1)} KB`;
  if (size < 1024 ** 3) return `${(size / 1024 ** 2).toFixed(1)} MB`;
  return `${(size / 1024 ** 3).toFixed(1)} GB`;
}

const diskUsageDialog = createDiskUsage({ $, call, t, entryIcon, formatSize, parentAndName, currentPath: () => session.currentPath });
const showDiskUsage = (path = session.currentPath) => diskUsageDialog.show(path);

const link = {};
const ops = createFileOperations({
  $, call, t, session, setStatus, toast, updateProgress, setOperationCancelable,
  openModal: (args) => contextMenu.openModal(args), loadDirectory, renderSelection, renderStatusBar, selectedItems,
  navigate, parentAndName, kindFromName: (name) => (name ? isTextFile({ name }) ? 'text' : 'other' : 'other'), markSelfOpened: watchController.markSelfOpened, link,
});
const preview = createPreviewControllers({
  $, call, t, session, setStatus, toast, formatSize, parentAndName, pathParts,
  entryIcon, iconElement, iconAction: (name, key, act) => {
    const btn = document.createElement('button');
    btn.append(iconElement(name));
    btn.onclick = act;
    btn.title = t(key, key);
    btn.setAttribute('aria-label', btn.title);
    return btn;
  }, isTextItem: isTextFile, TEXT_KINDS,
  isHostConnected: () => nativeClient.connected, selectedItems, renderSelection,
  loadDirectory, remember, revealPath: ops.revealPath, link,
});

const contextMenu = createFilesContextMenu({
  $, t, session, isTextFile,
  openItem: (item) => ops.openItem(item),
  renderSelection: (opts) => renderSelection(opts),
  guardDirty: (action) => preview.guardDirty(action),
  editorState: () => preview.getEditorState(),
  showDiskUsage,
  extractArchive: (item) => ops.extractArchive(item),
  revealSelected: () => ops.revealSelected(),
  copyPathSelected: () => ops.copyPathSelected(),
  renameSelected: () => ops.renameSelected(),
  transfer: (m) => ops.transfer(m),
  duplicateSelected: () => ops.duplicateSelected(),
  trashSelected: () => ops.trashSelected(),
  createEntry: (k) => ops.createEntry(k),
  ops,
  copyFileSelected: () => ops.copyFileSelected(),
  createZip: () => ops.createZip(),
  importFileList: (files) => ops.importFileList(files),
  searchController, loadDirectory, call, toast, setStatus,
});

Object.assign(link, {
  guardDirty: preview.guardDirty,
  hasDirtyEditor: preview.hasDirtyEditor,
  getEditorState: preview.getEditorState,
  migrateEditorViewPath: preview.migrateEditorViewPath,
  copyPathSelectedFor: ops.copyPathSelectedFor,
  openItem: ops.openItem,
  extractArchive: ops.extractArchive,
  openModal: (args) => contextMenu.openModal(args),
  setPendingSelectionPath: (value) => { pendingSelectionPath = value; },
  migrateChangedPath: (oldPath, newPath) => {
    const migrate = (path) => path === oldPath || path.startsWith(oldPath + '/') ? newPath + path.slice(oldPath.length) : path;
    const moved = [];
    for (const [path, value] of changedPaths) if (path === oldPath || path.startsWith(oldPath + '/')) moved.push([migrate(path), value]);
    for (const [path] of changedPaths) if (path === oldPath || path.startsWith(oldPath + '/')) changedPaths.delete(path);
    moved.forEach(([path, value]) => changedPaths.set(path, value));
  },
});

const recentOpenedPaths = [];
function remember(path) {
  const index = recentOpenedPaths.indexOf(path);
  if (index >= 0) recentOpenedPaths.splice(index, 1);
  recentOpenedPaths.unshift(path);
  if (recentOpenedPaths.length > 50) recentOpenedPaths.pop();
}

let filesSidebar;
let filesPreviewPanel;

function bindAppMenu(roots) {
  if (filesSidebar) {
    filesSidebar.setRoots(roots);
  } else {
    document.querySelectorAll('[data-root-id]').forEach((button) => {
      const root = roots.find((item) => item.id === button.dataset.rootId);
      button.disabled = !root;
      button.onclick = () => root && navigate(root.path);
    });
  }
}

async function navigateFromInput(raw) {
  const value = normalizePathInput(raw);
  if (!value) return;
  if (preview.getEditorState()?.dirty) {
    preview.guardDirty(() => navigateFromInput(value));
    return;
  }
  const token = ++pathToken;
  try {
    const result = await call('stat', { path: value }, crypto.randomUUID());
    if (token !== pathToken) return;
    if (result?.found && !result.isDir && result.path) {
      pendingSelectionPath = result.path;
      const parent = parentAndName(result.path).parent;
      if (parent === session.currentPath) {
        const index = session.entries.findIndex((item) => item.path === result.path);
        if (index >= 0) {
          session.selectedPaths = new Set([result.path]);
          session.lastSelectedIndex = index;
          pendingSelectionPath = undefined;
          renderSelection();
        } else loadDirectory(parent);
      } else navigate(parent);
      return;
    }
  } catch {
    if (token !== pathToken) return;
  }
  if (token === pathToken) navigate(value);
}

function openSearchDialog() {
  const dialog = $('search-dialog');
  if (!dialog) return;
  const qf = $('quick-filter');
  if (qf) qf.value = '';
  closeQuickFilter();
  applyQuickFilter();
  if (!dialog.open) dialog.showModal();
  queueMicrotask(() => {
    $('search')?.focus();
    $('search')?.select();
  });
}

async function init() {
  try {
    setStatus(t('connecting', '正在连接本地文件系统…'));
    const versionRequest = call('version');
    await loadUiState();
    const version = await versionRequest;
    if (version?.protocolVersion !== 1) throw new Error(t('nativeHostIncompatible', 'Native Host 协议不兼容'));
    const roots = await call('roots');
    rootPaths = roots.map((root) => root.path);
    bindAppMenu(roots);
    const stored = await storageGet('natives-last-path', '');
    const result = stored ? await call('stat', { path: stored }).catch(() => ({})) : {};
    const hasHarness = new URLSearchParams(location.search).has('ui-harness') || new URLSearchParams(location.search).has('self-test');
    if (result?.found && result.isDir) navigate(stored, false);
    else if (hasHarness && roots[0]) navigate(roots[0].path, false);
    else renderHomeWelcome();
  } catch (error) {
    setStatus(error.message || t('hostConnectionFailed', 'Native Host 连接失败'), 'error');
    if ($('retry')) $('retry').hidden = false;
  }
}

previewLayout = bindFilesPreviewLayout({ $, session, storageSet, t });

const toolbar = bindFilesToolbar({
  $, session, nativeClient, disconnectNative, init, searchController, loadDirectory,
  navigate, navigateFromInput, normalizePathInput, parentAndName,
  setPendingSelectionPath: (value) => { pendingSelectionPath = value; },
  pageSize: PAGE_SIZE, syncSortTabs, updateSortDirection, storageSet, ops, preview,
  selectedItems, renderEntries, applySidebarCollapsed, showDiskUsage, call, toast, setStatus, t,
  onFollowChange: (enabled) => {
    if (!enabled) return;
    watchController.clearPendingFollow();
  },
});

bindFilesShortcuts({
  $, session, ops, searchController, call,
  hideContextMenu: () => contextMenu.hideContextMenu(),
  openLanguageSettings: () => filesSidebar?.openSettings('language'),
  openEditorSelected: () => ops.openEditorSelected(),
  toggleSidebar: toolbar.toggleSidebar,
  syncGridControls: toolbar.syncGridControls,
  storageSet, renderEntries, renderSelection, openItemFromDoubleClick: ops.openItemFromDoubleClick, selectEntry,
  renameSelected: ops.renameSelected, trashSelected: ops.trashSelected, openQuickFilter, closeQuickFilter,
  applyQuickFilter, openSearchDialog, setStatus, t,
});

bindFilesWorkspaceInteractions({
  $, session, ops, searchController, contextMenu,
  guardDirty: preview.guardDirty, hasDirtyEditor: preview.hasDirtyEditor, renderSelection,
  setStatus, t, selectEntry, openQuickFilter, closeQuickFilter,
  applyQuickFilter, openSearchDialog, loadDirectory,
});

bindFilesEditorInteractions({
  $, ops, contextMenu, iconElement, setStatus, t,
});

bindFilesEntryEffects({
  $, session, searchController, entryIcon, call, t,
  getDirectoryToken: () => directoryToken,
  selectEntry, renderSelection, renderPreviewSelection: preview.renderPreviewSelection, editorState: preview.getEditorState,
});

updateSortDirection();
updateNavigationButtons();

async function bootstrap() {
  await Promise.race([localeReady, new Promise((resolve) => setTimeout(resolve, 1_000))]);
  document.documentElement.lang = filesLocale.language === 'en' ? 'en' : 'zh-CN';
  applyI18n();
  for (const id of Object.keys(rootLabels)) rootLabels[id] = t(id, rootLabels[id]);
  filesSidebar = createFilesSidebar({
    container: $('app-sidebar'),
    resizer: $('sidebar-resizer'),
    toggleButton: $('toggle-sidebar'),
    initialWidth: session.sidebarWidth,
    initialCollapsed: session.sidebarCollapsed,
    onNavigate: (path) => navigate(path),
    onSearch: () => openSearchDialog(),
    onWidthChange: (width) => {
      session.sidebarWidth = width;
      storageSet('natives-sidebar-width', width).catch(() => {});
    },
    onCollapsedChange: (collapsed) => {
      session.sidebarCollapsed = collapsed;
      storageSet('natives-sidebar-collapsed', collapsed).catch(() => {});
    },
    onLanguageChange: (lang) => switchLanguage(lang),
    onThemeChange: (theme) => applyTheme(theme),
    onModelSettings: (anchor) => openModelSettings(anchor).catch((error) => toast(error.message, 'error')),
    t: (key, fallback) => t(key, fallback),
  });
  init();
}

bootstrap();

function applySidebarCollapsed() {
  const sidebarCollapsed = session.sidebarCollapsed;
  if (filesSidebar) filesSidebar.setCollapsed(sidebarCollapsed);
  else {
    document.querySelector('.layout')?.classList.toggle('sidebar-collapsed', sidebarCollapsed);
    document.body.classList.toggle('sidebar-collapsed', sidebarCollapsed);
    const button = $('toggle-sidebar');
    if (button) {
      button.setAttribute('aria-pressed', String(sidebarCollapsed));
      button.title = t(sidebarCollapsed ? 'expandSidebar' : 'collapseSidebar', sidebarCollapsed ? 'Expand sidebar' : 'Collapse sidebar');
      button.setAttribute('aria-label', button.title);
    }
  }
}
