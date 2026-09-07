// App Center (ADR-0025 D40/D41/D42).
//
// Settings-level surface: CatalogEntry + InstalledApp = AppCenterItem.
// A catalog entry does NOT become an App until install_commit (D42).
//
// Phase A4: metadata-only install flow against a build-time fake catalog
// (Gate A4). Download / signature / artifact verification land in A5.
// The native port here talks ONLY the apps:* domain namespace.

import { createNativeClient } from './native-client.js';
import { saveAppNavigation, projectionFromApps } from './app-navigation-projection.js';
import {
  CATALOG_PUBLIC_KEY_B64,
  verifyCatalogSignature,
  downloadNapPackage,
  decompressNap,
  payloadToBase64,
} from './catalog-client.js';

const NATIVE_HOST = 'com.natives.file_manager';

// ADR-0025 D44: the catalog URL is build-time fixed, never user-configured.
// Dev ships the signed pair embedded in the extension: catalog-v1.json +
// catalog-v1.sig (signed by the Natives-App-Catalog dev key; the public
// key is compiled in via catalog-client.js). The release pipeline swaps in
// the production pair without touching App Center code.
const CATALOG_URL = new URL('apps/catalog-v1.json', import.meta.url).href;
const CATALOG_SIG_URL = new URL('apps/catalog-v1.sig', import.meta.url).href;

function base64EncodeUnicode(str) {
  const bytes = new TextEncoder().encode(str);
  let bin = '';
  for (let i = 0; i < bytes.length; i++) bin += String.fromCharCode(bytes[i]);
  return btoa(bin);
}

async function loadCatalog() {
  // Fetch raw bytes (signature verification needs the exact bytes, not the
  // re-serialized JSON of response.json()).
  const [catalogRes, sigRes] = await Promise.all([
    fetch(CATALOG_URL, { cache: 'no-store' }),
    fetch(CATALOG_SIG_URL, { cache: 'no-store' }),
  ]);
  if (!catalogRes.ok) throw new Error(`catalog ${catalogRes.status}`);
  if (!sigRes.ok) throw new Error(`catalog signature ${sigRes.status}`);
  const catalogBytes = new Uint8Array(await catalogRes.arrayBuffer());
  const signatureB64 = await sigRes.text();
  // D8: verify BEFORE parsing. A bad catalog is "catalog unavailable",
  // never a fallback to an unverified source.
  await verifyCatalogSignature({ catalogBytes, signatureB64, publicKeyB64: CATALOG_PUBLIC_KEY_B64 });
  const catalog = JSON.parse(new TextDecoder().decode(catalogBytes));
  if (!Array.isArray(catalog.apps)) throw new Error('catalog: missing apps[]');
  return catalog;
}

// A5 package transfer (D3/D8/D10): fetch the .nap (streaming, 5 MiB wire
// gate) → artifactSha256 → gzip decompress (20 MiB payload gate) →
// payloadSha256 → base64. Runs in the page; the host re-verifies size +
// payload hash independently (host-side trust boundary).
async function transferPackage(pkg, { fetchImpl } = {}) {
  const url = new URL(pkg.url, CATALOG_URL).href;
  const { artifactBytes, wireSize, digest } = await downloadNapPackage({
    url,
    wireSize: pkg.wire_size,
    fetchImpl,
  });
  const { payloadBytes, payloadSize } = await decompressNap(
    { artifactBytes, digest },
    {
      artifactSha256: pkg.artifact_sha256,
      payloadSha256: pkg.payload_sha256,
      payloadSize: pkg.payload_size,
    },
  );
  return { dataBase64: payloadToBase64(payloadBytes), wireSize, payloadSize };
}

