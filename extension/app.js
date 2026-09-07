// App Surface shell (ADR-0025 D50).
//
// app.html?app=<appId> — the ONLY browser entry point for an installed app:
//   1. read the authoritative App (apps:get via the Core host port)
//   2. confirm it is installed + enabled
//   3. resolve the UI module from the BUILD-TIME registry (never from the
//      catalog — D2: no online executable download)
//   4. lazy import the module and let it mount into #app-stage
//
// The app's own runtime host is opened by the UI module through
// native-app-client.js (D49: page owns the port; page closed → EOF →
// host exits ≤2 s). 0 Native Port until the app actually needs it.

import { isKnownUiModule, resolveUiModule } from './app-module-registry.js';
import { appHostFor, createAppNativeClient } from './native-app-client.js';
import { createNativeClient } from './native-client.js';

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
  stage,
  title,
  sub,
  toast,
  back,
  getNativeClient = () => createNativeClient({ host: CORE_HOST }),
  createHostClient = (app, handlers) =>
    createAppNativeClient({ app, onDisconnect: handlers?.onDisconnect }),
  t = makeT(),
} = {}) {
  const missing = [appId && 'appId', stage && 'stage'].filter((x) => !x);
  if (missing.length) {
    throw new Error(`createAppShell: missing ${missing.join(', ')}`);
  }
  const toastEl = toast || el('app-toast');
  const titleEl = title || el('app-title');
  const subEl = sub || el('app-sub');
  const backBtn = back || el('app-back');

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
    const client = getNativeClient();
    if (backBtn) backBtn.addEventListener('click', () => {
      openPage('files.html');
      client.disconnect();
    });
    try {
      const detail = await client.call('apps:get', { app_id: appId });
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

      // D9: build-time present, imported only now that install is confirmed.
      const moduleUrl = resolveUiModule(appId);
      const ui = await import(/* @vite-ignore */ moduleUrl);
      const ctx = {
        app,
        detail,
        appId,
        hostName: appHostFor(app),
        stage,
        t,
        setToast,
        createHostClient: (handlers) => createHostClient(app, handlers),
        disconnectHost: (client) => client?.disconnect(),
      };
      if (typeof ui.mountApp !== 'function') {
        throw new Error(`UI module for ${appId} does not export mountApp`);
      }
      const unmount = ui.mountApp(ctx);
      // page unload: close the host port so the runtime exits (D49)
      const cleanup = () => {
        if (unmount && typeof unmount === 'function') {
          try { unmount(); } catch { /* already unmounted */ }
        }
        client.disconnect();
      };
      window?.addEventListener?.('pagehide', cleanup);
      return cleanup;
    } catch (error) {
      setToast(error?.message || String(error), true);
      const code = error?.code || '';
      if (code === 'host_disconnected') {
        renderStatus({
          icon: 'box',
          heading: t('appSurfaceHostOffline', 'Native Host 未连接'),
          body: t('appSurfaceHostOfflineBody', '请确认 Natives Host 正在运行（开发模式：npm run dev）。'),
          actionLabel: t('retry', '重试'),
          onAction: open,
          error: true,
        });
      } else {
        renderStatus({
          icon: 'box',
          heading: t('appSurfaceOpenFailed', '应用无法打开'),
          body: error?.message || String(error),
          actionLabel: t('retry', '重试'),
          onAction: open,
          error: true,
        });
      }
      return undefined;
    }
  };

  function openAppCenter(client) {
    const opened = openPage('apps.html');
    if (!opened) setToast(t('appSurfaceOpenCenterHint', '请在设置中打开应用中心'), true);
    client.disconnect();
  }

  return { open, setToast, renderStatus };
}

// Boot only when this is the real page (guarded for Node tests).
if (typeof document !== 'undefined' && typeof window !== 'undefined'
  && el('app-stage')
  && globalThis.location?.pathname?.endsWith('app.html')) {
  const t = makeT();
  document.querySelectorAll('[data-i18n]').forEach((node) => {
    node.textContent = t(node.dataset.i18n, node.textContent);
  });
  createAppShell({ appId: parseAppId(globalThis.location?.search || '') })
    .then((shell) => shell.open());
}
