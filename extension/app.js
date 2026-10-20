import { storage } from './platform/storage.js';
// Generic owner page for product-declared built-in modules.
// Core verifies the product projection and owns the shared module worker.
import { createAppNativeClient } from './native-app-client.js';
import { createNativeClient } from './native-client.js';
import { classifyAppError } from './app-errors.js';
import { appLifecycle } from './app-lifecycle.js';
import { storageGet } from './files-preferences.js';
import { mountAppMenu, projectionFromApps, saveAppNavigation } from './app-navigation-projection.js';
import { createModuleInferenceBroker } from './module-inference-broker.js';
import { createModuleSecretBroker } from './module-secret-broker.js';
import { acquireCapabilityLeases } from './system-capability-leases.js';
import { isExtension } from './platform/env.js';
import { getUILanguage } from './platform/i18n.js';
import { ensureWebSurface } from './platform/surface.js';
import { createModuleChangeBroker } from './module-change-broker.js';

// AC-11: standalone owner page follows the product appearance preference.
// 计划 §31.4：主题变化经 auth 级事件转发给模块 iframe，模块不维护第二份主题设置。
const moduleFrames = new Set();
const applyOwnerTheme = (theme) => {
  document.documentElement.dataset.theme = ['volt', 'archive'].includes(theme) ? theme : 'archive';
  const appearance = theme === 'volt' ? 'dark' : 'light';
  for (const owned of moduleFrames) owned.contentWindow?.postMessage({ type: 'theme', appearance }, '*');
};
Promise.resolve()
  .then(() => storageGet('natives-theme', 'archive'))
  .then(applyOwnerTheme)
  .catch(() => {});
storage.onChanged?.addListener((changes, area) => {
  if (area === 'local' && changes['natives-theme']) {
    applyOwnerTheme(changes['natives-theme'].newValue);
  }
});

const CORE_HOST = 'com.natives.file_manager';
const IDLE_MS = 60_000;
const el = (id) => typeof document === 'undefined' ? null : document.getElementById(id);
let ownerMessages = {};
const defaultOwnerLanguage = () => isExtension ? 'zh_CN' : (getUILanguage().startsWith('en') ? 'en' : 'zh_CN');
const makeT = () => (key, fallback = key) => ownerMessages[key]?.message || globalThis.chrome?.i18n?.getMessage?.(key) || fallback;
async function loadOwnerLanguage() {
  const preference = await storageGet('natives-language', defaultOwnerLanguage());
  const language = preference === 'en' ? 'en' : 'zh_CN';
  const response = await fetch(`_locales/${language}/messages.json`);
  if (!response.ok) throw Error('APP_LOCALE_UNAVAILABLE');
  ownerMessages = await response.json();
  document.documentElement.lang = language === 'en' ? 'en' : 'zh-CN';
  const t = makeT();
  document.querySelectorAll('[data-i18n]').forEach(node => {
    node.textContent = t(node.dataset.i18n);
    if (node.parentElement?.classList.contains('app-shell-nav-item')) {
      node.parentElement.setAttribute('aria-label', node.textContent);
      node.parentElement.title = node.textContent;
    }
  });
}
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

export function validateNotification(payload) {
  if (!payload || typeof payload !== 'object') return null;
  const limits = { title: 64, body: 200, tag: 64 };
  for (const [field, limit] of Object.entries(limits)) {
    if (typeof payload[field] !== 'string' || payload[field].length > limit || /[<>]/.test(payload[field])) return null;
  }
  const text = payload.badge?.text ?? '';
  const tones = ['neutral', 'info', 'success', 'warning', 'danger'];
  if (typeof text !== 'string' || text.length > 16 || !tones.includes(payload.badge?.tone)) return null;
  return { title: payload.title, body: payload.body, tag: payload.tag, badge: { text, tone: payload.badge.tone } };
}

// ADR-0036：深链只允许固定短 token（纯导航意图，不含 URL/数据/HTML）。
export function isValidViewToken(value) {
  return typeof value === 'string' && /^[a-z][a-z0-9-]{0,31}$/.test(value);
}

// ADR-0038：模块声明非空 views 白名单时严格放行；未声明（旧模块）回退短 token 正则。
export function viewAllowedForModule(value, declaredViews) {
  if (!isValidViewToken(value)) return false;
  const list = Array.isArray(declaredViews) ? declaredViews : [];
  return list.length === 0 || list.includes(value);
}

