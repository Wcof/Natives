// App Surface shell (ADR-0026).
//
// app.html?app=<appId> — the ONLY browser entry point for an installed app:
//   1. read the authoritative App (apps:get via the Core host port)
//   2. confirm it is installed + enabled
//   3. resolve the UI module from the BUILD-TIME registry (never from the
//      catalog — D2: no online executable download)
//   4. lazy import the module and let it mount into #app-stage
//
// App modules share native-file-host through a page-owned port; page close
// drives stdin EOF. There is no per-App Native Host or downloaded runtime.

import { isKnownUiModule, resolveUiModule } from './app-module-registry.js';
import { appHostFor, createAppNativeClient } from './native-app-client.js';
import { createNativeClient } from './native-client.js';
import { classifyAppError } from './app-catalog-policy.js';
import { appLifecycle } from './app-lifecycle.js';

const CORE_HOST = 'com.natives.file_manager';

function el(id) {
  return (typeof document !== 'undefined' ? document : null)?.getElementById(id);
}

function makeT() {
  return (key, fallback) => {
    const msg = typeof globalThis.chrome?.i18n?.getMessage === 'function'
      ? globalThis.chrome.i18n.getMessage(key)
      : '';
    return msg || fallback;
  };
}

export function parseAppId(search, locationHref = '') {
  let s = search;
  if (!s && locationHref) {
    const i = locationHref.indexOf('?');
    if (i >= 0) s = locationHref.slice(i);
  }
  try {
    const url = new URLSearchParams(s.startsWith('?') ? s.slice(1) : s);
    return url.get('app') || '';
  } catch {
    return '';
  }
}

