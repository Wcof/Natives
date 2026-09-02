import { escapeHtml } from '../sanitizer.js';

const DEFAULT_TIME = Date.now();

export const timeTrackerWidget = {
  key: 'widget/timeTracker',
  name: 'Time Tracker',
  defaultData: {
    time: DEFAULT_TIME,
    title: '',
    showCompletedMessage: true,
    completedMessage: '',
    displayMode: 'compact',
    italicizeTime: false,
  },
  render(container, data = {}, display = {}, { t = (key, fallback) => fallback || key } = {}) {
    container.className = 'Widget TimeTracker';
    container.replaceChildren();
    const root = document.createElement('div');
    root.className = 'time-tracker-content';
    container.append(root);

    const renderTime = () => {
      const target = Number(data.time) || DEFAULT_TIME;
      const difference = target - Date.now();
      if (difference <= 0 && data.showCompletedMessage !== false) {
        root.innerHTML = `<div class="completed">${escapeHtml(data.completedMessage || t('eventHasArrived', '目标时间已到'))}</div>`;
        return;
      }

      const title = data.title ? `<span class="title">${escapeHtml(data.title)}</span>` : '';
      const italic = data.italicizeTime ? ' italic-time' : '';
      if (data.displayMode !== 'detailed') {
        const { value, unit } = relativeUnit(difference);
        const relative = new Intl.RelativeTimeFormat(undefined, { numeric: 'auto' }).format(value, unit);
        root.innerHTML = `<h3>${title}${title ? ' ' : ''}<span class="${italic.trim()}">${escapeHtml(relative)}</span></h3>`;
        return;
      }

      const secondsTotal = Math.floor(Math.abs(difference) / 1000);
      const parts = [
        [Math.floor(secondsTotal / 86400), t('days', '天')],
        [Math.floor((secondsTotal % 86400) / 3600), t('hours', '小时')],
        [Math.floor((secondsTotal % 3600) / 60), t('minutes', '分钟')],
        [secondsTotal % 60, t('seconds', '秒')],
      ];
      root.innerHTML = `<div class="detailed">${title ? `<h3 class="title">${escapeHtml(data.title)}</h3>` : ''}<div class="time-info"><div class="time-components">${parts.map(([value, unit]) => `<span class="time-component"><span class="value">${value}</span><span class="unit${italic}">${escapeHtml(unit)}</span></span>`).join('')}</div></div></div>`;
    };

    renderTime();
    const intervalId = setInterval(renderTime, 1000);
    return () => {
      clearInterval(intervalId);
      container.replaceChildren();
    };
  },
  renderSettings(container, data = {}, onChange = () => {}, { t = (key, fallback) => fallback || key } = {}) {
    container.replaceChildren();
    const wrap = document.createElement('div');
    wrap.className = 'inspector-field-group';
    wrap.innerHTML = `
      <label class="inspector-field"><span>${t('eventDateTime', '目标日期和时间')}</span><input id="tt-time" type="datetime-local" value="${toLocalDateTime(Number(data.time) || DEFAULT_TIME)}"></label>
      <label class="inspector-field"><span>${t('eventTitle', '标题（可选）')}</span><input id="tt-title" type="text" value="${escapeHtml(data.title || '')}"></label>
      <label class="inspector-field"><span>${t('displayMode', '显示模式')}</span><select id="tt-mode"><option value="compact" ${data.displayMode !== 'detailed' ? 'selected' : ''}>${t('compact', '紧凑')}</option><option value="detailed" ${data.displayMode === 'detailed' ? 'selected' : ''}>${t('detailed', '详细')}</option></select></label>
      <label class="inspector-checkbox"><input id="tt-completed" type="checkbox" ${data.showCompletedMessage !== false ? 'checked' : ''}><span>${t('showCompletionMessage', '到期后显示完成信息')}</span></label>
      <label class="inspector-field"><span>${t('completionMessage', '完成信息')}</span><input id="tt-message" type="text" value="${escapeHtml(data.completedMessage || '')}"></label>
      <label class="inspector-checkbox"><input id="tt-italic" type="checkbox" ${data.italicizeTime ? 'checked' : ''}><span>${t('italicizeTime', '时间使用斜体')}</span></label>
    `;
    wrap.querySelector('#tt-time').onchange = (event) => onChange({ ...data, time: new Date(event.target.value).getTime() });
    wrap.querySelector('#tt-title').onchange = (event) => onChange({ ...data, title: event.target.value });
    wrap.querySelector('#tt-mode').onchange = (event) => onChange({ ...data, displayMode: event.target.value });
    wrap.querySelector('#tt-completed').onchange = (event) => onChange({ ...data, showCompletedMessage: event.target.checked });
    wrap.querySelector('#tt-message').onchange = (event) => onChange({ ...data, completedMessage: event.target.value });
    wrap.querySelector('#tt-italic').onchange = (event) => onChange({ ...data, italicizeTime: event.target.checked });
    container.append(wrap);
  },
  styles: `
    .TimeTracker { text-align:center; }
    .TimeTracker .completed { font-size:1.6em; font-weight:bold; margin-top:.5em; }
    .TimeTracker .title { font-weight:bold; }
    .TimeTracker .italic-time { font-style:italic; }
    .TimeTracker .detailed, .TimeTracker .time-info { display:flex; flex-direction:column; align-items:center; }
    .TimeTracker .detailed .title { margin-bottom:.5em; }
    .TimeTracker .time-components { display:flex; flex-wrap:wrap; justify-content:center; gap:.8em; }
    .TimeTracker .time-component { display:flex; flex-direction:column; align-items:center; min-width:60px; }
    .TimeTracker .time-component .value { font-size:1.5em; font-weight:bold; }
    .TimeTracker .time-component .unit { font-size:.8em; opacity:.8; }
  `,
};

function relativeUnit(milliseconds) {
  const absoluteSeconds = Math.abs(milliseconds) / 1000;
  if (absoluteSeconds >= 86400) return { value: Math.round(milliseconds / 86400000), unit: 'day' };
  if (absoluteSeconds >= 3600) return { value: Math.round(milliseconds / 3600000), unit: 'hour' };
  if (absoluteSeconds >= 60) return { value: Math.round(milliseconds / 60000), unit: 'minute' };
  return { value: Math.round(milliseconds / 1000), unit: 'second' };
}

function toLocalDateTime(timestamp) {
  const date = new Date(timestamp - new Date(timestamp).getTimezoneOffset() * 60000);
  return date.toISOString().slice(0, 16);
}
