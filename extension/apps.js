// App Center owns its Native Port, catalog state and explicit install operations.
import { createNativeClient } from './native-client.js';
import { saveAppNavigation, projectionFromApps } from './app-navigation-projection.js';
import { loadVerifiedCatalog, downloadNapPackage, decompressNap, payloadToBase64 } from './catalog-client.js';
import { resolveAppPackages, compareAppVersions, classifyAppError } from './app-catalog-policy.js';
import { appLifecycle } from './app-lifecycle.js';

async function transferPackage(pkg, { signal, onProgress } = {}) {
  const downloaded = await downloadNapPackage({ url: pkg.url, wireSize: pkg.wire_size, signal, onProgress });
  const payload = await decompressNap(downloaded, {
    artifactSha256: pkg.artifact_sha256, payloadSha256: pkg.payload_sha256, payloadSize: pkg.payload_size,
  });
  signal?.throwIfAborted();
  return payloadToBase64(payload.payloadBytes);
}

export function createAppCenter({
  t = (key, fallback) => globalThis.chrome?.i18n?.getMessage?.(key) || fallback || key,
  client, catalogLoader = loadVerifiedCatalog, packageTransfer = transferPackage,
  storage = globalThis.chrome?.storage,
  list: customList,
  toast: customToast,
} = {}) {
  const list = customList || (typeof document !== 'undefined' ? document.getElementById('apps-list') : null);
  const toast = customToast || (typeof document !== 'undefined' ? document.getElementById('apps-toast') : null);
  const state = { catalog: [], apps: [], retained: [], revision: 0, host: null,
    loading: true, catalogError: null, error: null, embedded: false, busy: new Map(), failures: new Map() };
  let toastTimer, idleTimer, progressFrame, disposed = false, suspended = false;
  const dialogs = new Set();
  let catalogAbort = new AbortController();
  let lifecycle = appLifecycle();
  const native = client || createNativeClient({
    host: 'com.natives.file_manager', timeoutMs: 20_000,
    writeMethods: new Set(['apps:install_begin', 'apps:install_package', 'apps:install_commit',
      'apps:install_abort', 'apps:uninstall', 'apps:clear_data', 'apps:recover', 'apps:set_enabled', 'apps:set_sidebar']),
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
    if (host.appsProtocolVersion !== 3) {
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
      state.retained = snapshot.retainedData || [];
      state.revision = snapshot.revision;
      state.error = state.host ? null : (state.error || 'appsHostOffline');
      await saveAppNavigation(projectionFromApps(state.apps, state.revision), storage);
    } catch (error) { if (!disposed) state.error = classifyAppError(error); }
    render();
    scheduleIdle();
  }
  async function reloadCatalog() {
    catalogAbort.abort();
    catalogAbort = new AbortController();
    const attempt = catalogAbort;
    state.loading = true;
    state.catalogError = null;
    render();
    try {
      const catalog = await catalogLoader({ signal: catalogAbort.signal, allowEmbedded: true });
      if (disposed || attempt !== catalogAbort) return;
      state.catalog = catalog.apps;
      state.embedded = Boolean(catalog.embedded);
    } catch (error) {
      if (!disposed && attempt === catalogAbort && error.code !== 'APP_CANCELLED') {
        state.catalog = [];
        state.catalogError = classifyAppError(error);
      }
    } finally { if (attempt === catalogAbort) { state.loading = false; render(); } }
  }
  async function reconnect() {
    try { await handshake(); } catch (error) { state.error = classifyAppError(error); }
    await refresh();
  }
  function items() {
    const entries = new Map(state.catalog.map((entry) => [entry.app_id, { ...entry }]));
    for (const app of state.apps) {
      const entry = entries.get(app.app_id) || { app_id: app.app_id, name: app.name, version: app.version };
      Object.assign(entry, { installed: true, installedVersion: app.version, enabled: app.enabled,
        showInSidebar: app.show_in_sidebar, recoveryPending: app.recovery_pending,
        needsMigration: app.needs_migration, sidebarOrder: app.sidebar_order });
      entries.set(app.app_id, entry);
    }
    for (const retained of state.retained) {
      const entry = entries.get(retained.app_id) || { app_id: retained.app_id, name: retained.name };
      Object.assign(entry, { retained: true, cleanupPending: retained.cleanup_pending, purgeData: retained.purge_data });
      entries.set(retained.app_id, entry);
    }
    return [...entries.values()].sort((a, b) => String(a.name).localeCompare(String(b.name)));
  }
  function available(entry) {
    try {
      const extensionVersion = globalThis.chrome?.runtime?.getManifest?.()?.version;
      return resolveAppPackages(entry, state.host, extensionVersion);
    } catch { return { reason: 'appsVerificationFailed', packages: [] }; }
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
    if (state.catalogError) list.append(statusRow(state.catalogError, reloadCatalog));
    else if (state.embedded) list.append(statusRow('appsEmbeddedCatalog', reloadCatalog));
    if (state.loading) list.append(statusRow('appsCatalogLoading'));
    const entries = items();
    if (!entries.length && !state.loading && !state.catalogError && !state.error) {
      list.append(node('div', 'apps-empty', t('appsEmpty')));
    }
    for (const entry of entries) list.append(card(entry));
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
    const description = typeof entry.description === 'object'
      ? entry.description[globalThis.chrome?.i18n?.getUILanguage?.().startsWith('zh') ? 'zh_CN' : 'en']
      : entry.description;
    if (description) body.append(node('div', 'desc', description));
    const readiness = available(entry);
    const downloadBytes = readiness.packages.reduce((sum, pkg) => sum + pkg.wire_size, 0);
    const meta = node('div', 'meta');
    if (entry.version || entry.installedVersion) meta.append(node('span', '', `v${entry.installedVersion || entry.version}`));
    if (downloadBytes) meta.append(node('span', '', `${downloadBytes.toLocaleString()} B`));
    body.append(meta);
    const actions = node('div', 'actions');
    const busy = state.busy.get(entry.app_id);
    const disabled = state.busy.size > 0;
    if (busy) {
      actions.append(node('span', 'status', t(busy.stage)));
      const progress = node('progress', 'apps-progress');
      progress.max = 1;
      progress.setAttribute('aria-label', t(busy.stage));
      if (Number.isFinite(busy.progress)) progress.value = busy.progress;
      actions.append(progress);
      if (busy.controller) actions.append(action('close', t('cancel'), () => busy.controller.abort(), { disabled: busy.committing }));
    } else {
      let status = entry.installed ? (entry.enabled ? 'appsInstalled' : 'appsDisabled') : 'appsNotInstalled';
      if (!entry.installed && readiness.reason) status = readiness.reason;
      if (entry.cleanupPending) status = 'appsCleanupPending';
      else if (entry.recoveryPending) status = 'appsRecoveryPending';
      else if (entry.needsMigration) status = 'appsNeedsMigration';
      else if (entry.retained && !entry.installed) status = 'appsDataRetained';
      actions.append(node('span', 'status', t(status)));
      if (entry.recoveryPending) {
        actions.append(action('refresh', t('retry'), async () => {
          lifecycle.notify('maintenance', entry.app_id);
          try { await native.call('apps:recover', { appId: entry.app_id }); }
          finally { lifecycle.notify('changed', entry.app_id); await refresh(); }
        }, { disabled }));
      } else if (entry.installed && !entry.cleanupPending) {
        actions.append(action('open', t('appsOpen'), () => openApp(entry), { disabled: disabled || !entry.enabled || Boolean(entry.needsMigration) || Boolean(entry.recoveryPending) }));
        const canUpdate = (entry.needsMigration || compareAppVersions(entry.version, entry.installedVersion) === 1) && !readiness.reason;
        if (canUpdate) {
          actions.append(action('up', t('appsUpdate'), () => install(entry), { primary: true, disabled }));
        }
        body.append(toggle(t('appsEnabled'), entry.enabled, async (enabled) => {
          if (!enabled) lifecycle.notify('maintenance', entry.app_id);
          try { await native.call('apps:set_enabled', { appId: entry.app_id, enabled }); }
          finally { lifecycle.notify('changed', entry.app_id); }
        }));
        body.append(toggle(t('appsShowInSidebar'), entry.showInSidebar, (show) =>
          native.call('apps:set_sidebar', { appId: entry.app_id, show })));
        actions.append(action('trash', t('appsUninstall'), () => confirmUninstall(entry), { danger: true, disabled }));
      } else if (!entry.cleanupPending && !readiness.reason) {
        actions.append(action('download', t('appsInstall'), () => install(entry), { primary: true, disabled }));
      }
      if (entry.retained || entry.cleanupPending) {
        actions.append(action(entry.cleanupPending ? 'refresh' : 'trash', t(entry.cleanupPending ? 'retry' : 'appsClearData'),
          () => confirmUninstall(entry, !entry.installed), { danger: true, disabled }));
      }
    }
    const failure = state.failures.get(entry.app_id);
    if (failure) body.append(node('div', 'apps-inline-error', t(failure)));
    article.append(thumb, body, actions);
    return article;
  }

  async function install(entry) {
    if (state.busy.size) throw Object.assign(new Error('app busy'), { code: 'APP_BUSY' });
    const controller = new AbortController();
    const busy = { stage: 'appsResolving', controller, committing: false };
    state.busy.set(entry.app_id, busy);
    state.failures.delete(entry.app_id);
    render();
    let installId, committed = false;
    try {
      await handshake();
      const extensionVersion = globalThis.chrome?.runtime?.getManifest?.()?.version;
      const { packages, reason } = resolveAppPackages(entry, state.host, extensionVersion);
      if (reason) throw Object.assign(new Error(reason), { code: 'APP_PACKAGE_INVALID' });
      const existingApp = state.apps.find((a) => a.app_id === entry.app_id);
      const enabled = existingApp ? existingApp.enabled : true;
      const show_in_sidebar = existingApp ? existingApp.show_in_sidebar : true;
      const sidebar_order = existingApp ? (Number(existingApp.sidebar_order) || 0) : 0;
      const request = {
        app: { app_id: entry.app_id, kind: 'extension_app', name: entry.name, version: entry.version,
          enabled, show_in_sidebar, sidebar_order, runtime_spec: entry.runtime_spec || {},
          surface: entry.surface, manifest: entry.manifest || {} },
        packages: packages.map(({ package_id, kind, version, platform, arch, wire_size, payload_size,
          artifact_sha256, payload_sha256, required = true }) => ({
          package_id, kind, version, platform, arch, wire_size, payload_size, artifact_sha256, payload_sha256, required,
        })),
        permissions: entry.permissions || [],
        min_host_version: entry.minHostVersion || entry.minNativesVersion || null,
      };
      controller.signal.throwIfAborted();
      const tx = await native.call('apps:install_begin', {
        request: payloadToBase64(new TextEncoder().encode(JSON.stringify(request))),
      });
      installId = tx.install_id;
      let transferred = 0;
      const total = packages.reduce((sum, pkg) => sum + pkg.wire_size, 0);
      for (const pkg of packages) {
        busy.stage = 'appsDownloading'; render();
        const data = await packageTransfer(pkg, { signal: controller.signal, onProgress: (bytes) => {
          busy.progress = (transferred + bytes) / total;
          if (globalThis.requestAnimationFrame) {
            progressFrame ??= requestAnimationFrame(() => { progressFrame = undefined; render(); });
          } else render();
        } });
        controller.signal.throwIfAborted();
        transferred += pkg.wire_size;
        busy.stage = 'appsVerifying'; render();
        await native.call('apps:install_package', { installId, packageId: pkg.package_id, data });
      }
      controller.signal.throwIfAborted();
      busy.stage = 'appsCommitting'; busy.committing = true; busy.progress = undefined; render();
      lifecycle.notify('maintenance', entry.app_id);
      await native.call('apps:install_commit', { installId });
      committed = true;
      setToast(t('appsInstalledToast'));
    } catch (error) {
      if (installId && !disposed && !suspended && !committed) {
        try { await native.call('apps:install_abort', { installId, errorCode: 'APP_CANCELLED', errorMessage: 'install interrupted' }); }
        catch { state.failures.set(entry.app_id, 'appsRecoveryPending'); }
      }
      if (!state.failures.has(entry.app_id)) state.failures.set(entry.app_id,
        controller.signal.aborted ? 'appsCancelled' : classifyAppError(error));
      throw controller.signal.aborted ? Object.assign(new Error('cancelled'), { code: 'APP_CANCELLED' }) : error;
    } finally {
      lifecycle.notify('changed', entry.app_id);
      state.busy.delete(entry.app_id);
      if (!disposed) await refresh();
      render();
    }
  }

  async function uninstall(appId, { purgeData = false, confirmPurge = false, dataOnly = false } = {}) {
    if (state.busy.size) throw Object.assign(new Error('app busy'), { code: 'APP_BUSY' });
    state.busy.set(appId, { stage: 'appsCommitting' });
    state.failures.delete(appId);
    render();
    lifecycle.notify('maintenance', appId);
    try {
      await native.call(dataOnly ? 'apps:clear_data' : 'apps:uninstall',
        dataOnly ? { appId, confirmPurge } : { appId, purgeData, confirmPurge });
      setToast(t(purgeData ? 'appsDataDeleted' : 'appsUninstalledToast'));
    } catch (error) { state.failures.set(appId, classifyAppError(error)); throw error; }
    finally {
      lifecycle.notify('changed', appId);
      state.busy.delete(appId);
      await refresh();
    }
  }
  async function confirmUninstall(entry, dataOnly = false) {
    const choice = await dialog({ title: t(dataOnly ? 'appsClearData' : 'appsUninstallTitle'),
      body: entry.name + '\n' + t(dataOnly ? 'appsPurgeWarning' : 'appsUninstallBody'),
      confirmLabel: t(dataOnly ? 'appsClearData' : 'appsUninstall'), checkbox: !dataOnly,
      checked: Boolean(entry.purgeData), danger: dataOnly });
    if (!choice) return;
    const purgeData = dataOnly || choice.purgeData;
    if (purgeData && !await dialog({ title: t('appsPurgeConfirmTitle'), body: entry.name + '\n' + t('appsPurgeWarning'),
      confirmLabel: t('appsClearData'), danger: true })) return;
    await uninstall(entry.app_id, { purgeData, confirmPurge: purgeData, dataOnly });
  }
  function dialog({ title, body, confirmLabel, checkbox, checked, danger }) {
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
      if (checkbox) {
        const label = node('label', 'apps-purge-choice');
        input = node('input'); input.type = 'checkbox'; input.checked = checked;
        label.append(input, node('span', '', t('appsPurgeChoice')));
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
  function openApp(entry) {
    const route = `app.html?app=${encodeURIComponent(entry.app_id)}`;
    if (globalThis.chrome?.tabs?.create) chrome.tabs.create({ url: route });
    else globalThis.window?.open(route, '_blank');
  }
  function suspend() {
    suspended = true;
    catalogAbort.abort();
    for (const busy of state.busy.values()) busy.controller?.abort();
    for (const close of dialogs) close(null);
    native.disconnect?.();
    lifecycle.close();
    clearTimeout(toastTimer); clearTimeout(idleTimer);
    if (progressFrame !== undefined) globalThis.cancelAnimationFrame?.(progressFrame);
    progressFrame = undefined;
  }
  function resume(event) {
    if (!event.persisted || disposed || !suspended) return;
    suspended = false;
    lifecycle = appLifecycle();
    void Promise.all([reloadCatalog(), reconnect()]);
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
  document.querySelectorAll('[data-i18n]').forEach((el) => { el.textContent = t(el.dataset.i18n, el.textContent); });
  const ready = Promise.all([reloadCatalog(), reconnect()]);
  return { refresh, reloadCatalog, state, install, uninstall, dispose, ready };
}

if (typeof document !== 'undefined' && document.getElementById('apps-list')) createAppCenter();
