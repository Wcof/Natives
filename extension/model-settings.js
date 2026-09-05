import { createModelSettingsAPI } from './model-settings-api.js';
import { createModelSettingsView, renderNewProvider } from './model-settings-view.js';
import { handleAdvancedAction, handleAdvancedSubmit } from './model-advanced-controller.js';
import { localizedModelError } from './model-settings-errors.js';
import { handleUsageAction, handleUsageSubmit } from './model-usage-controller.js';
import { handleAgentAction, loadAgentClients } from './model-agent-controller.js';
import { openAccountModelsDialog } from './model-account-models-dialog.js';

let instance;

export async function openModelSettings(options) {
  if (!instance) instance = new ModelSettings(options);
  instance.setLocale(options);
  await instance.open();
}

class ModelSettings {
  constructor({ t, language, returnFocus }) {
    this.t = t;
    this.language = language;
    this.returnFocus = returnFocus;
    this.snapshot = null;
    this.selectedID = 'codex';
    this.pendingOAuth = null;
    this.oauthResults = {};

    // Agent clients (智能体配置) state
    this.agentStatuses = null;
    this.agentSelectedId = '';
    this.agentModels = null;
    this.agentModelsError = '';
    this.agentSelections = {};
    this.agentBusy = false;
    this.agentLoadError = '';
    this.agentActiveTab = 'core';
    this.agentSessions = [];

    this.usageFilter = { range: '4h', page: 1, limit: 20 };
    this.usageFilterOptions = null;
    this.overviewData = null;
    this.analyticsData = null;
    this.eventsData = null;
    this.pricingData = null;
    this.importerWizard = null;

    // OAuth Sub-views state
    this.authFiles = [];
    this.quotaMap = {};
    this.authFileFilter = { query: '', provider: 'all', status: 'all' };
    this.api = createModelSettingsAPI({
      onEvent: (event) => this.handleEvent(event),
      onDisconnect: (_error, intentional) => { if (!intentional && this.view?.dialog.open) this.showError(this.t('modelHostDisconnected', '模型 Host 已断开')); },
    });
    this.view = createModelSettingsView({ t: (...args) => this.t(...args), onAction: (...args) => this.handleAction(...args) });
    this.view.dialog.addEventListener('close', () => { this.api.disconnect(); this.returnFocus?.focus?.(); });
    this.view.dialog.addEventListener('submit', (event) => this.handleSubmit(event));
    this.view.dialog.addEventListener('model-provider-submit', (event) => this.saveProvider(event));
  }
  setLocale({ t, language, returnFocus }) { this.t = t; this.language = language; this.returnFocus = returnFocus; }
  async open() {
    this.view.open();
    await this.refresh();
  }
  async refresh() {
    this.view.setLoading(true);
    this.view.showError('');
    try {
      this.snapshot = await this.api.snapshot();
      if (!this.snapshot.providers.some((provider) => provider.id === this.selectedID)) this.selectedID = this.snapshot.providers[0]?.id;
      if (this.view.activePage === 'usage') {
        await this.loadUsageData();
      } else if (this.view.activePage === 'oauth') {
        await this.loadAuthFiles();
      }
      this.render();
    } catch (error) {
      this.showError(error);
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
    try {
      if (!this.usageFilterOptions) this.usageFilterOptions = await this.api.getUsageAnalysis({ range: 'all' });
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
      };
      if (tab === 'overview') {
        this.overviewData = await this.api.getUsageOverview(query);
      } else if (tab === 'analytics') {
        this.analyticsData = await this.api.getUsageAnalysis(query);
      } else if (tab === 'events') {
        this.eventsData = await this.api.getUsageEvents({
          ...query,
          offset: (this.usageFilter.page - 1) * this.usageFilter.limit,
          limit: this.usageFilter.limit,
        });
      } else if (tab === 'pricing') {
        this.pricingData = await this.api.getUsagePricing();
      }
    } catch (err) {
      this.showError(err);
    }
  }

