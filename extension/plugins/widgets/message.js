

import { escapeHtml } from '../sanitizer.js';

export const messageWidget = {
  key: 'widget/message',
  name: 'Message',
  defaultData: { message: 'Hello World' },
  render(container, data) {
    container.className = 'Widget Message';
    const h3 = document.createElement('h3');
    h3.style.whiteSpace = 'pre';
    h3.textContent = data.message || '';
    container.append(h3);
  },
  renderSettings(container, data, onChange, { t = (key, fallback) => fallback || key } = {}) {
    container.innerHTML = `
      <div class="inspector-field-group">
        <label class="inspector-field"><span>${t('message', '消息')}</span><input type="text" value="${escapeHtml(data.message || '')}" /></label>
      </div>
    `;
    container.querySelector('input').onchange = (e) => onChange({ ...data, message: e.target.value });
  },
  styles: '',
};
