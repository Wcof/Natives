/**
 * Custom Text Widget.
 */

import { escapeHtml } from '../sanitizer.js';

export const customTextWidget = {
  key: 'widget/customText',
  name: 'Custom Text',
  defaultData: { text: 'Custom Text' },
  render(container, data) {
    container.className = 'Widget CustomText';
    const h3 = document.createElement('h3');
    h3.textContent = data.text || '';
    container.append(h3);
  },
  renderSettings(container, data, onChange, { t = (key, fallback) => fallback || key } = {}) {
    container.innerHTML = `
      <div class="inspector-field-group">
        <label class="inspector-field"><span>${t('text', '文本')}</span><textarea rows="3">${escapeHtml(data.text || '')}</textarea></label>
      </div>
    `;
    container.querySelector('textarea').onchange = (e) => onChange({ ...data, text: e.target.value });
  },
  styles: `

  `,
};
