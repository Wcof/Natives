import { createNativeClient } from './native-client.js';
import { createFilesSession } from './files-session.js';
import { createFilesSidebar } from './files-sidebar.js';
import { createFilesPreviewPanel } from './files-preview-panel.js';
import { createFilesHomeView } from './files-home-view.js';
import { entryIcon, iconElement, htmlEscape, setIconTheme, kindFromName, isTextFile, TEXT_KINDS } from './files-icons.js';
import { createPreviewControllers } from './files-preview-controllers.js';
import { createFileOperations } from './files-operations.js';
import { createDiskUsage } from './files-disk-usage.js';

const PAGE_SIZE = 100;
const nativeHost = 'com.natives.file_manager';
const $ = (id) => document.getElementById(id);
let localeMessages = {};
let selectedLanguage = 'zh_CN';
let localeSwitchToken = 0;
let selectedTheme = 'archive';
function applyTheme(theme, persist = true) {
  selectedTheme = ['volt', 'archive'].includes(theme) ? theme : 'archive';
  document.documentElement.dataset.theme = selectedTheme;
  setIconTheme(selectedTheme);
  filesSidebar?.setPreferences({ theme: selectedTheme });
  if (persist) storageSet('natives-theme', selectedTheme).catch(() => {});
  if (session?.entries?.length) renderEntries();
}
storageGet('natives-theme', 'archive').then((theme) => applyTheme(theme, false));
async function loadLocale() {
  const stored = await storageGet('natives-language', '');
  // Manifest default_locale is zh_CN; only an explicit preference opts into English.
  selectedLanguage = stored === 'en' || stored === 'zh_CN' ? stored : 'zh_CN';
  try { const response = await fetch(`_locales/${selectedLanguage}/messages.json`); localeMessages = response.ok ? await response.json() : {}; } catch { localeMessages = {}; }
}
const localeReady = loadLocale();
const t = (key, fallback) => localeMessages[key]?.message || chrome.i18n?.getMessage(key) || fallback;
chrome.storage?.onChanged?.addListener((changes, area) => {
  if (area !== 'local') return;
  const next = changes['natives-language']?.newValue;
  if ((next === 'en' || next === 'zh_CN') && next !== selectedLanguage) switchLanguage(next);
});
const rootLabels = {
  home: t('home', '主目录'), desktop: t('desktop', '桌面'), documents: t('documents', '文稿'),
  downloads: t('downloads', '下载'), pictures: t('pictures', '图片'), projects: t('projects', '项目'),
  movies: t('movies', '影片'), music: t('music', '音乐'), tmp: t('tmp', '临时目录'),
};
const session = createFilesSession();
const historyScroll = new Map();
let pendingScrollTop;
let searchTruncated = false;
let pendingSelectionPath;
let directoryToken = 0;
let pathToken = 0;
let searchQuery = '';
let globalSearchMode = false;
let rootPaths = [];
let searchToken = 0;
let searchTimer;
let activeSearchId;
const activeSearchIds = new Set();
let recursiveSearch = false;
let resizingPreview = false;
let watchedPath;
let reloadTimer;
let pendingFollowPath;
let followApplyTimer;
let followAppliedAt = 0;
const changedPaths = new Map();
const selfOpenedPaths = new Map();
let changedCleanupTimer;
let pageClosing = false;
let idleDisconnectTimer;
let reconnectTimer;
let reconnectDelay = 1_000;
let entriesDragDepth = 0;
let dropHintDepth = 0;
const writeMethods = new Set([
  'write_file', 'create_file', 'create_folder', 'rename', 'copy', 'move', 'duplicate',
  'copy_batch', 'move_batch', 'duplicate_batch', 'trash', 'trash_batch',
  'extract_archive', 'create_zip', 'import_probe', 'import_begin', 'import_chunk',
  'import_end', 'import_cancel', 'copy_paths', 'copy_image',
]);
const nativeClient = createNativeClient({ host: nativeHost, writeMethods, onEvent: handleNativeMessage, onDisconnect: handleNativeDisconnect, onResponse: (message) => globalThis.__NATIVES_TEST_RESPONSE__?.(message) });

function applyI18n() {
  document.querySelectorAll('[data-i18n]').forEach((element) => { element.textContent = t(element.dataset.i18n, element.textContent); });
  document.querySelectorAll('[data-i18n-title]').forEach((element) => { element.title = t(element.dataset.i18nTitle, element.title); element.setAttribute('aria-label', element.title); });
  document.querySelectorAll('[data-i18n-placeholder]').forEach((element) => { element.placeholder = t(element.dataset.i18nPlaceholder, element.placeholder); });
  document.querySelectorAll('[data-i18n-aria-label]').forEach((element) => element.setAttribute('aria-label', t(element.dataset.i18nAriaLabel, element.getAttribute('aria-label') || '')));
}
async function switchLanguage(language) {
  const next = language === 'en' ? 'en' : 'zh_CN';
  const token = ++localeSwitchToken;
  // Re-apply even when the stored value already matches: the initial HTML is
  // English and a fast user selection can arrive before localeReady paints it.
  await storageSet('natives-language', next);
  try { const response = await fetch(`_locales/${next}/messages.json`); localeMessages = response.ok ? await response.json() : {}; } catch { localeMessages = {}; }
  if (token !== localeSwitchToken) return;
  selectedLanguage = next;
  document.documentElement.lang = next === 'en' ? 'en' : 'zh-CN';
  applyI18n();
  for (const id of Object.keys(rootLabels)) rootLabels[id] = t(id, rootLabels[id]);
  filesSidebar?.setPreferences({ language: next });
  renderBreadcrumb(session.currentPath || '/');
  renderEntries();
  renderSelection();
  updateSearchScopeButton();
  syncPreviewLayoutControls();
  renderStatusBar();
}
async function storageGet(key, fallback) {
  if (chrome.storage?.local) { try { const value = await Promise.race([chrome.storage.local.get({ [key]: fallback }), new Promise((resolve) => setTimeout(() => resolve(undefined), 500))]); if (value) return value[key] ?? fallback; } catch {} }
  try { return JSON.parse(localStorage.getItem(key) || JSON.stringify(fallback)); } catch { return fallback; }
}
async function storageSet(key, value) {
  if (chrome.storage?.local) { try { const saved = await Promise.race([chrome.storage.local.set({ [key]: value }).then(() => true), new Promise((resolve) => setTimeout(() => resolve(false), 500))]); if (saved) return; } catch {} }
  try { localStorage.setItem(key, JSON.stringify(value)); } catch {}
}
async function loadUiState() {
  const [storedViewMode, storedGridSize, storedSortBy, storedSortDirection, storedShowHidden, storedFollowChanges, storedPreviewWidth, storedPreviewHeight, storedPreviewBottom, storedSidebarWidth, storedSidebarCollapsed] = await Promise.all([
    storageGet('natives-view-mode', 'list'), storageGet('natives-grid-size', 'medium'), storageGet('natives-sort-by', 'name'), storageGet('natives-sort-direction', 'asc'), storageGet('natives-show-hidden', false), storageGet('natives-follow-changes', false), storageGet('natives-preview-width', 360), storageGet('natives-preview-height', 320), storageGet('natives-preview-bottom', false), storageGet('natives-sidebar-width', 248), storageGet('natives-sidebar-collapsed', false),
  ]);
  recursiveSearch = Boolean(await storageGet('natives-recursive-search', false));
  session.viewMode = storedViewMode === 'grid' ? 'grid' : 'list'; session.gridSize = ['small', 'medium', 'large'].includes(storedGridSize) ? storedGridSize : 'medium'; session.sortBy = ['name', 'mtime', 'size'].includes(storedSortBy) ? storedSortBy : 'name'; session.sortDirection = storedSortDirection === 'desc' ? 'desc' : 'asc'; session.showHidden = Boolean(storedShowHidden); session.followChanges = Boolean(storedFollowChanges); session.previewWidth = Math.min(620, Math.max(260, Number(storedPreviewWidth) || 360)); session.previewHeight = Math.min(600, Math.max(180, Number(storedPreviewHeight) || 320)); session.sidebarWidth = Math.min(420, Math.max(190, Number(storedSidebarWidth) || 248)); session.sidebarCollapsed = Boolean(storedSidebarCollapsed); session.previewBottom = Boolean(storedPreviewBottom); document.documentElement.style.setProperty('--sidebar-width', `${session.sidebarWidth}px`); document.documentElement.style.setProperty('--preview-width', `${session.previewWidth}px`); document.documentElement.style.setProperty('--preview-height', `${session.previewHeight}px`); $('sidebar-resizer').setAttribute('aria-valuenow', String(session.sidebarWidth)); $('preview-resizer').setAttribute('aria-valuenow', String(session.previewBottom ? session.previewHeight : session.previewWidth)); $('preview-resizer').setAttribute('aria-orientation', session.previewBottom ? 'horizontal' : 'vertical'); document.querySelector('.layout').classList.toggle('preview-bottom', session.previewBottom); applySidebarCollapsed();
  syncSortTabs(); $('show-hidden').checked = session.showHidden; $('follow-changes').checked = session.followChanges; $('recursive-search').checked = recursiveSearch; $('list-view').setAttribute('aria-pressed', String(session.viewMode === 'list')); $('grid-view').setAttribute('aria-pressed', String(session.viewMode === 'grid')); updateSortDirection();
  syncPreviewLayoutControls();
}
function syncSortTabs() {
  document.querySelectorAll('.sort-tab').forEach((tab) => {
    const active = tab.dataset.sort === session.sortBy;
    tab.setAttribute('aria-pressed', String(active));
    tab.classList.toggle('active', active);
  });
}
function syncPreviewLayoutControls() { const button = $('toggle-preview-layout'); if (!button) return; button.title = session.previewBottom ? t('movePreviewSide', 'Move preview to the side') : t('movePreviewBelow', 'Move preview below'); button.setAttribute('aria-label', button.title); button.setAttribute('aria-pressed', String(session.previewBottom)); const resizer = $('preview-resizer'); resizer.setAttribute('aria-orientation', session.previewBottom ? 'horizontal' : 'vertical'); resizer.setAttribute('aria-valuenow', String(session.previewBottom ? session.previewHeight : session.previewWidth)); resizer.setAttribute('aria-valuemin', String(session.previewBottom ? 180 : 240)); resizer.setAttribute('aria-valuemax', String(session.previewBottom ? 600 : 620)); }
function setStatus(message, kind = '') { $('status').textContent = message; $('status').className = kind; }
function renderStatusBar() { const selectedSize = selectedItems().reduce((total, item) => total + (item.isDir ? 0 : Number(item.size) || 0), 0); const fileClipboard = ops.getClipboard(); $('selection-status').textContent = `${session.entries.length} ${t('items', '个项目')} · ${session.selectedPaths.size} ${t('selected', '已选择')} · ${t('selectedSize', '选中大小')} ${formatSize(selectedSize)}`; const clip = $('file-clipboard-status'); const clear = $('clear-file-clipboard'); if (clip) clip.textContent = fileClipboard ? `${fileClipboard.mode === 'copy' ? t('clipboardCopied', '已复制') : t('clipboardCut', '已剪切')} ${fileClipboard.paths.length} ${t('items', '项')}` : ''; if (clear) clear.hidden = !fileClipboard; $('host-status').textContent = nativeClient.connected ? t('hostConnected', 'Host 已连接') : t('hostDisconnectedShort', 'Host 未连接'); }
function updateProgress(value, max, visible = true) { const progress = $('operation-progress'); progress.max = Math.max(1, max || 1); progress.value = Math.min(progress.max, Math.max(0, value || 0)); progress.hidden = !visible; }
function setOperationCancelable(visible) { $('cancel-operation').hidden = !visible; }
function toast(message, kind = '') { const element = $('toast'); element.textContent = message; element.className = kind; element.hidden = false; clearTimeout(toast.timer); toast.timer = setTimeout(() => { element.hidden = true; }, 3200); }
function markChangedPath(path, kind = 'modified') {
  if (!path) return;
  if (session.followChanges && !editorState()?.dirty && parentAndName(path).parent === session.currentPath) pendingFollowPath = path;
  const previous = changedPaths.get(path); changedPaths.set(path, { timestamp: Date.now(), count: (previous?.count || 0) + 1, kind });
  if (changedPaths.size > 256) changedPaths.delete(changedPaths.keys().next().value);
  clearTimeout(changedCleanupTimer);
  changedCleanupTimer = setTimeout(() => {
    const cutoff = Date.now() - 3_000;
    for (const [changedPath, change] of changedPaths) if (change.timestamp < cutoff) changedPaths.delete(changedPath);
    renderEntries();
  }, 3_050);
}
function isNoisyWatchPath(path) {
  const name = String(path || '').split('/').pop() || '';
  return /\.(swp|tmp|part|lock)$/i.test(name)
    || /(?:-journal|-shm|-wal)$/i.test(name);
}
function markSelfOpened(path) {
  if (!path) return;
  selfOpenedPaths.set(path, Date.now());
  setTimeout(() => {
    const timestamp = selfOpenedPaths.get(path);
    if (timestamp && Date.now() - timestamp >= 3_000) selfOpenedPaths.delete(path);
  }, 3_050);
}
function isSelfOpened(path) {
  const timestamp = selfOpenedPaths.get(path);
  if (!timestamp) return false;
  if (Date.now() - timestamp >= 3_000) { selfOpenedPaths.delete(path); return false; }
  return true;
}

