import { escapeHtml } from '../sanitizer.js';

export const countdownWidget = {
  key: 'widget/countdown',
  name: 'Countdown',
  defaultData: { time: Date.now(), title: '' },
  render(container, data = {}, display = {}, { t = (key, fallback) => fallback || key } = {}) {
    container.className = 'Widget Countdown';
    const update = () => {
      const target = Number(data.time) || (data.targetDate ? new Date(data.targetDate).getTime() : 0);
      container.replaceChildren();
      if (!target) {
        container.textContent = t('setCountdownDate', '设置目标日期以倒计时');
        return;
      }
      const heading = document.createElement('h3');
      const difference = target - Date.now();
      heading.textContent = difference <= 0
        ? t('countdownComplete', '目标时间已到')
        : new Intl.RelativeTimeFormat(undefined, { numeric: 'auto' }).format(...relativeArgs(difference));
      container.append(heading);
      if (data.title) {
        const title = document.createElement('h4');
        title.textContent = data.title;
        container.append(title);
      }
    };
    update();
    const intervalId = setInterval(update, 1000);
    return () => clearInterval(intervalId);
  },
  renderSettings(container, data = {}, onChange = () => {}, { t = (key, fallback) => fallback || key } = {}) {
    container.innerHTML = `<div class="inspector-field-group">
      <label class="inspector-field"><span>${t('eventTitle', '事件名称')}</span><input id="cd-title" type="text" value="${escapeHtml(data.title || '')}"></label>
      <label class="inspector-field"><span>${t('eventDateTime', '目标日期和时间')}</span><input id="cd-time" type="datetime-local" value="${toLocalInput(Number(data.time) || Date.now())}"></label>
    </div>`;
    const update = () => onChange({
      ...data,
      title: container.querySelector('#cd-title').value,
      time: new Date(container.querySelector('#cd-time').value).getTime(),
    });
    container.querySelectorAll('input').forEach((input) => { input.onchange = update; });
  },
  styles: '',
};

function relativeArgs(milliseconds) {
  const seconds = Math.abs(milliseconds) / 1000;
  if (seconds >= 86400) return [Math.round(milliseconds / 86400000), 'day'];
  if (seconds >= 3600) return [Math.round(milliseconds / 3600000), 'hour'];
  if (seconds >= 60) return [Math.round(milliseconds / 60000), 'minute'];
  return [Math.round(milliseconds / 1000), 'second'];
}

function toLocalInput(timestamp) {
  const date = new Date(timestamp - new Date(timestamp).getTimezoneOffset() * 60000);
  return date.toISOString().slice(0, 16);
}
