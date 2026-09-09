

import { escapeHtml } from '../sanitizer.js';

export const greetingWidget = {
  key: 'widget/greeting',
  name: 'Greeting',
  defaultData: { name: '' },
  render(container, data, display, { t } = {}) {
    container.classList.add('Greeting');
    container.replaceChildren();
    const hour = new Date().getHours();
    const prefix = hour < 12
      ? (t ? t('greetingMorning', '早上好') : '早上好')
      : hour < 18
      ? (t ? t('greetingAfternoon', '下午好') : '下午好')
      : (t ? t('greetingEvening', '晚上好') : '晚上好');
    const h2 = document.createElement('h2');
    h2.textContent = data.name ? `${prefix}，${data.name}` : prefix;
    container.append(h2);
  },
  renderSettings(container, data, onChange, { t = (key, fallback) => fallback || key } = {}) {
    container.innerHTML = `
      <label class="inspector-field"><span>${t('name', '称呼')}</span><input type="text" value="${escapeHtml(data.name || '')}" placeholder="${t('yourName', '你的名字')}" /></label>
    `;
    container.querySelector('input').onchange = (e) => onChange({ ...data, name: e.target.value.trim() });
  },
  styles: '',
};
