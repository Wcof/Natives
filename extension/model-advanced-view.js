const TABS = [
  ['basic', 'modelAdvBasic', '基础控制'],
  ['keys', 'modelAdvKeys', '密钥设置'],
  ['network', 'modelAdvNetworkRouting', '网络与路由'],
  ['kernel', 'modelAdvKernel', '内核设置'],
];

export function renderAdvancedView(container, { activeSubTab = 'basic', snapshot, t }) {
  container.replaceChildren();
  const subnav = document.createElement('div');
  subnav.className = 'model-subnav-bar';
  subnav.innerHTML = `<div class="model-subnav-tabs">${TABS.map(([id, key, fallback]) => `
    <button type="button" class="model-subnav-tab ${activeSubTab === id ? 'active' : ''}"
      data-action="select-advanced-tab" data-tab="${id}">${escapeText(t(key, fallback))}</button>`).join('')}</div>`;
  container.append(subnav);
  const content = document.createElement('div');
  content.className = 'model-advanced-content';
  const render = { basic: renderBasic, keys: renderKeys, network: renderNetwork, kernel: renderKernel }[activeSubTab] || renderBasic;
  render(content, snapshot, t);
  container.append(content);
}

function renderBasic(container, snapshot, t) {
  const gateway = snapshot?.gateway || {};
  const settings = gateway.settings || {};
  const running = gateway.state === 'running';
  const busy = ['starting', 'stopping', 'restarting'].includes(gateway.state);
  container.innerHTML = `
    <div class="model-adv-heading"><div><h4>${escapeText(t('modelAdvBasicTitle', '代理运行控制'))}</h4>
      <p class="muted">${escapeText(t('modelAdvBasicDesc', '管理本地内核状态、监听端口和兼容 API 地址。'))}</p></div>
      <span class="model-status-badge ${running ? 'status-ok' : 'status-err'}">${escapeText(gateway.state || 'stopped')}</span></div>
    <form class="model-adv-form" data-role="basic-settings-form">
      <label><span>${escapeText(t('modelPreferredPort', '首选监听端口'))}</span>
        <input type="number" name="preferredPort" min="0" max="65535" value="${settings.preferredPort || ''}" placeholder="0"></label>
      <label><span>${escapeText(t('modelGatewayBaseURL', '当前 API URL'))}</span>
        <input type="text" readonly value="${escapeText(gateway.baseUrl || t('modelGatewayNotRunning', '代理尚未运行'))}"></label>
      <div class="model-form-actions"><button type="submit">${escapeText(t('save', '保存设置'))}</button>
        <button type="button" class="primary" data-action="${running ? 'stop-gateway' : 'start-gateway'}" ${busy ? 'disabled' : ''}>${escapeText(running ? t('modelStopGateway', '停止') : t('modelStartGateway', '启动'))}</button>
        <button type="button" data-action="restart-gateway" ${!running || busy ? 'disabled' : ''}>${escapeText(t('modelRestartGateway', '重启'))}</button></div>
    </form>`;
}

function renderKeys(container, snapshot, t) {
  const keys = snapshot?.gateway?.accessKeys || [];
  const header = document.createElement('div');
  header.className = 'model-adv-heading';
  header.innerHTML = `<div><h4>${escapeText(t('modelAdvKeysTitle', '鉴权密钥'))}</h4><p class="muted">${escapeText(t('modelAdvKeysDesc', '密钥仅保存在系统钥匙串中，页面只显示掩码。'))}</p></div>
    <button type="button" class="primary" data-action="create-gateway-key">＋ ${escapeText(t('modelCreateKey', '新建密钥'))}</button>`;
  container.append(header);
  if (!keys.length) return container.append(emptyState(t('modelNoKeys', '暂无访问密钥'), t('modelNoKeysHint', '请创建至少一个访问密钥。')));
  const table = document.createElement('div');
  table.className = 'model-table-wrapper';
  table.innerHTML = `<table class="model-keys-table"><thead><tr><th class="model-col-name">${escapeText(t('name', '名称'))}</th><th class="model-col-mask">${escapeText(t('modelKeyMask', '密钥摘要'))}</th><th class="model-col-status">${escapeText(t('modelStatus', '状态'))}</th><th class="model-col-actions">${escapeText(t('modelActions', '操作'))}</th></tr></thead><tbody>
    ${keys.map((key) => `<tr><td class="model-col-name"><strong title="${escapeText(key.name)}">${escapeText(key.name)}</strong></td><td class="model-col-mask"><code title="${escapeText(key.mask)}">${escapeText(key.mask)}</code></td>
      <td class="model-col-status"><span class="model-status-badge ${key.enabled ? 'status-ok' : 'status-err'}">${escapeText(key.enabled ? t('enable', '启用') : t('disable', '禁用'))}</span></td>
      <td class="model-col-actions"><div class="model-row-actions"><button type="button" class="btn-sm" data-action="reveal-key-id" data-key-id="${escapeText(key.id)}">${escapeText(t('copy', '复制'))}</button>
      <button type="button" class="btn-sm" data-action="rotate-key-id" data-key-id="${escapeText(key.id)}">${escapeText(t('modelRotate', '重置'))}</button>
      <button type="button" class="btn-sm" data-action="toggle-key-id" data-key-id="${escapeText(key.id)}" data-enabled="${!key.enabled}">${escapeText(key.enabled ? t('disable', '禁用') : t('enable', '启用'))}</button>
      ${keys.length > 1 ? `<button type="button" class="danger btn-sm" data-action="delete-key-id" data-key-id="${escapeText(key.id)}">${escapeText(t('delete', '删除'))}</button>` : ''}</div></td></tr>`).join('')}
    </tbody></table>`;
  container.append(table);
}

