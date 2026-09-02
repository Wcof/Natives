/**
 * Giphy Background.
 */

import { fetchDedup } from '../plugins-cache.js';
import { escapeHtml } from '../sanitizer.js';

export const giphyBackground = {
  key: 'background/giphy',
  name: 'Giphy',
  defaultData: { tag: 'nature', apiKey: '' },
  render(container, data) {
    const tag = data.tag || 'nature';
    const key = data.apiKey || '';
    container.style.backgroundColor = 'transparent';

    if (!key) {
      if (!container.style.backgroundImage || container.style.backgroundImage === 'none') {
        container.innerHTML = '<div class="background-not-configured">GIPHY API Key 未配置（需填入 Giphy 开发者 Key）</div>';
      }
      return;
    }

    fetchDedup(
      `giphy_${tag}_${key}`,
      async () => {
        const res = await fetch(`https://api.giphy.com/v1/gifs/random?api_key=${encodeURIComponent(key)}&tag=${encodeURIComponent(tag)}&rating=g`);
        const json = await res.json();
        const url = json.data?.images?.original?.url;
        return url ? { url } : null;
      },
      10 * 60 * 1000,
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
      .catch(() => {
        if (!container.style.backgroundImage || container.style.backgroundImage === 'none') {
          container.innerHTML = '<div class="background-not-configured">Giphy 动画获取失败</div>';
        }
      });
  },
  renderSettings(container, data, onChange, { t } = {}) {
    container.innerHTML = `
      <div class="inspector-field-group">
        <label class="inspector-field"><span>标签 (Tag)</span><input type="text" id="g-tag" value="${escapeHtml(data.tag || 'nature')}" /></label>
        <label class="inspector-field"><span>API Key</span><input type="text" id="g-key" value="${escapeHtml(data.apiKey || '')}" placeholder="developers.giphy.com key" /></label>
      </div>
    `;
    const update = () => onChange({
      ...data,
      tag: container.querySelector('#g-tag').value.trim(),
      apiKey: container.querySelector('#g-key').value.trim(),
    });
    container.querySelector('#g-tag').onchange = update;
    container.querySelector('#g-key').onchange = update;
  },
};