function disconnectNative() {
  clearTimeout(idleDisconnectTimer); idleDisconnectTimer = undefined; clearTimeout(reconnectTimer); reconnectTimer = undefined; clearTimeout(reloadTimer); reloadTimer = undefined; watchedPath = undefined;
  nativeClient.disconnect();
}
function scheduleReconnect() {
  if (pageClosing || document.hidden || nativeClient.connected || reconnectTimer) return;
  const delay = reconnectDelay; reconnectDelay = Math.min(reconnectDelay * 2, 8_000);
  reconnectTimer = setTimeout(() => { reconnectTimer = undefined; if (pageClosing || document.hidden || nativeClient.connected) return; const port = connectNative(); if (port) { reconnectDelay = 1_000; if (session.currentPath) loadDirectory(session.currentPath); else init(); } else scheduleReconnect(); }, delay);
}
function connectNative() {
  let port;
  if (nativeClient.connected) port = nativeClient.connect();
  else {
    try { port = nativeClient.connect(); reconnectDelay = 1_000; renderStatusBar(); } catch (error) { setStatus(`${t('hostConnectionFailed', 'Native Host 连接失败')}：${error.message || ''}`, 'error'); $('retry').hidden = false; renderStatusBar(); scheduleReconnect(); return undefined; }
  }
  attachPreviewPortListeners(port);
  return port;
}
function attachPreviewPortListeners(port) {
  if (!port || port.__nativesPreviewCleanup) return;
  port.__nativesPreviewCleanup = true;
  port.onDisconnect.addListener(() => { if (!editorState()?.dirty && !imageEditorState?.dirty) resetPreviewNow(); });
  port.onMessage.addListener((message) => { if (message?.result?.event !== 'fs_changed' || !message.result.path) return; const changed = message.result.path; const item = session.entries.find((entry) => entry.path === changed); if (!item || !session.selectedPaths.has(changed) || editorPaths()) return; setTimeout(() => { if (session.currentPath && session.selectedPaths.has(changed) && !editorState?.path && !imageEditorState?.path) renderPreviewSelection(); }, 300); });
}
function handleNativeMessage(message) {
  const messageId = typeof message?.id === 'string' ? message.id : '';
  if ((message?.result?.event === 'batch_progress' || message?.result?.event === 'archive_progress') && activeBatch?.requestId === messageId.replace(/:progress$/, '')) {
    const completed = Number(message.result.completed) || 0; const total = Number(message.result.total) || 1;
    updateProgress(completed, total);
    const phase = message.result.event === 'archive_progress' ? (message.result.phase === 'validating' ? t('archiveValidating', '校验中') : t('archiveCompressing', '压缩中')) : t('processing', '处理中');
    setStatus(`${phase} · ${completed}/${total}`);
    return;
  }
  if (message?.result?.event === 'fs_changed' && session.currentPath) {
    const changedPath = message.result.path;
    if (isNoisyWatchPath(changedPath) || isSelfOpened(changedPath)) return;
    if (message.result.kind === 'removed' && (editorState()?.path === changedPath || preview.getImageEditorState()?.path === changedPath) && !preview.hasDirtyEditor()) resetPreviewNow();
    markChangedPath(changedPath, message.result.kind || 'modified');
    const watchedDirectory = session.currentPath; clearTimeout(reloadTimer); reloadTimer = setTimeout(() => { reloadTimer = undefined; if (session.currentPath !== watchedDirectory) return; const scrollTop = document.querySelector('.content')?.scrollTop || 0; const refresh = searchQuery ? search(searchQuery) : loadDirectory(watchedDirectory); Promise.resolve(refresh).then(() => { if (session.currentPath === watchedDirectory) { const viewport = document.querySelector('.content'); if (viewport) viewport.scrollTop = scrollTop; } }); if (editorState?.path === changedPath) refreshEditorAfterExternalChange(changedPath); }, 250);
  }
}
function handleNativeDisconnect(error, wasIntentional) {
  watchedPath = undefined;
  renderStatusBar();
  if (!wasIntentional) { $('retry').hidden = false; setStatus(`${t('hostDisconnected', 'Native Host 已断开')}：${error.message}`, 'error'); scheduleReconnect(); }
}
function scheduleIdleDisconnect() {
  clearTimeout(idleDisconnectTimer); idleDisconnectTimer = undefined;
  if (document.hidden && nativeClient.inFlight === 0 && nativeClient.writesInFlight === 0 && nativeClient.connected) idleDisconnectTimer = setTimeout(() => { if (document.hidden && nativeClient.inFlight === 0 && nativeClient.writesInFlight === 0) disconnectNative(); }, 60_000);
}
async function call(method, params = {}, requestId) {
  let result;
  try { result = await nativeClient.call(method, params, requestId); } catch (error) {
    if (!nativeClient.connected && error.message === 'Native Host 已断开') throw new Error(t('hostDisconnected', 'Native Host 已断开'));
    if (error.message === '请求超时') throw new Error(t('requestTimeout', '请求超时'));
    throw error.message === '操作失败' ? new Error(t('operationFailed', '操作失败')) : error;
  }
  if (method === 'move_batch' && (result?.errors?.length || result?.skipped?.length)) migrateBatchPaths(params.paths || [], result.moved || [], result.errors || [], result.skipped || []);
  if (method === 'trash_batch' && result?.errors?.length) rememberFailedOperation({ method: 'trash_batch', paths: result.errors.map((error) => error.path).filter(Boolean) });
  if (pageClosing && nativeClient.inFlight === 0) disconnectNative(); else scheduleIdleDisconnect();
  return result;
}
document.addEventListener('visibilitychange', () => { if (document.hidden) scheduleIdleDisconnect(); else { pageClosing = false; clearTimeout(idleDisconnectTimer); if (!nativeClient.connected) { if (session.currentPath) loadDirectory(session.currentPath); else init(); } } });
window.addEventListener('pagehide', () => { pageClosing = true; if (nativeClient.writesInFlight === 0) disconnectNative(); else scheduleIdleDisconnect(); });
window.addEventListener('beforeunload', (event) => { if (preview.hasDirtyEditor()) { event.preventDefault(); event.returnValue = ''; } });

function pathParts(path) { return path.split('/').filter(Boolean); }
function fileUri(path) { return `file://${path.split('/').map((segment, index) => index === 0 ? '' : encodeURIComponent(segment)).join('/')}`; }