export function createAppCenter({
  t = (k, f) => f || k,
  client,
  catalogLoader = loadCatalog,
  storage = globalThis.chrome?.storage,
} = {}) {
  const listEl = document.getElementById('apps-list');
  const toastEl = document.getElementById('apps-toast');
  let toastTimer;
  const state = {
    catalog: [],
    apps: [],
    revision: 0,
    busy: new Map(), // appId -> { stage, progress }
    error: null,
  };

  const writeMethods = new Set([
    'apps:install_begin', 'apps:install_commit', 'apps:install_abort',
    'apps:uninstall', 'apps:set_enabled', 'apps:set_sidebar',
  ]);
  const nativeClient = client || createNativeClient({
    host: NATIVE_HOST,
    writeMethods,
    timeoutMs: 20_000,
    onDisconnect: () => setToast(t('appsHostOffline', 'Native Host 未连接，请确认 Natives 已启动'), true),
  });

  function setToast(message, isError = false) {
    if (!message) { toastEl.hidden = true; return; }
    toastEl.textContent = message;
    toastEl.className = `apps-toast${isError ? ' error' : ''}`;
    toastEl.hidden = false;
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => { toastEl.hidden = true; }, 4000);
  }

  async function refresh() {
    try {
      const { apps, revision } = await nativeClient.call('apps:list');
      state.apps = apps || [];
      state.revision = revision || 0;
      state.error = null;
    } catch (error) {
      state.error = error.message || String(error);
    }
    render();
  }

  async function init() {
    try {
      const catalog = await catalogLoader();
      state.catalog = catalog.apps.map((entry) => ({ ...entry, catalogOnly: true }));
    } catch {
      state.catalog = [];
      setToast(t('appsCatalogUnavailable', '应用目录暂不可用，仅显示已安装应用'), true);
    }
    await refresh();
  }

  // D42: merge catalog + installed into AppCenterItem rows.
  function items() {
    const byId = new Map(state.catalog.map((entry) => [entry.app_id, { ...entry, installed: false }]));
    for (const app of state.apps) {
      const entry = byId.get(app.app_id);
      if (entry) {
        entry.installed = true;
        entry.installedVersion = app.version;
        entry.enabled = app.enabled;
        entry.showInSidebar = app.show_in_sidebar;
      } else {
        byId.set(app.app_id, {
          app_id: app.app_id,
          name: app.name,
          version: app.version,
          description: '',
          permissions: [],
          icon: 'box',
          installed: true,
          installedVersion: app.version,
          enabled: app.enabled,
          showInSidebar: app.show_in_sidebar,
        });
      }
    }
    return [...byId.values()].sort((a, b) => a.name.localeCompare(b.name));
  }

  function hasUpdate(entry) {
    return entry.installed && entry.version !== entry.installedVersion;
  }

  function render() {
    const rows = items();
    listEl.replaceChildren();
    if (state.error && rows.length === 0) {
      const empty = document.createElement('div');
      empty.className = 'apps-empty';
      empty.textContent = `${t('appsHostOffline', 'Native Host 未连接')}：${state.error}`;
      listEl.append(empty);
      return;
    }
    if (rows.length === 0) {
      const empty = document.createElement('div');
      empty.className = 'apps-empty';
      empty.textContent = t('appsEmpty', '暂无可安装应用');
      listEl.append(empty);
      return;
    }
    for (const entry of rows) {
      listEl.append(renderCard(entry));
    }
  }

  function renderCard(entry) {
    const card = document.createElement('article');
    card.className = 'app-card';
    card.setAttribute('role', 'listitem');
    card.dataset.appId = entry.app_id;

    const busy = state.busy.get(entry.app_id);

    const thumb = document.createElement('div');
    thumb.className = 'thumb';
    thumb.innerHTML = `<svg class="icon"><use href="#i-${entry.icon || 'box'}" /></svg>`;

    const body = document.createElement('div');
    body.className = 'body';

    const row1 = document.createElement('div');
    row1.className = 'row1';
    const name = document.createElement('span');
    name.className = 'name';
    name.textContent = entry.name || entry.app_id;
    row1.append(name);
    if (hasUpdate(entry)) {
      const badge = document.createElement('span');
      badge.className = 'badge update';
      badge.textContent = t('appsHasUpdate', '有更新');
      row1.append(badge);
    }
    if (busy && busy.stage !== 'installing') {
      const badge = document.createElement('span');
      badge.className = 'badge broken';
      badge.textContent = t('appsBroken', '异常');
      row1.append(badge);
    }

    const desc = document.createElement('div');
    desc.className = 'desc';
    desc.textContent = entry.description || '';

    const meta = document.createElement('div');
    meta.className = 'meta';
    const version = document.createElement('span');
    version.innerHTML = `<svg class="icon"><use href="#i-box" /></svg>`;
    version.append(`v${entry.installed ? entry.installedVersion : entry.version}`);
    const size = document.createElement('span');
    size.innerHTML = `<svg class="icon"><use href="#i-download" /></svg>`;
    size.append(entry.wireSize ? `${(entry.wireSize / 1048576).toFixed(1)} MiB` : '—');
    const perms = document.createElement('span');
    perms.innerHTML = `<svg class="icon"><use href="#i-target" /></svg>`;
    perms.append((entry.permissions || []).join(' / ') || t('appsNoPermissions', '无权限需求'));
    meta.append(version, size, perms);

    body.append(row1, desc, meta);

    const actions = document.createElement('div');
    actions.className = 'actions';
    const status = document.createElement('div');
    status.className = 'status';

    if (busy) {
      status.textContent = stageLabel(busy.stage);
      const progress = document.createElement('div');
      progress.className = 'progress';
      progress.innerHTML = `<div style="width:${busy.progress ?? 8}%"></div>`;
      actions.append(status, progress);
    } else if (entry.installed) {
      status.textContent = entry.enabled ? t('appsInstalled', '已安装') : t('appsDisabled', '已停用');
      const open = actionBtn('i-open', t('appsOpen', '打开'), async () => openApp(entry), !entry.enabled);
      open.disabled = !entry.enabled;
      const buttons = [status, open];
      if (hasUpdate(entry)) {
        // V1 update path: uninstall-then-reinstall keeps the state machine
        // honest (install_begin rejects a live app with APP_CONFLICT).
        const update = actionBtn('i-up', t('appsUpdate', '更新'), async () => {
          setToast(t('appsUpdateHint', 'V1 更新：先卸载当前版本，再安装新版本（个人数据保留）'));
          await uninstall(entry.app_id, { silentUninstall: true });
          await install(entry);
        }, false, true);
        buttons.push(update);
      }
      const uninstallBtn = actionBtn('i-trash', t('appsUninstall', '卸载'), async () => confirmUninstall(entry), true);
      buttons.push(uninstallBtn);
      actions.append(...buttons);
    } else {
      status.textContent = t('appsNotInstalled', '未安装');
      actions.append(status, actionBtn('i-download', t('appsInstall', '安装'), () => install(entry), false, true));
    }

    card.append(thumb, body, actions);
    return card;
  }

  function stageLabel(stage) {
    const map = {
      resolving: t('appsResolving', '解析目录…'),
      downloading: t('appsDownloading', '下载安装包…'),
      verifying: t('appsVerifying', '校验完整性…'),
      registering: t('appsRegistering', '注册运行时…'),
      committing: t('appsCommitting', '写入应用库…'),
    };
    return map[stage] || stage;
  }

  function actionBtn(icon, label, onClick, danger = false, primary = false) {
    const btn = document.createElement('button');
    btn.type = 'button';
    btn.className = `action${primary ? ' primary' : ''}${danger ? ' danger' : ''}`;
    btn.innerHTML = `<svg class="icon"><use href="#i-${icon}" /></svg><span>${label}</span>`;
    btn.onclick = async (event) => {
      event.stopPropagation();
      btn.disabled = true;
      try { await onClick(); } catch (error) {
        setToast(error.message || String(error), true);
      } finally { btn.disabled = false; }
    };
    return btn;
  }

  // A4 metadata-only install: begin → (A5: download/verify stages) → commit.
  async function install(entry) {
    const appId = entry.app_id;
    state.busy.set(appId, { stage: 'resolving', progress: 8 });
    render();
    try {
      const request = {
        app: {
          app_id: appId,
          kind: 'extension_app',
          name: entry.name || appId,
          version: entry.version,
          enabled: true,
          show_in_sidebar: true,
          sidebar_order: 0,
          runtime_spec: entry.runtime_spec || { host: `com.natives.app.${appId}`, version: entry.version },
          surface: entry.surface || { icon: entry.icon || 'box', route: `app.html?app=${encodeURIComponent(appId)}` },
          manifest: entry.manifest || { permissions: entry.permissions || [] },
        },
        packages: (entry.packages || []).map((pkg) => ({ ...pkg })),
        permissions: entry.permissions || [],
      };
      const tx = await nativeClient.call('apps:install_begin', { request: base64EncodeUnicode(JSON.stringify(request)) });

      // A5 package transfer (D10/D11): per package — download with the
      // 5 MiB streaming wire gate, verify artifactSha256, gzip-decompress
      // under the 20 MiB cap, verify payloadSha256, then hand the base64
      // payload to the host, which re-checks size/hash and stages it.
      // Any failure aborts the transaction (staging cleanup + failed
      // state) so a retry starts clean.
      const packages = entry.packages || [];
      try {
        for (const pkg of packages) {
          state.busy.set(appId, { stage: 'downloading', progress: 30 });
          render();
          const { dataBase64 } = await transferPackage(pkg);
          state.busy.set(appId, { stage: 'verifying', progress: 55 });
          render();
          await nativeClient.call('apps:install_package', {
            installId: tx.install_id,
            packageId: pkg.package_id,
            data: dataBase64,
          });
        }
        state.busy.set(appId, { stage: 'committing', progress: 80 });
        render();
        await nativeClient.call('apps:install_commit', { installId: tx.install_id });
      } catch (error) {
        // best-effort rollback of the open transaction so a failed stage
        // (download/hash/signature/commit) never blocks the next install
        // attempt (APP_CONFLICT guard).
        try { await nativeClient.call('apps:install_abort', { installId: tx.install_id }); } catch { /* aborted or already gone */ }
        throw error;
      }

      await syncProjection();
      setToast(t('appsInstalledToast', '应用已安装'));
      await refresh();
    } catch (error) {
      throw error;
    } finally {
      state.busy.delete(appId);
      render();
    }
  }

  async function uninstall(appId, { silentUninstall = false } = {}) {
    state.busy.set(appId, { stage: 'committing', progress: 40 });
    render();
    try {
      await nativeClient.call('apps:uninstall', { appId });
      await syncProjection();
      if (!silentUninstall) setToast(t('appsUninstalledToast', '应用已卸载（个人数据保留）'));
      await refresh();
    } finally {
      state.busy.delete(appId);
      render();
    }
  }

  async function confirmUninstall(entry) {
    const ok = await confirmDialog({
      title: t('appsUninstallTitle', '卸载应用？'),
      body: t('appsUninstallBody', '将删除程序、运行时与注册信息，保留个人数据。个人数据可在「删除应用及全部数据」中另行清除。'),
      confirmLabel: t('appsUninstall', '卸载'),
    });
    if (ok) await uninstall(entry.app_id);
  }

  // D37: projection refresh after every successful mutation.
  async function syncProjection() {
    try {
      const { apps, revision } = await nativeClient.call('apps:list');
      await saveAppNavigation(projectionFromApps(apps, revision), storage);
    } catch {
      // UI cache only; files page will rebuild on next live revision diff
    }
  }

  function openApp(entry) {
    const route = `app.html?app=${encodeURIComponent(entry.app_id)}`;
    if (globalThis.chrome?.tabs?.create) chrome.tabs.create({ url: route });
    else window.open(route, '_blank');
  }

  function confirmDialog({ title, body, confirmLabel }) {
    return new Promise((resolve) => {
      const backdrop = document.createElement('div');
      backdrop.className = 'apps-dialog-backdrop';
      backdrop.innerHTML = `
        <div class="apps-dialog" role="alertdialog" aria-modal="true">
          <h2></h2><p></p>
          <div class="actions">
            <button class="action" type="button" data-role="cancel"></button>
            <button class="action danger" type="button" data-role="ok"></button>
          </div>
        </div>`;
      backdrop.querySelector('h2').textContent = title;
      backdrop.querySelector('p').textContent = body;
      const cancel = backdrop.querySelector('[data-role="cancel"]');
      const okBtn = backdrop.querySelector('[data-role="ok"]');
      cancel.textContent = t('cancel', '取消');
      okBtn.textContent = confirmLabel;
      const close = (value) => { backdrop.remove(); resolve(value); };
      cancel.onclick = () => close(false);
      okBtn.onclick = () => close(true);
      backdrop.onclick = (event) => { if (event.target === backdrop) close(false); };
      document.body.append(backdrop);
      okBtn.focus();
    });
  }

  const back = document.getElementById('nav-back');
  if (back) back.onclick = () => {
    if (globalThis.chrome?.tabs?.query) {
      chrome.tabs.query({ url: `${location.origin}/space.html` }, (tabs) => {
        if (tabs?.[0]) chrome.tabs.update(tabs[0].id, { active: true });
        else window.close();
      });
    } else window.close();
  };

  // apply i18n to static text
  document.querySelectorAll('[data-i18n]').forEach((el) => { el.textContent = t(el.dataset.i18n, el.textContent); });

  init().catch((error) => setToast(error.message || String(error), true));

  return { refresh, state, install, uninstall };
}

// boot when loaded as a page (not imported in tests)
if (typeof document !== 'undefined' && document.getElementById('apps-list')) {
  const t = (key, fallback) => (globalThis.chrome?.i18n?.getMessage?.(key) || '') || fallback;
  createAppCenter({ t });
}
