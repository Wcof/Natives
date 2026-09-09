

import { createNativeClient } from './native-client.js';
import { widgetPlugins, backgroundPlugins } from './space-plugins.js';
import { createSpaceWorkspaceTree } from './space-workspace-tree.js';
import { createSpaceDashboard } from './space-dashboard.js';
import { createSpaceInspector } from './space-inspector.js';
import { createSpaceToolbar } from './space-toolbar.js';
import { createSpaceNameModal } from './space-modal-name.js';
import { createSpaceDeleteModal } from './space-modal-delete.js';
import { createSidebarController } from './sidebar-controller.js';
import { mountAppMenu } from './app-navigation-projection.js';
import { createSettingsMenu } from './settings-menu.js';
import { createGlobalSearchModal } from './global-search-modal.js';
import { createWorkspaceMutationQueue } from './space-write-queue.js';

let localeMessages = {};
let selectedLanguage = 'zh_CN';
let selectedTheme = 'archive';
let session = null;
let activeWorkspaceId = null;
let activeSnapshot = null;
let nativeClient = null;
let broadcastChannel = null;
let inspectorWidth = 360;

const $ = (id) => document.getElementById(id);
const t = (key, fallback) => localeMessages[key]?.message || fallback || key;


const SPACE_UI_KEY = 'natives-space-ui';
function readSpaceUi() {
  try { return JSON.parse(sessionStorage.getItem(SPACE_UI_KEY) || 'null') || {}; } catch { return {}; }
}
function persistSpaceUi(patch) {
  try {
    sessionStorage.setItem(SPACE_UI_KEY, JSON.stringify({ ...readSpaceUi(), ...patch }));
  } catch {}
}
function isSessionRestore() {
  const navType = performance.getEntriesByType?.('navigation')?.[0]?.type;
  return navType === 'reload' || navType === 'back_forward';
}

async function openModelSettings(returnFocus) {
  const module = await import('./model-settings.js');
  await module.openModelSettings({ t, language: selectedLanguage, returnFocus });
}

async function openAppsCenter(returnFocus) {
  const module = await import('./model-settings.js');
  await module.openModelSettings({ t, language: selectedLanguage, returnFocus, initialPage: 'apps' });
}

function toast(message, kind = 'info') {
  const el = $('toast');
  if (!el) return;
  el.textContent = message;
  el.className = kind === 'error' ? 'toast-error' : '';
  el.hidden = false;
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => { el.hidden = true; }, 3000);
}

function broadcastRevision() {
  if (!broadcastChannel) broadcastChannel = new BroadcastChannel('natives-workspace');
  broadcastChannel.postMessage({ type: 'revision-invalidated' });
}

async function getStored(key, fallback) {
  if (globalThis.chrome?.storage?.local) {
    try {
      const res = await chrome.storage.local.get({ [key]: fallback });
      return res?.[key] ?? fallback;
    } catch {}
  }
  try {
    const val = localStorage.getItem(key);
    if (val === null) return fallback;
    try { return JSON.parse(val); } catch { return val; }
  } catch { return fallback; }
}

async function setStored(key, value) {
  if (globalThis.chrome?.storage?.local) {
    try { await chrome.storage.local.set({ [key]: value }); return; } catch {}
  }
  try { localStorage.setItem(key, typeof value === 'string' ? value : JSON.stringify(value)); } catch {}
}

async function loadLocale() {
  try {
    const stored = await getStored('natives-language', '');
    selectedLanguage = stored === 'en' || stored === 'zh_CN' ? stored : 'zh_CN';
    const res = await fetch(`_locales/${selectedLanguage}/messages.json`);
    localeMessages = res.ok ? await res.json() : {};
  } catch {
    localeMessages = {};
  }
}

function applyI18n() {
  document.querySelectorAll('[data-i18n]').forEach((el) => { el.textContent = t(el.dataset.i18n, el.textContent); });
  document.querySelectorAll('[data-i18n-title]').forEach((el) => {
    el.title = t(el.dataset.i18nTitle, el.title);
    el.setAttribute('aria-label', el.title);
  });
  document.querySelectorAll('[data-i18n-aria-label]').forEach((el) => {
    el.setAttribute('aria-label', t(el.dataset.i18nAriaLabel, el.getAttribute('aria-label') || ''));
  });
}

function initNativeClient() {
  nativeClient = createNativeClient({
    host: 'com.natives.file_manager',
    writeMethods: [
      'workspace_create', 'workspace_rename', 'workspace_reorder', 'workspace_pin',
      'workspace_duplicate', 'workspace_delete', 'workspace_open_tab', 'workspace_close_tab',
      'workspace_reorder_tabs', 'workspace_widget_upsert', 'workspace_widget_remove',
      'workspace_widget_reorder', 'workspace_background_save', 'workspace_instantiate_template',
      'workspace_template_save', 'workspace_template_delete', 'workspace_save_from_tabliss',
      'workspace_reset', 'settings_set',
    ],
    onDisconnect: () => toast(t('hostDisconnected', 'Host 已断开连接'), 'error'),
  });
}

