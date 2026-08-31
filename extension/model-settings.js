import { createModelSettingsAPI } from './model-settings-api.js';
import { createModelSettingsView, renderNewProvider } from './model-settings-view.js';

let instance;

const ERROR_MESSAGES = {
  invalid_request: ['modelErrorInvalidRequest', '输入内容无效，请检查后重试。'], not_found: ['modelErrorNotFound', '对应配置已不存在，请刷新。'],
  revision_conflict: ['modelErrorRevisionConflict', '配置已在其他页面更新，请刷新后重试。'], keychain_unavailable: ['modelErrorKeychainUnavailable', '系统钥匙串当前不可用。'],
  secret_not_configured: ['modelErrorSecretMissing', '请先配置凭证。'], upstream_unreachable: ['modelErrorUpstreamUnreachable', '无法连接供应商，请检查地址和网络。'],
  upstream_unauthorized: ['modelErrorUpstreamUnauthorized', '供应商拒绝了凭证，请检查 API Key。'], upstream_error: ['modelErrorUpstream', '供应商请求失败，请稍后重试。'],
  upstream_invalid_response: ['modelErrorUpstreamInvalid', '供应商返回了无效响应。'], upstream_response_too_large: ['modelErrorUpstreamTooLarge', '供应商响应超过大小限制。'],
  gateway_start_failed: ['modelErrorGatewayStart', '本地模型代理启动失败。'], gateway_stop_failed: ['modelErrorGatewayStop', '本地模型代理停止失败。'],
  gateway_restart_failed: ['modelErrorGatewayRestart', '配置已保存，但代理重启失败，请重试。'], oauth_in_progress: ['modelErrorOAuthInProgress', '已有 OAuth 授权正在进行。'],
  internal_error: ['modelErrorInternal', '模型设置操作失败。'],
  host_disconnected: ['modelHostDisconnected', '模型 Host 已断开'], request_timeout: ['modelErrorTimeout', '请求超时，请重试。'],
};

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
      this.render();
    } catch (error) {
      this.showError(error);
    } finally {
      this.view.setLoading(false);
    }
  }

  render() { if (this.snapshot) this.view.render(this.snapshot, this.selectedID, this.pendingOAuth); }
  showError(error) {
    if (!error) { this.view.showError(''); return; }
    const localized = typeof error === 'object' && ERROR_MESSAGES[error.code];
    this.view.showError(localized ? this.t(...localized) : String(error.message || error));
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

  handleAction(action, target) {
    const revision = this.snapshot?.revision;
    if (action === 'close') return this.view.close();
    if (action === 'refresh') return this.refresh();
    if (action === 'select-provider') { this.selectedID = target.dataset.providerId; this.render(); return; }
    if (action === 'new-provider') { renderNewProvider(this.view.dialog.querySelector('[data-role="detail"]'), this.t); return; }
    if (action === 'cancel-new-provider') return this.render();
    if (action === 'start-gateway') return this.mutate(() => this.api.startGateway({ expectedRevision: revision }));
    if (action === 'stop-gateway') return this.mutate(() => this.api.stopGateway({ expectedRevision: revision }));
    if (action === 'resident') return this.mutate(() => this.api.setResident({ resident: target.checked, expectedRevision: revision }));
    if (action === 'copy-endpoint') return this.copyText(target.dataset.endpoint, this.t('modelEndpointUnavailable', '代理地址当前不可用'));
    if (action === 'reveal-key') return this.copyAccessKey();
    if (action === 'rotate-key') return this.rotateAccessKey();
    if (action === 'oauth-start') return this.pendingOAuth?.provider === target.dataset.provider ? this.cancelOAuth() : this.startOAuth(target.dataset.provider);
    if (action === 'toggle-account') return this.mutate(() => this.api.setAccountEnabled({ accountId: target.dataset.accountId, enabled: target.dataset.enabled === 'true', expectedRevision: revision }));
    if (action === 'reauth-account') return this.startOAuth(target.dataset.provider, target.dataset.accountId);
    if (action === 'delete-account') return this.confirmDelete(this.t('modelDeleteAccountConfirm', '确定删除这个 OAuth 账户吗？'), () => this.api.deleteAccount({ accountId: target.dataset.accountId, expectedRevision: revision }));
    if (action === 'delete-provider') return this.confirmDelete(this.t('modelDeleteProviderConfirm', '确定删除这个供应商及其模型吗？'), () => this.api.deleteProvider({ providerId: this.selectedID, expectedRevision: revision }), { select: (snapshot) => snapshot.providers[0]?.id });
    if (action === 'test-provider' || action === 'test-new-provider') return this.testProvider(target.closest('form'));
    if (action === 'refresh-models') return this.mutate(() => this.api.refreshModels({ providerId: this.selectedID, expectedRevision: revision }));
    if (action === 'toggle-model') return this.mutate(() => this.api.setModelEnabled({ providerId: this.selectedID, modelId: target.dataset.modelId, enabled: target.dataset.enabled === 'true', expectedRevision: revision }));
    if (action === 'edit-model') return this.editModel(target.dataset.modelId);
    if (action === 'cancel-model-edit') return this.resetModelForm();
    if (action === 'delete-model') return this.mutate(() => this.api.deleteModel({ providerId: this.selectedID, modelId: target.dataset.modelId, expectedRevision: revision }));
  }

  async handleSubmit(event) {
    const form = event.target;
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
    if (message?.event !== 'model_oauth_state_changed') return;
    const result = message.result;
    if (!result || result.sessionId !== this.pendingOAuth?.sessionId) return;
    if (result.snapshot) this.snapshot = result.snapshot;
    if (result.state !== 'pending') this.pendingOAuth = null;
    if (result.state === 'failed') this.showError(this.t('modelAuthorizationFailed', 'OAuth 授权失败，请重试。'));
    if (result.state === 'timeout') this.showError(this.t('modelAuthorizationTimeout', 'OAuth 授权已超时，请重新开始。'));
    this.render();
  }

  async copyAccessKey() {
    try {
      const result = await this.api.revealAccessKey();
      await this.copyText(result.accessKey, this.t('modelAccessKeyUnavailable', '尚未生成访问密钥'));
    } catch (error) { this.showError(error); }
  }

  async rotateAccessKey() {
    const result = await this.mutate(() => this.api.rotateAccessKey({ expectedRevision: this.snapshot.revision }));
    if (result?.accessKey) await this.copyText(result.accessKey);
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
    if (!value) { this.showError(unavailable); return; }
    try { await navigator.clipboard.writeText(value); }
    catch { this.showError(this.t('copyFailed', '复制失败')); }
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
