// App Center owns its Native Port and projects the fixed built-in modules of
// the complete Natives product (plan §3.4/§4): open, show/hide, preferences
// and data management. Modules are shipped in the complete product; the
// center has no module distribution or remote discovery path.
import { createNativeClient } from './native-client.js';
import { saveAppNavigation, projectionFromApps } from './app-navigation-projection.js';
import { storageGet, storageSet } from './files-preferences.js';
import { appLifecycle } from './app-lifecycle.js';
import { classifyAppError } from './app-errors.js';
import { createAppShell } from './app.js';

// AC-11: standalone pages inherit the product appearance preference and
// follow its changes instead of a hardcoded theme.
Promise.resolve()
  .then(() => storageGet('natives-theme', 'archive'))
  .then((theme) => {
    document.documentElement.dataset.theme = ['volt', 'archive'].includes(theme) ? theme : 'archive';
  })
  .catch(() => {});
globalThis.chrome?.storage?.onChanged?.addListener((changes, area) => {
  if (area === 'local' && changes['natives-theme']) {
    const theme = changes['natives-theme'].newValue;
    document.documentElement.dataset.theme = ['volt', 'archive'].includes(theme) ? theme : 'archive';
  }
});

export function createAppCenter({
  t = (key, fallback) => globalThis.chrome?.i18n?.getMessage?.(key) || fallback || key,
  client,
  storage = globalThis.chrome?.storage,
  list: customList,
  toast: customToast,
} = {}) {
  const list = customList || (typeof document !== 'undefined' ? document.getElementById('apps-list') : null);
  const toast = customToast || (typeof document !== 'undefined' ? document.getElementById('apps-toast') : null);
  const state = { apps: [], modules: [], pendingResets: [], product: null,
    revision: 0, host: null, loading: true, error: null, busy: new Map(), failures: new Map() };
  let toastTimer, idleTimer, disposed = false, suspended = false;
  const dialogs = new Set();
  let lifecycle = appLifecycle();
  const native = client || createNativeClient({
    host: 'com.natives.file_manager', timeoutMs: 20_000,
    writeMethods: new Set(['apps:clear_data', 'apps:set_enabled', 'apps:set_sidebar', 'apps:open_onboarding']),
    onDisconnect: (error, intentional) => {
      state.host = null;
      if (!intentional && !disposed) { state.error = classifyAppError(error); render(); }
    },
  });

  function setToast(message, error = false) {
    if (disposed) return;
    if (toast) {
      clearTimeout(toastTimer);
      toast.textContent = message;
      toast.className = error ? 'apps-toast error' : 'apps-toast';
      toast.hidden = false;
      toastTimer = setTimeout(() => { toast.hidden = true; }, 4000);
    } else if (typeof globalThis.showToast === 'function') {
      globalThis.showToast(message);
    }
  }
  function node(tag, className, text) {
    const el = document.createElement(tag);
    if (className) el.className = className;
    if (text !== undefined) el.textContent = text;
    return el;
  }
  function localized(value) {
    if (value == null) return '';
    if (typeof value !== 'object') return String(value);
    const lang = globalThis.chrome?.i18n?.getUILanguage?.() || 'zh_CN';
    return value[lang.startsWith('en') ? 'en' : 'zh_CN'] || value.zh_CN || value.en || '';
  }
  function action(icon, label, run, { danger = false, primary = false, disabled = false } = {}) {
    const button = node('button', `action${danger ? ' danger' : ''}${primary ? ' primary' : ''}`);
    button.type = 'button';
    button.title = label;
    button.innerHTML = `<svg class="icon" aria-hidden="true"><use href="#i-${icon}" /></svg>`;
    button.append(node('span', '', label));
    button.disabled = disabled;
    button.onclick = async () => {
      button.disabled = true;
      try { await run(); }
      catch (error) { setToast(t(classifyAppError(error)), true); }
      finally { button.disabled = disabled; scheduleIdle(); }
    };
    return button;
  }
  function scheduleIdle() {
    clearTimeout(idleTimer);
    if (document.visibilityState !== 'hidden' || disposed || suspended) return;
    idleTimer = setTimeout(() => {
      if (state.busy.size || native.inFlight) { scheduleIdle(); return; }
      native.disconnect?.();
    }, 60_000);
  }
  async function handshake() {
    state.host = null;
    const origin = globalThis.chrome?.runtime?.getURL?.('') || globalThis.location?.origin;
    const host = await native.call('apps:handshake', { origin });
    if (host.appsProtocolVersion !== 5) {
      throw Object.assign(new Error('app host update required'), { code: 'APP_HOST_UPDATE_REQUIRED' });
    }
    state.host = host;
    return state.host;
  }
  async function refresh() {
    if (disposed || suspended) return;
    try {
      const snapshot = await native.call('apps:list');
      if (disposed || suspended) return;
      state.apps = snapshot.apps || [];
      state.modules = snapshot.modules || [];
      state.pendingResets = snapshot.pendingDataResets || [];
      state.product = snapshot.product || null;
      state.revision = snapshot.revision;
      state.error = state.host ? null : (state.error || 'appsHostOffline');
      await saveAppNavigation(projectionFromApps(state.apps, state.revision), storage);
    } catch (error) { if (!disposed) state.error = classifyAppError(error); }
    render();
    scheduleIdle();
  }
  async function reconnect() {
    // A failed handshake stops every dependent call; there is no fallback host.
    try { await handshake(); }
    catch (error) { state.error = classifyAppError(error); render(); return; }
    await refresh();
  }
  async function stopOwner(appId) {
    // Notify same-profile pages immediately, then use the documented runtime
    // message so the actual app.html owner closes its direct Native Port.
    lifecycle.notify('stop', appId);
    try {
      const result = await globalThis.chrome?.runtime?.sendMessage?.({ type: 'natives-app-stop', appId });
      return result?.stopped === true;
    } catch {
      // No local owner is normal. Core's runtime lock remains the authority
      // for another browser profile or an uncooperative process.
      return false;
    }
  }
  function items() {
    const entries = new Map();
    // Fixed modules come from the Host's product manifest projection: an
    // empty preference table still shows every built-in module of the product.
    for (const module of state.modules) {
      entries.set(module.appId, {
        app_id: module.appId,
        name: localized(module.name) || module.appId,
        description: module.description || null,
        icon: module.icon || 'grid',
        entryRoute: module.entryRoute || '',
        present: Boolean(module.present),
        configured: Boolean(module.configured),
        enabled: module.enabled !== false,
        showInSidebar: module.showInSidebar !== false,
        sidebarOrder: module.sidebarOrder || 0,
        fixed: true,
      });
    }
    for (const app of state.apps) {
      const entry = entries.get(app.app_id) || {
        app_id: app.app_id,
        name: app.name || app.app_id,
        description: null,
        icon: 'box',
        entryRoute: `app.html?app=${encodeURIComponent(app.app_id)}`,
        fixed: false,
      };
      Object.assign(entry, {
        registered: true,
        version: app.version,
        enabled: app.enabled,
        showInSidebar: app.show_in_sidebar,
        needsMigration: app.needs_migration,
        hostRegistered: app.host_registered,
        sidebarOrder: app.sidebar_order,
      });
      entries.set(app.app_id, entry);
    }
    return [...entries.values()].sort((a, b) => String(a.name).localeCompare(String(b.name)));
  }
  function statusRow(message, retry) {
    const row = node('div', 'apps-notice');
    row.setAttribute('role', 'status');
    row.append(node('span', '', t(message)));
    if (retry) row.append(action('refresh', t('retry'), retry));
    return row;
  }
  function render() {
    if (disposed || suspended || !list) return;
    list.replaceChildren();
    if (state.error) list.append(statusRow(state.error, reconnect));
    // Product projection is prepared by the first verified Host handshake;
    // the center never exposes a module installation/configuration action.
    if (!state.error && state.product && !state.product.sourcePresent) {
      list.append(statusRow('appsProductSourceMissing'));
    }
    const entries = items();
    if (state.loading && !entries.length) list.append(statusRow('appsLoading'));
    if (!entries.length && !state.loading && !state.error) {
      list.append(node('div', 'apps-empty', t('appsEmpty')));
    }
    for (const entry of entries) list.append(card(entry));
    // §1.3：用户可随时重新查看同版本安装说明（固定安装路径，Host 打开）。
    const guide = node('div', 'apps-notice');
    guide.append(action('open', t('appsViewGuide'), async () => {
      await native.call('apps:open_onboarding', {});
    }));
    list.append(guide);
  }
  function toggle(label, checked, onChange) {
    const container = node('label', 'apps-toggle');
    const input = node('input');
    input.type = 'checkbox'; input.checked = Boolean(checked);
    input.disabled = state.busy.size > 0;
    input.onchange = async () => {
      try { await onChange(input.checked); }
      catch (error) { setToast(t(classifyAppError(error)), true); }
      await refresh();
    };
    container.append(input, node('span', '', label));
    return container;
  }
  function card(entry) {
    const article = node('article', 'app-card');
    article.dataset.appId = entry.app_id;
    article.setAttribute('role', 'listitem');
    const icon = ['grid', 'box', 'target'].includes(entry.icon) ? entry.icon : 'box';
    const thumb = node('div', 'thumb');
    thumb.innerHTML = `<svg class="icon" aria-hidden="true"><use href="#i-${icon}" /></svg>`;
    const body = node('div', 'body');
    body.append(node('div', 'name', entry.name || entry.app_id));
    const description = localized(entry.description);
    if (description) body.append(node('div', 'desc', description));
    const meta = node('div', 'meta');
    const version = entry.version;
    if (version) meta.append(node('span', '', `v${version}`));
    // §4.1: no store fields. Data usage stays honestly "not reported"
    // until a module owner provides real numbers (plan §3.4).
    meta.append(node('span', '', `${t('appsUserDataUsage')}: ${t('appsUsageNotReported')}`));
    body.append(meta);
    const actions = node('div', 'actions');
    const busy = state.busy.get(entry.app_id);
    const disabled = state.busy.size > 0;
    const resetPending = state.pendingResets.includes(entry.app_id);
    if (busy) {
      actions.append(node('span', 'status', t(busy.stage)));
    } else {
      let status;
      if (resetPending) status = 'appsClearing';
      else if (entry.needsMigration) status = 'appsNeedsMigration';
      else if (entry.registered && !entry.hostRegistered) status = 'appsRepairRequired';
      else if (entry.registered) status = entry.enabled ? 'appsReady' : 'appsDisabled';
      else if (entry.fixed) status = entry.present && entry.configured ? 'appsReady' : 'appsProductSourceMissing';
      else status = 'appsProductSourceMissing';
      actions.append(node('span', 'status', t(status)));
      const openable = (entry.registered || (entry.fixed && entry.present))
        && entry.enabled && !resetPending
        && !entry.needsMigration;
      if (openable) {
        article.classList.add('clickable');
        article.onclick = (e) => {
          if (e.target.closest('button, input, label, a')) return;
          openApp(entry);
        };
        actions.append(action('open', t('appsOpen'), () => openApp(entry), { primary: true, disabled }));
      }
      // Preferences only exist once the module is configured (an apps row
      // exists); a not-yet-configured module has no row to toggle.
      if (entry.registered) {
        body.append(toggle(t('appsEnabled'), entry.enabled, async (enabled) => {
          if (!enabled) { await stopOwner(entry.app_id); lifecycle.notify('maintenance', entry.app_id); }
          try { await native.call('apps:set_enabled', { appId: entry.app_id, enabled }); }
          finally { lifecycle.notify('changed', entry.app_id); }
        }));
        body.append(toggle(t('appsShowInSidebar'), entry.showInSidebar, (show) =>
          native.call('apps:set_sidebar', { appId: entry.app_id, show })));
        // §4.3: data management is the only destructive action, fully
        // separate from product code; it keeps code, registration
        // and preferences and requires double confirmation.
        actions.append(action('trash', t('appsClearData'), () => confirmClearData(entry), { danger: true, disabled }));
      }
    }
    const failure = state.failures.get(entry.app_id);
    if (failure) body.append(node('div', 'apps-inline-error', t(failure)));
    article.append(thumb, body, actions);
    return article;
  }

  async function clearData(entry, { credentials = false } = {}) {
    if (state.busy.size) throw Object.assign(new Error('app busy'), { code: 'APP_BUSY' });
    state.busy.set(entry.app_id, { stage: 'appsClearing' });
    state.failures.delete(entry.app_id);
    render();
    await stopOwner(entry.app_id);
    lifecycle.notify('maintenance', entry.app_id);
    try {
      const unique = globalThis.crypto?.randomUUID?.() || `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
      await native.call('apps:clear_data', {
        appId: entry.app_id,
        requestId: `${entry.app_id}:${unique}`,
        confirmPurge: true,
        deleteImports: true,
        deleteCache: true,
        deleteLogs: true,
        deleteCredentials: credentials,
      });
      setToast(t('appsDataDeleted'));
    } catch (error) { state.failures.set(entry.app_id, classifyAppError(error)); throw error; }
    finally {
      lifecycle.notify('changed', entry.app_id);
      state.busy.delete(entry.app_id);
      await refresh();
    }
  }
  async function confirmClearData(entry) {
    // Confirmation page lists the actual scope (§4.3): data, imported
    // originals, cache and logs are cleared; sign-in credentials are kept
    // unless separately confirmed. Code and preferences are never touched.
    const choice = await dialog({ title: t('appsClearData'),
      body: `${entry.name}\n${t('appsClearDataScope')}`,
      confirmLabel: t('appsClearData'), checkboxLabel: t('appsDeleteCredentials'),
      checked: false, danger: true });
    if (!choice) return;
    if (!await dialog({ title: t('appsPurgeConfirmTitle'), body: `${entry.name}\n${t('appsPurgeWarning')}`,
      confirmLabel: t('appsClearData'), danger: true })) return;
    await clearData(entry, { credentials: Boolean(choice.purgeData) });
  }
  function dialog({ title, body, confirmLabel, checkboxLabel, checked, danger }) {
    return new Promise((resolve) => {
      const previous = document.activeElement;
      const backdrop = node('div', 'apps-dialog-backdrop');
      backdrop.innerHTML = '<div class="apps-dialog" role="alertdialog" aria-modal="true" aria-labelledby="apps-dialog-title"><h2 id="apps-dialog-title"></h2><p></p><div class="actions"><button class="action" type="button" data-role="cancel"></button><button class="action" type="button" data-role="ok"></button></div></div>';
      backdrop.querySelector('h2').textContent = title;
      backdrop.querySelector('p').textContent = body;
      const cancel = backdrop.querySelector('[data-role="cancel"]');
      const ok = backdrop.querySelector('[data-role="ok"]');
      cancel.textContent = t('cancel'); ok.textContent = confirmLabel;
      let input;
      if (checkboxLabel) {
        const label = node('label', 'apps-purge-choice');
        input = node('input'); input.type = 'checkbox'; input.checked = Boolean(checked);
        label.append(input, node('span', '', checkboxLabel));
        const actions = backdrop.querySelector('.actions');
        actions.remove();
        backdrop.querySelector('.apps-dialog').append(label, actions);
        input.onchange = () => ok.classList.toggle('danger', input.checked);
      }
      ok.classList.toggle('danger', Boolean(danger || checked));
      const close = (result) => { dialogs.delete(close); backdrop.remove(); previous?.focus?.(); resolve(result); };
      dialogs.add(close);
      cancel.onclick = () => close(null);
      ok.onclick = () => close({ purgeData: Boolean(input?.checked) });
      backdrop.onclick = (event) => { if (event.target === backdrop) close(null); };
      backdrop.onkeydown = (event) => {
        if (event.key === 'Escape') { event.preventDefault(); close(null); }
        if (event.key === 'Tab') {
          const focusable = [...backdrop.querySelectorAll('button,input')];
          const current = focusable.indexOf(document.activeElement);
          const next = (current + (event.shiftKey ? -1 : 1) + focusable.length) % focusable.length;
          event.preventDefault(); focusable[next]?.focus();
        }
      };
      const container = list?.closest?.('dialog') || document.body;
      container.append(backdrop);
      cancel.focus();
    });
  }
  // 内嵌打开：模块视图渲染在本页内容区内（#app-stage），左侧导航保持
  // 不变；重复点击聚焦已有会话，不再另开 app.html 标签页（§4 去重语义
  // 由内嵌 shell 的 run 计数承担）。
  let embeddedShell, embeddedAppId;
  function openApp(entry) {
    const appId = entry.app_id || parseEmbeddedAppId(entry);
    if (!appId) return;
    const stage = document.getElementById('app-stage');
    if (!stage) return openAppExternal(entry);
    const head = document.querySelector('.apps-content-head');
    if (embeddedShell && embeddedAppId === appId) {
      list.hidden = true;
      if (head) head.hidden = true;
      stage.hidden = false;
      return;
    }
    closeEmbedded();
    list.hidden = true;
    if (head) head.hidden = true;
    stage.hidden = false;
    embeddedAppId = appId;
    embeddedShell = createAppShell({ appId, stage });
    void embeddedShell.open();
  }
  function parseEmbeddedAppId(entry) {
    const route = entry.entryRoute || '';
    return new URLSearchParams(route.split('?')[1] || '').get('app') || entry.appId || '';
  }
  function closeEmbedded() {
    embeddedShell?.dispose?.();
    embeddedShell = undefined;
    embeddedAppId = undefined;
    const stage = document.getElementById('app-stage');
    if (stage) { stage.replaceChildren(); stage.hidden = true; }
    const head = document.querySelector('.apps-content-head');
    if (head) head.hidden = false;
    if (list) list.hidden = false;
  }
  // 兜底：无内嵌容器时保持原有独立页打开路径。
  function openAppExternal(entry) {
    const route = entry.entryRoute || `app.html?app=${encodeURIComponent(entry.app_id)}`;
    const url = globalThis.chrome?.runtime?.getURL ? globalThis.chrome.runtime.getURL(route) : route;
    const base = url.split('?')[0];
    if (globalThis.chrome?.tabs?.query) {
      chrome.tabs.query({ url: `${base}*` }, (tabs) => {
        const existing = Array.isArray(tabs) ? tabs[0] : null;
        if (existing?.id !== undefined) {
          chrome.tabs.update(existing.id, { active: true });
          chrome.windows?.update?.(existing.windowId, { focused: true }).catch?.(() => {});
        } else if (chrome.tabs.create) chrome.tabs.create({ url });
        else globalThis.window?.open(url, '_blank');
      });
    } else if (chrome.tabs.create) chrome.tabs.create({ url });
    else globalThis.window?.open(url, '_blank');
  }
  function suspend() {
    suspended = true;
    closeEmbedded();
    for (const close of dialogs) close(null);
    native.disconnect?.();
    lifecycle.close();
    clearTimeout(toastTimer); clearTimeout(idleTimer);
  }
  function resume(event) {
    if (!event.persisted || disposed || !suspended) return;
    suspended = false;
    lifecycle = appLifecycle();
    void reconnect();
  }
  function dispose() {
    disposed = true;
    suspend();
    document.removeEventListener?.('visibilitychange', scheduleIdle);
    globalThis.window?.removeEventListener('pagehide', suspend);
    globalThis.window?.removeEventListener('pageshow', resume);
  }
  globalThis.window?.addEventListener('pagehide', suspend);
  globalThis.window?.addEventListener('pageshow', resume);
  document.addEventListener?.('visibilitychange', scheduleIdle);
  const back = document.getElementById('nav-back');
  if (back) back.onclick = () => { dispose(); globalThis.location.assign('space.html'); };
  const navCenter = document.getElementById('nav-center');
  if (navCenter) navCenter.onclick = () => { closeEmbedded(); };

  // 侧栏折叠：与空间模块同一交互（按钮 / ⌘B，状态持久化）。
  // 折叠后整个侧栏隐藏，左上角浮动 icon 按钮负责重新展开。
  const toggleSidebarBtn = document.getElementById('apps-toggle-sidebar-btn');
  const expandSidebarBtn = document.getElementById('apps-expand-sidebar-btn');
  const applySidebarCollapsed = (collapsed) => {
    document.body.classList.toggle('apps-sidebar-collapsed', collapsed);
    if (toggleSidebarBtn) {
      toggleSidebarBtn.setAttribute('aria-pressed', String(collapsed));
      const title = collapsed ? (t('expandSidebar', '展开侧栏')) : (t('collapseSidebar', '折叠侧栏'));
      toggleSidebarBtn.title = title;
      toggleSidebarBtn.setAttribute('aria-label', title);
    }
    if (expandSidebarBtn) {
      expandSidebarBtn.hidden = !collapsed;
      expandSidebarBtn.setAttribute('aria-pressed', String(!collapsed));
    }
    const frame = document.querySelector('#app-stage iframe, #app-stage .managed-app-frame');
    frame?.contentWindow?.postMessage({ type: 'sidebar-state', collapsed }, '*');
  };
  if (toggleSidebarBtn) {
    toggleSidebarBtn.onclick = () => {
      const collapsed = !document.body.classList.contains('apps-sidebar-collapsed');
      applySidebarCollapsed(collapsed);
      storageSet('natives-apps-sidebar-collapsed', collapsed).catch(() => {});
    };
    storageGet('natives-apps-sidebar-collapsed', false).then(applySidebarCollapsed).catch(() => {});
  }
  if (expandSidebarBtn) {
    expandSidebarBtn.onclick = () => {
      applySidebarCollapsed(false);
      storageSet('natives-apps-sidebar-collapsed', false).catch(() => {});
    };
  }
  document.addEventListener('keydown', (event) => {
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'b') {
      event.preventDefault();
      toggleSidebarBtn?.click();
    }
  });

  document.querySelectorAll('[data-i18n]').forEach((el) => { el.textContent = t(el.dataset.i18n, el.textContent); });
  const ready = reconnect();
  return { refresh, state, dispose, ready };
}

if (typeof document !== 'undefined' && document.getElementById('apps-list')) createAppCenter();
