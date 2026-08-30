/**
 * Space Inspector State Machine & Shell Controller (<300 lines).
 * States: closed | overview | catalog | widget(id) | background | importPreview.
 */

import { widgetPlugins, backgroundPlugins, pluginName } from './space-plugins.js';
import { createSpaceCatalog } from './space-catalog.js';
import { createSpaceWidgetSettings } from './space-widget-settings.js';
import { createSpaceBackgroundSettings } from './space-background-settings.js';
import { createSpaceImporter } from './space-importer.js';
import { createSpaceResetModal } from './space-modal-reset.js';

export function createSpaceInspector({
  $,
  t,
  language = 'zh_CN',
  nativeCall,
  broadcastRevision,
  updateSnapshot,
  onCloseFocusAnchor,
}) {
  const inspectorEl = $('inspector');
  const backdropEl = $('inspector-backdrop');
  const inspectorContainer = $('inspector-body') || inspectorEl;

  let currentSnapshot = null;
  let activeWorkspaceId = null;
  let currentState = 'closed';
  let activeWidgetId = null;

  const catalogCtrl = createSpaceCatalog({
    t,
    language,
    onAddWidget: async (key) => {
      const plugin = widgetPlugins[key];
      const nextWidget = {
        id: '',
        key,
        order: (currentSnapshot?.widgets || []).length,
        enabled: true,
        configJson: { ...(plugin?.defaultData || {}) },
        displayJson: { position: 'middleCentre' },
      };
      const result = await nativeCall('workspace_widget_upsert', {
        workspaceId: activeWorkspaceId,
        widget: nextWidget,
        expectedRevision: currentSnapshot.revision,
      });
      updateSnapshot(result);
      broadcastRevision();
      const created = result.widgets.find((w) => w.key === key && !currentSnapshot.widgets.some((old) => old.id === w.id)) || result.widgets.at(-1);
      routeTo(created ? 'widget' : 'overview', created?.id);
    },
    onBackToOverview: () => routeTo('overview'),
  });

  const widgetSettingsCtrl = createSpaceWidgetSettings({
    t,
    language,
    onUpdateWidget: async (updatedWidget) => {
      try {
        const result = await nativeCall('workspace_widget_upsert', {
          workspaceId: activeWorkspaceId,
          widget: updatedWidget,
          expectedRevision: currentSnapshot.revision,
        });
        updateSnapshot(result);
        broadcastRevision();
      } catch (err) {}
    },
    onRemoveWidget: async (widgetId) => {
      try {
        const result = await nativeCall('workspace_widget_remove', {
          workspaceId: activeWorkspaceId,
          widgetId,
          expectedRevision: currentSnapshot.revision,
        });
        updateSnapshot(result);
        broadcastRevision();
        routeTo('overview');
      } catch (err) {}
    },
    onReorderWidget: async (orderedIds) => {
      try {
        const result = await nativeCall('workspace_widget_reorder', {
          workspaceId: activeWorkspaceId,
          orderedIds,
          expectedRevision: currentSnapshot.revision,
        });
        updateSnapshot(result);
        broadcastRevision();
      } catch (err) {}
    },
    onBackToOverview: () => routeTo('overview'),
  });

  const bgSettingsCtrl = createSpaceBackgroundSettings({
    t,
    language,
    onUpdateBackground: async (bgData) => {
      try {
        const result = await nativeCall('workspace_background_save', {
          workspaceId: activeWorkspaceId,
          background: bgData,
          expectedRevision: currentSnapshot.revision,
        });
        updateSnapshot(result);
        broadcastRevision();
      } catch (err) {}
    },
    onBackToOverview: () => routeTo('overview'),
  });

  const importerCtrl = createSpaceImporter({
    t,
    nativeCall,
    broadcastRevision,
    updateSnapshot,
    onBackToOverview: () => routeTo('overview'),
  });

  const resetModal = createSpaceResetModal({
    $,
    onResetWorkspace: ({ workspaceId, expectedRevision }, template) => {
      importerCtrl.resetWorkspace(workspaceId, template, expectedRevision)
        .then(() => routeTo('overview'))
        .catch(() => {});
    },
  });

  function routeTo(state, widgetId = null) {
    currentState = state;
    activeWidgetId = widgetId;
    renderCurrentState();
  }

  function renderCurrentState() {
    const isClosed = currentState === 'closed';
    if (inspectorEl) inspectorEl.hidden = isClosed;
    if (backdropEl) backdropEl.hidden = isClosed;
    if (isClosed) return;

    if (currentState === 'catalog') {
      catalogCtrl.render(inspectorContainer, currentSnapshot, activeWorkspaceId);
    } else if (currentState === 'widget' && activeWidgetId) {
      widgetSettingsCtrl.render(inspectorContainer, currentSnapshot, activeWorkspaceId, activeWidgetId);
    } else if (currentState === 'background') {
      bgSettingsCtrl.render(inspectorContainer, currentSnapshot, activeWorkspaceId);
    } else {
      renderOverview();
    }
  }

  function renderOverview() {
    inspectorContainer.replaceChildren();

    const header = document.createElement('div');
    header.className = 'inspector-heading';
    header.innerHTML = `
      <h2>${currentSnapshot?.name || t('spaceSettings', '空间设置')}</h2>
      <button id="inspector-close-btn" class="icon-button" title="${t('close', '关闭')}"><svg class="icon"><use href="#i-close" /></svg></button>
    `;
    header.querySelector('#inspector-close-btn').onclick = () => close();

    const body = document.createElement('div');
    body.className = 'inspector-body';

    // Background Card
    const bgKey = currentSnapshot?.backgroundJson?.key || 'background/colour';
    const bgName = pluginName(bgKey, language, backgroundPlugins[bgKey]?.name || bgKey);
    const bgSec = document.createElement('div');
    bgSec.className = 'inspector-section';
    bgSec.innerHTML = `
      <h3>${t('background', '背景')}</h3>
      <div class="inspector-card-clickable" id="bg-overview-card">
        <svg class="icon"><use href="#i-image" /></svg><span>${bgName}</span><svg class="icon chev"><use href="#i-chevron-right" /></svg>
      </div>
    `;
    bgSec.querySelector('#bg-overview-card').onclick = () => routeTo('background');
    body.append(bgSec);

    // Widgets List
    const widgetSec = document.createElement('div');
    widgetSec.className = 'inspector-section';
    widgetSec.innerHTML = `
      <div style="display:flex;align-items:center;justify-content:space-between;">
        <h3>${t('widgets', '小组件')}</h3>
        <button class="icon-button add-widget-trigger" type="button" title="${t('addWidget', '添加组件')}"><svg class="icon"><use href="#i-plus" /></svg></button>
      </div>
      <div class="inspector-widget-list"></div>
    `;
    widgetSec.querySelector('.add-widget-trigger').onclick = () => routeTo('catalog');

    const widgetList = widgetSec.querySelector('.inspector-widget-list');
    const widgets = currentSnapshot?.widgets || [];
    if (!widgets.length) {
      const empty = document.createElement('div');
      empty.className = 'inspector-empty';
      empty.textContent = t('noWidgetsInWorkspace', '当前空间暂无组件');
      widgetList.append(empty);
    } else {
      widgets.forEach((w) => {
        const row = document.createElement('div');
        row.className = `inspector-row ${w.enabled ? '' : 'disabled'}`;
        const name = pluginName(w.key, language, widgetPlugins[w.key]?.name || w.key);
        row.innerHTML = `
          <svg class="icon"><use href="#i-box" /></svg><span>${name}</span>
          <button type="button" class="row-action-toggle" title="${w.enabled ? t('disable', '停用') : t('enable', '启用')}"><svg class="icon"><use href="#i-check" /></svg></button>
          <svg class="icon chev"><use href="#i-chevron-right" /></svg>
        `;
        row.onclick = (e) => {
          if (e.target.closest('.row-action-toggle')) {
            e.stopPropagation();
            nativeCall('workspace_widget_upsert', {
              workspaceId: activeWorkspaceId,
              widget: { ...w, enabled: !w.enabled },
              expectedRevision: currentSnapshot.revision,
            }).then((res) => { updateSnapshot(res); broadcastRevision(); }).catch(() => {});
            return;
          }
          routeTo('widget', w.id);
        };
        widgetList.append(row);
      });
    }
    body.append(widgetSec);

    // Management
    const manageSec = document.createElement('div');
    manageSec.className = 'inspector-section';
    manageSec.innerHTML = `
      <h3>${t('management', '配置管理')}</h3>
      <div class="inspector-actions">
        <button id="import-tabliss-btn" type="button">${t('importTabliss', '导入 Tabliss 配置')}</button>
        <button id="export-tabliss-btn" type="button">${t('exportTabliss', '导出 Tabliss 配置')}</button>
        <button id="reset-workspace-btn" type="button">${t('resetWorkspace', '重置空间...')}</button>
      </div>
    `;
    manageSec.querySelector('#import-tabliss-btn').onclick = () => importerCtrl.startImportFile(inspectorContainer, activeWorkspaceId, currentSnapshot);
    manageSec.querySelector('#export-tabliss-btn').onclick = () => importerCtrl.exportTabliss(activeWorkspaceId, currentSnapshot?.name);
    manageSec.querySelector('#reset-workspace-btn').onclick = () => resetModal.open({
      workspaceId: activeWorkspaceId,
      expectedRevision: currentSnapshot.revision,
    });
    body.append(manageSec);

    inspectorContainer.append(header, body);
  }

  function open(target = 'overview') {
    if (typeof target === 'object' && target?.widgetId) routeTo('widget', target.widgetId);
    else if (target === 'catalog' || target === 'background') routeTo(target);
    else routeTo('overview');
  }

  function close() {
    currentState = 'closed';
    activeWidgetId = null;
    renderCurrentState();
    onCloseFocusAnchor?.();
  }

  function sync(snapshot, wsId) {
    currentSnapshot = snapshot;
    activeWorkspaceId = wsId;
    if (currentState !== 'closed') renderCurrentState();
  }

  if (backdropEl) backdropEl.onclick = () => close();
  if (typeof document !== 'undefined' && typeof document.addEventListener === 'function') {
    document.addEventListener('keydown', (e) => {
      if (e.key === 'Escape' && currentState !== 'closed') close();
    });
  }

  return {
    open,
    close,
    sync,
    routeTo,
    get state() { return currentState; },
    get isOpen() { return currentState !== 'closed'; },
  };
}
