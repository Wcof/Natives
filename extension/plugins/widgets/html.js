

import { escapeHtml, sanitizeHtml } from '../sanitizer.js';

export const htmlWidget = {
  key: 'widget/html',
  name: 'Custom HTML',
  defaultData: { html: '<div>Hello HTML</div>' },
  render(container, data) {
    container.className = 'Widget CustomHTML';
    container.innerHTML = sanitizeHtml(data.html || '');
  },
  renderSettings(container, data, onChange, { t = (key, fallback) => fallback || key } = {}) {
    container.innerHTML = `
      <div class="inspector-field-group">
        <label class="inspector-field"><span>${t('safeHtml', 'HTML（安全清洗）')}</span><textarea rows="4">${escapeHtml(data.html || '')}</textarea></label>
      </div>
    `;
    container.querySelector('textarea').onchange = (e) => onChange({ ...data, html: e.target.value });
  },
};
