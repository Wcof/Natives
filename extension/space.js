import { storage } from './platform/storage.js';
import { subscribeCoreEvents } from './platform/events.js';


import { ensureAuthenticated } from './platform/web-session.js';
import { isExtension } from './platform/env.js';
import { createNativeClient } from './native-client.js';
import { widgetPlugins, backgroundPlugins } from './space-plugins.js';
import { createSpaceWorkspaceTree } from './space-workspace-tree.js';
import { createSpaceDashboard } from './space-dashboard.js';
import { createSpaceInspector } from './space-inspector.js';
import { createSpaceToolbar } from './space-toolbar.js';
import { createSpaceNameModal } from './space-modal-name.js';
import { createSpaceDeleteModal } from './space-modal-delete.js';
import { createSidebarController } from './sidebar-controller.js';
import { mountAppMenu, loadAppNavigation, onAppNavigationChange, saveAppNavigation, projectionFromApps, NAVIGATION_KIND_APP } from './app-navigation-projection.js';
import { createEmbeddedAppSurface } from './app.js';
import { createSettingsMenu } from './settings-menu.js';
import { createGlobalSearchModal } from './global-search-modal.js';
import { createWorkspaceMutationQueue } from './space-write-queue.js';
import {
  INVESTMENT_WIDGET_KEYS,
  initInvestmentFeed,
  suspendInvestmentFeed,
  resumeInvestmentFeed,
  invalidateInvestmentCards,
  setInvestmentCardsPresent,
  setInvestmentNavigationHandler,
  setMoneyVisible,
  isMoneyVisible,
} from './space-investment-feed.js';

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
const t = (key, fallback) => localeMessages[key]?.message || globalThis.chrome?.i18n?.getMessage?.(key) || fallback || key;


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
  const values = await storage.local.get({[key]:fallback});
  return values[key] ?? fallback;
}
async function setStored(key,value) { await storage.local.set({[key]:value}); }