export function createAppShell({
  appId, stage = el('app-stage'), title = el('app-title'), sub = el('app-sub'),
  toast = el('app-toast'), back = el('app-back'), idleMs = IDLE_MS,
  initialView = '',
  getPreference = storageGet,
  getNativeClient = () => createNativeClient({ host: CORE_HOST }),
  createHostClient = (runtimeHost, handlers) => createAppNativeClient({ runtimeHost, ...handlers }),
  t = makeT(),
  openSettings,
} = {}) {
  if (!appId || !stage) throw new Error('createAppShell: appId and stage are required');
  // mode/T06：设置打开器由装配层注入；Shell 组装 {t,language,returnFocus} 上下文。
  if (typeof openSettings !== 'function') throw new Error('createAppShell: openSettings is required (inject from the page assembly entry)');
  // ADR-0036/0038：深链视图经短 token 正则 + 模块 views 白名单（见 viewAllowedForModule）。
  let declaredViews = [];
  const isValidView = (value) => viewAllowedForModule(value, declaredViews);
  let run = 0, host, frame, instanceId, generation, challenge, idleTimer, busy = false, loadCount = 0, helloReceived = false, releaseCapabilityLeases, capabilities = [], notificationTimes = [], currentDisplayName = null;

  const inferenceBroker = createModuleInferenceBroker({context:()=>({frame,generation,challenge,capabilities}),openSettings: () => openSettings({ t, language: getPreference('natives-language', defaultOwnerLanguage()), returnFocus: document.activeElement })});
  const secretBroker = createModuleSecretBroker({context:()=>({frame,generation,challenge,capabilities,host,instanceId}),t});
  const changeBroker = createModuleChangeBroker({appId,context:()=>({frame,generation,challenge,host})});
  let restoringSession = false;
  const restoreWebSession = typeof __NATIVES_EXTENSION_BUILD__ !== 'undefined' && __NATIVES_EXTENSION_BUILD__ ? () => {} : async function () {
    if (isExtension || !frame || !host || restoringSession) return;
    restoringSession = true;
    const current = run, ownedFrame = frame, ownedHost = host;
    const core = getNativeClient();
    try {
      // Existing Web ports receive the same event and release their old leases first.
      await Promise.resolve();
      const detail = await core.call('apps:get', {appId});
      if (current !== run || frame !== ownedFrame) return;
      const app = detail.app;
      if (!app?.enabled) throw Error('APP_DISABLED');
      const activation = app.product_generation ?? app.productGeneration ?? app.activation_generation ?? app.activationGeneration;
      await ownedHost.call('app:handshake', {protocolVersion:2,appId,productVersion:app.version,activationGeneration:activation});
      const started = await ownedHost.call('app:start', {appId,requestId:randomId(),expectedActivationGeneration:activation});
      if (current !== run || frame !== ownedFrame) return;
      if (started.state !== 'ready' || !validLoopbackPort(started.port) || new URL(ownedFrame.src).port !== String(started.port)) throw Error('APP_RUNTIME_REPLACED');
      instanceId = started.instanceId; generation = started.generation; challenge = randomId();
      ownedFrame.contentWindow?.postMessage({type:'init',generation,challenge,appearance:document.documentElement.dataset.theme==='volt'?'dark':'light'},'*');
    } catch {
      if (current === run && frame === ownedFrame) {
        ownedHost.disconnect();
        setToast(t('appSurfaceSessionRestoreFailed'), true);
      }
    } finally { core.disconnect(); restoringSession = false; }
  };

  function setToast(message, error = false) {
    if (!toast) return;
    toast.textContent = message || '';
    toast.className = error ? 'app-toast error' : 'app-toast';
    toast.hidden = !message;
  }
  // 应用启动加载器样式：由本模块注入而非依赖宿主页面样式表——加载器会
  // 渲染在 app/apps/files/space 四种宿主中，只有 app.html 加载 app.css，
  // 其余宿主下曾表现为无样式裸文本。令牌全部带回退值，任意宿主都成立。
  const ABL_CSS = `
.abl-scope {
  --_accent: var(--accent, #cdf24b);
  --_text: var(--text, #f2f2ea);
  --_muted: var(--muted, #9b9d8c);
  --_surface: var(--surface, #0b0c0a);
  --_surface2: var(--surface-2, #131410);
  --_line: var(--line, #2a2b24);
  --_radius: var(--radius, 10px);
}
.app-boot-loader {
  display:flex; align-items:center; justify-content:center;
  min-height:440px; width:100%; padding:40px 16px; box-sizing:border-box;
  animation: abl-fade-in .28s cubic-bezier(.16,1,.3,1);
}
@keyframes abl-fade-in { from { opacity:0; transform:translateY(6px); } to { opacity:1; transform:none; } }
.abl-card {
  position:relative; width:100%; max-width:380px; padding:30px 28px 22px;
  background:var(--_surface2); border:1px solid var(--_line);
  border-radius:var(--_radius); box-shadow:0 12px 40px #0008;
  display:flex; flex-direction:column; align-items:center; text-align:center;
  box-sizing:border-box; overflow:hidden;
}
.abl-card::before {
  content:""; position:absolute; top:-80px; left:50%; transform:translateX(-50%);
  width:240px; height:160px; border-radius:50%;
  background:radial-gradient(circle, var(--_accent) 0%, transparent 70%);
  opacity:.10; filter:blur(30px); pointer-events:none;
}
/* 徽标：单环旋转 + 中心图标，克制不抢戏 */
.abl-emblem { position:relative; width:60px; height:60px; margin-bottom:18px; }
.abl-ring {
  position:absolute; inset:0; border-radius:50%;
  border:2px solid var(--_line); border-top-color:var(--_accent);
  animation: abl-spin 1.1s linear infinite;
}
.abl-core {
  position:absolute; inset:7px; border-radius:50%;
  background:var(--_surface); border:1px solid var(--_line);
  display:flex; align-items:center; justify-content:center;
}
.abl-core .icon { width:22px; height:22px; color:var(--_accent); }
@keyframes abl-spin { to { transform:rotate(360deg); } }
/* 标题区 */
.abl-name { font-size:15px; font-weight:600; color:var(--_text); letter-spacing:.2px; margin-bottom:4px; }
.abl-status { font-size:12.5px; color:var(--_muted); line-height:1.5; min-height:19px; margin-bottom:18px; }
/* 进度条 */
.abl-track { width:100%; height:3px; background:var(--_line); border-radius:2px; overflow:hidden; margin-bottom:18px; }
.abl-bar {
  height:100%; background:var(--_accent); border-radius:2px;
  transition:width .4s cubic-bezier(.2,.8,.2,1);
}
/* 步骤指示器：圆点 + 连接线，完成打勾 / 当前高亮 / 待办置灰 */
.abl-steps { display:flex; align-items:flex-start; width:100%; margin-bottom:18px; }
.abl-step { display:flex; flex-direction:column; align-items:center; gap:5px; flex:1; min-width:0; }
.abl-dot {
  width:20px; height:20px; border-radius:50%; flex:none;
  display:flex; align-items:center; justify-content:center;
  font-size:10px; font-style:normal; font-family:inherit;
  border:1.5px solid var(--_line); color:var(--_muted);
  background:var(--_surface); transition:all .25s ease;
}
.abl-label { font-size:10.5px; color:var(--_muted); opacity:.55; transition:all .25s ease; white-space:nowrap; }
.abl-line { flex:none; width:14px; height:1.5px; background:var(--_line); margin-top:9.5px; transition:background .25s ease; }
.abl-step.done .abl-dot { border-color:var(--_accent); background:var(--_accent); color:var(--_surface); }
.abl-step.done .abl-label { opacity:.8; color:var(--_text); }
.abl-step.done + .abl-line { background:var(--_accent); opacity:.6; }
.abl-step.current .abl-dot {
  border-color:var(--_accent); color:var(--_accent); font-weight:600;
  box-shadow:0 0 0 3px color-mix(in srgb, var(--_accent) 18%, transparent);
  animation: abl-breathe 1.6s ease-in-out infinite;
}
.abl-step.current .abl-label { opacity:1; color:var(--_accent); font-weight:500; }
@keyframes abl-breathe {
  0%,100% { box-shadow:0 0 0 3px color-mix(in srgb, var(--_accent) 18%, transparent); }
  50% { box-shadow:0 0 0 6px color-mix(in srgb, var(--_accent) 8%, transparent); }
}
/* 底部安全说明芯片 */
.abl-chips { display:flex; gap:6px; justify-content:center; flex-wrap:wrap; border-top:1px solid var(--_line); padding-top:12px; width:100%; }
.abl-chip {
  font-size:10px; font-family:ui-monospace,SFMono-Regular,Menlo,monospace;
  padding:2px 8px; border-radius:4px; background:transparent;
  color:var(--_muted); border:1px solid var(--_line); user-select:none;
}
@media (prefers-reduced-motion: reduce) {
  .app-boot-loader, .abl-ring, .abl-step.current .abl-dot { animation:none; }
  .abl-bar { transition:none; }
}`;
  function ensureLoaderStyle() {
    if (document.getElementById('abl-style')) return;
    // 测试环境（Node DOM shim）可能没有 head；挂到 documentElement 同样生效
    const parent = document.head || document.documentElement;
    if (!parent) return;
    const style = document.createElement('style');
    style.id = 'abl-style';
    style.textContent = ABL_CSS;
    parent.append(style);
  }
  function renderAppLoading({ name = '', phase = 1, status = '' } = {}) {
    ensureLoaderStyle();
    const existing = stage?.querySelector?.('.app-boot-loader');
    const phaseTexts = [
      t('appBootEnv', '正在连接端侧安全运行环境…'),
      t('appBootVerify', '验证应用权限与会话安全凭据…'),
      t('appBootAlloc', '分配隔离沙箱与本地回环通道…'),
      t('appBootMount', '初始化界面沙箱与事件管道…'),
    ];
    const currentText = status || phaseTexts[Math.min(phase - 1, phaseTexts.length - 1)] || phaseTexts[0];
    const displayName = name || title?.textContent || appId || 'Natives';
    const stepNames = [
      t('appBootStepEnv', '运行环境'),
      t('appBootStepAuth', '凭证握手'),
      t('appBootStepPort', '端口就绪'),
      t('appBootStepReady', '界面呈现'),
    ];

    if (existing) {
      const nameEl = existing.querySelector('.abl-name');
      if (nameEl && name && nameEl.textContent !== name) nameEl.textContent = name;
      const statusEl = existing.querySelector('.abl-status');
      if (statusEl) statusEl.textContent = currentText;
      const barEl = existing.querySelector('.abl-bar');
      if (barEl) barEl.style.width = `${Math.min(100, Math.max(16, phase * 25))}%`;
      existing.querySelectorAll('.abl-step').forEach((el, idx) => {
        el.classList.toggle('done', idx < phase - 1);
        el.classList.toggle('current', idx === phase - 1);
      });
      return;
    }

    if (!stage) return;
    stage.replaceChildren();
    const wrap = document.createElement('div');
    wrap.className = 'app-boot-loader abl-scope';
    wrap.setAttribute('role', 'status');
    wrap.setAttribute('aria-live', 'polite');

    const card = document.createElement('div');
    card.className = 'abl-card';

    // 徽标：旋转环 + 中心应用图标
    const emblem = document.createElement('div');
    emblem.className = 'abl-emblem';
    emblem.setAttribute('aria-hidden', 'true');
    const ring = document.createElement('div');
    ring.className = 'abl-ring';
    const core = document.createElement('div');
    core.className = 'abl-core';
    core.innerHTML = '<svg class="icon" aria-hidden="true"><use href="#i-bolt" /></svg>';
    emblem.append(ring, core);

    // 标题与当前阶段文案
    const nameEl = document.createElement('div');
    nameEl.className = 'abl-name';
    nameEl.textContent = displayName;
    const statusEl = document.createElement('div');
    statusEl.className = 'abl-status';
    statusEl.textContent = currentText;

    // 进度条
    const track = document.createElement('div');
    track.className = 'abl-track';
    const bar = document.createElement('div');
    bar.className = 'abl-bar';
    bar.style.width = `${Math.min(100, Math.max(16, phase * 25))}%`;
    track.append(bar);

    // 步骤指示器：完成打勾，当前呼吸高亮，节点间连接线
    const stepsRow = document.createElement('div');
    stepsRow.className = 'abl-steps';
    stepNames.forEach((sName, idx) => {
      if (idx > 0) {
        const line = document.createElement('div');
        line.className = 'abl-line';
        line.setAttribute('aria-hidden', 'true');
        stepsRow.append(line);
      }
      const step = document.createElement('div');
      step.className = 'abl-step' + (idx < phase - 1 ? ' done' : '') + (idx === phase - 1 ? ' current' : '');
      const dot = document.createElement('i');
      dot.className = 'abl-dot';
      dot.textContent = idx < phase - 1 ? '✓' : String(idx + 1);
      const label = document.createElement('span');
      label.className = 'abl-label';
      label.textContent = sName;
      step.append(dot, label);
      stepsRow.append(step);
    });

    // 底部安全说明：本地回环沙箱，数据不出端
    const chipsRow = document.createElement('div');
    chipsRow.className = 'abl-chips';
    const chips = [
      t('appBootChipLoopback', '127.0.0.1 隔离沙箱'),
      t('appBootChipNativeHost', 'Native Host 驱动'),
      t('appBootChipLocalOnly', '端侧零外部传输'),
    ];
    chips.forEach((cText) => {
      const chip = document.createElement('span');
      chip.className = 'abl-chip';
      chip.textContent = cText;
      chipsRow.append(chip);
    });

    card.append(emblem, nameEl, statusEl, track, stepsRow, chipsRow);
    wrap.append(card);
    stage.append(wrap);
  }

  function renderStatus({ heading, body = '', actionLabel, onAction, error = false }) {
    if (!error && !body && !actionLabel && (heading === t('appsStarting', '正在启动…') || heading.includes('启动') || heading.includes('加载'))) {
      renderAppLoading({ status: heading });
      return;
    }
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
    void inferenceBroker.dispose();
    secretBroker.dispose();
    clearTimeout(idleTimer);
    try { releaseCapabilityLeases?.(); } catch { /* ignore */ }
    releaseCapabilityLeases = undefined;
    capabilities = []; notificationTimes = [];
    moduleFrames.delete(frame);
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
  async function resumeOnVisible() {
    clearTimeout(idleTimer);
    if (busy) return;
    if (host && host.connected !== false && frame) return;
    if (!isExtension && frame && host) { await restoreWebSession(); return; }
    if (!isExtension) {
      try { await ensureWebSurface(); } catch { return; }
    }
    if (document.visibilityState !== 'hidden') void open();
  }
  async function createSandbox(port, current) {
    if (current !== run || typeof generation !== 'string') return;
    challenge = randomId();
    frame = document.createElement('iframe');
    moduleFrames.add(frame);
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
    // 首帧主题直传：模块 head 内联脚本在绘制前消费 ?theme=，消除深色用户
    // 开页时沙箱内默认浅色首帧；后续切换仍走 init/theme 消息。
    frame.src = `http://127.0.0.1:${port}/?theme=${document.documentElement.dataset.theme === 'volt' ? 'volt' : 'archive'}`;
    stage.replaceChildren(frame);
  }
  async function onWindowMessage(event) {
    if (!frame || event.source !== frame.contentWindow) return;
    const validOrigin = event.origin === 'null' || event.origin.startsWith('http://127.0.0.1:') || event.origin.startsWith('http://localhost:');
    if (!validOrigin) return;
    if (changeBroker.handle(event)) return;

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

    if (await inferenceBroker.handle(event)) return;
    if (await secretBroker.handle(event)) return;

    // 会话续期：页面 401（token 15 分钟 TTL 过期）后申请换发新 token。
    // 同 generation 内 issue 新 challenge 即可，无需 rotate/重载页面。
    if (event.data?.type === 'renew-session') {
      if (!host || !frame || !generation) return;
      try {
        challenge = randomId();
        const issued = await host.call('app:session', { instanceId, op: 'issue', challenge });
        if (!frame || issued.generation !== generation) return;
        frame.contentWindow.postMessage({ type: 'welcome', generation, challenge, token: issued.token, expiresAt: issued.expiresAt, view: isValidView(initialView) ? initialView : '' }, '*');
      } catch { /* 下一次请求仍会 401，页面按错误路径提示 */ }
      return;
    }

    // 通用应用通知 Broker（ADR-0031 / 架构 Wave 7 / 审计 A-06）：
    // 模块在受限沙箱 iframe 内无法直接使用 window.Notification，
    // 经通用 natives:notification 协议转发父级 Shell，由父级 Shell 校验后转送 Service Worker。
    // Shell 仅验证格式、限速与长度边界，严禁解析具体模块业务字段（symbol/changePercent 等）。
    if (event.data?.type === 'natives:notification') {
      const m=event.data;
      if(m.generation!==generation || m.challenge!==challenge || !/^[a-zA-Z0-9-]{1,128}$/.test(m.requestId||'') || Object.keys(m).some(k=>!['type','op','requestId','generation','challenge','payload'].includes(k)))return;
      const currentFrame=frame,currentGeneration=generation;
      let error;
      const payload=validateNotification(m.payload);
      const now=Date.now();notificationTimes=notificationTimes.filter(time=>now-time<60000);
      if(m.op==='status'&&capabilities.includes('system.notification')){
        let result;try{result=await chrome.runtime.sendMessage({type:'natives:notification-status'});}catch{result={permission:'unknown'};}
        if(frame===currentFrame&&generation===currentGeneration)frame.contentWindow.postMessage({type:'natives:reply',requestId:m.requestId,generation,challenge,result},'*');
        return;
      }
      if(!capabilities.includes('system.notification') || !payload || m.op!=='notify')error=t('moduleNotificationUnavailable');
      else if(notificationTimes.length>=10)error=t('moduleNotificationUnavailable');
      else {notificationTimes.push(now);try{const result=await chrome.runtime.sendMessage({type:'natives:notification',appId:String(appId).slice(0,32),payload});if(!result?.delivered)error=t('moduleNotificationUnavailable');}catch{error=t('moduleNotificationUnavailable');}}
      if(frame===currentFrame && generation===currentGeneration)frame.contentWindow.postMessage({type:'natives:reply',requestId:m.requestId,generation,challenge,result:error?null:{delivered:true},error},'*');
      return;
    }

    if (event.data?.type !== 'hello') return;
    helloReceived = true;
    if (event.data.generation !== generation || event.data.challenge !== challenge) return;
    try {
      const issued = await host.call('app:session', { instanceId, op: 'issue', challenge });
      if (!frame || issued.generation !== generation) return;
      frame.contentWindow.postMessage({ type: 'welcome', generation, challenge, token: issued.token, expiresAt: issued.expiresAt, view: isValidView(initialView) ? initialView : '' }, '*');
      // AC-11：页面级主题读取先于 iframe 创建，applyOwnerTheme 的首次
      // postMessage 会落在空 moduleFrame 上丢失。握手完成后补发当前外观，
      // 覆盖 iframe 启动晚于主题初始化的时序（init appearance 为兜底）。
      frame.contentWindow.postMessage({ type: 'theme', appearance: document.documentElement.dataset.theme === 'volt' ? 'dark' : 'light' }, '*');
      // 语言同步（07 §5.2 / 审计 C3）：与主题同序补发当前语言；语言切换走
      // storage 变更监听（下方），切换本身会 reload 页面重新握手。
      try {
        const language = await getPreference('natives-language', defaultOwnerLanguage()) === 'en' ? 'en' : 'zh-CN';
        frame.contentWindow?.postMessage({ type: 'language', language }, '*');
      } catch { /* storage 不可用时模块保持默认 zh-CN */ }
      const isCollapsed = document.body.classList.contains('apps-sidebar-collapsed') ||
                          document.body.classList.contains('sidebar-collapsed');
      frame.contentWindow.postMessage({ type: 'sidebar-state', collapsed: isCollapsed }, '*');
      // ADR-0036 深链：握手完成后一次性转发纯导航意图（初次打开场景）。
      if (isValidView(initialView)) {
        frame.contentWindow.postMessage({ type: 'navigate', view: initialView }, '*');
      }
    } catch (error) { setToast(t(classifyAppError(error)), true); }
  }
  async function open() {
    // 若当前会话已经建立且正常存活，直接复用既有进程与视图，无需重新起机
    if (frame && host && host.connected !== false && !busy) {
      scheduleIdle();
      return;
    }
    await stop('maintenance', false);
    const current = run;
    setToast('');
    renderAppLoading({ phase: 1, status: t('appBootEnv', '正在连接端侧安全运行环境…') });
    const core = getNativeClient();
    try {
      await openOnce(core, current);
    } catch (error) {
      core.disconnect(); disconnect();
      if (current !== run) return;
      setToast(t(classifyAppError(error)), true);
      renderStatus({ heading: t('appSurfaceOpenFailed', '应用无法打开'), body: t(classifyAppError(error)), actionLabel: t('retry', '重试'), onAction: open, error: true });
    }
  }
  async function openOnce(core, current) {
      renderAppLoading({ phase: 1, status: t('appBootEnv', '正在连接端侧安全运行环境…') });
      const origin = globalThis.chrome?.runtime?.id
        ? `chrome-extension://${globalThis.chrome.runtime.id}/`
        : 'chrome-extension://gehmgcnlpdepnpmcbbdaijabcjdnbfmh/';
      await core.call('apps:handshake', { origin });
      if (current !== run) { core.disconnect(); return; }
      const listed = await core.call('apps:list');
      if (current !== run) { core.disconnect(); return; }
      await saveAppNavigation(projectionFromApps(listed.apps, listed.revision, listed.modules));
      let detail = await core.call('apps:get', { appId });
      let app = detail?.app;
      if (!app) {
        core.disconnect();
        throw Object.assign(new Error('app not found'), { code: 'APP_NOT_FOUND' });
      }
      const language = await getPreference('natives-language', defaultOwnerLanguage());
      currentDisplayName = detail.displayName || null;
      const appName = currentDisplayName?.[language === 'en' ? 'en' : 'zh_CN'] || app.name;
      if (title) title.textContent = appName;
      document.title = `${appName} · Natives`;
      if (sub) sub.textContent = app.version;
      capabilities = Array.isArray(app.capabilities) ? app.capabilities : [];
      declaredViews = Array.isArray(app.views) ? app.views.filter((view) => isValidViewToken(view)) : [];
      const settingsButton = document.getElementById('app-module-settings');
      if(settingsButton){settingsButton.hidden=!declaredViews.includes('settings');settingsButton.onclick=()=>{
        if(frame&&isValidView('settings'))frame.contentWindow.postMessage({type:'navigate',view:'settings'},'*');
      };}
      if (!app.enabled) {
        core.disconnect();
        renderStatus({ heading: t('appSurfaceDisabled', '应用已停用'), body: t('appSurfaceDisabledBody', '请在应用中心重新启用。') });
        return;
      }
      if (!app.app_id) {
        core.disconnect();
        renderStatus({ heading: t('appSurfaceOpenFailed', '应用无法打开'), body: t('appsNeedsUpdate', '请更新 Natives 后重试。'), error: true });
        return;
      }
      if (typeof detail.runtime_host !== 'string' || !detail.runtime_host) {
        core.disconnect();
        throw Object.assign(new Error('product runtime host missing'), { code: 'APP_START_FAILED' });
      }
      renderAppLoading({ name: app.name, phase: 2, status: t('appBootVerify', '验证应用权限与会话安全凭据…') });
      // 依据模块元数据声明的 requires 申请系统能力租约（架构 07 §3）：
      releaseCapabilityLeases = acquireCapabilityLeases(Array.isArray(app.requires) ? app.requires : []);
      const capabilityLease=releaseCapabilityLeases;
      await capabilityLease.ready;
      if(current!==run){capabilityLease();return;}
      host = createHostClient(detail.runtime_host, { onDisconnect: (_error, intentional) => {
        if (!intentional && current === run && !restoringSession) {
          if (!isExtension && frame) { setToast(t('appSurfaceHostOffline'), true); return; }
          disconnect();
          renderStatus({ heading: t('appSurfaceHostOffline', '应用已退出'), actionLabel: t('retry', '重试'), onAction: open, error: true });
        }
      }});
      let expectedActivationGeneration = app.product_generation ?? app.productGeneration ?? app.activation_generation ?? app.activationGeneration;
      const requestHandshake = () => host.call('app:handshake', {
        protocolVersion: 2, appId: app.app_id, productVersion: app.version,
        activationGeneration: expectedActivationGeneration,
      });
      let handshake;
      let attempts = 0;
      try {
        while (attempts < 6) {
          try {
            // 只接受 App Runtime Protocol v2（ADR-0031）；不向 v1 回退。
            handshake = await requestHandshake();
            break;
          } catch (handshakeError) {
            const errCode = handshakeError?.code || /APP_[A-Z_]+/.exec(handshakeError?.message || '')?.[0];
            if (errCode === 'APP_PACKAGE_INVALID') {
              // 本地开发或代码更新后 runtime 二进制变动未密封重配：向 Core 触发一次 apps:handshake 自动重配自愈
              try {
                await core.call('apps:handshake', { origin });
                detail = await core.call('apps:get', { appId });
                if (detail?.app) {
                  app = detail.app;
                  currentDisplayName = detail.displayName || currentDisplayName;
                  expectedActivationGeneration = app.product_generation ?? app.productGeneration ?? app.activation_generation ?? app.activationGeneration;
                  handshake = await requestHandshake();
                  break;
                } else {
                  throw handshakeError;
                }
              } catch {
                throw handshakeError;
              }
            } else if (['APP_RUNNING_ELSEWHERE', 'APP_ALREADY_RUNNING'].includes(errCode)) {
              attempts++;
              if (attempts >= 6) throw handshakeError;
              // An unrelated owner must never be stopped to acquire this session.
              await new Promise((resolve) => setTimeout(resolve, 300));
              continue;
            } else {
              throw handshakeError;
            }
          }
        }
      } finally {
        core.disconnect();
      }
      if (current !== run) return;
      const handshakeAppId = handshake.appId || handshake.app_id;
      if (handshake.protocolVersion !== 2 || handshakeAppId !== app.app_id) {
        throw Object.assign(new Error('app protocol mismatch'), { code: 'APP_PROTOCOL_MISMATCH' });
      }
      renderAppLoading({ name: app.name, phase: 3, status: t('appBootAlloc', '分配隔离沙箱与本地回环通道…') });
      const started = await host.call('app:start', { appId: app.app_id, requestId: randomId(), expectedActivationGeneration });
      if (started.state !== 'ready' || !validLoopbackPort(started.port) || typeof started.instanceId !== 'string' || typeof started.generation !== 'string') {
        throw Object.assign(new Error('invalid start result'), { code: 'APP_START_FAILED' });
      }
      instanceId = started.instanceId; generation = started.generation; loadCount = 0;
      renderAppLoading({ name: app.name, phase: 4, status: t('appBootMount', '初始化界面沙箱与事件管道…') });
      await createSandbox(started.port, current);
      scheduleIdle();
  }
  const lifecycle = appLifecycle(() => stop('maintenance'), appId);
  const pagehide = () => { void stop('page_closing', false); lifecycle.close(); };
  const pageshow = (event) => { if (event.persisted) { lifecycle.resume(); resumeOnVisible(); } };
  const onVisibilityChange = () => {
    if (document.visibilityState === 'visible') resumeOnVisible();
    else scheduleIdle();
  };
  globalThis.window?.addEventListener('message', onWindowMessage);
  globalThis.window?.addEventListener('natives-web-session-restored', restoreWebSession);
  // 语言变更监听（07 §5.2 / 审计 C3）：切换入口本身会 reload，这里兜底
  // 其他入口改语言时把新值推给存活中的模块会话。
  const onLanguageChange = (changes, area) => {
    if (area !== 'local' || !changes['natives-language']) return;
    const value = changes['natives-language'].newValue;
    const language = value === 'en' ? 'en' : 'zh-CN';
    document.documentElement.lang = language;
    const localized = currentDisplayName?.[language === 'en' ? 'en' : 'zh_CN'];
    if (localized) {
      if (title) title.textContent = localized;
      document.title = `${localized} · Natives`;
    }
    frame?.contentWindow?.postMessage({ type: 'language', language }, '*');
  };
  storage.onChanged?.addListener(onLanguageChange);
  globalThis.window?.addEventListener('pagehide', pagehide);
  globalThis.window?.addEventListener('pageshow', pageshow);
  document.addEventListener?.('visibilitychange', onVisibilityChange);
  if (back) back.onclick = () => { void stop('page_closing', false); globalThis.open?.(globalThis.chrome?.runtime?.getURL?.('apps.html') || 'apps.html', '_blank'); };
  return { open, stop, setToast, renderStatus, renderAppLoading,
    get busy() { return busy; },
    get hasSession() { return Boolean(frame && host); },
    // ADR-0036 深链二次导航：已打开会话内的纯导航意图（校验 iframe 存活）。
    navigate(view) {
      if (!isValidView(view)) return;
      if (frame && host && !busy) {
        frame.contentWindow?.postMessage({ type: 'navigate', view }, '*');
      }
    },
    dispose: () => {
    const stopped = stop('page_closing', false); lifecycle.close();
    changeBroker.dispose();
    globalThis.window?.removeEventListener('message', onWindowMessage);
    globalThis.window?.removeEventListener('natives-web-session-restored', restoreWebSession);
    globalThis.window?.removeEventListener('pagehide', pagehide);
    globalThis.window?.removeEventListener('pageshow', pageshow);
    storage.onChanged?.removeListener?.(onLanguageChange);
    document.removeEventListener?.('visibilitychange', onVisibilityChange);
    return stopped;
  }};
}

// Embedded app surface for sidebar hosts (files.html / space.html).
// Mounted into the host page's content area instead of navigating to
// app.html, so the sidebar stays put; sessions survive switching back to
// the host view and are reclaimed after EMBEDDED_IDLE_MS without access.
//
// ADR-0036：surface 必须是会话的唯一 owner——dispose() 释放全部资源
// （Port、idle 计时、自身 visibilitychange 监听）并返回可等待的停止
// Promise；宿主（space.js）返回 Workspace 前必须 await 完成后才能发起
// 卡片短命查询，否则会同 appId 锁竞争。
export const EMBEDDED_IDLE_MS = 5 * 60_000;

export function createEmbeddedAppSurface({ appId, stage, onClosed, initialView = '', getNativeClient, t, openSettings } = {}) {
  if (!appId || !stage) throw new Error('createEmbeddedAppSurface: appId and stage are required');
  if (typeof openSettings !== 'function') throw new Error('createEmbeddedAppSurface: openSettings is required (inject from the page assembly entry)');
  const shell = createAppShell({ appId, stage, idleMs: null, initialView, getNativeClient, t, openSettings });
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
  function navigate(view) {
    shell.navigate?.(view);
  }
  function onVisibilityTouch() {
    // idle 路径只在 hidden 时停会话；可见性变化本身不回收。
    if (document.visibilityState === 'visible') return;
    touch();
  }
  function dispose() {
    closed = true;
    clearTimeout(idleTimer);
    document.removeEventListener?.('visibilitychange', onVisibilityTouch);
    const stopped = shell.dispose();
    return Promise.resolve(stopped).catch(() => {});
  }
  document.addEventListener?.('visibilitychange', onVisibilityTouch);
  return { open, stop, dispose, navigate };
}

// mode/T06：页面装配移至 app-boot.js（装配汇点，注入 openSettings），
// Shell/Broker 不回导上层设置页，导入图无环。
export async function bootAppPage({ openSettings }) {
  if (typeof openSettings !== 'function') throw new Error('bootAppPage: openSettings is required (inject from the page assembly entry)');
  await loadOwnerLanguage();
  const t = makeT();
  document.querySelectorAll('[data-i18n]').forEach((node) => { node.textContent = t(node.dataset.i18n, node.textContent); });
  const params = new URLSearchParams(globalThis.location.search);
  const appId = parseAppId(globalThis.location.search);
  // ADR-0036 深链：独立 app.html 入口同样转发 view 导航意图（createAppShell
  // 内部经 isValidViewToken 过滤；早于 welcome 到达时由 fund 页暂存并初始化后应用）。
  const initialView = params.get('view') ?? '';
  const appMenu = document.getElementById('app-menu-apps');
  if (appMenu) {
    mountAppMenu(appMenu, { t }).then(() => {
      const item = appMenu.querySelector(`[data-app-id="${appId}"]`);
      item?.classList.add('active');
    }).catch(() => {});
  }
  void createAppShell({ appId, initialView, t, openSettings }).open();
  storage.onChanged?.addListener((changes, area) => {
    if (area === 'local' && changes['natives-language']) void loadOwnerLanguage().catch(() => {});
  });
}
