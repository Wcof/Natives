import { subscribeWidgetClock, relativeTimeArgs } from '../widget-clock.js';
import { escapeHtml } from '../sanitizer.js';

export const sinceWidget = {
  key: 'widget/since',
  name: 'Time Since',
  defaultData: { time: Date.now(), title: 'Since' },
  render(container, data = {}) {
    container.className = 'Widget Since';
    const update = () => {
      const target = Number(data.time) || (data.sinceDate ? new Date(data.sinceDate).getTime() : Date.now());
      const difference = target - Date.now();
      const relative = new Intl.RelativeTimeFormat(undefined, { numeric: 'auto' }).format(...relativeTimeArgs(difference));
      container.replaceChildren();
      const heading = document.createElement('h3');
      const title = document.createElement('span');
      title.textContent = `${data.title || 'Since'} `;
      const relativeTime = document.createElement('span');
      relativeTime.className = 'Since relativeTime';
      relativeTime.textContent = relative;
      heading.append(title, relativeTime);
      container.append(heading);
    };
    update();
    const stopClock = subscribeWidgetClock(update, 1000);
    return () => stopClock();
  },
  renderSettings(container, data = {}, onChange = () => {}, { t = (key, fallback) => fallback || key } = {}) {
    container.innerHTML = `<div class="inspector-field-group">
      <label class="inspector-field"><span>${t('eventTitle', '事件名称')}</span><input id="sc-title" type="text" value="${escapeHtml(data.title || '')}"></label>
      <label class="inspector-field"><span>${t('eventDateTime', '日期和时间')}</span><input id="sc-time" type="datetime-local" value="${toLocalInput(Number(data.time) || Date.now())}"></label>
    </div>`;
    const update = () => onChange({
      ...data,
      title: container.querySelector('#sc-title').value,
      time: new Date(container.querySelector('#sc-time').value).getTime(),
    });
    container.querySelectorAll('input').forEach((input) => { input.onchange = update; });
  },
  styles: '.Since.relativeTime { font-style:italic; }',
};

function toLocalInput(timestamp) {
  const date = new Date(timestamp - new Date(timestamp).getTimezoneOffset() * 60000);
  return date.toISOString().slice(0, 16);
}
