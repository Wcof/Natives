/**
 * Message Widget.
 */

import { escapeHtml } from '../sanitizer.js';

export const messageWidget = {
  key: 'widget/message',
  name: 'Message',
  defaultData: { message: 'Hello World' },
  render(container, data) {
    container.className = 'Widget Message';
    container.textContent = data.message || '';
  },
  renderSettings(container, data, onChange, { t = (key, fallback) => fallback || key } = {}) {
    container.innerHTML = `
      <div class="inspector-field-group">
        <label class="inspector-field"><span>${t('message', '消息')}</span><input type="text" value="${escapeHtml(data.message || '')}" /></label>
      </div>
    `;
    container.querySelector('input').onchange = (e) => onChange({ ...data, message: e.target.value });
  },
  styles: `
    .Message { line-height: 1.2; font-size: 1.2em; text-align: center; }
  `,
};