function iconAction(name, key, action) { const button = document.createElement('button'); button.append(iconElement(name)); button.onclick = action; const label = t(key, key); button.title = label; button.setAttribute('aria-label', label); return button; }
function renderBreadcrumb(path) {
  const box = $('breadcrumb'); box.replaceChildren(); if (!path) return; const parts = pathParts(path); let value = path.startsWith('/') ? '/' : '';
  const root = document.createElement('button'); root.className = `crumb${parts.length === 0 ? ' last' : ''}`; root.textContent = path.startsWith('/') ? '/' : path; root.onclick = () => path.startsWith('/') && navigate('/'); box.append(root);
  parts.forEach((part, index) => { value = `${value.replace(/\/$/, '')}/${part}`; const crumbPath = value; const separator = document.createElement('span'); separator.className = 'crumb-separator'; separator.textContent = ' / '; box.append(separator); const button = document.createElement('button'); const isLast = index === parts.length - 1; button.className = `crumb${isLast ? ' last' : ''}`; button.textContent = part; button.onclick = () => navigate(crumbPath); box.append(button); });
}
function stopFollowOnManual() { if (!session.followChanges) return; session.followChanges = false; pendingFollowPath = undefined; clearTimeout(followApplyTimer); followApplyTimer = undefined; $('follow-changes').checked = false; storageSet('natives-follow-changes', false).catch(() => {}); setStatus(t('followChangesStopped', '手动浏览，已停止跟随')); }
function navigate(path, push = true) { if (typeof path === 'string' && path) storageSet('natives-last-path', path).catch(() => {}); if (!path) return; if (path === session.currentPath) { loadDirectory(path); return; } const followChanges = session.followChanges; if (session.currentPath && followChanges && !pendingSelectionPath) stopFollowOnManual(); if (editorState()?.dirty) { guardDirty(() => navigateNow(path, push)); return; } navigateNow(path, push); }
function navigateNow(path, push) {
  pathToken++;
  clearTimeout(followApplyTimer); followApplyTimer = undefined;
  if (pendingSelectionPath && path !== parentAndName(pendingSelectionPath).parent) pendingSelectionPath = undefined;
  if (session.currentPath) { historyScroll.set(session.currentPath, document.querySelector('.content')?.scrollTop || 0); while (historyScroll.size > 100) historyScroll.delete(historyScroll.keys().next().value); }
  pendingScrollTop = historyScroll.get(path) ?? 0;
  clearTimeout(searchTimer); searchToken++; directoryToken++; globalSearchMode = false; updateSearchScopeButton(); cancelActiveSearches(); session.navigate(path, push); searchQuery = ''; $('search').value = ''; $('quick-filter').value = ''; closeQuickFilter(); updateNavigationButtons(); renderSelection(); renderBreadcrumb(path); filesSidebar?.setActivePath(path); loadDirectory(path);
}
function updateNavigationButtons() { $('back').disabled = session.historyIndex <= 0; $('forward').disabled = session.historyIndex < 0 || session.historyIndex >= session.history.length - 1; $('up').disabled = !session.currentPath || session.currentPath === '/'; }
function updatePager(count = session.entries.length) { $('previous-page').disabled = session.pageOffset === 0; $('next-page').disabled = !session.pageHasMore; $('page-label').textContent = count ? `${session.pageOffset + 1}–${session.pageOffset + count}` : ''; }
function updateSortDirection() { const descending = session.sortDirection === 'desc'; const button = $('sort-direction'); button.dataset.direction = session.sortDirection; const key = descending ? 'sortDescending' : 'sortAscending'; button.title = t(key, descending ? 'Descending' : 'Ascending'); button.setAttribute('aria-label', button.title); }
function sortEntries(list) { return [...list].sort((a, b) => { if (searchQuery && (Number(a.searchScore) || Number(b.searchScore))) { const score = (Number(b.searchScore) || 0) - (Number(a.searchScore) || 0); if (score) return score; } if (a.isDir !== b.isDir) return a.isDir ? -1 : 1; let result = session.sortBy === 'mtime' ? Number(a.mtime) - Number(b.mtime) : session.sortBy === 'size' ? Number(a.size) - Number(b.size) : a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: 'base' }); return session.sortDirection === 'desc' ? -result : result; }); }
async function loadDirectory(path) {
  const token = ++directoryToken;
  const viewport = document.querySelector('.content'); const scrollTop = pendingScrollTop ?? (viewport?.scrollTop || 0); pendingScrollTop = undefined;
  setStatus(t('loading', '加载中…')); $('empty').hidden = true; $('entries').setAttribute('aria-busy', 'true');
  try { const result = await call('list_dir', { path, offset: session.pageOffset, limit: PAGE_SIZE, sortBy: session.sortBy, sortDir: session.sortDirection, showHidden: session.showHidden }); if (token !== directoryToken || session.currentPath !== path) return; session.entries = sortEntries(result.entries || []); const visiblePaths = new Set(session.entries.map((item) => item.path)); session.selectedPaths = new Set([...session.selectedPaths].filter((selectedPath) => visiblePaths.has(selectedPath))); const previewPending = Boolean(pendingSelectionPath && visiblePaths.has(pendingSelectionPath)); if (previewPending) { session.selectedPaths = new Set([pendingSelectionPath]); pendingSelectionPath = undefined; } const previewPath = editorPaths(); if (previewPath && !visiblePaths.has(previewPath) && !preview.hasDirtyEditor()) resetPreviewNow(); session.lastSelectedIndex = session.selectedPaths.size ? session.entries.findIndex((item) => session.selectedPaths.has(item.path)) : -1; session.pageHasMore = Boolean(result.hasMore); renderEntries(); viewport.scrollTop = scrollTop; updatePager(); renderStatusBar(); if (previewPending) renderSelection(); setStatus(''); if (watchedPath !== path) { if (watchedPath) call('watch_stop', { path: watchedPath }).catch(() => {}); await call('watch_start', { path }); if (token !== directoryToken || session.currentPath !== path) { if (session.currentPath !== path) call('watch_stop', { path }).catch(() => {}); return; } watchedPath = path; } await applyPendingFollow(path); } catch (error) { if (token !== directoryToken || session.currentPath !== path) return; session.entries = []; session.selectedPaths.clear(); session.lastSelectedIndex = -1; renderEntries(); renderStatusBar(); setStatus(error.message, 'error'); $('retry').hidden = false; } finally { if (token === directoryToken) $('entries').removeAttribute('aria-busy'); }
}
async function applyPendingFollow(path) {
  const target = pendingFollowPath;
  if (!session.followChanges || !target || editorState()?.dirty || parentAndName(target).parent !== path) return;
  const wait = followAppliedAt ? Math.max(0, 900 - (Date.now() - followAppliedAt)) : 0;
  if (wait) { clearTimeout(followApplyTimer); followApplyTimer = setTimeout(() => { followApplyTimer = undefined; applyPendingFollow(path); }, wait); return; }
  const item = session.entries.find((entry) => entry.path === target);
  if (!item || item.isDir) return;
  pendingFollowPath = undefined;
  followAppliedAt = Date.now(); session.selectedPaths = new Set([target]); session.lastSelectedIndex = session.entries.indexOf(item); renderSelection(); document.querySelector(`[data-path="${CSS.escape(target)}"]`)?.scrollIntoView({ block: 'nearest' });
}
let filesHomeView;
function renderHomeWelcome() {
  session.currentPath = '';
  session.entries = [];
  session.selectedPaths.clear();
  session.lastSelectedIndex = -1;
  renderBreadcrumb('');
  $('item-count').textContent = '';
  const box = $('entries');
  box.replaceChildren();
  box.classList.remove('grid');
  $('empty').hidden = true;
  renderStatusBar();
  if (!filesHomeView) {
    filesHomeView = createFilesHomeView({
      container: $('entries'),
      onNavigate: (path) => navigate(path),
      onSearch: () => openSearchDialog(),
      t,
    });
  }
  filesHomeView.render();
  setStatus(t('ready', '就绪'));
}
function renderEntries() {
  if (!session.currentPath) {
    renderHomeWelcome();
    return;
  }
  const box = $('entries'); box.replaceChildren(); box.classList.toggle('grid', session.viewMode === 'grid'); $('empty').hidden = session.entries.length > 0; if (!session.entries.length) $('empty').textContent = searchQuery ? t('noSearchResults', '没有匹配的文件') : t('emptyFolder', '此文件夹为空');
  if (session.viewMode === 'list' && session.entries.length) {
    const head = document.createElement('div'); head.className = 'list-head'; head.setAttribute('aria-hidden', 'true');
    for (const label of ['', t('name', '名称'), t('modified', '修改时间'), t('size', '大小')]) head.append(Object.assign(document.createElement('span'), { textContent: label }));
    box.append(head);
  }
  for (const [index, item] of session.entries.entries()) {
    const row = document.createElement('div'); row.className = 'entry'; row.setAttribute('role', 'option'); row.dataset.path = item.path; row.dataset.index = String(index); row.tabIndex = 0; row.setAttribute('aria-selected', session.selectedPaths.has(item.path)); if (session.selectedPaths.has(item.path)) row.classList.add('selected'); const change = changedPaths.get(item.path); if (change) { row.classList.add('changed'); row.style.setProperty('--change-heat', String(Math.min(1, 0.35 + change.count * 0.08))); }
    const icon = document.createElement('span'); icon.className = `entry-icon${item.isDir ? '' : ` entry-icon-${item.kind || 'text'}`}`; icon.append(entryIcon(item)); icon.setAttribute('aria-hidden', 'true'); const name = document.createElement('button'); name.className = 'entry-name'; name.textContent = item.name; name.title = item.name; name.draggable = true; if (change) { name.dataset.changed = change.count > 1 ? `改·${change.count}` : '改'; name.title = `${item.name} · ${name.dataset.changed}`; } const badge = document.createElement('span'); badge.className = `project-badge${item.projectBadge ? ` proj-${item.projectBadge}` : ''}`; badge.textContent = item.projectBadge ? String(item.projectBadge).toUpperCase() : ''; badge.hidden = !item.projectBadge; if (item.projectBadge) name.append(badge); const modified = document.createElement('span'); modified.className = 'entry-meta modified'; modified.textContent = Number(item.mtime) ? new Date(item.mtime).toLocaleString() : ''; const size = document.createElement('span'); size.className = 'entry-meta size'; size.textContent = item.isDir ? t('folder', '文件夹') : formatSize(item.size); const source = document.createElement(globalSearchMode ? 'button' : 'span'); source.className = 'entry-source entry-meta'; source.textContent = globalSearchMode ? (item.dirHint || parentAndName(item.path).parent) : ''; source.title = item.path; if (globalSearchMode) { source.type = 'button'; source.setAttribute('aria-label', `${t('path', '路径')}: ${item.path}`); source.onclick = (event) => { event.stopPropagation(); pendingSelectionPath = item.path; navigate(parentAndName(item.path).parent); }; } else source.hidden = true;
    row.append(icon, name, modified, size, source); row.onclick = (event) => selectEntry(index, event); row.ondblclick = () => openItemFromDoubleClick(item); row.oncontextmenu = (event) => { event.preventDefault(); const show = () => { if (!session.selectedPaths.has(item.path)) { session.selectedPaths.clear(); session.selectedPaths.add(item.path); session.lastSelectedIndex = index; renderSelection(); } showContextMenu(event.clientX, event.clientY, item); }; if (editorState()?.dirty) guardDirty(show); else show(); }; row.ondragstart = (event) => { const paths = [...session.selectedPaths.size ? session.selectedPaths : [item.path]]; const uris = paths.map(fileUri); event.dataTransfer.setData('text/plain', JSON.stringify(paths)); event.dataTransfer.setData('text/uri-list', uris.join('\r\n')); event.dataTransfer.setData('text/html', uris.map((uri) => `<a href="${htmlEscape(uri)}">${htmlEscape(uri)}</a>`).join('\n')); event.dataTransfer.effectAllowed = 'copyMove'; }; if (item.isDir) { row.ondragover = (event) => { event.preventDefault(); row.classList.add('drop-target'); event.dataTransfer.dropEffect = event.dataTransfer.types.includes('Files') ? 'copy' : 'move'; }; row.ondragleave = () => row.classList.remove('drop-target'); row.ondrop = async (event) => { event.preventDefault(); row.classList.remove('drop-target'); const raw = event.dataTransfer.getData('text/plain'); const urls = event.dataTransfer.getData('text/uri-list'); try { if (event.dataTransfer.files.length) await importFileList(event.dataTransfer.files, item.path); else if (raw && raw.startsWith('[')) await moveDroppedPaths(raw, item.path); else if (urls) await copyDroppedUris(urls, item.path); } catch (error) { setStatus(error.message, 'error'); } }; } box.append(row);
  }
  applyQuickFilter();
}
function applyQuickFilter() {
  if (!session.currentPath) return;
  const query = ($('quick-filter')?.value || '').trim().toLocaleLowerCase();
  let visible = 0;
  document.querySelectorAll('#entries .entry').forEach((row) => {
    const item = session.entries[Number(row.dataset.index)];
    row.hidden = Boolean(query && !item?.name.toLocaleLowerCase().includes(query));
    if (!row.hidden) visible++;
  });
  $('item-count').textContent = `${visible}${visible !== session.entries.length ? ` / ${session.entries.length}` : ''} ${t('items', '个项目')}`;
  $('empty').hidden = visible > 0;
  if (!visible) $('empty').textContent = query ? t('noFilterResults', '当前目录没有匹配项目') : searchQuery ? t('noSearchResults', '没有匹配的文件') : t('emptyFolder', '此文件夹为空');
}
function openQuickFilter() {
  if (!session.currentPath) { openSearchDialog(); return; }
  $('quick-filter-wrap')?.classList.add('open');
  $('quick-filter-toggle')?.setAttribute('aria-expanded', 'true');
  const input = $('quick-filter');
  input.focus();
  input.select();
}
function closeQuickFilter() {
  $('quick-filter-wrap')?.classList.remove('open');
  $('quick-filter-toggle')?.setAttribute('aria-expanded', 'false');
}
function renderSelection() { document.querySelectorAll('.entry').forEach((row) => { const selected = session.selectedPaths.has(row.dataset.path); row.classList.toggle('selected', selected); row.setAttribute('aria-selected', String(selected)); }); if (session.lastSelectedIndex < 0 || !session.selectedPaths.has(session.entries[session.lastSelectedIndex]?.path)) session.lastSelectedIndex = session.selectedPaths.size ? session.entries.findIndex((item) => session.selectedPaths.has(item.path)) : -1; updateActions(); renderStatusBar(); renderPreviewSelection(); }
async function selectEntry(index, event = {}) {
  clearTimeout(followApplyTimer); followApplyTimer = undefined;
  pendingFollowPath = undefined;
  if (session.followChanges) stopFollowOnManual();
  if (editorState()?.dirty) { const previous = new Set(session.selectedPaths); await guardDirty(() => {}); if (editorState()?.dirty) { session.selectedPaths = previous; return; } }
  if (!session.entries[index]) return; session.select(index, event); renderSelection();
}
function updateActions() { for (const id of ['open', 'editor', 'reveal', 'rename', 'copy', 'move', 'trash']) { const button = $(id); if (button) button.disabled = session.selectedPaths.size === 0; } const archive = $('create-archive'); if (archive) archive.disabled = session.selectedPaths.size === 0 || !session.currentPath; }
function selectedItems() { return session.entries.filter((item) => session.selectedPaths.has(item.path)); }
function formatSize(size = 0) { if (size < 1024) return `${size} B`; if (size < 1024 ** 2) return `${(size / 1024).toFixed(1)} KB`; if (size < 1024 ** 3) return `${(size / 1024 ** 2).toFixed(1)} MB`; return `${(size / 1024 ** 3).toFixed(1)} GB`; }
const diskUsageDialog = createDiskUsage({ $, call, t, entryIcon, formatSize, parentAndName, currentPath: () => session.currentPath });
const showDiskUsage = (path = session.currentPath) => diskUsageDialog.show(path);

