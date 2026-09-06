export function renderGateway(container, snapshot, t) {
  const gateway = snapshot?.gateway || { state: 'stopped' };
  const running = gateway.state === 'running';
  const busy = ['starting', 'stopping', 'restarting'].includes(gateway.state);
  const keys = gateway.accessKeys || [];

  container.replaceChildren();
  const layout = element('div', 'model-gateway-layout');
  layout.append(
    renderRuntime(gateway, running, busy, t),
    renderEndpoints(running ? gateway.baseUrl : '', t),
    renderKeys(keys, t),
    renderSummary(snapshot, t),
  );
  container.append(layout);
}

function renderRuntime(gateway, running, busy, t) {
  const section = element('section', 'model-section model-gateway-runtime');
  const heading = element('header', 'model-section-heading');
  heading.innerHTML = `<div><h4>${escapeText(t('modelRunningControl', '运行控制'))}</h4><p>${escapeText(t('modelAdvBasicDesc', '管理本地内核状态、监听端口和兼容 API 地址。'))}</p></div>`;
  const state = element('span', `model-status model-status-${gateway.state}`, stateLabel(gateway.state, t));
  state.setAttribute('role', 'status');
  state.setAttribute('aria-live', 'polite');
  heading.append(state);

  const latestVer = gateway.latestKernelVersion || gateway.kernelVersion || '—';
  const hasUpdate = Boolean(gateway.latestKernelVersion && gateway.kernelVersion && isNewerVersion(gateway.latestKernelVersion, gateway.kernelVersion));

  const latestRow = element('div', 'model-settings-row');
  const latestLabel = element('span', '', t('modelLatestKernelVersion', '最新版本'));
  const latestVal = element('div', 'model-kernel-latest-val');
  latestVal.innerHTML = `
    <span class="font-mono">${escapeText(latestVer)}</span>
    ${!hasUpdate && latestVer !== '—'
      ? `<span class="badge-up-to-date" style="font-size:11px;padding:1px 6px;border-radius:999px;background:rgba(34,197,94,0.12);color:#22c55e;margin:0 4px;">${escapeText(t('modelKernelUpToDate', '已是最新'))}</span>`
      : ''}
    ${hasUpdate
      ? `<button type="button" class="btn-af-tool btn-kernel-update primary" data-action="update-kernel">${escapeText(t('update', '更新'))}</button>`
      : `<button type="button" class="btn-af-tool btn-kernel-check" data-action="check-kernel-update" title="${escapeText(t('checkUpdate', '检查更新'))}"><svg class="icon" aria-hidden="true" style="width:12px;height:12px;"><use href="#i-refresh" /></svg><span>${escapeText(t('checkUpdate', '检查更新'))}</span></button>`}
  `;
  latestRow.append(latestLabel, latestVal);

  const details = element('div', 'model-settings-rows');
  details.append(
    detailRow(t('modelRunStatus', '运行状态'), stateLabel(gateway.state, t)),
    detailRow(t('modelProcessPID', '进程 PID'), running && gateway.pid ? gateway.pid : '—', true),
    detailRow(t('modelKernelVersion', '内核版本'), gateway.kernelVersion || '—', true),
    latestRow,
    detailRow(t('modelSoftwareVersion', '软件版本'), gateway.version || '—', true),
  );

  const footer = element('div', 'model-section-footer');
  const resident = document.createElement('label');
  resident.className = 'model-switch';
  resident.innerHTML = `<input type="checkbox" data-action="resident" ${gateway.resident ? 'checked' : ''}><span>${escapeText(t('modelResident', '常驻'))}</span>`;
  const actions = element('div', 'model-form-actions');
  actions.append(
    actionButton('refresh-gateway', t('modelRefreshStatus', '刷新状态'), '', busy),
    actionButton('restart-gateway', t('modelRestartGateway', '重启'), '', busy || !running),
    actionButton(running ? 'stop-gateway' : 'start-gateway', running ? t('modelStopGateway', '停止') : gateway.state === 'failed' ? t('retry', '重试') : t('modelStartGateway', '启动'), running ? 'danger' : 'primary', busy),
  );
  footer.append(resident, actions);
  section.append(heading, details, footer);
  return section;
}

function isNewerVersion(candidate, current) {
  const parse = (value) => String(value).trim().replace(/^v/, '').split('.').map((part) => Number.parseInt(part, 10) || 0);
  const next = parse(candidate);
  const old = parse(current);
  for (let index = 0; index < Math.max(next.length, old.length); index += 1) {
    if ((next[index] || 0) !== (old[index] || 0)) return (next[index] || 0) > (old[index] || 0);
  }
  return false;
}

function renderEndpoints(baseUrl, t) {
  const section = element('section', 'model-section model-gateway-endpoints');
  const heading = element('header', 'model-section-heading');
  heading.innerHTML = `<div><h4>API URL</h4><p>${escapeText(t('modelLocalProxyDescription', '管理本地兼容接口、访问密钥与常驻状态。'))}</p></div>`;
  section.append(heading);

  if (!baseUrl) {
    section.append(emptyState(t('modelGatewayNotRunning', '代理尚未运行')));
    return section;
  }

  const list = element('div', 'model-endpoint-list');
  for (const [label, hint, suffix] of [
    ['OpenAI', t('modelOpenAICompat', 'OpenAI 兼容格式'), '/v1'],
    ['Claude', t('modelClaudeCompat', 'Anthropic 兼容格式'), ''],
    ['Gemini', t('modelGeminiCompat', 'Gemini 兼容格式'), ''],
  ]) {
    const endpoint = `${baseUrl}${suffix}`;
    const row = element('div', 'model-endpoint-row');
    row.innerHTML = `<span class="model-provider-icon" aria-hidden="true"><svg class="icon"><use href="#i-box" /></svg></span><span class="model-endpoint-name"><strong>${label}</strong><small>${escapeText(hint)}</small></span><code title="${escapeText(endpoint)}">${escapeText(endpoint)}</code>`;
    const copy = iconButton('copy-endpoint', 'i-link', t('copy', '复制'));
    copy.dataset.endpoint = endpoint;
    row.append(copy);
    list.append(row);
  }
  section.append(list);
  return section;
}

