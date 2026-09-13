// Generic owner page for ADR-0027 managed applications.
// Core verifies installation only; business traffic goes directly to the app.
import { createAppNativeClient } from './native-app-client.js';
import { createNativeClient } from './native-client.js';
import { classifyAppError } from './app-errors.js';
import { appLifecycle } from './app-lifecycle.js';
import { storageGet } from './files-preferences.js';

// AC-11: standalone owner page follows the product appearance preference.
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
globalThis.chrome?.storage?.onChanged?.addListener((changes, area) => {
  if (area === 'local' && changes['natives-theme']) {
    const theme = changes['natives-theme'].newValue;
    document.documentElement.dataset.theme = ['volt', 'archive'].includes(theme) ? theme : 'archive';
  }
});

const CORE_HOST = 'com.natives.file_manager';
const IDLE_MS = 60_000;
const el = (id) => typeof document === 'undefined' ? null : document.getElementById(id);
const makeT = () => (key, fallback = key) => globalThis.chrome?.i18n?.getMessage?.(key) || fallback;
const randomId = () => {
  if (!globalThis.crypto?.getRandomValues) throw Object.assign(new Error('OS randomness unavailable'), { code: 'APP_START_FAILED' });
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  return btoa(String.fromCharCode(...bytes)).replaceAll('+', '-').replaceAll('/', '_').replaceAll('=', '');
};

export function parseAppId(search, locationHref = '') {
  try {
    const value = search || (locationHref ? new URL(locationHref).search : '');
    return new URLSearchParams(value.replace(/^\?/, '')).get('app') || '';
  } catch { return ''; }
}

export function validLoopbackPort(port) {
  return Number.isInteger(port) && port > 0 && port <= 65_535;
}