function renderNetwork(container, snapshot, t) {
  const settings = snapshot?.gateway?.settings || {};
  container.innerHTML = `<form class="model-adv-form" data-role="network-settings-form">
    <div class="model-adv-heading"><div><h4>${escapeText(t('modelNetworkRoutingTitle', '网络与会话路由'))}</h4><p class="muted">${escapeText(t('modelNetworkRoutingDesc', '配置上游代理、凭证选择策略和会话粘性。'))}</p></div></div>
    <label><span>${escapeText(t('modelProxyURL', '上游代理 URL'))}</span><input type="url" name="proxyUrl" value="${escapeText(settings.proxyUrl || '')}" placeholder="http://127.0.0.1:7890"></label>
    <label><span>${escapeText(t('modelRoutingStrategy', '路由策略'))}</span><select name="routingStrategy"><option value="round_robin" ${settings.routingStrategy !== 'fill_first' ? 'selected' : ''}>${escapeText(t('modelStrategyRoundRobin', '轮询'))}</option><option value="fill_first" ${settings.routingStrategy === 'fill_first' ? 'selected' : ''}>${escapeText(t('modelStrategyFillFirst', '优先填充'))}</option></select></label>
    <label class="model-checkbox"><input type="checkbox" name="sessionAffinity" ${settings.sessionAffinity ? 'checked' : ''}><span>${escapeText(t('modelSessionAffinity', '启用会话粘性路由'))}</span></label>
    <label><span>${escapeText(t('modelSessionAffinityTTL', '会话粘性 TTL（秒）'))}</span><input type="number" name="sessionAffinityTtl" min="0" max="604800" value="${settings.sessionAffinityTtl || 3600}"></label>
    <div class="model-form-actions"><button type="submit" class="primary">${escapeText(t('save', '保存设置'))}</button></div></form>`;
}

function renderKernel(container, snapshot, t) {
  const settings = snapshot?.gateway?.settings || {};
  container.innerHTML = `<form class="model-adv-form" data-role="kernel-settings-form">
    <div class="model-adv-heading"><div><h4>${escapeText(t('modelAdvKernelTitle', '内核重试设置'))}</h4><p class="muted">${escapeText(t('modelAdvKernelDesc', '设置失败重试范围；保存后运行中的代理会安全重启。'))}</p></div></div>
    ${numberField('requestRetry', t('modelRequestRetry', '请求重试轮数'), settings.requestRetry, 0, 100)}
    ${numberField('maxRetryCredentials', t('modelMaxRetryCredentials', '每轮最多尝试凭证数'), settings.maxRetryCredentials, 0, 100)}
    ${numberField('maxRetryIntervalSeconds', t('modelMaxRetryInterval', '最大重试等待（秒）'), settings.maxRetryIntervalSeconds, 0, 3600)}
    ${numberField('streamingBootstrapRetries', t('modelStreamingRetries', '流式启动重试次数'), settings.streamingBootstrapRetries, 0, 100)}
    <div class="model-form-actions"><button type="submit" class="primary">${escapeText(t('save', '保存设置'))}</button></div></form>`;
}

function numberField(name, label, value, min, max) {
  return `<label><span>${escapeText(label)}</span><input type="number" name="${name}" min="${min}" max="${max}" value="${Number(value) || 0}"></label>`;
}

function emptyState(title, hint) {
  const node = document.createElement('div');
  node.className = 'model-empty-state';
  node.innerHTML = `<strong>${escapeText(title)}</strong><p class="muted">${escapeText(hint)}</p>`;
  return node;
}

function escapeText(value) {
  return String(value ?? '').replace(/[&<>'"]/g, (char) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', "'": '&#39;', '"': '&quot;' })[char]);
}