async function nativeCall(method, params = {}) {
  if (!nativeClient) initNativeClient();
  try {
    return await nativeClient.call(method, params);
  } catch (error) {
    toast(error?.message || '操作失败', 'error');
    throw error;
  }
}

// 统一空间写入口：串行执行 workspace 写操作，每次使用该空间的最新 revision，
// 迟到的响应只更新对应空间，避免切换空间后覆盖当前画面。
const queueWorkspaceMutation = createWorkspaceMutationQueue({
  nativeCall,
  getActiveWorkspaceId: () => activeWorkspaceId,
  getActiveSnapshot: () => activeSnapshot,
  applySnapshot: (result) => {
    activeSnapshot = result;
    renderView();
  },
});

let wsTree;
let dashboard;
let inspector;
let toolbar;
let nameModal;
let deleteModal;

function updateSnapshot(newSnapshot) {
  activeSnapshot = newSnapshot;
  renderView();
}

function renderView() {
  dashboard.render(activeSnapshot, activeWorkspaceId, updateSnapshot);
  toolbar.sync(activeSnapshot);
  inspector.sync(activeSnapshot, activeWorkspaceId);
}

async function loadWorkspaceSnapshot(id) {
  try {
    activeSnapshot = await nativeCall('workspace_snapshot', { workspaceId: id });
    renderView();
  } catch (error) { toast(error.message); }
}

async function activateWorkspace(id) {
  if (id === activeWorkspaceId && activeSnapshot) return;
  activeWorkspaceId = id;
  try { await nativeCall('workspace_open_tab', { workspaceId: id }); } catch {}
  await loadWorkspaceSnapshot(id);
  wsTree.render(session, activeWorkspaceId);
}

async function refreshSession() {
  session = await nativeCall('workspace_session');
  if (!activeWorkspaceId || !session.workspaces.some(w => w.id === activeWorkspaceId)) {
    activeWorkspaceId = session.activeWorkspaceId || session.workspaces[0]?.id || null;
  }
  wsTree.render(session, activeWorkspaceId);
  if (activeWorkspaceId) await loadWorkspaceSnapshot(activeWorkspaceId);
}

function initResizers() {
  const resizer = $('inspector-resizer');
  if (!resizer) return;
  let isResizing = false;
  resizer.onpointerdown = (e) => {
    isResizing = true;
    resizer.setPointerCapture(e.pointerId);
    e.preventDefault();
  };
  resizer.onpointermove = (e) => {
    if (!isResizing) return;
    inspectorWidth = Math.max(240, Math.min(620, window.innerWidth - e.clientX));
    document.documentElement.style.setProperty('--inspector-width', `${inspectorWidth}px`);
    resizer.setAttribute('aria-valuenow', String(inspectorWidth));
  };
  resizer.onpointerup = (e) => {
    if (!isResizing) return;
    isResizing = false;
    resizer.releasePointerCapture(e.pointerId);
    setStored('natives-inspector-width', inspectorWidth);
  };
}

async function handleSaveWorkspaceName(workspace, name) {
  try {
    if (workspace) {
      await nativeCall('workspace_rename', { workspaceId: workspace.id, name, expectedRevision: workspace.revision });
      toast(t('workspaceRenamed', '空间已重命名'));
      await refreshSession();
    } else {
      const snap = await nativeCall('workspace_create', { name });
      activeWorkspaceId = snap.workspace.id;
      toast(t('workspaceCreated', 'Workspace 已创建'));
      await refreshSession();
      inspector.open('catalog');
    }
    broadcastRevision();
  } catch {}
}

async function handleDeleteWorkspace(workspace) {
  try {
    await nativeCall('workspace_delete', { workspaceId: workspace.id, expectedRevision: workspace.revision });
    await refreshSession();
    toast(t('workspaceDeleted', '空间已删除'));
    broadcastRevision();
  } catch {}
}