export function createAppShell({
  appId,
  stage = el('app-stage'),
  title,
  sub,
  toast,
  back,
  getNativeClient = () => createNativeClient({ host: CORE_HOST }),
  createHostClient = (app, handlers) =>
    createAppNativeClient({ app, onDisconnect: handlers?.onDisconnect }),
  t = makeT(),
} = {}) {
  const missing = Object.entries({ appId, stage }).filter(([, value]) => !value).map(([key]) => key);
  if (missing.length) {
    throw new Error(`createAppShell: missing ${missing.join(', ')}`);
  }
  const toastEl = toast || el('app-toast');
  const titleEl = title || el('app-title');
  const subEl = sub || el('app-sub');
  const backBtn = back || el('app-back');
  let generation = 0;
  let coreClient;
  let unmount;
  let idleTimer;
  const hosts = new Set();
  const stop = () => {
    generation++;
    clearTimeout(idleTimer);
    try { unmount?.(); } finally {
      unmount = null;
      for (const host of hosts) host.disconnect();
      hosts.clear();
      coreClient?.disconnect();
    }
  };
  const scheduleIdle = () => {
    clearTimeout(idleTimer);
    if (document.visibilityState !== 'hidden') return;
    idleTimer = setTimeout(() => {
      if (coreClient?.inFlight || [...hosts].some((host) => host.inFlight)) { scheduleIdle(); return; }
      stop();
      renderStatus({ icon: 'box', heading: t('appSurfacePaused'), actionLabel: t('retry'), onAction: open });
    }, 60_000);
  };
  const listen = () => appLifecycle(({ type, appId: changedId }) => {
    if (changedId !== appId) return;
    if (type === 'maintenance') {
      stop();
      renderStatus({ icon: 'box', heading: t('appsCommitting') });
    } else void open();
  });
  let lifecycle = listen();
  const pagehide = () => { stop(); lifecycle.close(); };
  const pageshow = (event) => { if (event.persisted) { lifecycle = listen(); void open(); } };
  globalThis.window?.addEventListener('pagehide', pagehide);
  globalThis.window?.addEventListener('pageshow', pageshow);
  document.addEventListener?.('visibilitychange', scheduleIdle);

  function setToast(message, isError = false) {
    if (!toastEl) return;
    if (!message) {
      toastEl.hidden = true;
      toastEl.textContent = '';
      return;
    }
    toastEl.textContent = message;
    toastEl.className = isError ? 'app-toast error' : 'app-toast';
    toastEl.hidden = false;
  }

  // Navigate back into the workspace shell / App Center page.
  function openPage(name) {
    if (typeof globalThis.open !== 'function') return false;
    const url = globalThis.chrome?.runtime?.getURL?.(name)
      || `${globalThis.location?.href?.split('#')[0] || ''}${name}`;
    const win = globalThis.open(url, '_blank');
    return Boolean(win);
  }

  function renderStatus({ icon, heading, body, actionLabel, onAction, error = false }) {
    stage.innerHTML = '';
    const box = document.createElement('div');
    box.className = 'app-status';
    box.innerHTML = `<svg class="icon"><use href="#i-${icon}" /></svg>`;
    const h2 = document.createElement('h2');
    h2.textContent = heading;
    const p = document.createElement('p');
    p.textContent = body || '';
    box.append(h2);
    if (body) box.append(p);
    if (actionLabel) {
      const btn = document.createElement('button');
      btn.className = 'action primary';
      btn.type = 'button';
      btn.textContent = actionLabel;
      btn.addEventListener('click', () => onAction && onAction());
      box.append(btn);
    }
    if (error) box.classList.add('error');
    stage.append(box);
  }

  const open = async () => {
    stop();
    setToast('');
    const current = generation;
    const client = getNativeClient();
    coreClient = client;
    if (backBtn) backBtn.onclick = () => { stop(); openPage('apps.html'); };
    try {
      const detail = await client.call('apps:get', { appId });
      if (current !== generation) return;
      const app = detail?.app;
      if (!app) throw Object.assign(new Error(t('appSurfaceNotFound', '应用不存在或已卸载')), { code: 'APP_NOT_FOUND' });
      if (titleEl) titleEl.textContent = app.name;
      if (subEl) subEl.textContent = `${app.kind} · ${app.version}`;

      // D2/D51: UI code comes ONLY from the build-time registry.
      if (!isKnownUiModule(appId)) {
        renderStatus({
          icon: 'box',
          heading: t('appSurfaceNeedsUpdate', '需要更新 Natives 后才能打开'),
          body: t('appSurfaceNeedsUpdateBody', '当前 Natives 不包含此应用的界面模块，请更新 Natives 后重试。'),
          actionLabel: t('appSurfaceOpenCenter', '打开应用中心'),
          onAction: () => openAppCenter(client),
        });
        return;
      }
      if (!app.enabled) {
        renderStatus({
          icon: 'grid',
          heading: t('appSurfaceDisabled', '应用已停用'),
          body: t('appSurfaceDisabledBody', '请在应用中心重新启用后打开。'),
          actionLabel: t('appSurfaceOpenCenter', '打开应用中心'),
          onAction: () => openAppCenter(client),
        });
        return;
      }
      if (app.needs_migration) {
        renderStatus({
          icon: 'box',
          heading: t('appSurfaceNeedsMigration', '应用需要升级迁移'),
          body: t('appSurfaceNeedsMigrationBody', '此应用来自旧版本架构，请在应用中心完成资源升级迁移后使用。'),
          actionLabel: t('appSurfaceOpenCenter', '打开应用中心'),
          onAction: () => openAppCenter(client),
        });
        return;
      }
      if (app.recovery_pending) {
        renderStatus({
          icon: 'box',
          heading: t('appSurfaceRepairRequired', '应用需要修复'),
          actionLabel: t('appSurfaceOpenCenter', '打开应用中心'),
          onAction: () => openAppCenter(client),
        });
        return;
      }

      // D9: build-time present, imported only now that install is confirmed.
      const moduleUrl = resolveUiModule(appId);
      const ui = await import(/* @vite-ignore */ moduleUrl);
      if (current !== generation) return;
      const ctx = {
        app,
        detail,
        appId,
        hostName: appHostFor(app),
        stage,
        t,
        setToast: (...args) => { if (current === generation) setToast(...args); },
        createHostClient: (handlers) => {
          const host = createHostClient(app, handlers);
          hosts.add(host);
          return host;
        },
        disconnectHost: (client) => client?.disconnect(),
      };
      if (typeof ui.mountApp !== 'function') {
        throw new Error(`UI module for ${appId} does not export mountApp`);
      }
      unmount = ui.mountApp(ctx);
      scheduleIdle();
      return stop;
    } catch (error) {
      if (current !== generation) return;
      stop();
      setToast(t(classifyAppError(error)), true);
      const code = error?.code || '';
      if (code === 'host_disconnected') {
        renderStatus({
          icon: 'box',
          heading: t('appSurfaceHostOffline', 'Native Host 未连接'),
          body: t('appsHostOffline'),
          actionLabel: t('retry', '重试'),
          onAction: open,
          error: true,
        });
      } else {
        renderStatus({
          icon: 'box',
          heading: t('appSurfaceOpenFailed', '应用无法打开'),
          body: t(classifyAppError(error)),
          actionLabel: t('retry', '重试'),
          onAction: open,
          error: true,
        });
      }
      return undefined;
    } finally {
      client.disconnect();
    }
  };

  function openAppCenter(client) {
    const opened = openPage('apps.html');
    if (!opened) setToast(t('appSurfaceOpenCenterHint', '请在设置中打开应用中心'), true);
    client.disconnect();
  }

  const dispose = () => {
    pagehide();
    document.removeEventListener?.('visibilitychange', scheduleIdle);
    globalThis.window?.removeEventListener('pagehide', pagehide);
    globalThis.window?.removeEventListener('pageshow', pageshow);
  };
  return { open, setToast, renderStatus, dispose };
}

// Boot only when this is the real page (guarded for Node tests).
if (typeof document !== 'undefined' && typeof window !== 'undefined'
  && el('app-stage')
  && globalThis.location?.pathname?.endsWith('app.html')) {
  const t = makeT();
  document.querySelectorAll('[data-i18n]').forEach((node) => {
    node.textContent = t(node.dataset.i18n, node.textContent);
  });
  createAppShell({ appId: parseAppId(globalThis.location?.search || '') }).open();
}
