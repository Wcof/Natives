

import { escapeHtml } from '../sanitizer.js';
import { renderActivityCalendar, activityCalendarStyles } from './activity-calendar.js';

export const githubWidget = {
  key: 'widget/github',
  name: 'GitHub',
  defaultData: {
    username: '',
    theme: 'green',
    showStreak: true,
  },
  render(container, data = {}, display = {}, { t = (k, f) => f || k } = {}) {
    const user = (data.username || '').trim();
    container.className = 'Widget GitHub';
    container.replaceChildren();

    if (!user) {
      const promptCard = document.createElement('div');
      promptCard.className = 'activity-calendar-wrap github-unconfigured';
      promptCard.innerHTML = `
        <div style="display:flex;align-items:center;justify-content:center;gap:8px;padding:16px 20px;text-align:center;">
          <svg class="icon" style="width:20px;height:20px;flex-shrink:0;"><use href="#i-box" /></svg>
          <span style="font-size:13px;font-weight:500;">${t('githubPromptUsername', '设置 GitHub 用户名后显示贡献日历')}</span>
        </div>
      `;
      container.append(promptCard);
      return () => container.replaceChildren();
    }

    const cardContainer = document.createElement('div');
    container.append(cardContainer);

    let cancelled = false;

    async function loadData() {
      try {
        const res = await fetch(`https://github-contributions-api.jogruber.de/v4/${encodeURIComponent(user)}?y=last`);
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        const json = await res.json();
        if (cancelled) return;

        const total = json.total?.[new Date().getFullYear()] || json.total?.lastYear || 0;
        const contributions = json.contributions || [];

        renderActivityCalendar({
          container: cardContainer,
          data: contributions,
          totalCount: total,
          totalLabel: t('contributionsLastYear', 'contributions in the last year'),
          theme: data.theme || 'green',
          profileUrl: `https://github.com/${encodeURIComponent(user)}`,
          t,
        });
      } catch (err) {
        if (cancelled) return;
        
        renderActivityCalendar({
          container: cardContainer,
          data: [],
          totalCount: 0,
          totalLabel: t('contributionsLastYear', 'contributions in the last year'),
          theme: data.theme || 'green',
          profileUrl: `https://github.com/${encodeURIComponent(user)}`,
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
        <input type="text" id="gh-user" value="${escapeHtml(data.username || '')}" placeholder="octocat" />
      </label>
      <label class="inspector-field">
        <span>${t('theme', '主题色彩')}</span>
        <select id="gh-theme">
          <option value="green" ${data.theme === 'green' ? 'selected' : ''}>${t('colorGreen', '经典绿')}</option>
          <option value="blue" ${data.theme === 'blue' ? 'selected' : ''}>${t('colorBlue', '天空蓝')}</option>
          <option value="orange" ${data.theme === 'orange' ? 'selected' : ''}>${t('colorOrange', '活力橙')}</option>
          <option value="purple" ${data.theme === 'purple' ? 'selected' : ''}>${t('colorPurple', '极光紫')}</option>
        </select>
      </label>
      <label class="inspector-checkbox">
        <input type="checkbox" id="gh-streak" ${data.showStreak !== false ? 'checked' : ''} />
        <span>${t('showStreak', '显示连续提交天数 (Streak)')}</span>
      </label>
    `;
    wrap.querySelector('#gh-user').onchange = (e) => onChange({ ...data, username: e.target.value.trim() });
    wrap.querySelector('#gh-theme').onchange = (e) => onChange({ ...data, theme: e.target.value });
    wrap.querySelector('#gh-streak').onchange = (e) => onChange({ ...data, showStreak: e.target.checked });
    container.append(wrap);
  },
  styles: activityCalendarStyles,
};