const link = {};
const ops = createFileOperations({ $, call, t, session, setStatus, toast, updateProgress, setOperationCancelable, openModal, loadDirectory, renderSelection, renderStatusBar, selectedItems, navigate, parentAndName, kindFromName, markSelfOpened, link });
const preview = createPreviewControllers({ $, call, t, session, setStatus, toast, formatSize, parentAndName, pathParts, entryIcon, iconElement, iconAction, isTextItem: isTextFile, TEXT_KINDS, isHostConnected: () => nativeClient.connected, selectedItems, renderSelection, loadDirectory, remember, revealPath: ops.revealPath, link });
Object.assign(link, {
  guardDirty: preview.guardDirty,
  hasDirtyEditor: preview.hasDirtyEditor,
  getEditorState: preview.getEditorState,
  migrateEditorViewPath: preview.migrateEditorViewPath,
  copyPathSelectedFor: ops.copyPathSelectedFor,
  openItem: ops.openItem,
  extractArchive: ops.extractArchive,
  setPendingSelectionPath: (value) => { pendingSelectionPath = value; },
  migrateChangedPath: (oldPath, newPath) => { const migrate = (path) => path === oldPath || path.startsWith(oldPath + '/') ? newPath + path.slice(oldPath.length) : path; const moved = []; for (const [path, value] of changedPaths) if (path === oldPath || path.startsWith(oldPath + '/')) moved.push([migrate(path), value]); for (const [path] of changedPaths) if (path === oldPath || path.startsWith(oldPath + '/')) changedPaths.delete(path); moved.forEach(([path, value]) => changedPaths.set(path, value)); },
});
// Recently-opened tracker (bounded): preserves the "remember on preview" intent
// without resurrecting the removed recent list.
const recentOpenedPaths = [];
function remember(path) { const index = recentOpenedPaths.indexOf(path); if (index >= 0) recentOpenedPaths.splice(index, 1); recentOpenedPaths.unshift(path); if (recentOpenedPaths.length > 50) recentOpenedPaths.pop(); }
// Atomic capability aliases (kept call-site compatible).
const openItem = ops.openItem;
const openItemFromDoubleClick = ops.openItemFromDoubleClick;
const revealPath = ops.revealPath;
const revealSelected = ops.revealSelected;
const openEditorSelected = ops.openEditorSelected;
const copyPathSelected = ops.copyPathSelected;
const createEntry = ops.createEntry;
const importFileList = ops.importFileList;
const importDroppedImageUrls = ops.importDroppedImageUrls;
const importImageIntoEditor = ops.importImageIntoEditor;
const copyImageIntoEditor = ops.copyImageIntoEditor;
const moveDroppedPaths = ops.moveDroppedPaths;
const copyDroppedUris = ops.copyDroppedUris;
const renameSelected = ops.renameSelected;
const transfer = ops.transfer;
const duplicateSelected = ops.duplicateSelected;
const pasteClipboard = ops.pasteClipboard;
const setClipboard = ops.setClipboard;
const clearFileClipboard = ops.clearFileClipboard;
const trashSelected = ops.trashSelected;
const extractArchive = ops.extractArchive;
const createZip = ops.createZip;
const migrateTrackedPath = ops.migrateTrackedPath;
const migrateBatchPaths = ops.migrateBatchPaths;
const rememberFailedOperation = ops.rememberFailedOperation;
const retryFailedOperation = ops.retryFailedOperation;
const renderPreviewSelection = preview.renderPreviewSelection;
const resetPreview = preview.resetPreview;
const resetPreviewNow = preview.resetPreviewNow;
const guardDirty = preview.guardDirty;
const refreshEditorAfterExternalChange = preview.refreshEditorAfterExternalChange;
const showImageLightbox = preview.showImageLightbox;
const editorState = preview.getEditorState;
const editorPaths = () => editorState()?.path || preview.getImageEditorState()?.path;


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

function normalizePathInput(raw) { let value = String(raw || '').trim(); const quoted = value.match(/^("|')(.*)\1$/); if (quoted) value = quoted[2]; return value.replace(/\\([ \\])/g, '$1'); }
function parentAndName(path) { const separator = path.lastIndexOf('/'); return { parent: separator > 0 ? path.slice(0, separator) : '/', name: path.slice(separator + 1) }; }
async function navigateFromInput(raw) { const value = normalizePathInput(raw); if (!value) return; if (editorState()?.dirty) { guardDirty(() => navigateFromInput(value)); return; } const token = ++pathToken; try { const result = await call('stat', { path: value }, crypto.randomUUID()); if (token !== pathToken) return; if (result?.found && !result.isDir && result.path) { pendingSelectionPath = result.path; const parent = parentAndName(result.path).parent; if (parent === session.currentPath) { const index = session.entries.findIndex((item) => item.path === result.path); if (index >= 0) { session.selectedPaths = new Set([result.path]); session.lastSelectedIndex = index; pendingSelectionPath = undefined; renderSelection(); } else loadDirectory(parent); } else navigate(parent); return; } } catch { if (token !== pathToken) return; /* fall back to directory navigation for unresolved paths */ } if (token === pathToken) navigate(value); }
function openModal({ title, message = '', label = '', value = '', submit = () => {} }) { const dialog = $('modal'); $('modal-title').textContent = title; $('modal-message').textContent = message; $('modal-field-label').textContent = label; $('modal-input').value = value; $('modal-input').hidden = !label; $('modal-field-label').hidden = !label; dialog.returnValue = 'cancel'; const form = $('modal-form'); form.onsubmit = (event) => { event.preventDefault(); const next = $('modal-input').value.trim(); if (label && !next) return; dialog.close('default'); submit(next); }; $('modal-cancel').onclick = () => dialog.close('cancel'); dialog.showModal(); if (label) { $('modal-input').focus(); $('modal-input').select(); } }
function showContextMenu(x, y, item) { const menu = $('context-menu'); menu.replaceChildren(); const actions = item ? [['open', 'open', () => openItem(item)], ['previewAction', 'preview', () => { const show = () => { session.selectedPaths = new Set([item.path]); renderSelection(); }; if (editorState()?.dirty) guardDirty(show); else show(); }], ...(item.isDir ? [['diskUsage', 'diskUsage', () => showDiskUsage(item.path)]] : []), ...(item.kind === 'archive' && /\.(zip|jar|tar|tgz|tbz2?|txz|tar\.(gz|bz2|xz|zst))$/i.test(item.name) ? [['extractArchive', 'extractArchive', () => extractArchive(item)]] : []), ['reveal', 'reveal', () => revealSelected()], ['copyPath', 'copyPath', () => copyPathSelected()], ['rename', 'rename', () => renameSelected()], ['copy', 'copy', () => transfer('copy')], ['duplicate', 'duplicate', () => duplicateSelected()], ...(item.isDir && ops.getClipboard() ? [['paste', 'paste', () => pasteClipboard(item.path)]] : []), ['move', 'move', () => transfer('move')], ['trash', 'trash', () => trashSelected()]] : [['newFolder', 'newFolder', () => createEntry('directory')], ['newFile', 'newFile', () => createEntry('file')], ...(ops.getClipboard() ? [['paste', 'paste', () => pasteClipboard()]] : [])];
  for (const [id, key, action] of actions) { const button = document.createElement('button'); button.dataset.action = id; button.setAttribute('role', 'menuitem'); button.textContent = t(key, key); button.onclick = () => { hideContextMenu(); action(); }; menu.append(button); }
  if (!item && session.currentPath) {
    const refresh = document.createElement('button'); refresh.dataset.action = 'refresh'; refresh.setAttribute('role', 'menuitem'); refresh.textContent = t('refresh', '刷新'); refresh.onclick = () => { hideContextMenu(); searchQuery ? search(searchQuery) : loadDirectory(session.currentPath); };
    const importButton = document.createElement('button'); importButton.dataset.action = 'importFiles'; importButton.setAttribute('role', 'menuitem'); importButton.textContent = t('importFiles', '导入文件'); importButton.onclick = () => { hideContextMenu(); const input = document.createElement('input'); input.type = 'file'; input.multiple = true; input.onchange = () => importFileList(input.files); input.click(); };
    menu.append(refresh, importButton);
  }
  if (item && session.selectedPaths.size && session.currentPath) {
    const archiveButton = document.createElement('button'); archiveButton.dataset.action = 'createArchive'; archiveButton.setAttribute('role', 'menuitem'); archiveButton.textContent = t('createArchive', '创建 ZIP'); archiveButton.onclick = () => { hideContextMenu(); createZip(); };
    menu.append(archiveButton);
  }
  if (item && !item.isDir && item.kind !== 'image') {
    const fileButton = document.createElement('button'); fileButton.dataset.action = 'copyFile'; fileButton.setAttribute('role', 'menuitem'); fileButton.textContent = t('copyFile', '复制文件'); fileButton.onclick = () => { hideContextMenu(); copyFileSelected(); }; menu.append(fileButton);
  }
  if (item && item.kind === 'image') {
    const imageButton = document.createElement('button'); imageButton.dataset.action = 'copyImage'; imageButton.setAttribute('role', 'menuitem'); imageButton.textContent = t('copyImage', '复制图片'); imageButton.setAttribute('aria-label', imageButton.textContent);
    imageButton.onclick = async () => { hideContextMenu(); try { await call('copy_image', { path: item.path }); toast(t('imageCopied', '图片已复制')); } catch (error) { setStatus(error.message, 'error'); } };
    menu.append(imageButton);
    if (!item.isDir) {
      const editorButton = document.createElement('button'); editorButton.dataset.action = 'editor'; editorButton.setAttribute('role', 'menuitem'); editorButton.textContent = t('openEditor', '在编辑器打开'); editorButton.setAttribute('aria-label', editorButton.textContent);
      editorButton.onclick = async () => { hideContextMenu(); try { await call('editor', { path: item.path }); toast(t('openedEditor', '已在编辑器打开')); } catch (error) { setStatus(error.message, 'error'); } };
      const copyFileButton = document.createElement('button'); copyFileButton.dataset.action = 'copyFile'; copyFileButton.setAttribute('role', 'menuitem'); copyFileButton.textContent = t('copyFile', '复制文件'); copyFileButton.setAttribute('aria-label', copyFileButton.textContent);
      copyFileButton.onclick = async () => { hideContextMenu(); try { await call('copy_paths', { paths: [item.path] }); toast(t('fileCopied', '文件已复制')); } catch (error) { setStatus(error.message, 'error'); } };
      menu.append(editorButton, copyFileButton);
    }
  }
  menu.hidden = false; menu.style.left = `${Math.min(x, innerWidth - 190)}px`; menu.style.top = `${Math.min(y, innerHeight - menu.offsetHeight - 10)}px`;
}
function hideContextMenu() { $('context-menu').hidden = true; }
function updateSearchScopeButton() { const button = $('scope-toggle'); if (!button) return; button.replaceChildren(iconElement(globalSearchMode ? 'globe' : 'target'), document.createTextNode(` ${globalSearchMode ? t('globalSearch', '全机') : t('currentDirectorySearch', '当前目录')}`)); button.title = globalSearchMode ? t('globalSearch', '全机搜索') : t('currentDirectorySearch', '当前目录搜索'); button.setAttribute('aria-pressed', String(globalSearchMode)); }
function toggleSearchScope() { globalSearchMode = !globalSearchMode; updateSearchScopeButton(); $('search').focus(); $('search').select(); setStatus(t(globalSearchMode ? 'globalSearch' : 'search', globalSearchMode ? '全机搜索' : '当前目录搜索')); if (searchQuery) { session.pageOffset = 0; search(searchQuery); } }
function openSearchDialog() {
  const dialog = $('search-dialog');
  $('quick-filter').value = '';
  closeQuickFilter();
  applyQuickFilter();
  if (!dialog.open) dialog.showModal();
  queueMicrotask(() => { $('search').focus(); $('search').select(); });
}
function cancelActiveSearches() { searchTruncated = false; if (activeSearchId) call('search_cancel', { requestId: activeSearchId }).catch(() => {}); activeSearchIds.forEach((requestId) => call('search_cancel', { requestId }).catch(() => {})); activeSearchIds.clear(); activeSearchId = undefined; }
async function search(query) { const token = ++searchToken; cancelActiveSearches(); if (globalSearchMode) return searchGlobal(query, token); const content = /^content:\s*/i.test(query); const normalizedQuery = query.replace(/^content:\s*/i, '').trim(); const requestId = crypto.randomUUID(); activeSearchId = requestId; setStatus(t('loading', '加载中…')); try { const result = await call('search', { path: session.currentPath, query: normalizedQuery, offset: session.pageOffset, limit: PAGE_SIZE, recursive: recursiveSearch, content, showHidden: session.showHidden }, requestId); if (token !== searchToken) return; searchTruncated = Boolean(result?.truncated); session.entries = sortEntries(result.entries || []); const visiblePaths = new Set(session.entries.map((item) => item.path)); session.selectedPaths = new Set([...session.selectedPaths].filter((selectedPath) => visiblePaths.has(selectedPath))); session.lastSelectedIndex = session.selectedPaths.size ? session.entries.findIndex((item) => session.selectedPaths.has(item.path)) : -1; session.pageHasMore = Boolean(result.hasMore); renderEntries(); renderSelection(); updatePager(); setStatus(`${session.entries.length} ${t('searchResults', '个搜索结果')}${result?.contentUnavailable ? ` · ${t('contentSearchUnavailable', 'PDF 文本搜索不可用')}` : ''}`); } catch (error) { if (token === searchToken && error.message !== 'search cancelled') setStatus(error.message, 'error'); } finally { if (activeSearchId === requestId) activeSearchId = undefined; } }
async function searchGlobal(query, token) { const content = /^content:\s*/i.test(query); const normalizedQuery = query.replace(/^content:\s*/i, '').trim(); const requests = rootPaths.map((path) => ({ path, requestId: crypto.randomUUID() })); requests.forEach(({ requestId }) => activeSearchIds.add(requestId)); activeSearchId = requests[0]?.requestId; setStatus(t('loading', '加载中…')); try { const results = await Promise.all(requests.map(({ path, requestId }) => call('search', { path, query: normalizedQuery, offset: session.pageOffset, limit: PAGE_SIZE * 4, recursive: true, content, showHidden: session.showHidden }, requestId))); if (token !== searchToken) return; searchTruncated = results.some((result) => Boolean(result?.truncated)); const seen = new Set(); const merged = sortEntries(results.flatMap((result) => result.entries || []).filter((item) => !seen.has(item.path) && seen.add(item.path))); session.entries = merged.slice(0, PAGE_SIZE); session.selectedPaths.clear(); session.lastSelectedIndex = -1; session.pageHasMore = results.some((result) => Boolean(result.hasMore)) || merged.length > PAGE_SIZE; renderEntries(); renderSelection(); updatePager(); setStatus(`${session.entries.length}${session.pageHasMore ? '+' : ''} ${t('searchResults', '个搜索结果')}${results.some((result) => result?.contentUnavailable) ? ` · ${t('contentSearchUnavailable', 'PDF 文本搜索不可用')}` : ''}`); } catch (error) { if (token === searchToken && error.message !== 'search cancelled') setStatus(error.message, 'error'); } finally { requests.forEach(({ requestId }) => { activeSearchIds.delete(requestId); call('search_cancel', { requestId }).catch(() => {}); }); activeSearchId = undefined; } }
async function init() { try { setStatus(t('connecting', '正在连接本地文件系统…')); const versionRequest = call('version'); await loadUiState(); const version = await versionRequest; if (version?.protocolVersion !== 1) throw new Error(t('nativeHostIncompatible', 'Native Host 协议不兼容')); const roots = await call('roots'); rootPaths = roots.map((root) => root.path); bindAppMenu(roots); const hasHarness = new URLSearchParams(location.search).has('ui-harness') || new URLSearchParams(location.search).has('self-test'); if (hasHarness && roots[0]) navigate(roots[0].path, false); else renderHomeWelcome(); } catch (error) { setStatus(error.message || t('hostConnectionFailed', 'Native Host 连接失败'), 'error'); if ($('retry')) $('retry').hidden = false; } }
function handleKeydown(event) { if (event.key === 'Escape') { if (ops.isBusy()) { ops.cancelActive(); const uploadId = ops.activeUploadId(); if (uploadId) call('import_cancel', { uploadId }).catch(() => {}); } if (activeSearchId) { call('search_cancel', { requestId: activeSearchId }).catch(() => {}); activeSearchId = undefined; searchToken++; setStatus(t('searchCancelled', '搜索已取消')); } hideContextMenu(); if ($('modal').open) $('modal').close('cancel'); if ($('dirty-modal').open) $('dirty-modal').close('cancel'); if ($('conflict-modal').open) $('conflict-modal').close('cancel'); return; } const modifier = event.metaKey || event.ctrlKey; const shortcut = event.key.toLowerCase(); if (modifier && shortcut === 'b') { event.preventDefault(); toggleSidebar(); return; } if (modifier && event.shiftKey && shortcut === 'g') { event.preventDefault(); let viewMode = session.viewMode; viewMode = viewMode === 'grid' ? 'list' : 'grid'; session.viewMode = viewMode; $('list-view').setAttribute('aria-pressed', String(session.viewMode === 'list')); $('grid-view').setAttribute('aria-pressed', String(session.viewMode === 'grid')); renderEntries(); syncGridControls(); storageSet('natives-view-mode', session.viewMode).catch(() => {}); return; } if (modifier && event.shiftKey && shortcut === 'r') { event.preventDefault(); if (session.currentPath) searchQuery ? search(searchQuery) : loadDirectory(session.currentPath); return; } const hasTextSelection = Boolean(window.getSelection() && !window.getSelection().isCollapsed && window.getSelection().toString().length > 0); if (event.target.matches('input,textarea,select,[contenteditable="true"]') || (modifier && shortcut === 'c' && hasTextSelection) || (hasTextSelection && ['c', 'x', 'a'].includes(shortcut))) return; if (modifier && shortcut === 'a') { event.preventDefault(); session.selectedPaths = new Set(session.entries.map((item) => item.path)); renderSelection(); return; } if (modifier && shortcut === 'c') { event.preventDefault(); setClipboard('copy'); return; } if (modifier && shortcut === 'x') { event.preventDefault(); setClipboard('move'); return; } if (modifier && shortcut === 'v') { event.preventDefault(); pasteClipboard(); return; } if (modifier && shortcut === 'd') { event.preventDefault(); duplicateSelected(); return; } const row = event.target.closest('.entry'); if (!row) return; const index = Number(row.dataset.index); if (event.key === 'ContextMenu' || (event.key === 'F10' && event.shiftKey)) { event.preventDefault(); row.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, clientX: row.getBoundingClientRect().left + 12, clientY: row.getBoundingClientRect().bottom })); } else if (event.key === 'Enter') { event.preventDefault(); openItemFromDoubleClick(session.entries[index]); } else if (event.key === ' ') { event.preventDefault(); selectEntry(index, event); } else if (event.key === 'F2') { event.preventDefault(); renameSelected(); } else if (event.key === 'Delete' || event.key === 'Backspace') { event.preventDefault(); trashSelected(); } else if (event.key === 'ArrowDown' || event.key === 'ArrowUp') { event.preventDefault(); const next = Math.max(0, Math.min(session.entries.length - 1, index + (event.key === 'ArrowDown' ? 1 : -1))); selectEntry(next, event); document.querySelector(`[data-index="${next}"]`)?.focus(); } }

