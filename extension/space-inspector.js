

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
  queueWorkspaceMutation,
  onPositionEditChange,
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
      const result = await queueWorkspaceMutation(activeWorkspaceId, async (latestSnapshot) => {
        const nextWidget = {
          id: '',
          key,
          order: (latestSnapshot?.widgets || []).length,
          enabled: true,
          configJson: { ...(plugin?.defaultData || {}) },
          displayJson: { position: 'middleCentre' },
        };
        return nativeCall('workspace_widget_upsert', {
          workspaceId: activeWorkspaceId,
          widget: nextWidget,
          expectedRevision: latestSnapshot.revision,
        });
      });
      broadcastRevision();
      const created = result?.widgets?.find((w) => w.key === key && !currentSnapshot.widgets.some((old) => old.id === w.id)) || result?.widgets?.at(-1);
      routeTo(created ? 'widget' : 'overview', created?.id);
    },
    onBackToOverview: () => routeTo('overview'),
  });

  const widgetSettingsCtrl = createSpaceWidgetSettings({
    t,
    language,
    onUpdateWidget: async (updatedWidget) => {
      const result = await queueWorkspaceMutation(activeWorkspaceId, async (latestSnapshot) => {
        const latestWidget = (latestSnapshot.widgets || []).find((w) => w.id === updatedWidget.id);
        if (!latestWidget) throw new Error(t('widgetMissing', '卡片不存在'));
        const merged = {
          ...latestWidget,
          configJson: { ...(latestWidget.configJson || {}), ...(updatedWidget.configJson || {}) },
          displayJson: { ...(latestWidget.displayJson || {}), ...(updatedWidget.displayJson || {}) },
        };
        return nativeCall('workspace_widget_upsert', {
          workspaceId: activeWorkspaceId,
          widget: merged,
          expectedRevision: latestSnapshot.revision,
        });
      }).catch(() => null);
      if (!result) return null;
      broadcastRevision();
      return result;
    },
    onRemoveWidget: async (widgetId) => {
      const result = await queueWorkspaceMutation(activeWorkspaceId, async (latestSnapshot) => {
        return nativeCall('workspace_widget_remove', {
          workspaceId: activeWorkspaceId,
          widgetId,
          expectedRevision: latestSnapshot.revision,
        });
      }).catch(() => null);
      if (!result) return null;
      broadcastRevision();
      onPositionEditChange?.(null);
      routeTo('overview');
      return result;
    },
    onPositionEditChange: (widgetId) => onPositionEditChange?.(widgetId),
    onBackToOverview: () => {
      onPositionEditChange?.(null);
      routeTo('overview');
    },
  });

  const bgSettingsCtrl = createSpaceBackgroundSettings({
    t,
    language,
    onUpdateBackground: async (bgData) => {
      const result = await queueWorkspaceMutation(activeWorkspaceId, async (latestSnapshot) => {
        const latestBackground = latestSnapshot.backgroundJson || {};
        const background = bgData.key && bgData.key !== latestBackground.key
          ? bgData
          : { ...latestBackground, ...bgData, display: { ...(latestBackground.display || {}), ...(bgData.display || {}) } };
        return nativeCall('workspace_background_save', {
          workspaceId: activeWorkspaceId,
          background,
          expectedRevision: latestSnapshot.revision,
        });
      }).catch(() => null);
      if (!result) return null;
      broadcastRevision();
      return result;
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
    if (currentState === 'widget' && state !== 'widget') {
      onPositionEditChange?.(null);
    }
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

    
    const bgKey = currentSnapshot?.backgroundJson?.key || 'background/colour';
    const bgName = pluginName(bgKey, language, backgroundPlugins[bgKey]?.name || bgKey);
    const bgSec = document.createElement('div');
    bgSec.className = 'inspector-section';
    bgSec.innerHTML = `
      <h3>${t('background', '背景')}</h3>
      <button type="button" class="inspector-card-clickable" id="bg-overview-card" aria-label="${t('background', '背景')}: ${bgName}">
        <svg class="icon" aria-hidden="true"><use href="#i-image" /></svg><span>${bgName}</span><svg class="icon chev" aria-hidden="true"><use href="#i-chevron-right" /></svg>
      </button>
    `;
    bgSec.querySelector('#bg-overview-card').onclick = () => routeTo('background');
    body.append(bgSec);

    
    const widgetSec = document.createElement('div');
    widgetSec.className = 'inspector-section';
    widgetSec.innerHTML = `
      <div class="inspector-section-header">
        <h3>${t('widgets', '卡片')}</h3>
        <button class="icon-button add-widget-trigger" type="button" title="${t('addWidget', '添加卡片')}" aria-label="${t('addWidget', '添加卡片')}"><svg class="icon" aria-hidden="true"><use href="#i-plus" /></svg></button>
      </div>
      <div class="inspector-widget-list" role="list" aria-label="${t('widgets', '卡片')}"></div>
    `;
    widgetSec.querySelector('.add-widget-trigger').onclick = () => routeTo('catalog');

    const widgetList = widgetSec.querySelector('.inspector-widget-list');
    const widgets = currentSnapshot?.widgets || [];
    if (!widgets.length) {
      const empty = document.createElement('div');
      empty.className = 'inspector-empty';
      empty.textContent = t('noWidgetsInWorkspace', '当前空间暂无卡片');
      widgetList.append(empty);
    } else {
      let draggedIndex = null;
      let isDragging = false;
      widgets.forEach((w, index) => {
        const row = document.createElement('div');
        row.className = `inspector-row ${w.enabled ? '' : 'disabled'}`;
        row.setAttribute('role', 'listitem');
        row.draggable = true;
        const name = pluginName(w.key, language, widgetPlugins[w.key]?.name || w.key);
        row.innerHTML = `
          <span class="inspector-row-order" title="${t('dragToReorder', '按住拖拽排序')}">${index + 1}</span>
          <svg class="icon" aria-hidden="true"><use href="#i-box" /></svg><span>${name}</span>
          <button type="button" class="row-action-toggle" title="${w.enabled ? t('disable', '停用') : t('enable', '启用')}" aria-label="${w.enabled ? t('disable', '停用') : t('enable', '启用')} ${name}" aria-pressed="${w.enabled ? 'true' : 'false'}"><svg class="icon" aria-hidden="true"><use href="#i-check" /></svg></button>
          <svg class="icon chev" aria-hidden="true"><use href="#i-chevron-right" /></svg>
        `;

        row.ondragstart = (e) => {
          if (e.target?.closest?.('.row-action-toggle')) {
            e.preventDefault?.();
            return;
          }
          draggedIndex = index;
          isDragging = true;
          row.classList.add('dragging');
          if (e.dataTransfer) {
            e.dataTransfer.effectAllowed = 'move';
            e.dataTransfer.setData('text/plain', String(index));
          }
        };

        row.ondragover = (e) => {
          if (draggedIndex === null || draggedIndex === index) return;
          e.preventDefault?.();
          if (e.dataTransfer) e.dataTransfer.dropEffect = 'move';
          const rect = typeof row.getBoundingClientRect === 'function' ? row.getBoundingClientRect() : { top: 0, height: 36 };
          const midY = rect.top + rect.height / 2;
          const clientY = Number(e.clientY) || 0;
          if (clientY < midY) {
            row.classList.add('drag-over-top');
            row.classList.remove('drag-over-bottom');
          } else {
            row.classList.add('drag-over-bottom');
            row.classList.remove('drag-over-top');
          }
        };

        row.ondragleave = () => {
          row.classList.remove('drag-over-top', 'drag-over-bottom');
        };

        row.ondrop = (e) => {
          e.preventDefault?.();
          row.classList.remove('drag-over-top', 'drag-over-bottom');
          if (draggedIndex === null || draggedIndex === index) return;
          const fromIndex = draggedIndex;
          const rect = typeof row.getBoundingClientRect === 'function' ? row.getBoundingClientRect() : { top: 0, height: 36 };
          const clientY = Number(e.clientY) || 0;
          const insertBefore = clientY < (rect.top + rect.height / 2);

          const nextWidgets = [...widgets];
          const [moved] = nextWidgets.splice(fromIndex, 1);
          let targetIndex = nextWidgets.findIndex((item) => item.id === w.id);
          if (!insertBefore) {
            targetIndex += 1;
          }
          nextWidgets.splice(targetIndex, 0, moved);

          const orderedIds = nextWidgets.map((item) => item.id);
          queueWorkspaceMutation(activeWorkspaceId, async (latestSnapshot) => {
            return nativeCall('workspace_widget_reorder', {
              workspaceId: activeWorkspaceId,
              orderedIds,
              expectedRevision: latestSnapshot.revision,
            });
          }).then(() => broadcastRevision()).catch(() => {});
        };

        row.ondragend = () => {
          row.classList.remove('dragging');
          widgetList.querySelectorAll?.('.inspector-row')?.forEach?.((r) => {
            r.classList?.remove?.('dragging', 'drag-over-top', 'drag-over-bottom');
          });
          draggedIndex = null;
          setTimeout(() => { isDragging = false; }, 50);
        };

        row.onclick = (e) => {
          if (isDragging) return;
          if (e.target.closest('.row-action-toggle')) {
            e.stopPropagation();
            queueWorkspaceMutation(activeWorkspaceId, async (latestSnapshot) => {
              const latestWidget = (latestSnapshot.widgets || []).find((item) => item.id === w.id);
              if (!latestWidget) throw new Error(t('widgetMissing', '卡片不存在'));
              return nativeCall('workspace_widget_upsert', {
                workspaceId: activeWorkspaceId,
                widget: { ...latestWidget, enabled: !latestWidget.enabled },
                expectedRevision: latestSnapshot.revision,
              });
            }).then(() => broadcastRevision()).catch(() => {});
            return;
          }
          routeTo('widget', w.id);
        };
        widgetList.append(row);
      });
    }
    body.append(widgetSec);

    
    const manageSec = document.createElement('div');
    manageSec.className = 'inspector-section';
    manageSec.innerHTML = `
      <h3>${t('management', '配置管理')}</h3>
      <div class="inspector-actions" role="group" aria-label="${t('management', '配置管理')}">
        <button id="import-tabliss-btn" type="button">${t('importTabliss', '导入 Tabliss 配置')}</button>
        <button id="export-tabliss-btn" type="button">${t('exportTabliss', '导出 Tabliss 配置')}</button>
        <button id="reset-workspace-btn" class="danger" type="button">${t('resetWorkspace', '重置空间...')}</button>
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
    onPositionEditChange?.(null);
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
