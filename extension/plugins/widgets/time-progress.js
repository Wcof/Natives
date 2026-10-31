/**
 * 时间进度：今天、本周、本月、今年的流逝比例。
 * 复用文档级共享时钟（plugins/widget-clock.js），不额外创建 setInterval。
 */

import {
  cardMeta,
  createCardShell,
  createProgressRow,
  timeProgress,
} from '../../space-kit/index.js';
import { subscribeWidgetClock } from '../widget-clock.js';

const KEY = 'widget/timeProgress';

function rowsFor(data, t) {
  const progress = timeProgress(new Date());
  const rows = [['progressToday', '今天', progress.day]];
  if (data.showWeek !== false) rows.push(['progressWeek', '本周', progress.week]);
  if (data.showMonth !== false) rows.push(['progressMonth', '本月', progress.month]);
  if (data.showYear !== false) rows.push(['progressYear', '今年', progress.year]);
  return rows.map(([key, fallback, value]) => createProgressRow({
    label: t(key, fallback),
    value,
  }));
}

export const timeProgressWidget = {
  key: KEY,
  name: 'Time Progress',
  defaultData: { showWeek: true, showMonth: true, showYear: true },

  render(container, data = {}, display = {}, { t = (key, fallback) => fallback || key } = {}) {
    const shell = createCardShell({
      title: t('timeProgressTitle', '时间进度'),
      icon: cardMeta(KEY)?.icon || 'gauge',
      className: 'space-card-time-progress',
    });
    container.replaceChildren(shell.root);

    const draw = () => {
      shell.body.replaceChildren(...rowsFor(data, t));
    };
    draw();
    shell.setFooter(t('progressHint', '时间正在流逝'));

    const unsubscribe = subscribeWidgetClock(draw, 60000);
    return () => {
      unsubscribe();
      container.replaceChildren();
    };
  },

  renderSettings(container, data = {}, onChange = () => {}, { t = (key, fallback) => fallback || key } = {}) {
    container.replaceChildren();
    const wrap = document.createElement('div');
    wrap.className = 'inspector-field-group';
    const options = [
      ['showWeek', 'progressShowWeek', '显示本周'],
      ['showMonth', 'progressShowMonth', '显示本月'],
      ['showYear', 'progressShowYear', '显示今年'],
    ];
    for (const [field, key, fallback] of options) {
      const label = document.createElement('label');
      label.className = 'inspector-checkbox';
      const input = document.createElement('input');
      input.type = 'checkbox';
      input.checked = data[field] !== false;
      input.onchange = () => {
        onChange({ ...data, [field]: input.checked });
      };
      const text = document.createElement('span');
      text.textContent = t(key, fallback);
      label.append(input, text);
      wrap.append(label);
    }
    const notice = document.createElement('div');
    notice.className = 'inspector-notice';
    notice.textContent = t('progressSettingsHint', '「今天」始终显示，其余维度可以单独关闭。');
    wrap.append(notice);
    container.append(wrap);
  },

  styles: '',
};