$('back').onclick = () => { if (session.historyIndex > 0) { session.historyIndex--; navigate(session.history[session.historyIndex], false); } }; $('forward').onclick = () => { if (session.historyIndex < session.history.length - 1) { session.historyIndex++; navigate(session.history[session.historyIndex], false); } }; $('up').onclick = () => session.currentPath && navigate(session.currentPath.split('/').slice(0, -1).join('/') || '/'); $('refresh').onclick = () => { if (!nativeClient.connected) { disconnectNative(); init(); } else if (session.currentPath) { searchQuery ? search(searchQuery) : loadDirectory(session.currentPath); } else { init(); } }; if ($('retry')) $('retry').onclick = () => { disconnectNative(); init(); }; $('previous-page').onclick = () => { if (session.pageOffset >= PAGE_SIZE) { session.pageOffset -= PAGE_SIZE; searchQuery ? search(searchQuery) : loadDirectory(session.currentPath); } }; $('next-page').onclick = () => { if (session.pageHasMore) { session.pageOffset += PAGE_SIZE; searchQuery ? search(searchQuery) : loadDirectory(session.currentPath); } }; if ($('path-form')) $('path-form').onsubmit = (event) => { event.preventDefault(); navigateFromInput($('path-input')?.value.trim()); }; $('search').oninput = (event) => { searchQuery = event.target.value.trim(); session.pageOffset = 0; session.selectedPaths.clear(); session.lastSelectedIndex = -1; renderSelection(); clearTimeout(searchTimer); searchTimer = setTimeout(() => { if (searchQuery) search(searchQuery); else { searchToken++; cancelActiveSearches(); loadDirectory(session.currentPath); } }, 180); };
document.querySelectorAll('.sort-tab').forEach((tab) => {
  tab.onclick = () => {
    const sortBy = tab.dataset.sort;
    if (session.sortBy === sortBy) {
      session.sortDirection = session.sortDirection === 'asc' ? 'desc' : 'asc';
      updateSortDirection();
      storageSet('natives-sort-direction', session.sortDirection).catch(() => {});
    } else {
      session.sortBy = sortBy;
      syncSortTabs();
      storageSet('natives-sort-by', session.sortBy).catch(() => {});
    }
    session.pageOffset = 0;
    searchQuery ? search(searchQuery) : loadDirectory(session.currentPath);
  };
});
if (false) $('sort').onchange = (event) => { let sortBy; if (false) { sortBy = event.target.value; session.sortBy = sortBy; session.entries = sortEntries(session.entries); storageSet('natives-sort-by', sortBy); renderEntries(); return; } session.sortBy = event.target.value; syncSortTabs(); session.pageOffset = 0; searchQuery ? search(searchQuery) : loadDirectory(session.currentPath); }; $('new-menu').onclick = () => { const popover = $('new-popover'); popover.hidden = !popover.hidden; if (!popover.hidden) { const anchor = $('new-menu').getBoundingClientRect(); const width = popover.offsetWidth || 180; const height = popover.offsetHeight || 180; const left = Math.max(8, Math.min(anchor.right - width, window.innerWidth - width - 8)); const top = anchor.bottom + height + 6 <= window.innerHeight ? anchor.bottom + 6 : Math.max(8, anchor.top - height - 6); popover.style.left = `${left}px`; popover.style.top = `${top}px`; } }; $('new-folder').onclick = () => { $('new-popover').hidden = true; createEntry('directory'); }; $('new-file').onclick = () => { $('new-popover').hidden = true; createEntry('file'); }; $('import-files').onclick = () => { $('new-popover').hidden = true; const input = document.createElement('input'); input.type = 'file'; input.multiple = true; input.onchange = () => importFileList(input.files); input.click(); }; if ($('open')) $('open').onclick = () => selectedItems().length === 1 && openItem(selectedItems()[0]); if ($('reveal')) $('reveal').onclick = revealSelected; if ($('rename')) $('rename').onclick = renameSelected; if ($('copy')) $('copy').onclick = () => transfer('copy'); if ($('move')) $('move').onclick = () => transfer('move'); if ($('trash')) $('trash').onclick = trashSelected; $('list-view').onclick = () => { session.viewMode = 'list'; renderEntries(); $('list-view').setAttribute('aria-pressed', 'true'); $('grid-view').setAttribute('aria-pressed', 'false'); }; $('grid-view').onclick = () => { session.viewMode = 'grid'; renderEntries(); $('list-view').setAttribute('aria-pressed', 'false'); $('grid-view').setAttribute('aria-pressed', 'true'); }; $('close-preview').onclick = resetPreview;
$('sort-direction').onclick = () => { session.sortDirection = session.sortDirection === 'asc' ? 'desc' : 'asc'; if ($('sort')) $('sort').dataset.direction = session.sortDirection; updateSortDirection(); storageSet('natives-sort-direction', session.sortDirection).catch(() => {}); session.pageOffset = 0; searchQuery ? search(searchQuery) : loadDirectory(session.currentPath); };
if ($('create-archive')) $('create-archive').onclick = createZip;
$('clear-file-clipboard').onclick = clearFileClipboard;
$('cancel-operation').onclick = () => { if (ops.isBusy()) { ops.cancelActive(); const uploadId = ops.activeUploadId(); if (uploadId) call('import_cancel', { uploadId }).catch(() => {}); const batch = ops.getBatch?.(); if (batch?.requestId) call('batch_cancel', { requestId: batch.requestId }).catch(() => {}); } };
$('retry-operation').onclick = retryFailedOperation;
$('show-hidden').onchange = (event) => { session.showHidden = event.target.checked; session.pageOffset = 0; session.currentPath && (searchQuery ? search(searchQuery) : loadDirectory(session.currentPath)); }; $('recursive-search').onchange = (event) => { recursiveSearch = event.target.checked; session.pageOffset = 0; searchQuery ? search(searchQuery) : session.currentPath && loadDirectory(session.currentPath); };
$('follow-changes').onchange = (event) => { session.followChanges = event.target.checked; if (session.followChanges) { pendingFollowPath = undefined; clearTimeout(followApplyTimer); followApplyTimer = undefined; } storageSet('natives-follow-changes', session.followChanges); };
if ($('sort')) $('sort').addEventListener('change', () => storageSet('natives-sort-by', session.sortBy)); $('sort-direction').addEventListener('click', () => storageSet('natives-sort-direction', session.sortDirection)); $('show-hidden').addEventListener('change', () => storageSet('natives-show-hidden', session.showHidden)); $('list-view').addEventListener('click', () => storageSet('natives-view-mode', 'list')); $('grid-view').addEventListener('click', () => storageSet('natives-view-mode', 'grid'));
 $('entries').addEventListener('click', (event) => { if (event.target === $('entries')) { if (editorState()?.dirty) guardDirty(() => { session.selectedPaths.clear(); renderSelection(); }); else { session.selectedPaths.clear(); renderSelection(); } } }); $('entries').addEventListener('dragenter', (event) => { event.preventDefault(); entriesDragDepth++; $('entries').classList.add('drop-target'); }); $('entries').addEventListener('dragover', (event) => { event.preventDefault(); $('entries').classList.add('drop-target'); event.dataTransfer.dropEffect = event.dataTransfer.types.includes('text/plain') ? 'move' : 'copy'; }); $('entries').addEventListener('dragleave', () => { entriesDragDepth = Math.max(0, entriesDragDepth - 1); if (!entriesDragDepth) $('entries').classList.remove('drop-target'); }); $('entries').addEventListener('dragend', () => { entriesDragDepth = 0; $('entries').classList.remove('drop-target'); }); $('entries').addEventListener('drop', async (event) => { event.preventDefault(); entriesDragDepth = 0; $('entries').classList.remove('drop-target'); if (!session.currentPath) return; const raw = event.dataTransfer.getData('text/plain'); if (raw) await moveDroppedPaths(raw, session.currentPath); else if (event.dataTransfer.files.length) importFileList(event.dataTransfer.files); });
