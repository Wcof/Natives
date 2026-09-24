// Generic owner page for product-declared built-in modules.
// Core verifies the product projection; business traffic goes directly to the module Host.
import { createAppNativeClient } from './native-app-client.js';
import { createNativeClient } from './native-client.js';
import { classifyAppError } from './app-errors.js';
import { appLifecycle } from './app-lifecycle.js';
import { storageGet } from './files-preferences.js';

// AC-11: standalone owner page follows the product appearance preference.
// 计划 §31.4：主题变化经 auth 级事件转发给模块 iframe，模块不维护第二份主题设置。
// moduleFrame 是 open() 创建的 iframe 的顶层引用（局部 frame 变量保持原语义）。
let moduleFrame = null;
const applyOwnerTheme = (theme) => {
  document.documentElement.dataset.theme = ['volt', 'archive'].includes(theme) ? theme : 'archive';
  const appearance = theme === 'volt' ? 'dark' : 'light';
  moduleFrame?.contentWindow?.postMessage({ type: 'theme', appearance }, '*');
};
Promise.resolve()
  .then(() => storageGet('natives-theme', 'archive'))
  .then(applyOwnerTheme)
  .catch(() => {});
globalThis.chrome?.storage?.onChanged?.addListener((changes, area) => {
  if (area === 'local' && changes['natives-theme']) {
    applyOwnerTheme(changes['natives-theme'].newValue);
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
  toast = el('app-toast'), back = el('app-back'), idleMs = IDLE_MS,
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
    frame?.remove(); frame = undefined; moduleFrame = null;
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
    // 短暂离开（切页签/最小化）不再主动停止会话：会话保持后台静默，
    // 由 Core/runtime 侧的 stdin EOF 空闲回收兜底。返回可见时若会话已
    // 失效（onDisconnect 触发过），pageshow/embedded touch 路径会自动重开。
    if (!idleMs || document.visibilityState !== 'hidden' || busy) return;
    idleTimer = setTimeout(() => {
      if (host?.inFlight) return scheduleIdle();
      if (!host) return;
      // 长时间隐藏才礼貌停会话（进程 ≤2s 自退，不弹死屏）。
      void stop('hidden', false);
    }, idleMs);
  }
  // 返回可见：会话还在则直接继续；已被停止/断开则自动重开（保留原状态渲染）。
  function resumeOnVisible() {
    clearTimeout(idleTimer);
    if (busy) return;
    if (host && frame) return; // 会话仍存活：回到之前页面，无需任何操作
    open();
  }
  async function createSandbox(port, current) {
    const rotated = await host.call('app:session', { instanceId, op: 'rotate', generation });
    generation = rotated.newGeneration || rotated.generation;
    if (current !== run || typeof generation !== 'string') return;
    challenge = randomId();
    frame = document.createElement('iframe');
    moduleFrame = frame;
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
      frame?.contentWindow?.postMessage({
        type: 'init',
        generation,
        challenge,
        // 计划 §31.3：初始化上下文携带外观偏好（volt=dark / archive=light）。
        appearance: document.documentElement.dataset.theme === 'volt' ? 'dark' : 'light',
      }, '*');
    });
    helloReceived = false;
    frame.src = `http://127.0.0.1:${port}/`;
    stage.replaceChildren(frame);
  }
  async function onWindowMessage(event) {
    if (!frame || event.source !== frame.contentWindow) return;
    const validOrigin = event.origin === 'null' || event.origin.startsWith('http://127.0.0.1:') || event.origin.startsWith('http://localhost:');
    if (!validOrigin) return;

    if (event.data?.type === 'toggle-sidebar') {
      const appsCollapse = document.getElementById('apps-toggle-sidebar-btn');
      const appsExpand = document.getElementById('apps-expand-sidebar-btn');
      const filesToggle = document.getElementById('toggle-sidebar') || document.getElementById('app-stage-toggle-sidebar-btn');
      const spaceToggle = document.getElementById('space-toggle-sidebar-btn');

      if (appsCollapse || appsExpand) {
        const isAppsCollapsed = document.body.classList.contains('apps-sidebar-collapsed');
        if (isAppsCollapsed && appsExpand) {
          appsExpand.click();
        } else if (!isAppsCollapsed && appsCollapse) {
          appsCollapse.click();
        } else {
          document.body.classList.toggle('apps-sidebar-collapsed');
        }
      } else if (filesToggle) {
        filesToggle.click();
      } else if (spaceToggle) {
        spaceToggle.click();
      } else {
        document.body.classList.toggle('sidebar-collapsed');
      }

      const isCollapsed = document.body.classList.contains('apps-sidebar-collapsed') ||
                          document.body.classList.contains('sidebar-collapsed');
      frame.contentWindow?.postMessage({ type: 'sidebar-state', collapsed: isCollapsed }, '*');
      return;
    }

    if (event.data?.type === 'get-sidebar-state') {
      const isCollapsed = document.body.classList.contains('apps-sidebar-collapsed') ||
                          document.body.classList.contains('sidebar-collapsed');
      frame.contentWindow?.postMessage({ type: 'sidebar-state', collapsed: isCollapsed }, '*');
      return;
    }

    // 会话续期：页面 401（token 15 分钟 TTL 过期）后申请换发新 token。
    // 同 generation 内 issue 新 challenge 即可，无需 rotate/重载页面。
    if (event.data?.type === 'renew-session') {
      if (!host || !frame || !generation) return;
      try {
        const challenge = randomId();
        const issued = await host.call('app:session', { instanceId, op: 'issue', challenge });
        if (!frame || issued.generation !== generation) return;
        frame.contentWindow.postMessage({ type: 'welcome', generation, challenge, token: issued.token, expiresAt: issued.expiresAt }, '*');
      } catch { /* 下一次请求仍会 401，页面按错误路径提示 */ }
      return;
    }

    if (event.data?.type !== 'hello') return;
    helloReceived = true;
    if (event.data.generation !== generation || event.data.challenge !== challenge) return;
    try {
      const issued = await host.call('app:session', { instanceId, op: 'issue', challenge });
      if (!frame || issued.generation !== generation) return;
      frame.contentWindow.postMessage({ type: 'welcome', generation, challenge, token: issued.token, expiresAt: issued.expiresAt }, '*');
      // AC-11：页面级主题读取先于 iframe 创建，applyOwnerTheme 的首次
      // postMessage 会落在空 moduleFrame 上丢失。握手完成后补发当前外观，
      // 覆盖 iframe 启动晚于主题初始化的时序（init appearance 为兜底）。
      frame.contentWindow.postMessage({ type: 'theme', appearance: document.documentElement.dataset.theme === 'volt' ? 'dark' : 'light' }, '*');
      const isCollapsed = document.body.classList.contains('apps-sidebar-collapsed') ||
                          document.body.classList.contains('sidebar-collapsed');
      frame.contentWindow.postMessage({ type: 'sidebar-state', collapsed: isCollapsed }, '*');
    } catch (error) { setToast(t(classifyAppError(error)), true); }
  }
  async function open() {
    await stop('maintenance', false);
    const current = run;
    setToast('');
    renderStatus({ heading: t('appsStarting', '正在启动…') });
    const core = getNativeClient();
    try {
      await openOnce(core, current);
    } catch (error) {
      core.disconnect(); disconnect();
      if (current !== run) return;
      // 残留会话窗口（APP_RUNNING_ELSEWHERE / APP_ALREADY_RUNNING）：旧
      // runtime 在 stdin EOF 后 ≤2s 自退，属瞬态失败——等待后自动重试一次。
      if (['APP_RUNNING_ELSEWHERE', 'APP_ALREADY_RUNNING'].includes(error?.code || /APP_[A-Z_]+/.exec(error?.message || '')?.[0])) {
        await new Promise((resolve) => setTimeout(resolve, 2_500));
        if (current === run) await open();
        return;
      }
      setToast(t(classifyAppError(error)), true);
      renderStatus({ heading: t('appSurfaceOpenFailed', '应用无法打开'), body: t(classifyAppError(error)), actionLabel: t('retry', '重试'), onAction: open, error: true });
    }
  }
  async function openOnce(core, current) {
      let detail = await core.call('apps:get', { appId });
      let app = detail?.app;
      if (!app) {
        core.disconnect();
        throw Object.assign(new Error('app not found'), { code: 'APP_NOT_FOUND' });
      }
      if (title) title.textContent = app.name;
      if (sub) sub.textContent = app.version;
      if (!app.enabled) {
        core.disconnect();
        renderStatus({ heading: t('appSurfaceDisabled', '应用已停用'), body: t('appSurfaceDisabledBody', '请在应用中心重新启用。') });
        return;
      }
      if (app.needs_migration) {
        core.disconnect();
        renderStatus({ heading: t('appSurfaceRepairRequired', '应用需要更新'), body: t('appsNeedsUpdate', '请更新 Natives 后重试。') });
        return;
      }
      if (app.kind !== 'managed_local' || !app.runtime_host) {
        core.disconnect();
        renderStatus({ heading: t('appSurfaceOpenFailed', '应用无法打开'), body: t('appsNeedsUpdate', '请更新 Natives 后重试。'), error: true });
        return;
      }
      host = createHostClient(app, { onDisconnect: (_error, intentional) => {
        if (!intentional && current === run) renderStatus({ heading: t('appSurfaceHostOffline', '应用已退出'), actionLabel: t('retry', '重试'), onAction: open, error: true });
      }});
      let expectedActivationGeneration = app.activation_generation ?? app.activationGeneration ?? app.revision;
      let handshake;
      try {
        // 只接受 App Runtime Protocol v2（ADR-0031）；不向 v1 回退。
        handshake = await host.call('app:handshake', {
          protocolVersion: 2,
          appId: app.app_id,
          productVersion: app.version,
          activationGeneration: expectedActivationGeneration,
        });
      } catch (handshakeError) {
        const errCode = handshakeError?.code || /APP_[A-Z_]+/.exec(handshakeError?.message || '')?.[0];
        if (errCode === 'APP_PACKAGE_INVALID') {
          // 本地开发或代码更新后 runtime 二进制变动未密封重配：向 Core 触发一次 apps:handshake 自动重配自愈
          try {
            const origin = globalThis.location?.origin || `chrome-extension://${globalThis.chrome?.runtime?.id}/`;
            await core.call('apps:handshake', { origin });
            detail = await core.call('apps:get', { appId });
            if (detail?.app) {
              app = detail.app;
              expectedActivationGeneration = app.activation_generation ?? app.activationGeneration ?? app.revision;
              handshake = await host.call('app:handshake', {
                protocolVersion: 2,
                appId: app.app_id,
                productVersion: app.version,
                activationGeneration: expectedActivationGeneration,
              });
            } else {
              throw handshakeError;
            }
          } catch {
            throw handshakeError;
          }
        } else {
          throw handshakeError;
        }
      } finally {
        core.disconnect();
      }
      if (current !== run) return;
      const handshakeAppId = handshake.appId || handshake.app_id;
      if (handshake.protocolVersion !== 2 || handshakeAppId !== app.app_id) {
        throw Object.assign(new Error('app protocol mismatch'), { code: 'APP_PROTOCOL_MISMATCH' });
      }
      const started = await host.call('app:start', { requestId: randomId(), expectedActivationGeneration });
      if (started.state !== 'ready' || !validLoopbackPort(started.port) || typeof started.instanceId !== 'string' || typeof started.generation !== 'string') {
        throw Object.assign(new Error('invalid start result'), { code: 'APP_START_FAILED' });
      }
      instanceId = started.instanceId; generation = started.generation; loadCount = 0;
      await createSandbox(started.port, current);
      scheduleIdle();
  }
  const lifecycle = appLifecycle(async ({ type, appId: changedId }) => {
    if (changedId !== appId) return;
    if (type === 'maintenance' || type === 'stop') await stop(type === 'stop' ? 'user' : 'maintenance');
    // AC-10 (§5.6): a `changed` notification must never restart the surface.
    // Sidebar/metadata changes only matter to the center; a running session
    // keeps its owner and a stopped page stays stopped — the user reopens it.
  });
  const pagehide = () => { void stop('page_closing', false); lifecycle.close(); };
  const pageshow = (event) => { if (event.persisted) resumeOnVisible(); };
  const onVisibilityChange = () => {
    if (document.visibilityState === 'visible') resumeOnVisible();
    else scheduleIdle();
  };
  globalThis.window?.addEventListener('message', onWindowMessage);
  globalThis.window?.addEventListener('pagehide', pagehide);
  globalThis.window?.addEventListener('pageshow', pageshow);
  document.addEventListener?.('visibilitychange', onVisibilityChange);
  if (back) back.onclick = () => { void stop('page_closing', false); globalThis.open?.(globalThis.chrome?.runtime?.getURL?.('apps.html') || 'apps.html', '_blank'); };
  return { open, stop, setToast, renderStatus,
    get busy() { return busy; },
    get hasSession() { return Boolean(frame && host); },
    dispose: () => {
    void stop('page_closing', false); lifecycle.close();
    globalThis.window?.removeEventListener('message', onWindowMessage);
    globalThis.window?.removeEventListener('pagehide', pagehide);
    globalThis.window?.removeEventListener('pageshow', pageshow);
    document.removeEventListener?.('visibilitychange', onVisibilityChange);
  }};
}

