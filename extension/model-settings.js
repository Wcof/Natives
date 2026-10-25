import { createModelSettingsAPI } from './model-settings-api.js';
import { createModelSettingsView, renderNewProvider } from './model-settings-view.js';
import { localizedModelError } from './model-settings-errors.js';
import { handleUsageAction, handleUsageSubmit } from './model-usage-controller.js';
import { userTimezone, acquireModelApi, releaseModelApi, sharedCall } from './ai-usage-client/index.js';

let instance;

export async function openModelSettings(options) {
  if (!instance) instance = new ModelSettings(options);
  instance.setLocale(options);
  if (options?.initialPage) instance.setInitialPage(options.initialPage);
  if (options?.initialUsageFilter) instance.setInitialUsageFilter(options.initialUsageFilter);
  await instance.open();
}

class ModelSettings {
  constructor({ t, language, returnFocus }) {
    this.t = t;
    this.language = language;
    this.returnFocus = returnFocus;
    this.snapshot = null;
    this.selectedID = '';
    this.appCenter = null;
    this.initialPage = null;

    this.usageFilter = { range: '4h', page: 1, limit: 20 };
    this.usageFilterOptions = null;
    this.overviewData = null;
    this.analyticsData = null;
    this.eventsData = null;
    this.pricingData = null;
    this.billingData = null;
    this.importerWizard = null;

    this.authFiles = [];
    this.quotaMap = {};

    this.api = createModelSettingsAPI({
      onEvent: (event) => this.handleEvent(event),
      onDisconnect: (_error, intentional) => { if (!intentional && this.view?.dialog.open) this.showError(this.t('modelHostDisconnected', '模型 Host 已断开')); },
    });
    this.view = createModelSettingsView({ t: (...args) => this.t(...args), onAction: (...args) => this.handleAction(...args) });
    this.view.dialog.addEventListener('close', () => {
      this.api.disconnect();
      this.appCenter?.dispose();
      this.appCenter = null;
      this.returnFocus?.focus?.();
    });
    this.view.dialog.addEventListener('submit', (event) => this.handleSubmit(event));
    this.view.dialog.addEventListener('model-provider-submit', (event) => this.saveProvider(event));
    this.view.dialog.addEventListener('model-default-model-change', (event) => {
      const { providerId = '', modelId = '' } = event.detail || {};
      const revision = this.snapshot?.revision;
      this.mutate(() => this.api.setDefaultModel({ providerId, modelId, expectedRevision: revision }));
    });
  }
  setLocale({ t, language, returnFocus }) { this.t = t; this.language = language; this.returnFocus = returnFocus; }
  setInitialPage(page) { this.initialPage = page; }
  setInitialUsageFilter(filter) {
    if (!filter || typeof filter !== 'object') return;
    const allowed = ['range', 'startTime', 'endTime', 'model', 'provider', 'source', 'accessKeyId', 'result'];
    this.usageFilter = { range: '4h', page: 1, limit: 20, ...Object.fromEntries(allowed.filter((k) => filter[k] != null && filter[k] !== '').map((k) => [k, filter[k]])) };
  }
  async open() {
    this.view.open();
    if (this.initialPage) {
      this.view.setPage(this.initialPage);
      this.initialPage = null;
    }
    await this.refresh();
  }
  async ensureAppCenter() {
    if (this.appCenter) {
      await this.appCenter.refresh();
      return;
    }
    this.view.setLoading(true);
    try {
      const { createAppCenter } = await import('./apps.js');
      const appsList = this.view.dialog.querySelector('[data-role="appsList"]');
      this.appCenter = createAppCenter({
        list: appsList,
        language: this.language,
        t: (...args) => this.t(...args),
        openSettings: (context) => openModelSettings({ ...context, returnFocus: context?.returnFocus ?? document.activeElement }),
      });
      await this.appCenter.ready;
    } catch (err) {
      this.showError(err);
    } finally {
      this.view.setLoading(false);
    }
  }
  async refresh() {
    this.view.setLoading(true);
    this.view.showError('');
    try {
      if (this.view.activePage === 'apps') {
        await this.ensureAppCenter();
        return;
      }
      this.snapshot = await this.api.snapshot();
      if (!this.snapshot.providers.some((provider) => provider.id === this.selectedID)) this.selectedID = this.snapshot.providers[0]?.id || '';
      if (this.view.activePage === 'usage') {
        await this.loadUsageData();
      } else if (this.view.activePage === 'quota') {
        await this.loadAuthFiles();
      }
      this.render();
    } catch (error) {
      if (this.view.activePage !== 'apps') this.showError(error);
    } finally {
      this.view.setLoading(false);
    }
  }

