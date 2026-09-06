

import { escapeHtml } from '../sanitizer.js';

export const cssWidget = {
  key: 'widget/css',
  name: 'Custom CSS',
  defaultData: { css: '' },
  render(container, data = {}, display = {}, { shadowRoot, t = (k, f) => f || k } = {}) {
    container.className = 'Widget CustomCSS';
    container.style.display = 'none';
    if (!shadowRoot) return () => {};

    let styleTag = shadowRoot.querySelector('style#custom-css-widget');
    if (!styleTag) {
      styleTag = document.createElement('style');
      styleTag.id = 'custom-css-widget';
      shadowRoot.append(styleTag);
    }
    styleTag.textContent = data.css || '';

    return () => {
      styleTag?.remove();
    };
  },
  renderSettings(container, data = {}, onChange = () => {}, { t = (k, f) => f || k } = {}) {
    container.replaceChildren();
    const wrap = document.createElement('div');
    wrap.className = 'inspector-field-group';
    wrap.innerHTML = `
      <label class="inspector-field">
        <span>${t('customCssLabel', 'CSS (仅作用于个人空间画布)')}</span>
        <textarea rows="5" placeholder="/* custom css */">${escapeHtml(data.css || '')}</textarea>
      </label>
    `;
    wrap.querySelector('textarea').onchange = (e) => onChange({ ...data, css: e.target.value });
    container.append(wrap);
  },
};