function renderKeys(keys, t) {
  const section = element('section', 'model-section model-gateway-keys');
  const heading = element('header', 'model-section-heading');
  heading.innerHTML = `<div><h4>${escapeText(t('modelAuthKeysTitle', '鉴权密钥管理'))}</h4><p>${escapeText(t('modelAuthKeysDesc', '管理访问本地代理所用的 API Key 密钥。'))}</p></div>`;
  heading.append(actionButton('create-gateway-key', `＋ ${t('modelCreateKey', '新建密钥')}`, 'primary'));
  section.append(heading);

  if (!keys.length) {
    section.append(emptyState(t('modelNoKeys', '暂无访问密钥'), t('modelNoKeysHint', '请创建至少一个访问密钥。')));
    return section;
  }

  const wrapper = element('div', 'model-table-wrapper');
  wrapper.innerHTML = `<table class="model-keys-table"><thead><tr><th class="model-col-name">${escapeText(t('name', '名称'))}</th><th class="model-col-mask">${escapeText(t('modelKeyMask', '密钥摘要'))}</th><th class="model-col-status">${escapeText(t('modelStatus', '状态'))}</th><th class="model-col-actions">${escapeText(t('modelActions', '操作'))}</th></tr></thead><tbody>${keys.map((key) => `
    <tr><td class="model-col-name"><span class="model-key-name-cell"><strong title="${escapeText(key.name)}">${escapeText(key.name)}</strong><button type="button" class="model-icon-button" data-action="rename-key-id" data-key-id="${escapeText(key.id)}" data-current-name="${escapeText(key.name)}" aria-label="${escapeText(t('modelRename', '重命名'))}" title="${escapeText(t('modelRename', '重命名'))}"><svg class="icon" aria-hidden="true"><use href="#i-pen" /></svg></button></span></td><td class="model-col-mask"><code title="${escapeText(key.mask)}">${escapeText(key.mask)}</code></td><td class="model-col-status"><button type="button" class="model-status-badge ${key.enabled ? 'status-ok' : 'status-err'}" data-action="toggle-key-id" data-key-id="${escapeText(key.id)}" data-enabled="${!key.enabled}">${escapeText(key.enabled ? t('enable', '启用') : t('disable', '禁用'))}</button></td><td class="model-col-actions"><div class="model-row-actions"><button type="button" class="btn-sm" data-action="reveal-key-id" data-key-id="${escapeText(key.id)}">${escapeText(t('copy', '复制'))}</button><button type="button" class="btn-sm" data-action="rotate-key-id" data-key-id="${escapeText(key.id)}">${escapeText(t('modelRotate', '重置'))}</button>${keys.length > 1 ? `<button type="button" class="danger btn-sm" data-action="delete-key-id" data-key-id="${escapeText(key.id)}">${escapeText(t('delete', '删除'))}</button>` : ''}</div></td></tr>`).join('')}</tbody></table>`;
  section.append(wrapper);
  return section;
}

function renderSummary(snapshot, t) {
  const accounts = (snapshot?.accounts || []).filter((account) => account.enabled && account.status === 'active');
  const accountIDs = new Set(accounts.map((account) => account.id));
  const providers = (snapshot?.providers || []).filter((provider) => provider.enabled && (provider.kind === 'custom' ? provider.models.some((model) => model.enabled) : accounts.some((account) => account.provider === provider.oauthProvider)));
  const models = (snapshot?.providers || []).reduce((count, provider) => count + (!provider.enabled ? 0 : provider.models.filter((model) => model.enabled && (provider.kind === 'custom' || model.accountIds?.some((id) => accountIDs.has(id)))).length), 0);
  return element('p', 'model-gateway-summary', t('modelGatewaySummary', '$1 个供应商 · $2 个账户 · $3 个模型').replace('$1', providers.length).replace('$2', accounts.length).replace('$3', models));
}

function detailRow(label, value, mono = false) {
  const row = element('div', 'model-settings-row');
  row.append(element('span', '', label), element('span', mono ? 'font-mono' : '', String(value)));
  return row;
}

function actionButton(action, label, className = '', disabled = false) {
  const node = element('button', className, label);
  node.type = 'button';
  node.dataset.action = action;
  node.disabled = disabled;
  return node;
}

function iconButton(action, icon, label) {
  const node = actionButton(action, '', 'model-icon-button');
  node.title = label;
  node.setAttribute('aria-label', label);
  node.innerHTML = `<svg class="icon" aria-hidden="true"><use href="#${icon}" /></svg>`;
  return node;
}

function emptyState(title, hint = '') {
  const node = element('div', 'model-empty-state');
  node.append(element('strong', '', title));
  if (hint) node.append(element('p', 'muted', hint));
  return node;
}

function element(tag, className = '', text = '') {
  const node = document.createElement(tag);
  node.className = className;
  node.textContent = text;
  return node;
}

function escapeText(value) {
  return String(value ?? '').replace(/[&<>"']/g, (match) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[match]);
}

function stateLabel(state, t) {
  return ({
    running: t('modelRunning', '运行中'),
    starting: t('modelStarting', '启动中'),
    stopping: t('modelStopping', '停止中'),
    failed: t('modelFailed', '异常'),
    restarting: t('modelRestarting', '重启中'),
  })[state] || t('modelStopped', '已停止');
}