async function loadLocale() {
  try {
    const stored = await getStored('natives-language', '');
    selectedLanguage = stored === 'en' || stored === 'zh_CN'
      ? stored
      : (!isExtension && globalThis.navigator?.language && !globalThis.navigator.language.startsWith('zh') ? 'en' : 'zh_CN');
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
    // 与 files-host-connection 一致：首包 version 握手注册来源，
    // 否则 Host 不响应业务方法（workspace_session 等会一直挂起）。
    handshake: {
      method: 'version',
      params: { extensionVersion: globalThis.chrome?.runtime?.getManifest?.().version || '' },
    },
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
  syncInvestmentCards();
  if (embeddedAppId) showEmbeddedView(true);
}

// ADR-0036：投资卡存在性由 Workspace 快照驱动；卡片数量不改变查询次数。
function syncInvestmentCards() {
  const hasInvestment = (activeSnapshot?.widgets || [])
    .some((w) => w.enabled && INVESTMENT_WIDGET_KEYS.includes(w.key));
  setInvestmentCardsPresent(hasInvestment);
  const moneyBtn = $('space-money-visibility-btn');
  if (moneyBtn) {
    moneyBtn.hidden = !hasInvestment;
    if (hasInvestment) applyMoneyVisibilityLabel(moneyBtn);
  }
}

function applyMoneyVisibilityLabel(button) {
  const visible = isMoneyVisible();
  const label = visible ? t('moneyHide', '隐藏金额') : t('moneyShow', '显示金额');
  button.title = label;
  button.setAttribute('aria-label', label);
  button.setAttribute('aria-pressed', String(visible));
}

async function loadWorkspaceSnapshot(id) {
  try {
    activeSnapshot = await nativeCall('workspace_snapshot', { workspaceId: id });
    renderView();
  } catch (error) { toast(error.message); }
}

async function activateWorkspace(id) {
  // 从内嵌应用视图切回空间视图：先释放嵌入会话（ADR-0036），再切显示。
  if (typeof embeddedAppId !== 'undefined' && embeddedAppId) leaveEmbeddedView();
  await activateWorkspaceInner(id);
}

async function activateWorkspaceInner(id, { pushState = true } = {}) {
  if (id === activeWorkspaceId && activeSnapshot) return;
  activeWorkspaceId = id;
  if (pushState) {
    const url = new URL(location.href);
    url.searchParams.delete('app');
    url.searchParams.set('workspace', id);
    if (location.search !== url.search) {
      history.pushState({ type: 'workspace', workspaceId: id }, '', url.pathname + url.search);
    }
  }
  try { await nativeCall('workspace_open_tab', { workspaceId: id }); } catch {}
  await loadWorkspaceSnapshot(id);
  wsTree.render(session, activeWorkspaceId);
}

async function refreshSession() {
  session = await nativeCall('workspace_session');
  if (!activeWorkspaceId || !session.workspaces.some(w => w.id === activeWorkspaceId)) {
    const urlWs = new URLSearchParams(location.search).get('workspace');
    if (urlWs && session.workspaces.some(w => w.id === urlWs)) {
      activeWorkspaceId = urlWs;
    } else {
      activeWorkspaceId = session.activeWorkspaceId || session.workspaces[0]?.id || null;
    }
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

async function handleSaveWorkspaceName(workspace, name, template = 'classic') {
  try {
    if (workspace) {
      await nativeCall('workspace_rename', { workspaceId: workspace.id, name, expectedRevision: workspace.revision });
      toast(t('workspaceRenamed', '空间已重命名'));
      await refreshSession();
    } else {
      const snap = await nativeCall('workspace_create', { name, template });
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
  if (!restoring) persistSpaceUi({ sidebarCollapsed: false, widgetsHidden: false });

  const sidebarController = createSidebarController({
    resizer: $('sidebar-resizer'),
    toggleButton: $('space-toggle-sidebar-btn'),
    initialWidth: sidebarWidth,
    initialCollapsed: Boolean(storedUi.sidebarCollapsed),
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
    getEditingWidgetId:()=>dashboard.getEditingWidgetId(),
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

  const settingsMenu = createSettingsMenu({
    anchorButton: $('settings-entry'),
    initialLanguage: selectedLanguage, initialTheme: selectedTheme,
    onLanguageChange: async (lang) => { await setStored('natives-language', lang); },
    onThemeChange: async (theme) => {
      await setStored('natives-theme', theme);
    },
    onModelSettings: (anchor) => openModelSettings(anchor).catch((error) => toast(error.message, 'error')),
    onAppsCenter: (anchor) => openAppsCenter(anchor).catch((error) => toast(error.message, 'error')),
    onModuleSettings:(appId,view)=>openEmbeddedApp(appId,{view}),
    t,
  });
  storage.onChanged.addListener(async (changes,area) => {
    if (area !== 'local') return;
    if (changes['natives-theme']) {
      selectedTheme=changes['natives-theme'].newValue;
      document.documentElement.dataset.theme=selectedTheme;
      settingsMenu.setPreferences({theme:selectedTheme});
    }
    if(changes['natives-sidebar-width'])sidebarController.setWidth(changes['natives-sidebar-width'].newValue,false);
    if(changes['natives-inspector-width']){
      inspectorWidth=changes['natives-inspector-width'].newValue;
      document.documentElement.style.setProperty('--inspector-width',`${inspectorWidth}px`);
    }
    if (changes['natives-language']) {
      await loadLocale(); applyI18n();
      dashboard.setLanguage(selectedLanguage);inspector.setLanguage(selectedLanguage);
      settingsMenu.setPreferences({language:selectedLanguage});
      renderView();
    }
  });
  window.addEventListener('natives-preferences-error', () => toast(t('sharedPreferencesFailed','偏好未保存，请重试。'),'error'));
  const settingsItems = (projection) => projection.items.map((item) => ({
    ...item,
    label: item.labels?.[selectedLanguage] || item.label,
  }));
  settingsMenu.setModules(settingsItems(await loadAppNavigation()));
  const stopSettingsProjection=onAppNavigationChange(projection=>settingsMenu.setModules(settingsItems(projection)));
  // Reuse the workspace's authenticated Core connection, without another Port.
  // A fresh profile may have no App Center navigation cache yet.
  nativeCall('apps:list').then(snapshot=>saveAppNavigation(projectionFromApps(snapshot.apps,snapshot.revision,snapshot.modules)))
    .catch(()=>toast(t('appsNetworkError'),'error'));
  let pageDisposed=false;
  window.addEventListener('pagehide',(event)=>{
    if(event.persisted||pageDisposed)return;
    pageDisposed=true;
    stopSettingsProjection();
    settingsMenu.destroy();
  });

  // ADR-0025 D33/D36: space.html stays 0-Native-Port for apps — the
  // "应用" section renders from the projection UI cache written by the
  // App Center / files page, and tracks chrome.storage changes live.
  // afterRender：菜单渲染（初始 + 每次 projection 刷新）后回填当前内嵌
  // 应用的选中态——深链/刷新 ？app=fund 进入时菜单尚未渲染、握手后 revision
  // 变更会整体重渲染，两次时机都不回填就会出现「进入投资子应用但菜单项
  // 无高亮」。
  mountAppMenu($('app-menu-apps'), { t, afterRender: () => setAppActive(embeddedAppId) }).catch(() => {});
  bindEmbeddedAppNavigation();

  // ADR-0036：空间投资卡数据源——页面级一次短命只读投影查询。复用本页
  // 共享 Files Host client 做 apps:get 校验；App Runtime Port 由 feed 独立
  // 持有并在响应后立即断开。
  initInvestmentFeed({ coreCall: (method, params) => nativeClient.call(method, params) });
  setInvestmentNavigationHandler((view) => openEmbeddedApp('fund', { view }));
  const moneyBtn = $('space-money-visibility-btn');
  if (moneyBtn) {
    moneyBtn.addEventListener('click', () => {
      setMoneyVisible(!isMoneyVisible());
      applyMoneyVisibilityLabel(moneyBtn);
    });
  }

  $('workspace-create').onclick = () => nameModal.open(null);

  document.addEventListener('keydown', (e) => {
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'b') {
      e.preventDefault();
      sidebarController.toggle();
    }
  });

  try { await refreshSession(); } catch (err) { toast(`${t('hostConnectionFailed', 'Native Host 连接失败')}：${err.message}`); }
  let recovering;
  const stopEvents=subscribeCoreEvents(event=>{
    if ((event.type==='module_changed' && event.appId==='fund') || (event.type==='resync' && event.reason!=='unavailable')) invalidateInvestmentCards();
    if ((event.type==='resync' && event.reason!=='unavailable') || event.type==='workspace_changed') {
      if (!recovering) recovering=refreshSession().catch(error=>toast(error.message,'error')).finally(()=>{recovering=undefined;});
    }
  }, ['workspace','modules']);
  window.addEventListener('pagehide',(event)=>{if(!event.persisted)stopEvents();});

  if (!broadcastChannel) broadcastChannel = new BroadcastChannel('natives-workspace');
  broadcastChannel.onmessage = (e) => { if (e.data?.type === 'revision-invalidated') refreshSession().catch(() => {}); };
  window.addEventListener('pagehide', (event) => {
    if (event.persisted) return;
    if (nativeClient) nativeClient.disconnect();
    suspendInvestmentFeed();
  });
  window.addEventListener('pageshow', (event) => {
    if (!event.persisted) return;
    // BFCache restore: the document and menu survived; reopen the Core Port,
    // resume cards and reconcile against the authoritative snapshot.
    clearTimeout(idleTimer);
    try { nativeClient?.connect?.(); } catch {}
    if (!embeddedAppId) resumeInvestmentFeed();
    void refreshSession().catch((error) => toast(error.message, 'error'));
  });

  let idleTimer;
  document.addEventListener('visibilitychange', () => {
    if (document.hidden) {
      suspendInvestmentFeed();
      idleTimer = setTimeout(() => { if (document.hidden && nativeClient) nativeClient.disconnect(); }, 60_000);
    } else {
      clearTimeout(idleTimer);
      try { nativeClient?.connect?.(); } catch {}
      if (!embeddedAppId) resumeInvestmentFeed();
    }
  });

  // 原生路由还原：刷新或带参数访问时，优先还原指定应用或工作区
  const initialParams = new URLSearchParams(location.search);
  const targetApp = initialParams.get('app');
  if (targetApp) {
    openEmbeddedApp(targetApp, { pushState: false, view: initialParams.get('view') || '' });
  }

  // 浏览器前进/后退监听
  window.addEventListener('popstate', () => {
    const params = new URLSearchParams(location.search);
    const appId = params.get('app');
    const wsId = params.get('workspace');
    if (appId) {
      openEmbeddedApp(appId, { pushState: false, view: params.get('view') || '' });
    } else {
      if (embeddedAppId) {
        leaveEmbeddedView({ pushState: false });
      }
      if (wsId && wsId !== activeWorkspaceId) {
        void activateWorkspaceInner(wsId, { pushState: false });
      }
    }
  });
}

// 内嵌应用视图：点击侧边栏「应用」分区项时不做页面导航，而是在内容区
// 内嵌挂载模块 iframe；会话跨视图保活，空闲 5 分钟后自动回收。
let embeddedSurface, embeddedAppId;
// 进入应用视图时记住工作区树的选中项，退出时恢复（互斥高亮：内容区显示
// 基金时侧边栏只允许「应用」分区高亮，避免基金与个人空间同时选中）。
let rememberedWsActive;
function setAppActive(appId) {
  const items = document.querySelectorAll('#app-menu-apps .app-menu-item');
  items.forEach((item) => {
    const match = appId && item.dataset.kind === NAVIGATION_KIND_APP && item.dataset.appId === appId;
    item.classList.toggle('active', Boolean(match));
    item.setAttribute('aria-current', match ? 'page' : 'false');
  });
}
function showEmbeddedView(showApp) {
  const dashboard = $('dashboard-host');
  if (dashboard) dashboard.hidden = showApp;
  const toolbar = $('dashboard-toolbar');
  if (toolbar) toolbar.hidden = showApp;
  const empty = $('space-empty-state');
  // 恢复时按当前快照的卡片数还原空态显示。dashboard-host 是 Shadow 宿主，
  // 光层 childElementCount 恒为 0，不能用其判断是否有卡片（会误显空态）。
  if (empty) empty.hidden = showApp || (activeSnapshot?.widgets || []).length > 0;
  const stage = $('app-stage');
  if (stage) stage.hidden = !showApp;
  // 工作区树与应用项互斥高亮
  if (showApp) {
    rememberedWsActive = $('workspace-tree')?.querySelector('.ws-item.active') || null;
    rememberedWsActive?.classList.remove('active');
    rememberedWsActive?.querySelector('.ws-select')?.setAttribute('aria-current', 'false');
  } else if (rememberedWsActive?.isConnected) {
    rememberedWsActive.classList.add('active');
    rememberedWsActive.querySelector('.ws-select')?.setAttribute('aria-current', 'page');
  }
  if (!showApp) rememberedWsActive = undefined;
}
function leaveEmbeddedView({ pushState = true } = {}) {
  const surface = embeddedSurface;
  const wasAppId = embeddedAppId;
  embeddedSurface = undefined;
  embeddedAppId = undefined;
  showEmbeddedView(false);
  setAppActive('');
  if (pushState) {
    const url = new URL(location.href);
    url.searchParams.delete('app');
    if (activeWorkspaceId) url.searchParams.set('workspace', activeWorkspaceId);
    if (location.search !== url.search) {
      history.pushState({ type: 'workspace', workspaceId: activeWorkspaceId }, '', url.pathname + (url.search || ''));
    }
  }
  // ADR-0036 P0 修正：返回 Workspace 必须先明确释放嵌入会话（断开 Port、
  // 移除本 surface 监听），完成后才允许投资卡短命查询，避免同 appId 锁
  // 竞争和孤儿会话（旧行为只清引用不释放）。
  Promise.resolve(surface?.dispose?.()).catch(() => {}).finally(() => {
    if (wasAppId && !embeddedAppId) resumeInvestmentFeed();
  });
}
function openEmbeddedApp(appId, { pushState = true, view = '' } = {}) {
  if (embeddedAppId === appId) {
    showEmbeddedView(true);
    setAppActive(appId);
    // 已打开会话的二次导航：经 surface 转发纯导航意图（ADR-0036 §2.4）。
    if (view) embeddedSurface?.navigate?.(view);
    if (pushState) {
      const url = new URL(location.href);
      url.searchParams.set('app', appId);
      if (location.search !== url.search) {
        history.pushState({ type: 'app', appId }, '', url.pathname + url.search);
      }
    }
    return;
  }
  // 新会话前先挂起投资卡查询并取消进行中的 Port（页面一次只持有一个
  // Fund 会话 owner，方案 02 §4）。
  suspendInvestmentFeed();
  embeddedSurface?.dispose?.();
  embeddedSurface = undefined;
  const stage = $('app-stage');
  if (!stage) return;
  embeddedAppId = appId;
  showEmbeddedView(true);
  setAppActive(appId);
  if (pushState) {
    const url = new URL(location.href);
    url.searchParams.set('app', appId);
    if (location.search !== url.search) {
      history.pushState({ type: 'app', appId }, '', url.pathname + url.search);
    }
  }
  embeddedSurface = createEmbeddedAppSurface({
    t,
    appId,
    stage,
    initialView: typeof view === 'string' ? view : '',
    // mode/T06：空间嵌入路径同样由装配层注入设置打开器（动态导入不回经 Shell）。
    openSettings: (context) => import('./model-settings.js').then((m) => m.openModelSettings(context)),
    onClosed: () => { if (embeddedAppId === appId) leaveEmbeddedView(); },
  });
  void embeddedSurface.open();
}
function bindEmbeddedAppNavigation() {
  const nav = $('app-menu-apps');
  if (!nav) return;
  nav.addEventListener('click', (event) => {
    const item = event.target.closest('.app-menu-item');
    if (!item || item.dataset.kind !== NAVIGATION_KIND_APP || !item.dataset.appId) return;
    event.preventDefault();
    openEmbeddedApp(item.dataset.appId);
  });
  globalThis.window?.addEventListener('pagehide', (event) => { if (!event.persisted) embeddedSurface?.dispose?.(); });
}

ensureAuthenticated().then((ok) => {
  if (ok === false) return;
  return init();
}).catch((err) => toast(`${t('pageError', '页面初始化失败')}：${err.message}`));
