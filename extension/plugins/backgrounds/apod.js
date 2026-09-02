/**
 * NASA Astronomy Picture of the Day Background.
 */

import { fetchDedup } from '../plugins-cache.js';
import { escapeHtml } from '../sanitizer.js';

export const apodBackground = {
  key: 'background/apod',
  name: 'NASA APOD',
  defaultData: { apiKey: '' },
  render(container, data) {
    const key = data.apiKey || 'DEMO_KEY';
    container.style.backgroundColor = 'transparent';

    fetchDedup(
      `apod_wallpaper_${key}`,
      async () => {
        const res = await fetch(`https://api.nasa.gov/planetary/apod?api_key=${encodeURIComponent(key)}`);
        const json = await res.json();
        const url = json.hdurl || json.url;
        if (url && (json.media_type === 'image' || !json.media_type)) {
          return { url };
        }
        if (json.error) {
          throw new Error(json.error.message || 'API Key 超限');
        }
        return null;
      },
      4 * 60 * 60 * 1000,
    )
      .then((cached) => {
        if (cached?.url) {
          container.replaceChildren();
          container.style.backgroundColor = 'transparent';
          container.style.backgroundImage = `url("${cached.url}")`;
          container.style.backgroundSize = 'cover';
          container.style.backgroundPosition = 'center';
        }
      })
      .catch((err) => {
        if (!container.style.backgroundImage || container.style.backgroundImage === 'none') {
          container.innerHTML = `<div class="background-not-configured">NASA APOD: ${escapeHtml(err.message || '获取失败')}</div>`;
        }
      });
  },
  renderSettings(container, data, onChange, { t } = {}) {
    container.innerHTML = `
      <label class="inspector-field">
        <span>API Key</span>
        <input type="text" id="apod-key" value="${escapeHtml(data.apiKey || '')}" placeholder="DEMO_KEY (可选)" />
      </label>
      <div class="inspector-notice">默认使用 NASA DEMO Key，也可填入 api.nasa.gov 申请的专属 Key。</div>
    `;
    container.querySelector('#apod-key').onchange = (e) => onChange({ ...data, apiKey: e.target.value.trim() });
  },
};
