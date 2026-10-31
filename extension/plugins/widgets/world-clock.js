/**
 * 世界时钟：并排显示最多 6 个时区，30 秒刷新一次。
 * 时间格式化使用 Intl，不依赖网络；城市列表是固定事实数据。
 */

import {
  cardMeta,
  createCardShell,
  zonedClock,
} from '../../space-kit/index.js';
import { subscribeWidgetClock } from '../widget-clock.js';

const KEY = 'widget/worldClock';

const CITIES = [
  { zone: 'Asia/Shanghai', zh: '北京', en: 'Beijing' },
  { zone: 'Asia/Tokyo', zh: '东京', en: 'Tokyo' },
  { zone: 'Asia/Singapore', zh: '新加坡', en: 'Singapore' },
  { zone: 'Europe/London', zh: '伦敦', en: 'London' },
  { zone: 'Europe/Paris', zh: '巴黎', en: 'Paris' },
  { zone: 'America/New_York', zh: '纽约', en: 'New York' },
  { zone: 'America/Los_Angeles', zh: '洛杉矶', en: 'Los Angeles' },
  { zone: 'Australia/Sydney', zh: '悉尼', en: 'Sydney' },
];

const DEFAULT_ZONES = ['Asia/Shanghai', 'America/New_York', 'Europe/London'];
const MAX_ZONES = 6;

function normalizeZones(raw) {
  const known = new Set(CITIES.map((city) => city.zone));
  const values = (Array.isArray(raw) ? raw : []).filter((zone) => known.has(zone)).slice(0, MAX_ZONES);
  return values.length ? values : [...DEFAULT_ZONES];
}

function cityOf(zone) {
  return CITIES.find((city) => city.zone === zone);
}

export const worldClockWidget = {
  key: KEY,
  name: 'World Clock',
  defaultData: { zones: [...DEFAULT_ZONES] },

  render(container, data = {}, display = {}, { t = (key, fallback) => fallback || key, lang = 'zh_CN' } = {}) {
    const zones = normalizeZones(data.zones);
    const shell = createCardShell({
      title: t('worldClockTitle', '世界时钟'),
      icon: cardMeta(KEY)?.icon || 'globe',
      className: 'space-card-world-clock',
    });
    container.replaceChildren(shell.root);

    const draw = () => {
      const rows = zones.map((zone) => {
        const city = cityOf(zone);
        const { time, offset } = zonedClock(new Date(), zone);
        const row = document.createElement('div');
        row.className = 'space-clock-row';

        const name = document.createElement('span');
        name.className = 'space-clock-city';
        name.textContent = lang === 'zh_CN' ? city.zh : city.en;

        const delta = document.createElement('span');
        delta.className = 'space-clock-offset';
        delta.textContent = offset > 0
          ? t('worldClockTomorrow', '明天')
          : offset < 0
            ? t('worldClockYesterday', '昨天')
            : '';

        const value = document.createElement('span');
        value.className = 'space-clock-time';
        value.textContent = time;

        row.append(name, delta, value);
        return row;
      });
      shell.body.replaceChildren(...rows);
    };

    draw();
    shell.setFooter(t('worldClockHint', '本地时间之外的常用时区'));
    const unsubscribe = subscribeWidgetClock(draw, 30000);
    return () => {
      unsubscribe();
      container.replaceChildren();
    };
  },

  renderSettings(container, data = {}, onChange = () => {}, { t = (key, fallback) => fallback || key, lang = 'zh_CN' } = {}) {
    container.replaceChildren();
    const zones = normalizeZones(data.zones);
    const wrap = document.createElement('div');
    wrap.className = 'inspector-field-group';

    const title = document.createElement('div');
    title.className = 'inspector-notice';
    title.textContent = t('worldClockZones', '显示城市（最多 6 个）');
    wrap.append(title);

    const inputs = new Map();
    for (const city of CITIES) {
      const label = document.createElement('label');
      label.className = 'inspector-checkbox';
      const input = document.createElement('input');
      input.type = 'checkbox';
      input.checked = zones.includes(city.zone);
      inputs.set(city.zone, input);
      input.onchange = () => {
        const selected = CITIES
          .filter((entry) => inputs.get(entry.zone)?.checked)
          .map((entry) => entry.zone);
        // 至少保留一个城市，最多 6 个，避免空卡片或溢出的时钟行。
        if (!selected.length || selected.length > MAX_ZONES) {
          input.checked = !input.checked;
          return;
        }
        onChange({ ...data, zones: selected });
      };
      const text = document.createElement('span');
      text.textContent = lang === 'zh_CN' ? city.zh : city.en;
      label.append(input, text);
      wrap.append(label);
    }

    const notice = document.createElement('div');
    notice.className = 'inspector-notice';
    notice.textContent = t('worldClockMax', '已选城市按固定顺序展示。');
    wrap.append(notice);
    container.append(wrap);
  },

  styles: '',
};
