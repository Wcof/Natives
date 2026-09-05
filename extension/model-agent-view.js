/**
 * Model Agent Clients View (<300 lines).
 * 智能体配置: left client list + right core-config panel, mirroring the
 * reference GUI (安装状态 / 客户端版本 / 使用模型 / 配置修改 / 启动).
 */

function escapeHtml(value) {
  return String(value ?? '').replace(/[&<>"']/g, (match) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[match]);
}

export function renderAgentView(container, { clients = [], selectedId = '', models = null, modelsError = '', selection = {}, busy = false, t, onAction }) {
  container.replaceChildren();

  const topbar = document.createElement('div');
  topbar.className = 'model-agent-topbar';
  topbar.innerHTML = `
    <div class="model-agent-stats">${clients.length} ${t('agentClientsDetected', '个本机客户端')}</div>
    <button type="button" class="btn-quota-action" data-action="agent-refresh">
      <svg class="icon" aria-hidden="true"><use href="#i-refresh" /></svg>
      <span>${t('agentRedetect', '重新检测')}</span>
    </button>
  `;
  container.append(topbar);

  const layout = document.createElement('div');
  layout.className = 'model-agent-layout';

  layout.append(renderClientList(clients, selectedId, t, onAction));

  const selected = clients.find((client) => client.id === selectedId) || null;
  layout.append(renderDetail(selected, models, modelsError, selection, busy, t, onAction));

  container.append(layout);
}

function renderClientList(clients, selectedId, t, onAction) {
  const pane = document.createElement('aside');
  pane.className = 'model-agent-list';
  pane.innerHTML = `
    <div class="model-agent-list-header">
      <strong>${t('agentLocalClients', '本机客户端')}</strong>
      <p class="muted">${t('agentLocalClientsHint', '选择需要管理的智能体')}</p>
    </div>
    <div class="model-agent-client-scroll"></div>
  `;
  const scroll = pane.querySelector('.model-agent-client-scroll');
  for (const client of clients) {
    const item = document.createElement('button');
    item.type = 'button';
    item.className = `model-agent-client${client.id === selectedId ? ' selected' : ''}`;
    item.dataset.action = 'agent-select';
    item.dataset.client = client.id;
    const dot = client.installed ? ' installed' : '';
    item.innerHTML = `
      <span class="model-agent-client-main">
        <strong>${escapeHtml(client.name)}</strong>
        <small>${escapeHtml(listStatusText(client, t))}</small>
      </span>
      <span class="model-agent-dot${dot}" aria-hidden="true"></span>
    `;
    scroll.append(item);
  }
  if (!clients.length) {
    scroll.innerHTML = `<div class="model-empty-state"><strong>${t('agentDetecting', '正在检测本机客户端…')}</strong></div>`;
  }
  return pane;
}

function listStatusText(client, t) {
  if (!client.installed) return t('agentNotInstalled', '未检测到安装');
  if (client.modificationState === 'applied' && client.appliedModel) {
    return `${t('agentModified', '已修改')} · ${client.appliedModel}`;
  }
  return client.version ? `${t('agentInstalled', '已安装')} · ${client.version}` : t('agentInstalledKeepConfig', '已安装 · 保持原配置');
}

function renderDetail(client, models, modelsError, selection, busy, t, onAction) {
  const panel = document.createElement('section');
  panel.className = 'model-agent-detail';
  if (!client) {
    panel.append(emptyState(t('agentSelectHint', '从左侧选择一个智能体客户端'), t('agentSelectHintDetail', '检测完成后即可查看安装状态并应用网关配置。')));
    return panel;
  }

  const heading = document.createElement('header');
  heading.className = 'model-page-heading';
  heading.innerHTML = `<div><span class="model-oauth-section-tag">${t('agentCoreConfig', '核心配置')}</span><h3>${escapeHtml(client.name)}</h3></div>`;
  panel.append(heading);

  const body = document.createElement('div');
  body.className = 'model-agent-body';

  const statusGrid = document.createElement('div');
  statusGrid.className = 'model-agent-status-grid';
  statusGrid.innerHTML = `
    <div class="model-agent-status-card"><span>${t('agentInstallStatus', '安装状态')}</span><strong>${client.installed ? t('agentClientDetected', '已检测到客户端') : t('agentClientMissing', '未检测到客户端')}</strong></div>
    <div class="model-agent-status-card"><span>${t('agentClientVersion', '客户端版本')}</span><strong>${escapeHtml(client.version || client.appVersion || t('agentVersionUnknown', '未获取'))}</strong></div>
  `;
  body.append(statusGrid);

  if (client.error) {
    const errorLine = document.createElement('p');
    errorLine.className = 'model-quota-error';
    errorLine.textContent = client.error;
    body.append(errorLine);
  }
  for (const warning of client.warnings || []) {
    const line = document.createElement('p');
    line.className = 'model-adv-hint muted';
    line.textContent = warning;
    body.append(line);
  }

  if (client.installed && client.modelPicker) {
    body.append(renderModelSection(client, models, modelsError, selection, busy, t, onAction));

    const modify = document.createElement('div');
    modify.className = 'model-section';
    modify.innerHTML = `
      <h4>${t('agentModifyTitle', '配置修改')}</h4>
      <p class="muted">${t('agentModifyHint', '应用或关闭配置后，请完全退出并重新启动客户端；默认模型和上下文长度会在新会话中生效。')}</p>
      <div class="model-agent-actions">
        <button type="button" class="primary" data-action="agent-apply" data-client="${escapeHtml(client.id)}" ${busy ? 'disabled' : ''}>${t('agentApply', '应用配置修改')}</button>
        <button type="button" data-action="agent-default" data-client="${escapeHtml(client.id)}" ${busy ? 'disabled' : ''}>${t('agentDefault', '默认配置')}</button>
        ${client.modificationState === 'applied' ? `<button type="button" data-action="agent-close-config" data-client="${escapeHtml(client.id)}" ${busy ? 'disabled' : ''}>${t('agentCloseConfig', '关闭配置修改')}</button>` : ''}
      </div>
    `;
    body.append(modify);
  } else if (client.installed) {
    body.append(emptyState(t('agentManualClient', '该客户端由本工具自动接管网关参数'), t('agentManualClientHint', '启动后即通过本地代理调用模型。')));
  } else {
    body.append(emptyState(t('agentNotInstalledTitle', '未检测到安装'), t('agentNotInstalledHint', '安装该客户端后点击「重新检测」。')));
  }

  const footer = document.createElement('div');
  footer.className = 'model-agent-footer';
  for (const target of client.launchTargets || []) {
    footer.innerHTML += `<button type="button" class="primary" data-action="agent-launch" data-client="${escapeHtml(client.id)}" data-target="${escapeHtml(target.id)}" ${client.installed ? '' : 'disabled'}>▷ ${t('agentLaunch', '启动')} ${escapeHtml(target.label)}</button>`;
  }
  if (footer.children.length) body.append(footer);

  panel.append(body);
  return panel;
}

function renderModelSection(client, models, modelsError, selection, busy, t, onAction) {
  const section = document.createElement('div');
  section.className = 'model-section';
  const current = selection[client.id] || '';
  let inner = `<h4>${t('agentUseModel', '使用模型')}</h4>`;
  if (modelsError) {
    inner += `<p class="model-quota-error">${escapeHtml(modelsError)}</p>`;
  } else if (!models) {
    inner += `<p class="muted">${t('agentModelsLoading', '正在读取可用模型…')}</p>`;
  } else if (!models.length) {
    inner += `<p class="muted">${t('agentModelsEmpty', '本地代理暂未发现可用模型，请先在模型设置中添加供应商或账户。')}</p>`;
  } else {
    inner += `
      <label class="inspector-field"><span>${t('agentModelLabel', '默认模型')}</span>
        <select data-agent-model-select="${escapeHtml(client.id)}">
          ${models.map((model) => `<option value="${escapeHtml(model.name)}" ${model.name === current ? 'selected' : ''}>${escapeHtml(model.name)}</option>`).join('')}
        </select>
      </label>
      <p class="muted">${models.length} ${t('agentModelsHint', '个可用模型，首次默认选择第一项')}</p>
    `;
  }
  section.innerHTML = inner;
  const select = section.querySelector('select');
  if (select) {
    select.onchange = () => onAction('agent-model-change', { client: client.id, model: select.value });
  }
  return section;
}

function emptyState(title, hint = '') {
  const node = document.createElement('div');
  node.className = 'model-empty-state';
  node.innerHTML = `<strong>${escapeHtml(title)}</strong>${hint ? `<p class="muted">${escapeHtml(hint)}</p>` : ''}`;
  return node;
}
