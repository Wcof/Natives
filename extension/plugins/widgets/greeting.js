/**
 * Greeting Widget.
 * Formats time-of-day greetings with user name (Tabliss Greeting).
 */

import { escapeHtml } from '../sanitizer.js';

export const greetingWidget = {
  key: 'widget/greeting',
  name: 'Greeting',
  defaultData: { name: '' },
  render(container, data, display, { t } = {}) {
    container.classList.add('Greeting');
    const hour = new Date().getHours();
    const prefix = hour < 12
      ? (t ? t('greetingMorning', '早上好') : '早上好')
      : hour < 18
      ? (t ? t('greetingAfternoon', '下午好') : '下午好')
      : (t ? t('greetingEvening', '晚上好') : '晚上好');
    container.textContent = data.name ? `${prefix}，${data.name}` : prefix;
  },
  renderSettings(container, data, onChange, { t = (key, fallback) => fallback || key } = {}) {
    container.innerHTML = `
      <label class="inspector-field"><span>${t('name', '称呼')}</span><input type="text" value="${escapeHtml(data.name || '')}" placeholder="${t('yourName', '你的名字')}" /></label>
    `;
    container.querySelector('input').onchange = (e) => onChange({ ...data, name: e.target.value.trim() });
  },
  styles: `
    .Greeting { line-height: 1.2; font-weight: 300; }
  `,
};