export function createAppShell({
  appId, stage = el('app-stage'), title = el('app-title'), sub = el('app-sub'),
  toast = el('app-toast'), back = el('app-back'),
  getNativeClient = () => createNativeClient({ host: CORE_HOST }),
  createHostClient = (app, handlers) => createAppNativeClient({ app, ...handlers }),
  t = makeT(),
} = {}) {
  if (!appId || !stage) throw new Error('createAppShell: appId and stage are required');
  let run = 0, host, frame, instanceId, generation, challenge, idleTimer, busy = false, loadCount = 0, helloReceived = false;

  function setToast(message, error = false) {
    if (!toast) return;
    toast.textContent = message || '';
    toast.className = error ? 'app-toast error' : 'app-toast';
    toast.hidden = !message;
  }
  function renderStatus({ heading, body = '', actionLabel, onAction, error = false }) {
    stage.replaceChildren();
    const box = document.createElement('div'); box.className = `app-status${error ? ' error' : ''}`;
    const h2 = document.createElement('h2'); h2.textContent = heading; box.append(h2);
    if (body) { const p = document.createElement('p'); p.textContent = body; box.append(p); }
    if (actionLabel) {
      const button = document.createElement('button'); button.type = 'button'; button.className = 'action primary';
      button.textContent = actionLabel; button.onclick = onAction; box.append(button);
    }
    stage.append(box);
  }
  function disconnect() {
    clearTimeout(idleTimer);
    frame?.remove(); frame = undefined;
    host?.disconnect(); host = undefined;
    instanceId = undefined; generation = undefined; challenge = undefined; busy = false;
  }
  async function stop(reason = 'user', notify = true) {
    ++run;
    const client = host, id = instanceId;
    if (client && id && notify) {
      busy = true;
      try { await client.call('app:stop', { instanceId: id, reason, requestId: randomId() }); }
      catch { /* disconnect drives the same bounded EOF shutdown path */ }
    }
    disconnect();
  }
  function scheduleIdle() {
    clearTimeout(idleTimer);
    if (document.visibilityState !== 'hidden' || busy) return;
    idleTimer = setTimeout(async () => {
      if (host?.inFlight) return scheduleIdle();
      await stop('hidden');
      renderStatus({ heading: t('appSurfacePaused', '应用已停止'), actionLabel: t('retry', '重新打开'), onAction: open });
    }, IDLE_MS);
  }
  async function createSandbox(port, current) {
    const rotated = await host.call('app:session', { instanceId, op: 'rotate', generation });
    generation = rotated.newGeneration;
    if (current !== run || typeof generation !== 'string') return;
    challenge = randomId();
    frame = document.createElement('iframe');
    frame.className = 'managed-app-frame';
    frame.setAttribute('sandbox', 'allow-scripts allow-forms');
    frame.title = title?.textContent || appId;
    frame.addEventListener('load', async () => {
      // 双阶段握手的意外导航判定：只有会话建立（hello 收到）之后的再次
      // 加载才是未授权导航。iframe 的 about:blank 初始加载会先触发一次
      // load（引擎行为差异），把它当作握手起点而不是导航逃逸。
      if (helloReceived) {
        await stop('page_closing');
        renderStatus({ heading: t('appSurfaceOpenFailed', '应用无法打开'), body: t('appSurfaceUnexpectedNavigation', '应用页面发生了未授权导航。'), actionLabel: t('retry', '重试'), onAction: open, error: true });
        return;
      }
      frame?.contentWindow?.postMessage({ type: 'init', generation, challenge }, '*');
    });
    helloReceived = false;
    frame.src = `http://127.0.0.1:${port}/`;
    stage.replaceChildren(frame);
  }
  async function onWindowMessage(event) {
    if (!frame || event.source !== frame.contentWindow || event.origin !== 'null' || event.data?.type !== 'hello') return;
    helloReceived = true;
    if (event.data.generation !== generation || event.data.challenge !== challenge) return;
    try {
      const issued = await host.call('app:session', { instanceId, op: 'issue', challenge });
      if (!frame || issued.generation !== generation) return;
      frame.contentWindow.postMessage({ type: 'welcome', generation, challenge, token: issued.token, expiresAt: issued.expiresAt }, '*');
    } catch (error) { setToast(t(classifyAppError(error)), true); }
  }
  async function open() {
    await stop('maintenance', false);
    const current = run;
    setToast('');
    renderStatus({ heading: t('appsStarting', '正在启动…') });
    const core = getNativeClient();
    try {
      const detail = await core.call('apps:get', { appId });
      core.disconnect();
      if (current !== run) return;
      const app = detail?.app;
      if (!app) throw Object.assign(new Error('app not found'), { code: 'APP_NOT_FOUND' });
      if (title) title.textContent = app.name;
      if (sub) sub.textContent = app.version;
      if (!app.enabled) {
        renderStatus({ heading: t('appSurfaceDisabled', '应用已停用'), body: t('appSurfaceDisabledBody', '请在应用中心重新启用。') });
        return;
      }
      if (app.needs_migration) {
        renderStatus({ heading: t('appSurfaceNeedsMigration', '应用需要升级迁移'), body: t('appSurfaceNeedsMigrationBody', '请在应用中心重新安装新版。') });
        return;
      }
      if (app.recovery_pending) {
        renderStatus({ heading: t('appSurfaceRepairRequired', '应用需要修复') });
        return;
      }
      if (app.kind !== 'managed_local' || !app.runtime_host) {
        renderStatus({ heading: t('appSurfaceOpenFailed', '应用无法打开'), body: t('appSurfaceNeedsMigrationBody', '请在应用中心重新安装新版。'), error: true });
        return;
      }
      host = createHostClient(app, { onDisconnect: (_error, intentional) => {
        if (!intentional && current === run) renderStatus({ heading: t('appSurfaceHostOffline', '应用已退出'), actionLabel: t('retry', '重试'), onAction: open, error: true });
      }});
      const handshake = await host.call('app:handshake', { protocolVersion: 1, expectedAppId: app.app_id, expectedVersion: app.version });
      if (handshake.protocolVersion !== 1 || handshake.appId !== app.app_id || handshake.appVersion !== app.version) {
        throw Object.assign(new Error('app protocol mismatch'), { code: 'APP_PROTOCOL_MISMATCH' });
      }
      const expectedActivationGeneration = app.activation_generation ?? app.activationGeneration ?? app.revision;
      const started = await host.call('app:start', { requestId: randomId(), expectedActivationGeneration });
      if (started.state !== 'ready' || !validLoopbackPort(started.port) || typeof started.instanceId !== 'string' || typeof started.generation !== 'string') {
        throw Object.assign(new Error('invalid start result'), { code: 'APP_START_FAILED' });
      }
      instanceId = started.instanceId; generation = started.generation; loadCount = 0;
      await createSandbox(started.port, current);
      scheduleIdle();
    } catch (error) {
      core.disconnect(); disconnect();
      if (current !== run) return;
      setToast(t(classifyAppError(error)), true);
      renderStatus({ heading: t('appSurfaceOpenFailed', '应用无法打开'), body: t(classifyAppError(error)), actionLabel: t('retry', '重试'), onAction: open, error: true });
    }
  }
  const lifecycle = appLifecycle(async ({ type, appId: changedId }) => {
    if (changedId !== appId) return;
    if (type === 'maintenance' || type === 'stop') await stop(type === 'stop' ? 'user' : 'maintenance');
    // AC-10 (§5.6): a `changed` notification must never restart the surface.
    // Sidebar/metadata changes only matter to the center; a running session
    // keeps its owner and a stopped page stays stopped — the user reopens it.
  });
  const pagehide = () => { void stop('page_closing', false); lifecycle.close(); };
  const pageshow = (event) => { if (event.persisted) void open(); };
  globalThis.window?.addEventListener('message', onWindowMessage);
  globalThis.window?.addEventListener('pagehide', pagehide);
  globalThis.window?.addEventListener('pageshow', pageshow);
  document.addEventListener?.('visibilitychange', scheduleIdle);
  if (back) back.onclick = () => { void stop('page_closing', false); globalThis.open?.(globalThis.chrome?.runtime?.getURL?.('apps.html') || 'apps.html', '_blank'); };
  return { open, stop, setToast, renderStatus, dispose: () => {
    void stop('page_closing', false); lifecycle.close();
    globalThis.window?.removeEventListener('message', onWindowMessage);
    globalThis.window?.removeEventListener('pagehide', pagehide);
    globalThis.window?.removeEventListener('pageshow', pageshow);
    document.removeEventListener?.('visibilitychange', scheduleIdle);
  }};
}

if (typeof document !== 'undefined' && globalThis.location?.pathname?.endsWith('app.html') && el('app-stage')) {
  const t = makeT();
  document.querySelectorAll('[data-i18n]').forEach((node) => { node.textContent = t(node.dataset.i18n, node.textContent); });
  void createAppShell({ appId: parseAppId(globalThis.location.search) }).open();
}