async function init() {
  await Promise.race([loadLocale(), new Promise((resolve) => setTimeout(resolve, 1000))]);
  const [storedTheme, storedWidth, sidebarWidth] = await Promise.all([
    getStored('natives-theme', 'archive'),
    getStored('natives-inspector-width', 360),
    getStored('natives-sidebar-width', 248),
  ]);
  selectedTheme = ['volt', 'archive'].includes(storedTheme) ? storedTheme : 'archive';
  inspectorWidth = Math.max(240, Math.min(620, Number(storedWidth) || 360));
  document.documentElement.style.setProperty('--inspector-width', `${inspectorWidth}px`);
  document.documentElement.dataset.theme = selectedTheme;
  document.documentElement.lang = selectedLanguage === 'en' ? 'en' : 'zh-CN';
  applyI18n();

  initNativeClient();
  initResizers();

  const restoring = isSessionRestore();
  const storedUi = restoring ? readSpaceUi() : {};
  if (!restoring) persistSpaceUi({ sidebarCollapsed: true, widgetsHidden: false });

  const sidebarController = createSidebarController({
    resizer: $('sidebar-resizer'),
    toggleButton: $('space-toggle-sidebar-btn'),
    initialWidth: sidebarWidth,
    initialCollapsed: restoring ? storedUi.sidebarCollapsed !== false : true,
    t,
    onWidthChange: (w) => setStored('natives-sidebar-width', w),
    onCollapsedChange: (collapsed) => persistSpaceUi({ sidebarCollapsed: Boolean(collapsed) }),
  });

  nameModal = createSpaceNameModal({ $, t, onSaveWorkspaceName: handleSaveWorkspaceName });
  deleteModal = createSpaceDeleteModal({ $, t, onDeleteWorkspaceConfirmed: handleDeleteWorkspace });
  createGlobalSearchModal({ $, t, nativeCall });

  wsTree = createSpaceWorkspaceTree({ $, t, activateWorkspace, renameWorkspace: (ws) => nameModal.open(ws), deleteWorkspace: (ws) => deleteModal.open(ws) });
  dashboard = createSpaceDashboard({
    $, t, selectedLanguage, backgroundPlugins, widgetPlugins, nativeCall, broadcastRevision,
    queueWorkspaceMutation,
    toast,
  });

  inspector = createSpaceInspector({
    $, t, language: selectedLanguage, nativeCall, broadcastRevision, updateSnapshot,
    queueWorkspaceMutation,
    onPositionEditChange: (widgetId) => dashboard.setEditingWidget(widgetId, activeSnapshot, activeWorkspaceId, updateSnapshot),
    onCloseFocusAnchor: () => toolbar?.settingsButton?.focus(),
  });

  toolbar = createSpaceToolbar({
    $, t,
    onToggleSettings: () => { if (inspector.isOpen) inspector.close(); else inspector.open('overview'); },
    onToggleWidgets: (hidden) => {
      dashboard.setWidgetsHidden(hidden);
      persistSpaceUi({ widgetsHidden: Boolean(hidden) });
    },
    onToggleSidebar: null,
    onOpenCatalog: () => inspector.open('catalog'),
  });
  if (restoring && storedUi.widgetsHidden) toolbar.setWidgetsHidden(true);

  createSettingsMenu({
    anchorButton: $('settings-entry'),
    initialLanguage: selectedLanguage, initialTheme: selectedTheme,
    onLanguageChange: async (lang) => { await setStored('natives-language', lang); location.reload(); },
    onThemeChange: async (theme) => {
      selectedTheme = theme;
      document.documentElement.dataset.theme = theme;
      await setStored('natives-theme', theme);
    },
    onModelSettings: (anchor) => openModelSettings(anchor).catch((error) => toast(error.message, 'error')),
    onAppsCenter: (anchor) => openAppsCenter(anchor).catch((error) => toast(error.message, 'error')),
    t,
  });

  // ADR-0025 D33/D36: space.html stays 0-Native-Port for apps — the
  // "应用" section renders from the projection UI cache written by the
  // App Center / files page, and tracks chrome.storage changes live.
  mountAppMenu($('app-menu-apps'), { t }).catch(() => {});

  $('workspace-create').onclick = () => nameModal.open(null);

  document.addEventListener('keydown', (e) => {
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'b') {
      e.preventDefault();
      sidebarController.toggle();
    }
  });

  try { await refreshSession(); } catch (err) { toast(`${t('hostConnectionFailed', 'Native Host 连接失败')}：${err.message}`); }

  if (!broadcastChannel) broadcastChannel = new BroadcastChannel('natives-workspace');
  broadcastChannel.onmessage = (e) => { if (e.data?.type === 'revision-invalidated') refreshSession().catch(() => {}); };
  window.addEventListener('pagehide', () => { if (nativeClient) nativeClient.disconnect(); });

  let idleTimer;
  document.addEventListener('visibilitychange', () => {
    if (document.hidden) {
      idleTimer = setTimeout(() => { if (document.hidden && nativeClient) nativeClient.disconnect(); }, 60_000);
    } else {
      clearTimeout(idleTimer);
    }
  });
}

init().catch((err) => toast(`${t('pageError', '页面初始化失败')}：${err.message}`));
