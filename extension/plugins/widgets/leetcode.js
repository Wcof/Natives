/** LeetCode activity calendar widget. */

import { escapeHtml } from '../sanitizer.js';
import { renderActivityCalendar } from './activity-calendar.js';

export const leetcodeWidget = {
  key: 'widget/leetcode',
  name: 'LeetCode',
  defaultData: {
    username: '',
    theme: 'orange',
    showStreak: true,
  },
  render(container, data = {}, display = {}, { t = (k, f) => f || k } = {}) {
    const user = (data.username || '').trim();
    container.className = 'Widget LeetCode';
    container.replaceChildren();

    if (!user) {
      return () => container.replaceChildren();
    }

    const cardContainer = document.createElement('div');
    container.append(cardContainer);

    let cancelled = false;

    async function loadData() {
      try {
        const res = await fetch(`https://alfa-leetcode-api.onrender.com/userProfileCalendar?username=${encodeURIComponent(user)}`);
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        const json = await res.json();
        if (cancelled) return;

        let totalSubmissions = 0;
        const submissions = [];

        if (json.submissionCalendar) {
          const rawCal = typeof json.submissionCalendar === 'string' ? JSON.parse(json.submissionCalendar) : json.submissionCalendar;
          for (const [timestamp, count] of Object.entries(rawCal)) {
            const dateStr = new Date(Number(timestamp) * 1000).toISOString().slice(0, 10);
            const num = Number(count) || 0;
            totalSubmissions += num;
            submissions.push({ date: dateStr, count: num });
          }
        }

        renderActivityCalendar({
          container: cardContainer,
          data: submissions,
          totalCount: totalSubmissions,
          totalLabel: t('submissionsLastYear', 'submissions in the last year'),
          theme: data.theme || 'orange',
          profileUrl: `https://leetcode.com/${encodeURIComponent(user)}`,
          t,
        });
      } catch (err) {
        if (cancelled) return;
        renderActivityCalendar({
          container: cardContainer,
          data: [],
          totalCount: 0,
          totalLabel: t('submissionsLastYear', 'submissions in the last year'),
          theme: data.theme || 'orange',
          profileUrl: `https://leetcode.com/${encodeURIComponent(user)}`,
          t,
        });
      }
    }

    loadData();

    return () => {
      cancelled = true;
      container.replaceChildren();
    };
  },
  renderSettings(container, data = {}, onChange = () => {}, { t = (k, f) => f || k } = {}) {
    container.replaceChildren();
    const wrap = document.createElement('div');
    wrap.className = 'inspector-field-group';
    wrap.innerHTML = `
      <label class="inspector-field">
        <span>${t('username', '用户名')}</span>
        <input type="text" id="lc-user" value="${escapeHtml(data.username || '')}" placeholder="username" />
      </label>
      <label class="inspector-field">
        <span>${t('theme', '主题色彩')}</span>
        <select id="lc-theme">
          <option value="orange" ${data.theme === 'orange' ? 'selected' : ''}>${t('colorOrange', '力扣橙')}</option>
          <option value="gold" ${data.theme === 'gold' ? 'selected' : ''}>${t('colorGold', '琥珀金')}</option>
          <option value="green" ${data.theme === 'green' ? 'selected' : ''}>${t('colorGreen', '极客绿')}</option>
          <option value="blue" ${data.theme === 'blue' ? 'selected' : ''}>${t('colorBlue', '科技蓝')}</option>
        </select>
      </label>
      <label class="inspector-checkbox">
        <input type="checkbox" id="lc-streak" ${data.showStreak !== false ? 'checked' : ''} />
        <span>${t('showStreak', '显示连续提交天数 (Streak)')}</span>
      </label>
    `;
    wrap.querySelector('#lc-user').onchange = (e) => onChange({ ...data, username: e.target.value.trim() });
    wrap.querySelector('#lc-theme').onchange = (e) => onChange({ ...data, theme: e.target.value });
    wrap.querySelector('#lc-streak').onchange = (e) => onChange({ ...data, showStreak: e.target.checked });
    container.append(wrap);
  },
  styles: '',
};