  render() {
    if (this.snapshot) {
      this.view.render(
        this.snapshot,
        this.selectedID,
        this.pendingOAuth,
        {
          overviewData: this.overviewData,
          analyticsData: this.analyticsData,
          eventsData: this.eventsData,
          pricingData: this.pricingData,
          currentFilter: this.usageFilter,
          filterOptions: this.usageFilterOptions,
        },
        {
          results: this.oauthResults,
          authFiles: this.authFiles,
          quotaMap: this.quotaMap,
          authFileFilter: this.authFileFilter,
          agentStatuses: this.agentStatuses,
          agentSelectedId: this.agentSelectedId,
          agentModels: this.agentModels,
          agentModelsError: this.agentModelsError,
          agentSelections: this.agentSelections,
          agentBusy: this.agentBusy,
          agentActiveTab: this.agentActiveTab,
          agentSessions: this.agentSessions,
        },
      );
    }
  }

  showError(error) {
    if (!error) { this.view.showError(''); return; }
    this.view.showError(localizedModelError(error, this.t));
  }

  async mutate(work, { select } = {}) {
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
    }
  }

  async handleAction(action, target, event) {
    const revision = this.snapshot?.revision;
    if (action === 'close') return this.view.close();
    if (action === 'refresh') return this.refresh();
    if (action === 'select-model-page') {
      const page = target.dataset.page;
      const kind = page === 'custom' ? 'custom' : page === 'oauth' ? 'oauth' : '';
      if (kind && !this.snapshot.providers.some((provider) => provider.id === this.selectedID && provider.kind === kind)) {
        this.selectedID = this.snapshot.providers.find((provider) => provider.kind === kind)?.id;
      }
      this.view.setPage(page);
      if (page === 'usage') {
        this.view.setLoading(true);
        await this.loadUsageData();
        this.view.setLoading(false);
      }
      if (page === 'agent' && !this.agentStatuses) {
        this.view.setLoading(true);
        await loadAgentClients(this, { force: true });
        this.view.setLoading(false);
      }
      this.render();
      return;
    }
    if (action === 'select-provider') { this.selectedID = target.dataset.providerId; this.render(); return; }
    if (action === 'new-provider') { renderNewProvider(this.view.dialog.querySelector('[data-role="detail"]'), this.t); return; }
    if (action === 'cancel-new-provider') return this.render();
    if (action === 'start-gateway') return this.mutate(() => this.api.startGateway({ expectedRevision: revision }));
    if (action === 'stop-gateway') return this.mutate(() => this.api.stopGateway({ expectedRevision: revision }));
    if (action === 'restart-gateway') return this.mutate(() => this.api.restartGateway({ expectedRevision: revision }));
    if (action === 'refresh-gateway') {
      this.view.setLoading(true);
      try {
        this.snapshot = await this.api.loadSnapshot();
        this.view.showNotice(this.t('modelStatusRefreshed', '状态已刷新'));
      } catch (error) {
        this.showError(error);
      } finally {
        this.view.setLoading(false);
        this.render();
      }
      return;
    }
    if (action === 'check-kernel-update') {
      try {
        const res = await this.api.checkKernelUpdate();
        if (this.snapshot?.gateway) {
          this.snapshot.gateway.latestKernelVersion = res?.latestVersion;
        }
        this.render();
        this.view.showToast(this.t('modelKernelCheckDone', '已检查内核最新版本'));
      } catch (err) {
        this.showError(err);
      }
      return;
    }
    if (action === 'update-kernel') {
      this.view.showToast(this.t('modelKernelUpdating', '正在更新内核…'));
      try {
        const res = await this.api.updateKernel();
        this.snapshot = await this.api.loadSnapshot();
        this.render();
        this.view.showNotice(res?.message || this.t('modelKernelUpdated', '内核已成功更新'));
      } catch (err) {
        this.showError(err);
      }
      return;
    }
    if (action === 'resident') return this.mutate(() => this.api.setResident({ resident: target.checked, expectedRevision: revision }));
    if (action === 'copy-endpoint') {
      if (await this.copyText(target.dataset.endpoint, this.t('modelEndpointUnavailable', '代理地址当前不可用'))) this.view.showToast(this.t('copied', '已复制'));
      return;
    }
    if (action === 'reveal-key' || action === 'copy-first-key') return this.copyAccessKey();
    if (action === 'rotate-key') return this.rotateAccessKey();
    if (action === 'oauth-start') return this.pendingOAuth?.provider === target.dataset.provider ? this.cancelOAuth() : this.startOAuth(target.dataset.provider);
    if (action === 'toggle-account') {
      await this.mutate(() => this.api.setAccountEnabled({ accountId: target.dataset.accountId, enabled: target.dataset.enabled === 'true', expectedRevision: revision }));
      await this.loadAuthFiles();
      this.render();
      return;
    }
    if (action === 'reauth-account') return this.startOAuth(target.dataset.provider, target.dataset.accountId);
    if (action === 'delete-account') return this.confirmDelete(this.t('modelDeleteAccountConfirm', '确定删除这个 OAuth 账户吗？'), async () => {
      const result = await this.api.deleteAccount({ accountId: target.dataset.accountId, expectedRevision: revision });
      await this.loadAuthFiles();
      return result;
    });
    if (action === 'delete-provider') return this.confirmDelete(this.t('modelDeleteProviderConfirm', '确定删除这个供应商及其模型吗？'), () => this.api.deleteProvider({ providerId: this.selectedID, expectedRevision: revision }), { select: (snapshot) => snapshot.providers[0]?.id });
    if (action === 'test-provider' || action === 'test-new-provider') return this.testProvider(target.closest('form'));
    if (action === 'refresh-models') return this.mutate(() => this.api.refreshModels({ providerId: this.selectedID, expectedRevision: revision }));
    if (action === 'toggle-model') return this.mutate(() => this.api.setModelEnabled({ providerId: this.selectedID, modelId: target.dataset.modelId, enabled: target.dataset.enabled === 'true', expectedRevision: revision }));
    if (action === 'edit-model') return this.editModel(target.dataset.modelId);
    if (action === 'cancel-model-edit') return this.resetModelForm();
    if (action === 'delete-model') return this.mutate(() => this.api.deleteModel({ providerId: this.selectedID, modelId: target.dataset.modelId, expectedRevision: revision }));

    // OAuth Sub-tabs & Actions
    if (action === 'select-oauth-tab') {
      this.view.setOAuthTab(target.dataset.tab);
      if (target.dataset.tab === 'authFiles' || target.dataset.tab === 'quota') {
        await this.loadAuthFiles();
        this.render();
      }
      return;
    }
    if (action === 'auth-files-refresh' || action === 'quota-read-list') {
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
    if (action === 'auth-files-priority') {
      const name = target.dataset.name;
      const current = Number(target.dataset.priority) || 0;
      this.promptInput(this.t('authFilePriorityPrompt', '请输入优先级（数字，越大越优先）：'), String(current), (value) => {
        const priority = Number(value);
        if (!Number.isInteger(priority)) {
          this.showError(this.t('authFilePriorityInvalid', '优先级必须是整数'));
          return;
        }
        this.api.updateAuthFile({ name, priority })
          .then(() => this.loadAuthFiles())
          .then(() => { this.render(); this.view.showToast(this.t('authFilePrioritySaved', '优先级已更新')); })
          .catch((error) => this.showError(error));
      });
      return;
    }
    if (action === 'auth-files-models') {
      return openAccountModelsDialog(this, {
        accountId: target.dataset.accountId || '',
        provider: target.dataset.provider || '',
        name: target.dataset.name || '',
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
    if (action === 'auth-files-filter') {
      this.authFileFilter = { ...this.authFileFilter, ...target };
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
	if (await handleAgentAction(this, action, target)) return;
	await handleAdvancedAction(this, action, target, revision);
  }

  async handleSubmit(event) {
    const form = event.target;
    if (form.dataset.role?.startsWith('usage-')) {
      event.preventDefault();
      await handleUsageSubmit(this, form);
      return;
    }
    if (['basic-settings-form', 'network-settings-form', 'kernel-settings-form'].includes(form.dataset.role)) {
      event.preventDefault();
      await handleAdvancedSubmit(this, form);
      return;
    }
    if (form.matches('[data-new-provider]')) {
      event.preventDefault();
      const data = Object.fromEntries(new FormData(form));
      const result = await this.mutate(() => this.api.createProvider({ ...data, enabled: form.elements.enabled.checked, allowLan: form.elements.allowLan.checked, expectedRevision: this.snapshot.revision }), { select: (snapshot) => snapshot.providers.at(-1)?.id });
      if (result) this.render();
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

  async startOAuth(provider, accountId = '') {
    this.view.showError('');
    try {
      const result = accountId
        ? await this.api.reauthAccount({ provider, accountId, expectedRevision: this.snapshot.revision })
        : await this.api.startOAuth({ provider, expectedRevision: this.snapshot.revision });
      if (!result?.sessionId) return;
      this.oauthResults[provider] = 'pending';
      this.pendingOAuth = result;
      this.render();
    } catch (error) {
      this.showError(error);
    }
  }

  async cancelOAuth() {
    if (!this.pendingOAuth) return;
    await this.api.cancelOAuth({ sessionId: this.pendingOAuth.sessionId, expectedRevision: this.snapshot.revision }).catch((error) => this.showError(error));
  }

  handleEvent(message) {
    if (message?.event === 'model_gateway_state_changed' && message.result?.snapshot) {
      this.snapshot = message.result.snapshot;
      this.render();
      return;
    }
    if (message?.event === 'model_account_state_changed' && message.result?.snapshot) {
      this.snapshot = message.result.snapshot;
      this.render();
      return;
    }
    if (message?.event === 'model_catalog_changed' && message.result?.snapshot) {
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
    if (message?.event !== 'model_oauth_state_changed') return;
    const result = message.result;
    if (!result || result.sessionId !== this.pendingOAuth?.sessionId) return;
    if (result.snapshot) this.snapshot = result.snapshot;
    this.oauthResults[result.provider || this.pendingOAuth.provider] = result.state;
    if (result.state === 'succeeded') {
      this.view.showNotice(this.t('modelLoginSucceeded', '已登录'));
      this.loadAuthFiles().then(() => this.render());
    }
    if (result.state !== 'pending') this.pendingOAuth = null;
    if (result.state === 'failed') this.showError(this.t('modelAuthorizationFailed', 'OAuth 授权失败，请重试。'));
    if (result.state === 'timeout') this.showError(this.t('modelAuthorizationTimeout', 'OAuth 授权已超时，请重新开始。'));
    this.render();
  }

  async copyAccessKey() {
    try {
      const result = await this.api.revealAccessKey();
      if (await this.copyText(result.accessKey, this.t('modelAccessKeyUnavailable', '尚未生成访问密钥'))) this.view.showToast(this.t('copied', '已复制'));
    } catch (error) { this.showError(error); }
  }

  async rotateAccessKey() {
    const result = await this.mutate(() => this.api.rotateAccessKey({ expectedRevision: this.snapshot.revision }));
    if (result?.accessKey && await this.copyText(result.accessKey)) this.view.showToast(this.t('modelKeyRotatedAndCopied', '密钥已重置并复制到剪贴板'));
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
    form.elements.alias.value = model.alias || '';
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

  async copyText(value, unavailable = '') {
    if (!value) { this.showError(unavailable); return false; }
    try { await navigator.clipboard.writeText(value); return true; }
    catch { this.showError(this.t('copyFailed', '复制失败')); return false; }
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

  promptInput(message, initialValue, onConfirm, datalistHtml = '') {
    const dialog = document.createElement('dialog');
    dialog.className = 'model-confirm-dialog';
    const form = document.createElement('form'); form.method = 'dialog';
    const text = document.createElement('p'); text.textContent = message;
    const input = document.createElement('input'); input.type = 'text'; input.value = initialValue || '';
    input.style.cssText = 'width:100%;min-height:36px;padding:6px 12px;margin:10px 0 16px;border:1px solid var(--border);border-radius:6px;background:var(--surface-2);box-sizing:border-box;color:var(--text);font-size:13px;';
    if (datalistHtml) {
      input.setAttribute('list', 'prompt-input-datalist');
      input.insertAdjacentHTML('afterend', `<datalist id="prompt-input-datalist">${datalistHtml}</datalist>`);
    }
    const actions = document.createElement('div'); actions.className = 'modal-actions';
    const cancel = document.createElement('button'); cancel.value = 'cancel'; cancel.textContent = this.t('cancel', '取消');
    const submit = document.createElement('button'); submit.value = 'default'; submit.className = 'primary'; submit.textContent = this.t('save', '保存');
    actions.append(cancel, submit); form.append(text, input, actions); dialog.append(form); document.body.append(dialog);
    dialog.addEventListener('close', () => {
      if (dialog.returnValue === 'default' && input.value.trim()) onConfirm(input.value.trim());
      dialog.remove();
    }, { once: true });
    dialog.showModal();
    input.focus(); input.select();
  }
}
