

import { BACKGROUND_KEYS, backgroundPlugins, pluginName } from './space-plugins.js';

export function createSpaceBackgroundSettings({
  t,
  language = 'zh_CN',
  onUpdateBackground,
  onBackToOverview,
}) {
  function render(container, snapshot, workspaceId) {
    const bgData = snapshot?.backgroundJson || { key: 'background/colour', display: { colour: '#101010' } };
    const currentKey = bgData.key || 'background/colour';
    const currentPlugin = backgroundPlugins[currentKey] || backgroundPlugins['background/colour'];
    const currentDisplay = bgData.display || bgData.data || {};

    container.replaceChildren();

    
    const header = document.createElement('div');
    header.className = 'inspector-heading';
    header.innerHTML = `
      <div class="inspector-heading-main">
        <button class="inspector-back" type="button"><svg class="icon"><use href="#i-chevron-left" /></svg><span>${t('back', '返回')}</span></button>
        <h2>${t('backgroundSettings', '背景设置')}</h2>
      </div>
    `;
    header.querySelector('.inspector-back').onclick = () => onBackToOverview();

    
    const body = document.createElement('div');
    body.className = 'inspector-body';

    
    const typeSec = document.createElement('div');
    typeSec.className = 'inspector-section';
    typeSec.innerHTML = `
      <h3>${t('backgroundSource', '背景源')}</h3>
      <div class="inspector-field-group">
        <label class="inspector-field">
          <span>${t('type', '类型')}</span>
          <select id="bg-type-select">
            ${BACKGROUND_KEYS.map((k) => `
              <option value="${k}" ${k === currentKey ? 'selected' : ''}>
                ${pluginName(k, language, backgroundPlugins[k]?.name || k)}
              </option>
            `).join('')}
          </select>
        </label>
      </div>
    `;

    typeSec.querySelector('#bg-type-select').onchange = (e) => {
      const nextKey = e.target.value;
      const nextPlugin = backgroundPlugins[nextKey];
      const nextData = {
        key: nextKey,
        display: { ...(nextPlugin?.defaultData || {}) },
      };
      onUpdateBackground(nextData);
    };
    body.append(typeSec);

    
    if (currentPlugin?.renderSettings) {
      const pluginSec = document.createElement('div');
      pluginSec.className = 'inspector-section';
      pluginSec.innerHTML = `<h3>${t('sourceSettings', '源配置')}</h3><div class="bg-plugin-host"></div>`;
      currentPlugin.renderSettings(
        pluginSec.querySelector('.bg-plugin-host'),
        currentDisplay,
        (nextDisplay) => {
          onUpdateBackground({ ...bgData, display: nextDisplay });
        },
        { t, language },
      );
      body.append(pluginSec);
    }

    
    if (currentKey !== 'background/colour') {
      const filterSec = document.createElement('div');
      filterSec.className = 'inspector-section';
      filterSec.innerHTML = `
        <h3>${t('visualEffects', '视觉效果')}</h3>
        <div class="inspector-field-group">
          <div class="inspector-field-range">
            <div class="inspector-field-range-header"><span>${t('blur', '模糊度')}</span><span id="bg-blur-val">${currentDisplay.blur || 0}px</span></div>
            <input type="range" id="bg-blur" min="0" max="40" value="${currentDisplay.blur || 0}" />
          </div>
          <div class="inspector-field-range">
            <div class="inspector-field-range-header"><span>${t('brightness', '亮度')}</span><span id="bg-bright-val">${Math.round((currentDisplay.brightness ?? 1) * 100)}%</span></div>
            <input type="range" id="bg-bright" min="20" max="150" value="${Math.round((currentDisplay.brightness ?? 1) * 100)}" />
          </div>
          <label class="inspector-checkbox">
            <input type="checkbox" id="bg-night" ${currentDisplay.nightDim ? 'checked' : ''} />
            <span>${t('nightDim', '夜间自动调暗')}</span>
          </label>
        </div>
      `;

      const blurInp = filterSec.querySelector('#bg-blur');
      const blurVal = filterSec.querySelector('#bg-blur-val');
      blurInp.oninput = () => { blurVal.textContent = `${blurInp.value}px`; };
      blurInp.onchange = () => {
        onUpdateBackground({ ...bgData, display: { ...currentDisplay, blur: Number(blurInp.value) } });
      };

      const brightInp = filterSec.querySelector('#bg-bright');
      const brightVal = filterSec.querySelector('#bg-bright-val');
      brightInp.oninput = () => { brightVal.textContent = `${brightInp.value}%`; };
      brightInp.onchange = () => {
        onUpdateBackground({ ...bgData, display: { ...currentDisplay, brightness: Number(brightInp.value) / 100 } });
      };

      filterSec.querySelector('#bg-night').onchange = (e) => {
        onUpdateBackground({ ...bgData, display: { ...currentDisplay, nightDim: e.target.checked } });
      };

      body.append(filterSec);
    }

    container.append(header, body);
  }

  return { render };
}