// Embedded app surface for sidebar hosts (files.html / space.html).
// Mounted into the host page's content area instead of navigating to
// app.html, so the sidebar stays put; sessions survive switching back to
// the host view and are reclaimed after EMBEDDED_IDLE_MS without access.
export const EMBEDDED_IDLE_MS = 5 * 60_000;

export function createEmbeddedAppSurface({ appId, stage, onClosed, getNativeClient } = {}) {
  if (!appId || !stage) throw new Error('createEmbeddedAppSurface: appId and stage are required');
  const shell = createAppShell({ appId, stage, idleMs: null, getNativeClient });
  let idleTimer, closed = false;
  const touch = () => {
    clearTimeout(idleTimer);
    idleTimer = setTimeout(async () => {
      if (document.visibilityState !== 'hidden' || shell.busy) return touch();
      await stop('idle');
      onClosed?.();
    }, EMBEDDED_IDLE_MS);
  };
  async function stop(reason = 'user') {
    clearTimeout(idleTimer);
    await shell.stop(reason, false);
  }
  async function open() {
    if (closed) return;
    touch();
    if (!shell.hasSession) await shell.open();
  }
  function dispose() {
    closed = true;
    clearTimeout(idleTimer);
    void stop('page_closing');
  }
  document.addEventListener?.('visibilitychange', touch);
  return { open, stop, dispose };
}

if (typeof document !== 'undefined' && globalThis.location?.pathname?.endsWith('app.html') && el('app-stage')) {
  const t = makeT();
  document.querySelectorAll('[data-i18n]').forEach((node) => { node.textContent = t(node.dataset.i18n, node.textContent); });
  void createAppShell({ appId: parseAppId(globalThis.location.search) }).open();
}