  async loadAuthFiles() {
    try {
      this.authFiles = await this.api.listAuthFiles() || [];
    } catch (err) {
      this.showError(err);
    }
  }

  async loadUsageData() {
    const usageApi = acquireModelApi();
    try {
      if (!this.usageFilterOptions) this.usageFilterOptions = await sharedCall(usageApi, 'getUsageAnalysis', { range: 'all' });
      const tab = this.view.activeUsageTab;
      const query = {
        range: this.usageFilter.range,
        startTime: this.usageFilter.startTime,
        endTime: this.usageFilter.endTime,
        models: this.usageFilter.model ? [this.usageFilter.model] : [],
        providers: this.usageFilter.provider ? [this.usageFilter.provider] : [],
        sources: this.usageFilter.source ? [this.usageFilter.source] : [],
        accessKeyIds: this.usageFilter.accessKeyId ? [this.usageFilter.accessKeyId] : [],
        result: this.usageFilter.result || undefined,
        timezone: userTimezone(),
      };
      if (tab === 'overview') {
        this.overviewData = await sharedCall(usageApi, 'getUsageOverview', query);
      } else if (tab === 'analytics') {
        this.analyticsData = await sharedCall(usageApi, 'getUsageAnalysis', query);
      } else if (tab === 'events') {
        this.eventsData = await sharedCall(usageApi, 'getUsageEvents', {
          ...query,
          offset: (this.usageFilter.page - 1) * this.usageFilter.limit,
          limit: this.usageFilter.limit,
        });
      } else if (tab === 'pricing') {
        this.pricingData = await sharedCall(usageApi, 'getUsagePricing');
      } else if (tab === 'billing') {
        this.billingData = await sharedCall(usageApi, 'getUsageBilling', { includeEntries: true });
      }
    } catch (err) {
      this.showError(err);
    } finally {
      releaseModelApi();
    }
  }

  render() {
    if (!this.snapshot) return;
    this.view.render(
      this.snapshot,
      this.selectedID,
      {
        overviewData: this.overviewData,
        analyticsData: this.analyticsData,
        eventsData: this.eventsData,
        pricingData: this.pricingData,
        billingData: this.billingData,
        currentFilter: this.usageFilter,
        filterOptions: this.usageFilterOptions,
      },
      {
        authFiles: this.authFiles,
        quotaMap: this.quotaMap,
      },
    );
  }

  showError(error) {
    if (!error) { this.view.showError(''); return; }
    this.view.showError(localizedModelError(error, this.t));
  }

  async mutate(work, { select } = {}) {
    if (this.mutating) return null;
    this.mutating = true;
    this.view.setLoading(true);
    this.view.showError('');
    try {
      const result = await work();
      this.snapshot = result?.snapshot || result;
      if (select) this.selectedID = select(this.snapshot);
      this.render();
      return result;
    } catch (error) {
      this.showError(error);
      return null;
    } finally {
      this.mutating = false;
      this.view.setLoading(false);
    }
  }

  async setResident(resident, expectedRevision) {
    return this.mutate(() => this.api.setResident({ resident, expectedRevision }));
  }