document.addEventListener('drop', (event) => { const row = event.target.closest('.entry'); const uris = event.dataTransfer?.getData('text/uri-list'); if (!row || !uris || !row.dataset.path || !session.entries.find((item) => item.path === row.dataset.path)?.isDir) return; event.preventDefault(); event.stopImmediatePropagation(); copyDroppedUris(uris, row.dataset.path).catch((error) => setStatus(error.message, 'error')); }, true);
document.addEventListener('drop', (event) => { const uris = event.dataTransfer?.getData('text/uri-list'); if (!uris || event.target.closest('.entry') || !event.target.closest('#entries') || !session.currentPath) return; event.preventDefault(); event.stopImmediatePropagation(); copyDroppedUris(uris, session.currentPath).catch((error) => setStatus(error.message, 'error')); }, true);
document.addEventListener('paste', (event) => { const editor = event.target.closest('.file-editor'); const file = [...(event.clipboardData?.files || [])].find((candidate) => /^image\//i.test(candidate.type || '')); if (!editor || !file) return; event.preventDefault(); importImageIntoEditor(file, editor).catch((error) => setStatus(error.message, 'error')); });
document.addEventListener('drop', async (event) => { const editor = event.target.closest('.file-editor'); const files = [...(event.dataTransfer?.files || [])].filter((candidate) => /^image\//i.test(candidate.type || '')).slice(0, 20); const html = event.dataTransfer?.getData('text/html') || ''; const src = html.match(/<img[^>]+src=["']([^"']+)["']/i)?.[1]; if (!editor || (!files.length && !src)) return; event.preventDefault(); if (files.length) { for (const file of files) if (!await importImageIntoEditor(file, editor)) break; } else if (/^file:/i.test(src)) { try { const url = new URL(src); if (url.hostname && url.hostname !== 'localhost') throw new Error(t('invalidPath', '路径无效')); copyImageIntoEditor(decodeURIComponent(url.pathname), editor); } catch (error) { setStatus(error.message, 'error'); } } else if (/^(https?:|data:image\/)/i.test(src)) fetch(src).then((response) => { if (!response.ok) throw new Error(t('imagePreviewUnavailable', '图片读取失败')); return response.blob(); }).then((blob) => importImageIntoEditor(new File([blob], `image-${Date.now()}.png`, { type: blob.type }), editor)).catch((error) => setStatus(error.message, 'error')); }, true);
new MutationObserver((records) => { for (const record of records) for (const node of record.addedNodes) { if (!(node instanceof HTMLElement) || !node.classList.contains('entry')) continue; const item = session.entries.find((entry) => entry.path === node.dataset.path); if (item?.match) { const count = Number(item.matchCount) > 1 ? ` · ${item.matchCount} matches` : ''; const lines = Array.isArray(item.matchLines) ? item.matchLines.join(' | ') : item.match; node.title = `${lines}${count}`; node.setAttribute('aria-label', `${item.name}: ${lines}${count}`); const excerpt = document.createElement('span'); excerpt.className = 'entry-match'; excerpt.textContent = `${item.match}${count}`; node.append(excerpt); } } }).observe($('entries'), { childList: true });
new MutationObserver((records) => { for (const record of records) for (const node of record.addedNodes) { if (!(node instanceof HTMLElement) || !node.classList.contains('entry-match') || node.dataset.highlighted) continue; const needle = searchQuery.replace(/^content:\s*/i, '').trim(); if (!needle) continue; const text = node.textContent || ''; const fragment = document.createDocumentFragment(); let cursor = 0; const lower = text.toLowerCase(); const query = needle.toLowerCase(); while (cursor < text.length) { const index = lower.indexOf(query, cursor); if (index < 0) { fragment.append(document.createTextNode(text.slice(cursor))); break; } fragment.append(document.createTextNode(text.slice(cursor, index))); const mark = document.createElement('mark'); mark.textContent = text.slice(index, index + needle.length); fragment.append(mark); cursor = index + needle.length; } node.replaceChildren(fragment); node.dataset.highlighted = 'true'; } }).observe($('entries'), { childList: true });
document.addEventListener('click', (event) => { const excerpt = event.target.closest('.entry-match'); if (!excerpt) return; const row = excerpt.closest('.entry'); const item = session.entries.find((entry) => entry.path === row?.dataset.path); if (!item?.matchLines?.length) return; event.stopPropagation(); const expanded = excerpt.dataset.expanded === 'true'; excerpt.dataset.expanded = String(!expanded); excerpt.setAttribute('aria-expanded', String(!expanded)); const first = String(item.matchLines[0]); const separator = first.indexOf(':'); const prefix = separator >= 0 ? `${first.slice(0, separator + 1)} ` : ''; excerpt.textContent = expanded ? `${prefix}${item.match}${Number(item.matchCount) > 1 ? ` · ${item.matchCount} matches` : ''}` : item.matchLines.join(' | '); excerpt.title = expanded ? String(item.match) : item.matchLines.join(' | '); excerpt.dataset.lineNumbered = 'true'; });
document.addEventListener('keydown', (event) => { const excerpt = event.target.closest?.('.entry-match'); if (!excerpt || !['Enter', ' '].includes(event.key)) return; event.preventDefault(); excerpt.click(); if (event.key === 'Enter') excerpt.dispatchEvent(new MouseEvent('dblclick', { bubbles: true })); });
new MutationObserver((records) => { for (const record of records) for (const node of record.addedNodes) { if (!(node instanceof HTMLElement) || !node.classList.contains('entry-match')) continue; node.tabIndex = 0; node.setAttribute('role', 'button'); node.setAttribute('aria-expanded', 'false'); } }).observe($('entries'), { childList: true, subtree: true });
new MutationObserver((records) => { for (const record of records) for (const node of record.addedNodes) { if (!(node instanceof HTMLElement) || !node.classList.contains('entry-match') || node.dataset.lineNumbered) continue; const row = node.closest('.entry'); const item = session.entries.find((entry) => entry.path === row?.dataset.path); if (!item?.matchLines?.length) continue; const first = String(item.matchLines[0]); const separator = first.indexOf(':'); if (separator >= 0) node.prepend(document.createTextNode(`${first.slice(0, separator + 1)} `)); node.title = item.matchLines.join(' | '); node.dataset.lineNumbered = 'true'; } }).observe($('entries'), { childList: true, subtree: true });
document.addEventListener('dblclick', async (event) => { const excerpt = event.target.closest('.entry-match'); if (!excerpt) return; const row = excerpt.closest('.entry'); const item = session.entries.find((entry) => entry.path === row?.dataset.path); const line = Number(String(item?.matchLines?.[0] || '').match(/^\d+/)?.[0]); if (!item || !line) return; event.preventDefault(); session.selectedPaths = new Set([item.path]); renderSelection(); await renderPreviewSelection(); const editor = $('preview-body').querySelector('.file-editor'); if (!editor) return; const lines = editor.value.split(/\n/); const offset = lines.slice(0, line - 1).reduce((total, value) => total + value.length + 1, 0); editor.focus(); editor.setSelectionRange(offset, offset); });
document.addEventListener('dblclick', (event) => { if (!event.target.closest('.entry-match')) return; setTimeout(() => { const editor = $('preview-body').querySelector('.file-editor'); if (!editor || !editorState()) return; const lineHeight = Number.parseFloat(getComputedStyle(editor).lineHeight) || 18; const line = Number(String(session.entries.find((item) => item.path === editorState()?.path)?.matchLines?.[0] || '').match(/^\d+/)?.[0]); if (line) editor.scrollTop = Math.max(0, (line - 1) * lineHeight - editor.clientHeight / 2); }, 0); });
document.addEventListener('click', (event) => { if (!event.target.closest('.context-menu') && !event.target.closest('#new-menu')) hideContextMenu(); if (!event.target.closest('#new-popover') && !event.target.closest('#new-menu')) $('new-popover').hidden = true; }); document.addEventListener('keydown', handleKeydown); document.addEventListener('keydown', (event) => { if (event.target.matches('.preview-image') && ['+', '=', '-', '_', '0'].includes(event.key)) { event.preventDefault(); const image = event.target; const current = Number(image.dataset.zoom || 1); const next = event.key === '0' ? 1 : Math.min(4, Math.max(0.5, current + (event.key === '-' || event.key === '_' ? -0.1 : 0.1))); image.dataset.zoom = String(next); image.style.transform = `scale(${next})`; image.style.cursor = next === 1 ? 'zoom-in' : 'zoom-out'; return; } if (!['Home', 'End', 'PageDown', 'PageUp'].includes(event.key) || event.target.matches('input,textarea,select')) return; const row = event.target.closest('.entry'); if (!row) return; event.preventDefault(); const index = Number(row.dataset.index); const delta = event.key === 'End' ? session.entries.length : event.key === 'Home' ? -session.entries.length : event.key === 'PageDown' ? 10 : -10; const next = Math.max(0, Math.min(session.entries.length - 1, index + delta)); selectEntry(next, event); document.querySelector(`[data-index="${next}"]`)?.focus(); }); document.addEventListener('contextmenu', (event) => { if (!event.target.closest('.entry')) { event.preventDefault(); session.currentPath && showContextMenu(event.clientX, event.clientY); } });
document.addEventListener('keydown', (event) => { if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'f') { if (document.activeElement?.classList.contains('pdf-preview')) return; event.preventDefault(); openQuickFilter(); } });
document.addEventListener('keydown', (event) => { if (event.key === '/' && !event.metaKey && !event.ctrlKey && !event.altKey && !event.target.matches('input,textarea,select,[contenteditable="true"]')) { event.preventDefault(); openQuickFilter(); } });
document.addEventListener('keydown', (event) => { if (event.key !== 'Escape' || event.target !== $('quick-filter')) return; if ($('quick-filter').value) { $('quick-filter').value = ''; applyQuickFilter(); return; } closeQuickFilter(); });
document.addEventListener('keydown', (event) => { if (event.key !== 'Escape' || !activeSearchIds.size) return; activeSearchIds.forEach((requestId) => call('search_cancel', { requestId }).catch(() => {})); activeSearchIds.clear(); activeSearchId = undefined; searchToken++; setStatus(t('searchCancelled', '搜索已取消')); }, true);
document.addEventListener('keydown', (event) => { if (!event.target.matches('.preview-image') || !['Enter', ' '].includes(event.key)) return; event.preventDefault(); event.target.click(); });
document.addEventListener('keydown', (event) => { if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') { event.preventDefault(); openSearchDialog(); } });
document.addEventListener('keydown', (event) => { if (event.key === 'Tab' && !event.shiftKey && event.target === $('search')) { event.preventDefault(); toggleSearchScope(); } });
document.addEventListener('DOMContentLoaded', () => { $('scope-toggle')?.addEventListener('click', toggleSearchScope); $('command-search-trigger')?.addEventListener('click', openSearchDialog); $('quick-filter-toggle')?.addEventListener('click', openQuickFilter); $('quick-filter')?.addEventListener('input', applyQuickFilter); $('search')?.addEventListener('keydown', (event) => { if (event.key === 'Enter') { event.preventDefault(); $('search-dialog').close(); } }); updateSearchScopeButton(); });
document.addEventListener('keydown', (event) => { if (event.key === 'Escape' && event.target === $('search') && $('search').value) { event.preventDefault(); $('search').value = ''; searchQuery = ''; session.pageOffset = 0; searchToken++; globalSearchMode = false; updateSearchScopeButton(); session.currentPath && loadDirectory(session.currentPath); } });
function iconForItem(item) { return entryIcon(item); }
function applyMarkdownFormat(action) { const editor = $('preview-body').querySelector('.file-editor'); if (!editor) return; const start = editor.selectionStart; const end = editor.selectionEnd; const selected = editor.value.slice(start, end) || t('selectedText', 'text'); const wrappers = { bold: ['**', '**'], italic: ['_', '_'], code: ['`', '`'], list: ['- ', ''], heading: ['# ', ''] }; let [prefix, suffix] = wrappers[action] || wrappers.bold; if (action === 'link') { const url = window.prompt(t('linkPrompt', 'Link URL'), 'https://'); if (!url || !/^https?:\/\//i.test(url.trim())) return; prefix = '['; suffix = `](${url.trim()})`; } editor.setRangeText(`${prefix}${selected}${suffix}`, start, end, 'end'); editor.dispatchEvent(new Event('input', { bubbles: true })); editor.focus(); }
const gridThumbObserver = typeof IntersectionObserver === 'undefined' ? undefined : new IntersectionObserver((observations) => { for (const observation of observations) { if (!observation.isIntersecting) continue; const image = observation.target; gridThumbObserver.unobserve(image); const row = image.closest('.entry'); const item = session.entries.find((entry) => entry.path === row?.dataset.path); const token = directoryToken; if (!item || !row || image.dataset.loading) continue; image.dataset.loading = 'true'; call('image_preview', { path: item.path }, crypto.randomUUID()).then((result) => { if (token !== directoryToken || !row.isConnected || !/^image\//.test(result?.mimeType || '') || typeof result.data !== 'string') throw new Error('thumbnail unavailable'); image.src = `data:${result.mimeType};base64,${result.data}`; image.classList.add('loaded'); }).catch(() => { image.remove(); const icon = row.querySelector('.entry-icon'); if (icon) icon.replaceChildren(iconForItem(item)); }); } });
new MutationObserver((records) => { for (const record of records) for (const node of record.addedNodes) { if (!(node instanceof HTMLElement) || !node.classList.contains('entry')) continue; const item = session.entries.find((entry) => entry.path === node.dataset.path); const icon = node.querySelector('.entry-icon'); if (!item || !icon) continue; icon.replaceChildren(iconForItem(item)); icon.classList.add(`entry-icon-${item.isDir ? 'dir' : item.kind || 'other'}`); } }).observe($('entries'), { childList: true });
new MutationObserver((records) => { for (const record of records) for (const node of record.addedNodes) { if (!(node instanceof HTMLElement) || !node.classList.contains('entry') || session.viewMode !== 'grid' || node.dataset.path === undefined) continue; const item = session.entries.find((entry) => entry.path === node.dataset.path); if (!item || item.isDir || item.kind !== 'image' || !gridThumbObserver) continue; const icon = node.querySelector('.entry-icon'); if (!icon) continue; const image = document.createElement('img'); image.className = 'grid-thumbnail'; image.alt = item.name; image.loading = 'lazy'; icon.replaceChildren(image); gridThumbObserver.observe(image); } }).observe($('entries'), { childList: true });
document.addEventListener('keydown', async (event) => { const dialog = $('image-lightbox'); if (!dialog?.open || !['ArrowLeft', 'ArrowRight'].includes(event.key)) return; const candidates = session.entries.filter((item) => item.kind === 'image' && !item.isDir); const index = candidates.findIndex((item) => item.path === dialog.dataset.path); if (index < 0 || !candidates.length) return; event.preventDefault(); const item = candidates[(index + (event.key === 'ArrowRight' ? 1 : -1) + candidates.length) % candidates.length]; const requestId = crypto.randomUUID(); dialog.dataset.request = requestId; try { const result = await call('image_preview', { path: item.path }, requestId); if (!dialog.open || dialog.dataset.request !== requestId || !/^image\//.test(result?.mimeType || '') || typeof result.data !== 'string') return; const image = dialog.querySelector('img'); image.src = `data:${result.mimeType};base64,${result.data}`; image.alt = item.name; dialog.dataset.path = item.path; } catch { /* keep current image when navigation fails */ } });
document.addEventListener('keydown', (event) => { if (session.viewMode !== 'grid' || !['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown'].includes(event.key) || event.target.matches('input,textarea,select')) return; const row = event.target.closest('.entry'); if (!row) return; const index = Number(row.dataset.index); const columns = Math.max(1, getComputedStyle($('entries')).gridTemplateColumns.split(' ').length); const delta = event.key === 'ArrowLeft' ? -1 : event.key === 'ArrowRight' ? 1 : event.key === 'ArrowUp' ? -columns : columns; const next = Math.max(0, Math.min(session.entries.length - 1, index + delta)); event.preventDefault(); event.stopImmediatePropagation(); selectEntry(next, event); document.querySelector(`[data-index="${next}"]`)?.focus(); }, true);
new MutationObserver(() => { const toggle = $('preview-body').querySelector('.markdown-mode-toggle'); const toolbar = toggle?.closest('.editor-toolbar'); if (!toolbar || toolbar.dataset.formatReady) return; toolbar.dataset.formatReady = 'true'; const tools = document.createElement('span'); tools.className = 'markdown-tools'; for (const action of ['bold', 'italic', 'code', 'list', 'heading', 'link', 'image']) { const button = document.createElement('button'); button.type = 'button'; button.dataset.mdAction = action; button.append(iconElement({ bold: 'bold', italic: 'italic', code: 'code', list: 'list', heading: 'heading', link: 'link', image: 'image' }[action])); button.title = t(`markdown${action[0].toUpperCase()}${action.slice(1)}`, action); button.setAttribute('aria-label', button.title); tools.append(button); } toolbar.insertBefore(tools, toggle); }).observe($('preview-body'), { childList: true, subtree: true });
document.addEventListener('click', (event) => { const button = event.target.closest('[data-md-action]'); if (!button) return; if (button.dataset.mdAction === 'image') { const input = document.createElement('input'); input.type = 'file'; input.multiple = true; input.accept = 'image/*'; input.onchange = async () => { const editor = $('preview-body').querySelector('.file-editor'); if (!editor) return; for (const file of input.files || []) if (!await importImageIntoEditor(file, editor)) break; }; input.click(); } else applyMarkdownFormat(button.dataset.mdAction); });
updateSortDirection(); updateNavigationButtons();
$('refresh').onclick = () => { if (session.currentPath) searchQuery ? search(searchQuery) : loadDirectory(session.currentPath); };
$('back').onclick = () => { if (session.historyIndex > 0) { session.historyIndex--; navigate(session.history[session.historyIndex], false); } };
if ($('sort')) $('sort').onchange = (event) => { session.sortBy = event.target.value; session.pageOffset = 0; searchQuery ? search(searchQuery) : loadDirectory(session.currentPath); };
async function bootstrap() {
  await Promise.race([localeReady, new Promise((resolve) => setTimeout(resolve, 1_000))]);
  document.documentElement.lang = selectedLanguage === 'en' ? 'en' : 'zh-CN'; applyI18n(); for (const id of Object.keys(rootLabels)) rootLabels[id] = t(id, rootLabels[id]);
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
    t: (key, fallback) => t(key, fallback),
  });
  filesPreviewPanel = createFilesPreviewPanel({
    container: $('preview'),
    body: $('preview-body'),
    resizer: $('preview-resizer'),
    layoutButton: $('toggle-preview-layout'),
    maximizeButton: $('maximize-preview'),
    closeButton: $('close-preview'),
    initialWidth: session.previewWidth,
    initialHeight: session.previewHeight,
    initialBottom: session.previewBottom,
    call,
    t,
    entryIcon,
    iconElement,
    formatSize,
    onStatus: (msg, type) => setStatus(msg, type),
    onToast: (msg, type) => toast(msg, type),
    onOpenItem: (item) => openItem(item),
    onRevealItem: (p) => revealPath(p),
    onLayoutChange: ({ bottom, width, height }) => {
      session.previewBottom = bottom;
      session.previewWidth = width;
      session.previewHeight = height;
      storageSet('natives-preview-bottom', bottom).catch(() => {});
      storageSet(bottom ? 'natives-preview-height' : 'natives-preview-width', bottom ? height : width).catch(() => {});
    },
    onRememberPath: (p) => remember(p),
    onDirectoryReload: () => loadDirectory(session.currentPath),
  });
  filesSidebar.setPreferences({ language: selectedLanguage, theme: selectedTheme });
  await init();
  setGridSize(session.gridSize); const stored = await storageGet('natives-last-path', ''); if (typeof stored !== 'string' || !stored || stored === session.currentPath) return; try { const result = await call('stat', { path: stored }, crypto.randomUUID()); if (result?.found && result.isDir) navigate(stored, false); } catch { /* stale or temporarily unavailable path: keep the root */ }
}
bootstrap().catch((error) => { setStatus(error?.message || '页面启动失败', 'error'); $('retry').hidden = false; });
document.addEventListener('keydown', (event) => { if (!event.altKey || event.ctrlKey || event.metaKey || event.key.toLowerCase() !== 'l' || event.target.matches('input,textarea,select')) return; event.preventDefault(); filesSidebar?.openSettings('language'); });

// Finder may include a non-JSON text/plain label alongside File objects. Capture
// that shape before the row's internal-move handler can misclassify it.
document.addEventListener('drop', (event) => {
  const target = event.target.closest?.('#entries, .entry');
  const raw = event.dataTransfer?.getData('text/plain') || '';
  const files = event.dataTransfer?.files;
  if (!target || !files?.length || raw.trim().startsWith('[') || !session.currentPath) return;
  event.preventDefault();
  event.stopImmediatePropagation();
  const destination = target.closest('.entry')?.dataset.path;
  importFileList(files, session.entries.find((item) => item.path === destination)?.isDir ? destination : session.currentPath)
    .catch((error) => setStatus(error.message, 'error'));
}, true);
document.addEventListener('dragenter', (event) => {
  if (!event.target.closest?.('#entries, .entry') || !event.dataTransfer?.types?.length) return;
  dropHintDepth++;
  if (dropHintDepth === 1) setStatus(t('dropHint', '释放以导入或移动文件'));
}, true);
document.addEventListener('dragleave', (event) => {
  if (!event.target.closest?.('#entries, .entry')) return;
  dropHintDepth = Math.max(0, dropHintDepth - 1);
  if (!dropHintDepth && !ops.isBusy()) setStatus('');
}, true);
if ($('editor')) $('editor').onclick = openEditorSelected;
$('grid-small').onclick = () => setGridSize('small');
$('grid-medium').onclick = () => setGridSize('medium');
$('grid-large').onclick = () => setGridSize('large');
function setGridSize(size) {
  session.gridSize = ['small', 'medium', 'large'].includes(size) ? size : 'medium';
  session.viewMode = 'grid';
  $('list-view')?.setAttribute('aria-pressed', 'false');
  $('grid-view')?.setAttribute('aria-pressed', 'true');
  document.documentElement.dataset.gridSize = session.gridSize;
  for (const value of ['small', 'medium', 'large']) {
    $('grid-' + value)?.setAttribute('aria-pressed', String(value === session.gridSize));
  }
  storageSet('natives-grid-size', session.gridSize).catch(() => {});
  storageSet('natives-view-mode', 'grid').catch(() => {});
  renderEntries();
  const box = $('entries');
  box.classList.toggle('grid-small', session.gridSize === 'small');
  box.classList.toggle('grid-large', session.gridSize === 'large');
  syncGridControls();
}
function syncGridControls() { document.querySelectorAll('.grid-size-button').forEach((button) => { button.hidden = session.viewMode !== 'grid'; }); }
$('list-view').addEventListener('click', syncGridControls); $('grid-view').addEventListener('click', syncGridControls);
$('import-folder').onclick = () => { $('new-popover').hidden = true; const input = document.createElement('input'); input.type = 'file'; input.multiple = true; input.webkitdirectory = true; input.onchange = () => importFileList(input.files); input.click(); };
$('disk-usage').onclick = showDiskUsage;
$('open-trash').onclick = async () => { try { await call('open_trash'); toast(t('trashOpened', '已打开系统废纸篓')); } catch (error) { setStatus(error.message, 'error'); } };

$('entries').addEventListener('keydown', (event) => { if (event.key !== 'Enter' || !event.shiftKey || event.metaKey || event.ctrlKey || event.altKey) return; const row = event.target.closest('.entry'); if (!row) return; event.preventDefault(); event.stopImmediatePropagation(); openEditorSelected(); }, true);
	if ($('path-form')) $('path-form').addEventListener('submit', async (event) => {
	  const raw = normalizePathInput($('path-input')?.value);
  if (!raw || raw.includes('/') || raw.includes('\\')) return;
  event.preventDefault(); event.stopImmediatePropagation();
  try {
    const result = await call('locate', { query: raw }, crypto.randomUUID());
    const match = result?.entries?.[0];
    if (match?.path) { pendingSelectionPath = match.path; navigate(parentAndName(match.path).parent); }
    else navigate(raw);
  } catch { navigate(raw); }
}, true);

// fanbox parity: let the preview occupy the full workspace for editors and long documents.
$('maximize-preview').onclick = () => {
  const preview = $('preview');
  const maximized = preview.classList.toggle('is-maximized');
  const button = $('maximize-preview');
  button.setAttribute('aria-pressed', String(maximized));
  button.title = t(maximized ? 'previewRestore' : 'previewMaximize', maximized ? '还原预览' : '放大预览');
  button.setAttribute('aria-label', button.title);
};
document.addEventListener('keydown', (event) => {
  if (event.key !== 'Escape' || !$('preview')?.classList.contains('is-maximized') || $('modal')?.open || $('dirty-modal')?.open || $('conflict-modal')?.open || $('usage-modal')?.open) return;
  event.preventDefault(); $('maximize-preview').click();
}, true);
new MutationObserver(() => {
  if (!searchQuery || $('entries').childElementCount === 0) return;
  const count = $('entries').childElementCount;
  $('status').textContent = `${count}${session.pageHasMore ? '+' : ''} ${t('searchResults', '个搜索结果')}${session.pageHasMore ? ` · ${t('moreAvailable', '可继续加载')}` : ''}`;
}).observe($('entries'), { childList: true });
new MutationObserver(() => { if (!searchQuery || !searchTruncated || !$('entries').childElementCount) return; $('status').textContent += ` · ${t('usageTruncated', '结果可能不完整')}`; }).observe($('entries'), { childList: true });
$('toggle-preview-layout').onclick = () => {
  session.previewBottom = !session.previewBottom;
  document.querySelector('.layout').classList.toggle('preview-bottom', session.previewBottom);
  syncPreviewLayoutControls();
  $('preview-resizer').setAttribute('aria-orientation', session.previewBottom ? 'horizontal' : 'vertical'); $('preview-resizer').setAttribute('aria-valuenow', String(session.previewBottom ? session.previewHeight : session.previewWidth));
  storageSet('natives-preview-bottom', session.previewBottom).catch(() => {});
};

// fanbox parity: drag the preview's left edge; persistence keeps the layout stable between sessions.
function beginPreviewResize(event) {
  if ($('preview').classList.contains('is-maximized')) return;
  resizingPreview = true; document.body.style.cursor = session.previewBottom ? 'row-resize' : 'col-resize'; document.body.style.userSelect = 'none'; event.preventDefault();
}
$('preview-resizer').addEventListener('mousedown', beginPreviewResize);
$('preview-resizer').addEventListener('keydown', (event) => {
  if (!['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End'].includes(event.key) || $('preview').classList.contains('is-maximized')) return;
  const delta = event.key === 'ArrowLeft' || event.key === 'ArrowUp' ? -20 : event.key === 'ArrowRight' || event.key === 'ArrowDown' ? 20 : 0;
  if (session.previewBottom) session.previewHeight = event.key === 'Home' ? 180 : event.key === 'End' ? 600 : Math.min(600, Math.max(180, session.previewHeight + delta));
  else session.previewWidth = event.key === 'Home' ? 240 : event.key === 'End' ? 620 : Math.min(620, Math.max(240, session.previewWidth + delta));
  document.documentElement.style.setProperty(session.previewBottom ? '--preview-height' : '--preview-width', `${session.previewBottom ? session.previewHeight : session.previewWidth}px`); $('preview-resizer').setAttribute('aria-valuenow', String(session.previewBottom ? session.previewHeight : session.previewWidth)); storageSet(session.previewBottom ? 'natives-preview-height' : 'natives-preview-width', session.previewBottom ? session.previewHeight : session.previewWidth).catch(() => {}); event.preventDefault();
});
$('preview').addEventListener('mousedown', (event) => {
  if (event.target === $('preview-resizer')) return;
  const bounds = $('preview').getBoundingClientRect();
  const edgeDistance = session.previewBottom ? event.clientY - bounds.top : event.clientX - bounds.left;
  if (edgeDistance > 9) return;
  beginPreviewResize(event);
});
document.addEventListener('mousemove', (event) => {
  if (!resizingPreview) return;
  if (session.previewBottom) session.previewHeight = Math.min(600, Math.max(180, innerHeight - event.clientY));
  else session.previewWidth = Math.min(620, Math.max(240, innerWidth - event.clientX));
  document.documentElement.style.setProperty(session.previewBottom ? '--preview-height' : '--preview-width', `${session.previewBottom ? session.previewHeight : session.previewWidth}px`); $('preview-resizer').setAttribute('aria-valuenow', String(session.previewBottom ? session.previewHeight : session.previewWidth));
});
document.addEventListener('mouseup', () => {
  if (!resizingPreview) return;
  resizingPreview = false; document.body.style.cursor = ''; document.body.style.userSelect = '';
  $('preview-resizer').setAttribute('aria-valuenow', String(session.previewBottom ? session.previewHeight : session.previewWidth)); storageSet(session.previewBottom ? 'natives-preview-height' : 'natives-preview-width', session.previewBottom ? session.previewHeight : session.previewWidth).catch(() => {});
});

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
function toggleSidebar() {
  if (filesSidebar) filesSidebar.toggle();
  else {
    session.sidebarCollapsed = !session.sidebarCollapsed;
    applySidebarCollapsed();
    storageSet('natives-sidebar-collapsed', session.sidebarCollapsed).catch(() => {});
  }
}
$('sort').addEventListener('change', () => storageSet('natives-sort-by', session.sortBy).catch(() => {}));
$('list-view').addEventListener('click', () => storageSet('natives-view-mode', 'list').catch(() => {}));
$('grid-view').addEventListener('click', () => storageSet('natives-view-mode', 'grid').catch(() => {}));
$('show-hidden').addEventListener('change', (event) => storageSet('natives-show-hidden', Boolean(event.target.checked)).catch(() => {}));
$('recursive-search').addEventListener('change', (event) => storageSet('natives-recursive-search', Boolean(event.target.checked)).catch(() => {}));
document.addEventListener('dragstart', (event) => { const name = event.target.closest?.('.entry-name'); if (!name) return; const row = name.closest('.entry'); const item = session.entries.find((entry) => entry.path === row?.dataset.path); if (item?.kind === 'image') event.dataTransfer.setData('text/html', `<img src="${fileUri(item.path)}" alt="image">`); });
document.addEventListener('keydown', (event) => { if (!(event.metaKey || event.ctrlKey) || !event.shiftKey || event.key.toLowerCase() !== 'z' || !event.target.closest?.('.image-editor-canvas')) return; event.preventDefault(); document.querySelector('[data-image-redo="true"]')?.click(); });
document.addEventListener('keydown', (event) => { if (!['Home', 'End', 'PageDown', 'PageUp'].includes(event.key) || event.target.matches('input,textarea,select')) return; const row = event.target.closest('.entry'); if (!row || !session.entries.length) return; event.preventDefault(); event.stopImmediatePropagation(); const index = Number(row.dataset.index); const pageStep = Math.max(1, Math.floor(($('entries').clientHeight || 460) / 46)); const next = event.key === 'End' ? session.entries.length - 1 : event.key === 'Home' ? 0 : Math.max(0, Math.min(session.entries.length - 1, index + (event.key === 'PageDown' ? pageStep : -pageStep))); selectEntry(next, event); document.querySelector(`[data-index="${next}"]`)?.focus(); }, true);
document.addEventListener('keydown', (event) => { if (event.key !== 'Enter' || event.shiftKey || event.metaKey || event.ctrlKey || event.altKey || !searchQuery) return; const row = event.target.closest('.entry'); if (!row) return; const index = Number(row.dataset.index); if (!session.entries[index]) return; event.preventDefault(); event.stopImmediatePropagation(); selectEntry(index, event); renderPreviewSelection(); }, true);
document.addEventListener('keydown', (event) => { if (!['ContextMenu', 'F10'].includes(event.key) || (event.key === 'F10' && !event.shiftKey) || event.target.closest?.('.entry')) return; const entriesBox = event.target.closest?.('#entries'); if (!entriesBox || !session.currentPath) return; event.preventDefault(); const rect = entriesBox.getBoundingClientRect(); showContextMenu(rect.left + rect.width / 2, rect.top + rect.height / 2); });
document.addEventListener('keydown', (event) => { if (!(event.metaKey || event.ctrlKey) || event.key !== 'Enter' || event.target.matches('input,textarea,select') || session.selectedPaths.size !== 1) return; event.preventDefault(); event.stopImmediatePropagation(); openEditorSelected(); }, true);
document.addEventListener('keydown', (event) => { if (event.key !== 'Escape' || !searchQuery || event.target.matches('input,textarea,select')) return; searchQuery = ''; $('search').value = ''; session.pageOffset = 0; cancelActiveSearches(); if (session.currentPath) loadDirectory(session.currentPath); }, true);
document.addEventListener('keydown', (event) => { if (!(event.metaKey || event.ctrlKey) || !['[', ']'].includes(event.key) || event.target.matches('input,textarea,select')) return; const step = event.key === '[' ? -1 : 1; const target = session.history[session.historyIndex + step]; if (!target) return; event.preventDefault(); session.historyIndex += step; navigate(target, false); });
document.addEventListener('keydown', (event) => { if (!event.altKey || event.ctrlKey || event.metaKey || event.key.toLowerCase() !== 'l') return; const language = $('language'); if (!language) return; event.preventDefault(); language.focus(); });
