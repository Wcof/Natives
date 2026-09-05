/**
 * Model Agent Clients View.
 * 智能体配置: client list + detail with 核心配置 / 会话管理 (codex) tabs,
 * searchable model picker, Claude role mappings, Pi plugin section, launch bar.
 */

import { createAgentModelPicker } from './agent-model-picker.js';
import { renderClaudeMappings, renderPiSection } from './model-agent-sections.js';

function escapeHtml(value) {
  return String(value ?? '').replace(/[&<>"']/g, (match) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[match]);
}

export function renderAgentView(container, context) {
  const { clients = [], selectedId = '', models = null, modelsError = '', selection = {}, busy = false, activeTab = 'core', sessions = [], t, onAction } = context;
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
  layout.append(renderClientList(clients, selectedId, t));

  const selected = clients.find((client) => client.id === selectedId) || null;
  layout.append(renderDetail(selected, { models, modelsError, selection, busy, activeTab, sessions, t, onAction }));

  container.append(layout);
}

function renderClientList(clients, selectedId, t) {
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
    item.innerHTML = `
      <span class="model-agent-client-main">
        <strong>${escapeHtml(client.name)}</strong>
        <small>${escapeHtml(listStatusText(client, t))}</small>
      </span>
      <span class="model-agent-dot${client.installed ? ' installed' : ''}" aria-hidden="true"></span>
    `;
    scroll.append(item);
  }
  if (!clients.length) {
    scroll.innerHTML = `<div class="model-empty-state"><strong>${t('agentDetecting', '正在检测本机客户端…')}</strong></div>`;
  }
  return pane;
}

function listStatusText(client, t) {
  if (client.id === 'pi' && client.pluginVersion) return t('agentPiInstalled', 'Pi provider 插件已安装');
  if (!client.installed) return t('agentNotInstalled', '未检测到安装');
  if (client.modificationState === 'applied' && client.appliedModel) {
    return `${t('agentModified', '已修改')} · ${client.appliedModel}`;
  }
  return client.version ? `${t('agentInstalled', '已安装')} · ${client.version}` : t('agentInstalledKeepConfig', '已安装 · 保持原配置');
}

function renderDetail(client, context) {
  const { models, modelsError, selection, busy, activeTab, sessions, t, onAction } = context;
  const panel = document.createElement('section');
  panel.className = 'model-agent-detail';
  if (!client) {
    panel.innerHTML = `<div class="model-empty-state"><strong>${escapeHtml(t('agentSelectHint', '从左侧选择一个智能体客户端'))}</strong><p class="muted">${escapeHtml(t('agentSelectHintDetail', '检测完成后即可查看安装状态并应用网关配置。'))}</p></div>`;
    return panel;
  }

  const heading = document.createElement('header');
  heading.className = 'model-page-heading';
  heading.innerHTML = `<div><span class="model-oauth-section-tag">${t('agentCoreConfig', '核心配置')}</span><h3>${escapeHtml(client.name)}</h3></div>`;
  panel.append(heading);

  if (client.id === 'codex') {
    panel.append(renderDetailTabs(activeTab, t, onAction));
    if (activeTab === 'sessions') {
      panel.append(renderSessions(sessions, busy, t, onAction));
      return panel;
    }
  }

  const body = document.createElement('div');
  body.className = 'model-agent-body';

  body.append(renderStatusGrid(client, t));
  if (client.error) body.append(errorLine(client.error));
  for (const warning of client.warnings || []) body.append(warningLine(warning));

  if (client.installed && client.modelPicker) {
    body.append(renderModelSection(client, models, modelsError, selection, busy, t, onAction));
    if (client.id === 'claude-code' || client.id === 'claude-desktop') {
      const mappingHost = document.createElement('div');
      mappingHost.className = 'model-section';
      body.append(mappingHost);
      renderClaudeMappings(mappingHost, selection[`${client.id}:mappings`], models, t, (mappings) => {
        onAction('agent-mapping-change', { client: client.id, mappings });
      });
    }
    body.append(renderModifySection(client, selection, busy, t, onAction));
    if (client.id === 'codex') body.append(renderCodexDanger(client, busy, t, onAction));
  } else if (client.installed) {
    if (client.id === 'pi') body.append(renderPiSection(null, { installed: true, installedVersion: client.pluginVersion }, t, onAction));
    else body.append(emptyState(t('agentManualClient', '该客户端由本工具自动接管网关参数'), t('agentManualClientHint', '启动后即通过本地代理调用模型。')));
  } else {
    body.append(emptyState(t('agentNotInstalledTitle', '未检测到安装'), t('agentNotInstalledHint', '安装该客户端后点击「重新检测」。')));
  }

  body.append(renderLaunchFooter(client, t, onAction));
  panel.append(body);
  return panel;
}

function renderStatusGrid(client, t) {
  const grid = document.createElement('div');
  grid.className = 'model-agent-status-grid';
  const cards = [[t('agentInstallStatus', '安装状态'), client.installed ? t('agentClientDetected', '已检测到客户端') : t('agentClientMissing', '未检测到客户端')]];
  if (client.cliVersion || client.appVersion) {
    cards.push([t('agentCliVersion', 'CLI 版本'), client.cliVersion || '—']);
    cards.push([t('agentAppVersion', 'App 版本'), client.appVersion || '—']);
  } else if (client.id === 'pi') {
    cards.push([t('agentClientVersion', '客户端版本'), client.version || '—']);
    cards.push([t('agentPiPluginVersion', '插件版本'), client.pluginVersion || '—']);
  } else {
    cards.push([t('agentClientVersion', '客户端版本'), client.version || '—']);
  }
  for (const [label, value] of cards) {
    const card = document.createElement('div');
    card.className = 'model-agent-status-card';
    card.innerHTML = `<span>${escapeHtml(label)}</span><strong>${escapeHtml(value)}</strong>`;
    grid.append(card);
  }
  return grid;
}

function renderModelSection(client, models, modelsError, selection, busy, t, onAction) {
  const section = document.createElement('div');
  section.className = 'model-section';
  const current = selection[client.id] || '';
  section.innerHTML = `<h4>${t('agentUseModel', '使用模型')}</h4>`;
  if (modelsError) {
    const line = document.createElement('p');
    line.className = 'model-quota-error';
    line.textContent = modelsError;
    section.append(line);
  } else if (!models) {
    section.innerHTML += `<p class="muted">${t('agentModelsLoading', '正在读取可用模型…')}</p>`;
  } else if (!models.length) {
    section.innerHTML += `<p class="muted">${t('agentModelsEmpty', '本地代理暂未发现可用模型，请先在模型设置中添加供应商或账户。')}</p>`;
  } else {
    const picker = createAgentModelPicker({
      value: current,
      models,
      t,
      onChange: (model) => onAction('agent-model-change', { client: client.id, model }),
      onRefresh: () => onAction('agent-models-refresh', { client: client.id }),
    });
    section.append(picker);
  }
  return section;
}

function renderModifySection(client, selection, busy, t, onAction) {
  const section = document.createElement('div');
  section.className = 'model-section';
  const applied = client.modificationState === 'applied';
  const primaryLabel = !applied
    ? t('agentApply', '应用配置修改')
    : selection[client.id] && selection[client.id] !== client.appliedModel ? t('agentUpdate', '更新配置') : t('agentCloseConfig', '关闭配置修改');
  section.innerHTML = `
    <h4>${t('agentModifyTitle', '配置修改')}</h4>
    <p class="muted">${t('agentModifyHint', '应用或关闭配置后，请完全退出并重新启动客户端；默认模型和上下文长度会在新会话中生效。')}</p>
    <div class="model-agent-actions">
      <button type="button" class="primary" data-action="agent-apply" data-client="${escapeHtml(client.id)}" ${busy ? 'disabled' : ''}>${escapeHtml(primaryLabel)}</button>
      <button type="button" data-action="agent-default" data-client="${escapeHtml(client.id)}" ${busy ? 'disabled' : ''}>${t('agentDefault', '默认配置')}</button>
      ${applied && primaryLabel !== t('agentCloseConfig', '关闭配置修改') ? `<button type="button" data-action="agent-close-config" data-client="${escapeHtml(client.id)}" ${busy ? 'disabled' : ''}>${t('agentCloseConfig', '关闭配置修改')}</button>` : ''}
    </div>
  `;
  return section;
}

function renderCodexDanger(client, busy, t, onAction) {
  const section = document.createElement('div');
  section.className = 'model-section';
  section.innerHTML = `
    <h4>${t('agentCodexDangerTitle', 'Codex 配置清理')}</h4>
    <p class="muted">${t('agentCodexDangerHint', '移除由本工具写入的 Codex 配置与凭据文件，恢复未接管状态。')}</p>
    <div class="model-agent-actions">
      <button type="button" class="danger" data-action="agent-codex-clear" data-client="${escapeHtml(client.id)}" ${busy ? 'disabled' : ''}>${t('agentCodexClear', '清空配置')}</button>
    </div>
  `;
  return section;
}

function renderDetailTabs(activeTab, t, onAction) {
  const tabs = document.createElement('div');
  tabs.className = 'model-oauth-tabs-nav';
  for (const [tab, label] of [['core', t('agentTabCore', '核心配置')], ['sessions', t('agentTabSessions', '会话管理')]]) {
    const button = document.createElement('button');
    button.type = 'button';
    button.className = `model-oauth-tab-btn${activeTab === tab ? ' selected' : ''}`;
    button.dataset.action = 'agent-tab';
    button.dataset.tab = tab;
    button.textContent = label;
    tabs.append(button);
  }
  return tabs;
}

function renderSessions(sessions, busy, t, onAction) {
  const body = document.createElement('div');
  body.className = 'model-agent-body';
  const wrapper = document.createElement('div');
  wrapper.className = 'model-table-wrapper';
  wrapper.innerHTML = `<table class="model-keys-table"><thead><tr><th>${t('agentSessionName', '会话')}</th><th>${t('size', '大小')}</th><th>${t('modelEventTime', '时间')}</th><th></th></tr></thead><tbody>${sessions.map((session) => `
    <tr><td class="model-col-name"><code title="${escapeHtml(session.path)}">${escapeHtml(session.name)}</code></td><td class="model-col-num">${formatBytes(session.sizeBytes)}</td><td>${escapeHtml(session.updatedAt)}</td><td class="model-col-actions"><button type="button" class="danger btn-sm" data-action="agent-session-delete" data-path="${escapeHtml(session.path)}" ${busy ? 'disabled' : ''}>${t('delete', '删除')}</button></td></tr>`).join('')}</tbody></table>`;
  body.append(wrapper);
  if (!sessions.length) body.append(emptyState(t('agentSessionsEmpty', '没有找到 Codex 会话记录'), ''));
  return body;
}

function renderLaunchFooter(client, t, onAction) {
  const footer = document.createElement('div');
  footer.className = 'model-agent-footer';
  for (const target of client.launchTargets || []) {
    const needsDir = target.id === 'cli';
    const button = document.createElement('button');
    button.type = 'button';
    button.className = 'primary';
    button.dataset.action = 'agent-launch';
    button.dataset.client = client.id;
    button.dataset.target = target.id;
    button.disabled = !client.installed;
    button.innerHTML = `▷ ${t('agentLaunch', '启动')} ${escapeHtml(target.label)}`;
    button.onclick = (event) => {
      if (!needsDir) { onAction('agent-launch', event.currentTarget); return; }
      onAction('agent-launch-prompt', { client: client.id, target: target.id });
    };
    footer.append(button);
  }
  return footer;
}

function formatBytes(bytes) {
  if (!bytes) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB'];
  let index = 0;
  let value = bytes;
  while (value >= 1024 && index < units.length - 1) { value /= 1024; index += 1; }
  return `${value.toFixed(value >= 10 || index === 0 ? 0 : 1)} ${units[index]}`;
}

function errorLine(message) {
  const node = document.createElement('p');
  node.className = 'model-quota-error';
  node.textContent = message;
  return node;
}

function warningLine(message) {
  const node = document.createElement('p');
  node.className = 'muted';
  node.textContent = message;
  return node;
}

function emptyState(title, hint = '') {
  const node = document.createElement('div');
  node.className = 'model-empty-state';
  node.innerHTML = `<strong>${escapeHtml(title)}</strong>${hint ? `<p class="muted">${escapeHtml(hint)}</p>` : ''}`;
  return node;
}