  async handleAction(action, target, event) {
    const revision = this.snapshot?.revision;
    if (action === 'close') return this.view.close();
    if (this.mutating) return;
    if (!this.snapshot && action !== 'refresh' && !(action === 'select-model-page' && target.dataset.page === 'apps')) {
      if (action === 'select-model-page') this.view.setPage(target.dataset.page);
      return;
    }
    if (action === 'refresh') {
      if (this.view.activePage === 'apps') {
        this.view.setLoading(true);
        try {
          if (this.appCenter) await this.appCenter.refresh();
        } finally {
          this.view.setLoading(false);
        }
        return;
      }
      return this.refresh();
    }
    if (action === 'select-model-page') {
      this.view.showError('');
      const page = target.dataset.page;
      this.view.setPage(page);
      if (page === 'apps') {
        await this.ensureAppCenter();
        return;
      }
      if (page === 'usage') {
        this.view.setLoading(true);
        try {
          await this.loadUsageData();
        } finally {
          this.view.setLoading(false);
        }
      }
      if (page === 'quota') {
        this.view.setLoading(true);
        try {
          await this.loadAuthFiles();
        } finally {
          this.view.setLoading(false);
        }
      }
      this.render();
      return;
    }
    if (action === 'select-provider') { this.selectedID = target.dataset.providerId; this.render(); return; }
    if (action === 'new-provider') { renderNewProvider(this.view.dialog.querySelector('[data-role="detail"]'), this.t, this.snapshot); return; }
    if (action === 'cancel-new-provider') return this.render();
    if (action === 'resident') return this.setResident(target.checked, revision);
    if (action === 'delete-provider') return this.confirmDelete(this.t('modelDeleteProviderConfirm', '确定删除这个连接及其模型吗？'), () => this.api.deleteProvider({ providerId: this.selectedID, expectedRevision: revision }), { select: (snapshot) => snapshot.providers[0]?.id || '' });
    if (action === 'test-provider' || action === 'test-new-provider') return this.testProvider(target.closest('form'));
    if (action === 'refresh-models') return this.mutate(() => this.api.refreshModels({ providerId: this.selectedID, expectedRevision: revision }));
    if (action === 'toggle-model') return this.mutate(() => this.api.setModelEnabled({ providerId: this.selectedID, modelId: target.dataset.modelId, enabled: target.dataset.enabled === 'true', expectedRevision: revision }));
    if (action === 'edit-model') return this.editModel(target.dataset.modelId);
    if (action === 'cancel-model-edit') return this.resetModelForm();
    if (action === 'delete-model') return this.mutate(() => this.api.deleteModel({ providerId: this.selectedID, modelId: target.dataset.modelId, expectedRevision: revision }));

    if (action === 'auth-files-refresh') {
      this.view.setLoading(true);
      await this.loadAuthFiles();
      this.view.setLoading(false);
      this.render();
      return;
    }
    if (action === 'auth-files-open-dir') {
      try {
        await this.api.openAuthDir();
      } catch (err) {
        this.showError(err);
      }
      return;
    }
    if (action === 'auth-files-toggle') {
      const name = target.dataset.name;
      const disabled = target.dataset.disabled === 'true';
      try {
        await this.api.updateAuthFile({ name, disabled });
        await this.loadAuthFiles();
        this.render();
      } catch (err) {
        this.showError(err);
      }
      return;
    }
    if (action === 'auth-files-delete') {
      const name = target.dataset.name;
      return this.confirmDelete(this.t('confirmDeleteAuthFile', `确定删除凭据文件 ${name} 吗？`), async () => {
        await this.api.deleteAuthFile({ name });
        await this.loadAuthFiles();
        this.render();
      });
    }
    if (action === 'auth-files-copy') {
      const name = target.dataset.name;
      if (name) {
        await navigator.clipboard?.writeText(name);
        this.view.showNotice(this.t('copied', '已复制'));
      }
      return;
    }
    if (action === 'auth-files-quota-one' || action === 'quota-refresh-one') {
      const name = target.dataset.name;
      const provider = target.dataset.provider;
      const credentialKey = target.dataset.accountId || name;
      this.quotaMap[credentialKey] = { status: 'loading', windows: [] };
      this.render();
      try {
        const res = await this.api.queryQuota({ name, provider, accountId: target.dataset.accountId || '' });
        this.quotaMap[credentialKey] = res;
      } catch (err) {
        this.quotaMap[credentialKey] = { status: 'error', error: String(err) };
      }
      this.render();
      return;
    }
    if (action === 'quota-refresh-all') {
      const active = (this.authFiles || []).filter((f) => !f.disabled);
      for (const f of active) {
        this.quotaMap[f.accountId || f.name] = { status: 'loading', windows: [] };
      }
      this.render();
      await Promise.all(
        active.map(async (f) => {
          try {
            const res = await this.api.queryQuota({ name: f.name, provider: f.provider, accountId: f.accountId || '' });
            this.quotaMap[f.accountId || f.name] = res;
          } catch (err) {
            this.quotaMap[f.accountId || f.name] = { status: 'error', error: String(err) };
          }
        }),
      );
      this.render();
      return;
    }
    if (action === 'auth-files-import-file') {
      try {
        await this.api.importAuthFile(target);
        await this.loadAuthFiles();
        this.render();
        this.view.showNotice(this.t('authFileImported', '认证文件导入成功'));
      } catch (err) {
        this.showError(err);
      }
      return;
    }

    if (await handleUsageAction(this, action, target)) return;
  }

