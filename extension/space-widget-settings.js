

import { widgetPlugins, pluginName, escapeHtml } from './space-plugins.js';

export function createSpaceWidgetSettings({
  t,
  language = 'zh_CN',
  onUpdateWidget,
  onRemoveWidget,
  onPositionEditChange,
  onBackToOverview,
}) {
  let isEditingPosition = false;

  function render(container, snapshot, workspaceId, widgetId) {
    const widget = (snapshot?.widgets || []).find((w) => w.id === widgetId);
    if (!widget) {
      onBackToOverview();
      return;
    }

    const plugin = widgetPlugins[widget.key];
    const name = pluginName(widget.key, language, plugin?.name || widget.key);
    const disp = widget.displayJson || {};
    const config = widget.configJson || {};

    container.replaceChildren();

    
    const header = document.createElement('div');
    header.className = 'inspector-heading';
    header.innerHTML = `
      <div class="inspector-heading-main">
        <button class="inspector-back" type="button"><svg class="icon"><use href="#i-chevron-left" /></svg><span>${t('back', '返回')}</span></button>
        <h2>${name}</h2>
      </div>
      <button class="icon-button widget-delete-trigger" title="${t('deleteWidget', '删除卡片')}" aria-label="${t('deleteWidget', '删除卡片')}">
        <svg class="icon" style="color:var(--danger)"><use href="#i-trash" /></svg>
      </button>
    `;
    header.querySelector('.inspector-back').onclick = () => {
      if (isEditingPosition) {
        isEditingPosition = false;
        onPositionEditChange?.(null);
      }
      onBackToOverview();
    };
    header.querySelector('.widget-delete-trigger').onclick = () => showDeleteConfirm();

    
    const body = document.createElement('div');
    body.className = 'inspector-body';

    
    if (plugin?.renderSettings) {
      const pluginSec = document.createElement('div');
      pluginSec.className = 'inspector-section';
      pluginSec.innerHTML = `<h3>${t('widgetCustomSettings', '卡片设置')}</h3><div class="plugin-settings-host"></div>`;
      plugin.renderSettings(
        pluginSec.querySelector('.plugin-settings-host'),
        config,
        (nextConfig) => {
          onUpdateWidget({ ...widget, configJson: nextConfig });
        },
        { t, language },
      );
      body.append(pluginSec);
    }

    
    const posSec = document.createElement('div');
    posSec.className = 'inspector-section';
    posSec.innerHTML = `
      <h3>${t('positionAndLayout', '位置与布局')}</h3>
      <div class="inspector-field-group">
        <label class="inspector-field">
          <span>${t('position', '位置模式')}</span>
          <select id="w-position">
            <option value="topLeft" ${disp.position === 'topLeft' ? 'selected' : ''}>左上 (Top Left)</option>
            <option value="topCentre" ${disp.position === 'topCentre' ? 'selected' : ''}>中上 (Top Centre)</option>
            <option value="topRight" ${disp.position === 'topRight' ? 'selected' : ''}>右上 (Top Right)</option>
            <option value="middleLeft" ${disp.position === 'middleLeft' ? 'selected' : ''}>左中 (Middle Left)</option>
            <option value="middleCentre" ${disp.position === 'middleCentre' || !disp.position ? 'selected' : ''}>居中 (Middle Centre)</option>
            <option value="middleRight" ${disp.position === 'middleRight' ? 'selected' : ''}>右中 (Middle Right)</option>
            <option value="bottomLeft" ${disp.position === 'bottomLeft' ? 'selected' : ''}>左下 (Bottom Left)</option>
            <option value="bottomCentre" ${disp.position === 'bottomCentre' ? 'selected' : ''}>中下 (Bottom Centre)</option>
            <option value="bottomRight" ${disp.position === 'bottomRight' ? 'selected' : ''}>右下 (Bottom Right)</option>
            <option value="free" ${disp.position === 'free' ? 'selected' : ''}>自由拖动 (Free Canvas)</option>
          </select>
        </label>
        <div id="free-pos-controls" class="inspector-field-group" ${disp.position === 'free' ? '' : 'hidden'}>
          <div class="inspector-actions">
            <button id="btn-edit-pos" class="primary" type="button">
              ${isEditingPosition ? t('doneEditingPosition', '完成调整') : t('editPosition', '调整位置')}
            </button>
            <button id="btn-reset-pos" type="button">
              ${t('resetPosition', '重置位置')}
            </button>
          </div>
          <p class="inspector-empty">
            ${t('freeMoveHelp', '点击“调整位置”后，可在画布上自由拖动卡片，拖拽四角缩放手柄或顶部旋转手柄。')}
          </p>
        </div>
      </div>
    `;

    const posSelect = posSec.querySelector('#w-position');
    const freeControls = posSec.querySelector('#free-pos-controls');
    const editPosBtn = posSec.querySelector('#btn-edit-pos');
    const resetPosBtn = posSec.querySelector('#btn-reset-pos');

    posSelect.onchange = () => {
      const isFree = posSelect.value === 'free';
      freeControls.hidden = !isFree;
      if (!isFree && isEditingPosition) {
        isEditingPosition = false;
        onPositionEditChange?.(null);
      }
      onUpdateWidget({ ...widget, displayJson: { ...disp, position: posSelect.value } });
    };

    if (editPosBtn) {
      editPosBtn.onclick = () => {
        isEditingPosition = !isEditingPosition;
        editPosBtn.textContent = isEditingPosition ? t('doneEditingPosition', '完成调整') : t('editPosition', '调整位置');
        onPositionEditChange?.(isEditingPosition ? widget.id : null);
      };
    }

    if (resetPosBtn) {
      resetPosBtn.onclick = () => {
        onUpdateWidget({
          ...widget,
          displayJson: {
            ...disp,
            position: 'free',
            xPercent: 50,
            yPercent: 50,
            scale: 1,
            rotation: 0,
          },
        });
      };
    }

    body.append(posSec);

    
    const styleSec = document.createElement('div');
    styleSec.className = 'inspector-section';
    styleSec.innerHTML = `
      <h3>${t('typographyAndStyle', '字体与样式')}</h3>
      <div class="inspector-field-group">
        <div class="inspector-field-range">
          <div class="inspector-field-range-header"><span>${t('fontSize', '字号')}</span><span id="w-fontsize-val">${disp.fontSize || 32}px</span></div>
          <input type="range" id="w-fontsize" min="10" max="120" value="${disp.fontSize || 32}" />
        </div>
        <div class="inspector-field-range">
          <div class="inspector-field-range-header"><span>${t('scale', '缩放')}</span><span id="w-scale-val">${disp.scale ?? 1}x</span></div>
          <input type="range" id="w-scale" min="0.2" max="3" step="0.1" value="${disp.scale ?? 1}" />
        </div>
        <div class="inspector-field-range">
          <div class="inspector-field-range-header"><span>${t('rotation', '旋转')}</span><span id="w-rot-val">${disp.rotation ?? 0}°</span></div>
          <input type="range" id="w-rot" min="-180" max="180" step="1" value="${disp.rotation ?? 0}" />
        </div>
        <label class="inspector-field"><span>${t('textColor', '文本颜色')}</span><input type="color" id="w-colour" value="${disp.colour || '#ffffff'}" /></label>
        <label class="inspector-checkbox"><input type="checkbox" id="w-accent" ${disp.useAccentColor ? 'checked' : ''} /><span>${t('useAccentColor', '使用强调色')}</span></label>
        <label class="inspector-checkbox"><input type="checkbox" id="w-outline" ${disp.textOutline ? 'checked' : ''} /><span>${t('textOutline', '文字描边')}</span></label>
        <div id="outline-details" class="inspector-field-group" ${disp.textOutline ? '' : 'hidden'}>
          <label class="inspector-field"><span>描边颜色</span><input type="color" id="w-outline-color" value="${disp.textOutlineColor || '#000000'}" /></label>
          <label class="inspector-field"><span>描边模式</span>
            <select id="w-outline-style">
              <option value="basic" ${disp.textOutlineStyle === 'basic' || !disp.textOutlineStyle ? 'selected' : ''}>投影描边 (Basic)</option>
              <option value="advanced" ${disp.textOutlineStyle === 'advanced' ? 'selected' : ''}>粗边描摹 (Advanced)</option>
            </select>
          </label>
        </div>
        <label class="inspector-field"><span>${t('fontWeight', '字重')}</span>
          <select id="w-weight">
            <option value="200" ${disp.fontWeight === '200' ? 'selected' : ''}>极细 (200)</option>
            <option value="300" ${disp.fontWeight === '300' ? 'selected' : ''}>细体 (300)</option>
            <option value="400" ${disp.fontWeight === '400' || !disp.fontWeight ? 'selected' : ''}>常规 (400)</option>
            <option value="600" ${disp.fontWeight === '600' ? 'selected' : ''}>半粗 (600)</option>
            <option value="700" ${disp.fontWeight === '700' ? 'selected' : ''}>粗体 (700)</option>
          </select>
        </label>
        <label class="inspector-field"><span>${t('customClass', '自定义 CSS 类')}</span><input type="text" id="w-class" value="${escapeHtml(disp.customClass || '')}" placeholder="custom-box" /></label>
      </div>
    `;

    const fontSlider = styleSec.querySelector('#w-fontsize');
    const fontVal = styleSec.querySelector('#w-fontsize-val');
    fontSlider.oninput = () => { fontVal.textContent = `${fontSlider.value}px`; };
    fontSlider.onchange = () => { onUpdateWidget({ ...widget, displayJson: { ...disp, fontSize: Number(fontSlider.value) } }); };

    const scaleSlider = styleSec.querySelector('#w-scale');
    const scaleVal = styleSec.querySelector('#w-scale-val');
    scaleSlider.oninput = () => { scaleVal.textContent = `${scaleSlider.value}x`; };
    scaleSlider.onchange = () => { onUpdateWidget({ ...widget, displayJson: { ...disp, scale: Number(scaleSlider.value) } }); };

    const rotSlider = styleSec.querySelector('#w-rot');
    const rotVal = styleSec.querySelector('#w-rot-val');
    rotSlider.oninput = () => { rotVal.textContent = `${rotSlider.value}°`; };
    rotSlider.onchange = () => { onUpdateWidget({ ...widget, displayJson: { ...disp, rotation: Number(rotSlider.value) } }); };

    styleSec.querySelector('#w-colour').onchange = (e) => {
      onUpdateWidget({ ...widget, displayJson: { ...disp, colour: e.target.value } });
    };
    styleSec.querySelector('#w-accent').onchange = (e) => {
      onUpdateWidget({ ...widget, displayJson: { ...disp, useAccentColor: e.target.checked } });
    };

    const outlineCheckbox = styleSec.querySelector('#w-outline');
    const outlineDetails = styleSec.querySelector('#outline-details');
    outlineCheckbox.onchange = (e) => {
      outlineDetails.hidden = !e.target.checked;
      onUpdateWidget({ ...widget, displayJson: { ...disp, textOutline: e.target.checked } });
    };
    styleSec.querySelector('#w-outline-color').onchange = (e) => {
      onUpdateWidget({ ...widget, displayJson: { ...disp, textOutlineColor: e.target.value } });
    };
    styleSec.querySelector('#w-outline-style').onchange = (e) => {
      onUpdateWidget({ ...widget, displayJson: { ...disp, textOutlineStyle: e.target.value } });
    };

    styleSec.querySelector('#w-weight').onchange = (e) => {
      onUpdateWidget({ ...widget, displayJson: { ...disp, fontWeight: e.target.value } });
    };
    styleSec.querySelector('#w-class').onchange = (e) => {
      const val = e.target.value.trim();
      const sanitizedClass = /^[a-zA-Z0-9_-]+$/.test(val) ? val : '';
      onUpdateWidget({ ...widget, displayJson: { ...disp, customClass: sanitizedClass } });
    };

    body.append(styleSec);

    
    function showDeleteConfirm() {
      let confirmCard = container.querySelector('.delete-confirm-card');
      if (confirmCard) return;
      confirmCard = document.createElement('div');
      confirmCard.className = 'delete-confirm-card';
      confirmCard.innerHTML = `
        <p>${t('confirmRemoveWidget', '确定移除此卡片吗？')}</p>
        <div class="inspector-actions">
          <button type="button" class="cancel-btn">${t('cancel', '取消')}</button>
          <button type="button" class="danger confirm-btn">${t('delete', '删除')}</button>
        </div>
      `;
      confirmCard.querySelector('.cancel-btn').onclick = () => confirmCard.remove();
      confirmCard.querySelector('.confirm-btn').onclick = () => {
        confirmCard.remove();
        if (isEditingPosition) {
          isEditingPosition = false;
          onPositionEditChange?.(null);
        }
        onRemoveWidget(widget.id);
      };
      container.prepend(confirmCard);
    }

    container.append(header, body);
  }

  return { render };
}
