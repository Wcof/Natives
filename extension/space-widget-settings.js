/**
 * Widget Display and Plugin Settings Controller (<280 lines).
 * Full migration of Tabliss Widget.tsx and WidgetDisplay.tsx.
 */

import { widgetPlugins, pluginName, POSITIONS, escapeHtml } from './space-plugins.js';

export function createSpaceWidgetSettings({
  t,
  language = 'zh_CN',
  onUpdateWidget,
  onRemoveWidget,
  onReorderWidget,
  onBackToOverview,
}) {
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

    // Header
    const header = document.createElement('div');
    header.className = 'inspector-heading';
    header.innerHTML = `
      <div style="display:flex;align-items:center;gap:8px;">
        <button class="inspector-back" type="button"><svg class="icon"><use href="#i-chevron-left" /></svg><span>${t('back', '返回')}</span></button>
        <h2>${name}</h2>
      </div>
      <button class="icon-button widget-delete-trigger" title="${t('deleteWidget', '删除组件')}" aria-label="${t('deleteWidget', '删除组件')}">
        <svg class="icon" style="color:var(--danger)"><use href="#i-trash" /></svg>
      </button>
    `;
    header.querySelector('.inspector-back').onclick = () => onBackToOverview();
    header.querySelector('.widget-delete-trigger').onclick = () => showDeleteConfirm();

    // Body
    const body = document.createElement('div');
    body.className = 'inspector-body';

    // 1. Plugin-specific settings section
    if (plugin?.renderSettings) {
      const pluginSec = document.createElement('div');
      pluginSec.className = 'inspector-section';
      pluginSec.innerHTML = `<h3>${t('widgetCustomSettings', '组件设置')}</h3><div class="plugin-settings-host"></div>`;
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

    // 2. Position & Layout section
    const posSec = document.createElement('div');
    posSec.className = 'inspector-section';
    posSec.innerHTML = `
      <h3>${t('positionAndLayout', '位置与布局')}</h3>
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
      <div id="free-pos-fields" style="${disp.position === 'free' ? 'display:grid;gap:6px;' : 'display:none;'}">
        <label class="inspector-field"><span>X (%)</span><input type="number" id="w-x" min="0" max="100" value="${disp.xPercent ?? 50}" /></label>
        <label class="inspector-field"><span>Y (%)</span><input type="number" id="w-y" min="0" max="100" value="${disp.yPercent ?? 50}" /></label>
        <label class="inspector-field"><span>缩放</span><input type="number" id="w-scale" min="0.1" max="3" step="0.1" value="${disp.scale ?? 1}" /></label>
        <label class="inspector-field"><span>旋转 (°)</span><input type="number" id="w-rot" min="-180" max="180" value="${disp.rotation ?? 0}" /></label>
      </div>
    `;

    const posSelect = posSec.querySelector('#w-position');
    const freeFields = posSec.querySelector('#free-pos-fields');
    posSelect.onchange = () => {
      const isFree = posSelect.value === 'free';
      freeFields.style.display = isFree ? 'grid' : 'none';
      onUpdateWidget({ ...widget, displayJson: { ...disp, position: posSelect.value } });
    };

    const updateFreeFields = () => {
      onUpdateWidget({
        ...widget,
        displayJson: {
          ...disp,
          position: 'free',
          xPercent: Number(posSec.querySelector('#w-x').value) || 50,
          yPercent: Number(posSec.querySelector('#w-y').value) || 50,
          scale: Number(posSec.querySelector('#w-scale').value) || 1,
          rotation: Number(posSec.querySelector('#w-rot').value) || 0,
        },
      });
    };
    posSec.querySelectorAll('#free-pos-fields input').forEach((inp) => {
      inp.onchange = updateFreeFields;
    });

    body.append(posSec);

    // 3. Typography & Styling section
    const styleSec = document.createElement('div');
    styleSec.className = 'inspector-section';
    styleSec.innerHTML = `
      <h3>${t('typographyAndStyle', '字体与样式')}</h3>
      <label class="inspector-field"><span>${t('fontSize', '字号')}</span><input type="range" id="w-fontsize" min="10" max="120" value="${disp.fontSize || 32}" /><span id="w-fontsize-val">${disp.fontSize || 32}px</span></label>
      <label class="inspector-field"><span>${t('textColor', '文本颜色')}</span><input type="color" id="w-colour" value="${disp.colour || '#ffffff'}" /></label>
      <label class="inspector-checkbox"><input type="checkbox" id="w-accent" ${disp.useAccentColor ? 'checked' : ''} /><span>${t('useAccentColor', '使用强调色')}</span></label>
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
    `;

    const fontSlider = styleSec.querySelector('#w-fontsize');
    const fontVal = styleSec.querySelector('#w-fontsize-val');
    fontSlider.oninput = () => { fontVal.textContent = `${fontSlider.value}px`; };
    fontSlider.onchange = () => { onUpdateWidget({ ...widget, displayJson: { ...disp, fontSize: Number(fontSlider.value) } }); };

    styleSec.querySelector('#w-colour').onchange = (e) => {
      onUpdateWidget({ ...widget, displayJson: { ...disp, colour: e.target.value } });
    };
    styleSec.querySelector('#w-accent').onchange = (e) => {
      onUpdateWidget({ ...widget, displayJson: { ...disp, useAccentColor: e.target.checked } });
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

    // Delete Confirmation Card (Inline)
    function showDeleteConfirm() {
      let confirmCard = container.querySelector('.delete-confirm-card');
      if (confirmCard) return;
      confirmCard = document.createElement('div');
      confirmCard.className = 'delete-confirm-card';
      confirmCard.innerHTML = `
        <p>${t('confirmRemoveWidget', '确定移除此组件吗？')}</p>
        <div style="display:flex;gap:6px;justify-content:flex-end;">
          <button type="button" class="cancel-btn">${t('cancel', '取消')}</button>
          <button type="button" class="danger confirm-btn">${t('delete', '删除')}</button>
        </div>
      `;
      confirmCard.querySelector('.cancel-btn').onclick = () => confirmCard.remove();
      confirmCard.querySelector('.confirm-btn').onclick = () => {
        confirmCard.remove();
        onRemoveWidget(widget.id);
      };
      container.prepend(confirmCard);
    }

    container.append(header, body);
  }

  return { render };
}