  async handleSubmit(event) {
    const form = event.target;
    if (form.dataset.role?.startsWith('usage-')) {
      event.preventDefault();
      await handleUsageSubmit(this, form);
      return;
    }
    if (form.matches('[data-new-provider]')) {
      event.preventDefault();
      const data = Object.fromEntries(new FormData(form));
      await this.mutate(() => this.api.createProvider({ ...data, setAsDefault: form.elements.setAsDefault.checked, enabled: form.elements.enabled.checked, allowLan: form.elements.allowLan.checked, expectedRevision: this.snapshot.revision }), { select: (snapshot) => snapshot.providers.at(-1)?.id });
      return;
    }
    if (form.matches('[data-model-form]')) {
      event.preventDefault();
      const data = Object.fromEntries(new FormData(form));
      const existing = this.snapshot.providers.find((provider) => provider.id === this.selectedID)?.models.find((model) => model.id === form.dataset.editingId);
      await this.mutate(() => this.api.upsertModel({ providerId: this.selectedID, model: { ...data, contextLength: Number(data.contextLength) || 0, enabled: existing?.enabled ?? true, manual: true }, expectedRevision: this.snapshot.revision }));
      return;
    }
  }

  async saveProvider(event) {
    const form = event.target;
    const data = event.detail;
    await this.mutate(() => this.api.updateProvider({ ...data, providerId: form.dataset.providerId, enabled: form.elements.enabled.checked, allowLan: form.elements.allowLan.checked, expectedRevision: this.snapshot.revision }));
  }

  handleEvent(message) {
    if (message?.type === 'model_changed' || (message?.type === 'resync' && message.reason !== 'unavailable')) {
      if (this.recoveringEvents) return;
      if (document.activeElement?.matches?.('input,textarea,[contenteditable="true"]')) {
        this.view.showNotice(this.t('sharedPreferencesReviewLatest'));
        return;
      }
      this.recoveringEvents = (async () => {
        const snapshot = await this.api.snapshot();
        this.snapshot = snapshot;
        if (this.view.activePage === 'usage') await this.loadUsageData();
        if (this.view.activePage === 'quota') await this.loadAuthFiles();
        this.render();
      })().catch((error) => this.showError(error)).finally(() => { this.recoveringEvents = null; });
      return;
    }
    if (message?.event === 'model_host_state_changed' && message.result?.snapshot) {
      this.snapshot = message.result.snapshot;
      this.render();
      return;
    }
    if (message?.event === 'model_usage_updated') {
      if (this.view.activePage === 'usage') {
        this.loadUsageData().then(() => this.render());
      }
      return;
    }
  }

  async testProvider(form) {
    this.view.showError('');
    try {
      const data = form ? Object.fromEntries(new FormData(form)) : {};
      const result = await this.api.testProvider({ ...data, providerId: form?.matches('[data-new-provider]') ? '' : this.selectedID, allowLan: Boolean(form?.elements.allowLan.checked) });
      this.view.showNotice(this.t('modelConnectionSucceeded', '连接成功，发现 $1 个模型').replace('$1', result.modelCount));
    } catch (error) { this.showError(error); }
  }

  editModel(modelId) {
    const model = this.snapshot.providers.find((provider) => provider.id === this.selectedID)?.models.find((item) => item.id === modelId);
    const form = this.view.dialog.querySelector('[data-model-form]');
    if (!model || !form) return;
    form.dataset.editingId = model.id;
    form.elements.id.value = model.id;
    form.elements.id.readOnly = true;
    form.elements.displayName.value = model.displayName || '';
    form.elements.contextLength.value = model.contextLength || '';
    form.querySelector('[data-role="model-submit"]').textContent = this.t('save', '保存');
    form.querySelector('[data-action="cancel-model-edit"]').hidden = false;
    form.elements.displayName.focus();
  }

  resetModelForm() {
    const form = this.view.dialog.querySelector('[data-model-form]');
    if (!form) return;
    form.reset();
    delete form.dataset.editingId;
    form.elements.id.readOnly = false;
    form.querySelector('[data-role="model-submit"]').textContent = this.t('add', '添加');
    form.querySelector('[data-action="cancel-model-edit"]').hidden = true;
    form.elements.id.focus();
  }

  confirmDelete(message, work, options) {
    const dialog = document.createElement('dialog');
    dialog.className = 'model-confirm-dialog';
    const form = document.createElement('form'); form.method = 'dialog';
    const text = document.createElement('p'); text.textContent = message;
    const actions = document.createElement('div'); actions.className = 'modal-actions';
    const cancel = document.createElement('button'); cancel.value = 'cancel'; cancel.textContent = this.t('cancel', '取消');
    const submit = document.createElement('button'); submit.value = 'default'; submit.className = 'danger'; submit.textContent = this.t('delete', '删除');
    actions.append(cancel, submit); form.append(text, actions); dialog.append(form); document.body.append(dialog);
    dialog.addEventListener('close', () => { if (dialog.returnValue === 'default') this.mutate(work, options); dialog.remove(); }, { once: true });
    dialog.showModal();
  }
}
